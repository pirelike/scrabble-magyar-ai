"""Admin API: biztonság (belépési napló, IP tiltás, forgalomkorlát, kódok, munkamenetek) és rendszer (állapot,
konfiguráció, adatbázis, mentés, naplók, folyamatok, tunnel, push / SMTP teszt)."""
import os
import shutil
import tempfile

from flask import g, request, send_file

import admin
import admin_comm
import admin_live
import admin_mail
import admin_update
import admin_security
import admin_system
import auth
import email_service
import mail_config
import push_service
import routes as main_routes
import tunnel
from admin import AdminError
from admin_routes import (
    admin_bp, body_or_empty, csv_response, danger, json_ok, reason_of, sudo_required, wants_csv,
)


# ===== Biztonság =====

@admin_bp.route('/security/logins', methods=['GET'])
def security_logins():
    if wants_csv():
        data = admin_security.list_logins({**request.args.to_dict(), 'limit': '200', 'offset': '0'})
        with auth.transaction() as conn:
            admin.record(conn, g.admin_ctx, 'view.logins_export', 'security', None, {'rows': len(data['items'])})
        return csv_response('admin-logins.csv', admin_security.LOGIN_CSV_FIELDS, data['items'])
    return json_ok(**admin_security.list_logins(request.args))


@admin_bp.route('/security/rate-limits', methods=['GET'])
def security_rate_limits():
    return json_ok(**admin_security.rate_limit_state(main_routes._rate_limiter))


@admin_bp.route('/security/rate-limits/unblock', methods=['POST'])
@danger
def security_rate_unblock():
    body = body_or_empty()
    admin_security.unblock_ip(g.admin_ctx, main_routes._rate_limiter, body.get('ip'), reason_of(body))
    return json_ok()


@admin_bp.route('/security/ip-bans', methods=['GET'])
def security_ip_bans():
    return json_ok(items=admin_security.list_ip_bans(), own_ip=g.admin_ctx.ip)


@admin_bp.route('/security/ip-bans', methods=['POST'])
@sudo_required
def security_ip_ban_add():
    body = body_or_empty()
    ban_id = admin_security.add_ip_ban(g.admin_ctx, body.get('ip'), body.get('until'), reason_of(body),
                                       own_ip=g.admin_ctx.ip)
    return json_ok(id=ban_id)


@admin_bp.route('/security/ip-bans/<int:ban_id>', methods=['DELETE'])
@danger
def security_ip_ban_remove(ban_id):
    admin_security.remove_ip_ban(g.admin_ctx, ban_id, reason_of(body_or_empty()))
    return json_ok()


@admin_bp.route('/security/codes', methods=['GET'])
def security_codes():
    """A függő regisztrációs kódok (SMTP nélkül itt látszik a kód). Naplózva: `view.codes`."""
    return json_ok(items=admin_security.list_codes(g.admin_ctx), smtp=mail_config.is_configured())


@admin_bp.route('/security/codes/<int:code_id>', methods=['DELETE'])
@danger
def security_code_invalidate(code_id):
    admin_security.invalidate_code(g.admin_ctx, code_id, reason_of(body_or_empty()))
    return json_ok()


@admin_bp.route('/security/sessions', methods=['GET'])
def security_sessions():
    return json_ok(items=admin_security.list_sessions(request.args, g.admin_token))


@admin_bp.route('/security/sessions/<int:session_id>', methods=['DELETE'])
@danger
def security_session_revoke(session_id):
    admin_security.revoke_session(g.admin_ctx, session_id, reason_of(body_or_empty()), g.admin_token)
    return json_ok()


@admin_bp.route('/security/sessions/revoke-all', methods=['POST'])
@sudo_required
def security_sessions_revoke_all():
    """Mindenki kijelentkeztetése (pl. a SECRET_KEY cseréje után) — a saját munkameneted megmarad."""
    removed = admin_security.revoke_all_sessions(g.admin_ctx, g.admin_token, reason_of(body_or_empty()))
    admin_live.kick_all({g.admin_user['id']})
    return json_ok(removed=removed)


@admin_bp.route('/security/sessions/close-admins', methods=['POST'])
@danger
def security_sessions_close_admins():
    """Az adminok minden más munkamenetének bezárása (a sajátodon kívül)."""
    return json_ok(removed=admin_security.close_other_admin_sessions(g.admin_ctx, g.admin_token,
                                                                    reason_of(body_or_empty())))


# ===== Rendszer =====

@admin_bp.route('/system', methods=['GET'])
def system_overview():
    """A rendszer oldal teljes tartalma: szerver, szolgáltatások, konfiguráció (titkok maszkolva), verziók,
    adatbázis, mentések, háttérfolyamatok, elemzések sora."""
    return json_ok(
        server=admin_system.process_info(), services=admin_system.services_info(),
        config=admin_system.config_view(), versions=admin_system.versions(),
        database={'size': admin_system.db_file_size(), 'tables': admin_system.table_counts(),
                  'last_backup': admin_system.last_backup_time(), 'backups': admin_system.list_backups()},
        jobs=admin_system.jobs_status(), analysis=admin_system.analysis_queue(),
        push={'available': push_service.is_available(),
              'public_key': push_service.public_key() if push_service.is_available() else None,
              'subscriptions': _push_count()},
        tunnel=tunnel.status(), mail=admin_mail.view())


def _push_count():
    with auth.transaction() as conn:
        return conn.execute('SELECT COUNT(*) FROM push_subscriptions').fetchone()[0]


@admin_bp.route('/system/logs', methods=['GET'])
def system_logs():
    """A szerver naplójának utolsó sorai (gyűrűpuffer): szint szerint (az adott szinttől felfelé), szöveg szerint,
    `since_id` után (élő követéshez)."""
    level = request.args.get('level')
    if level and level not in admin_system.LOG_LEVELS:
        raise AdminError('Érvénytelen szűrő.', 400)
    items = admin_system.log_buffer.query(level, request.args.get('q'), request.args.get('since_id', 0, type=int),
                                          min(500, max(1, request.args.get('limit', 200, type=int))))
    return json_ok(items=items, groups=admin_system.log_buffer.error_groups())


@admin_bp.route('/system/backup', methods=['GET'])
@sudo_required
def system_backup_download():
    """Az adatbázis konzisztens pillanatképe letöltésként (SQLite backup API; naplózva)."""
    folder = tempfile.mkdtemp(prefix='scrabble-backup-')
    try:
        path = admin_system.make_backup(folder)
        with auth.transaction() as conn:
            admin.record(conn, g.admin_ctx, 'system.backup_download', 'system', None,
                         {'size': os.path.getsize(path)})
    except Exception:
        shutil.rmtree(folder, ignore_errors=True)
        raise
    response = send_file(path, as_attachment=True, download_name=os.path.basename(path),
                         mimetype='application/octet-stream')
    response.call_on_close(lambda: shutil.rmtree(folder, ignore_errors=True))
    return response


@admin_bp.route('/system/backup', methods=['POST'])
@danger
def system_backup_create():
    """Mentés a szerver mentési mappájába (a beállított számú legutóbbi megmarad)."""
    import settings
    path = admin_system.make_backup()
    removed = admin_system.prune_backups(int(settings.get('backup_keep')))
    with auth.transaction() as conn:
        admin.record(conn, g.admin_ctx, 'system.backup', 'system', os.path.basename(path),
                     {'size': os.path.getsize(path), 'pruned': removed})
    return json_ok(name=os.path.basename(path), size=os.path.getsize(path), pruned=removed)


@admin_bp.route('/system/vacuum', methods=['POST'])
@danger
def system_vacuum():
    with admin.action(g.admin_ctx, 'system.vacuum', 'system', None, reason=reason_of(body_or_empty())):
        pass
    before, after = admin_system.maintenance_vacuum()
    return json_ok(before=before, after=after)


@admin_bp.route('/system/cleanup', methods=['GET'])
def system_cleanup_preview():
    return json_ok(counts=admin_system.cleanup_preview(request.args.get('days', admin_system.ABANDONED_DEFAULT_DAYS,
                                                                       type=int)))


@admin_bp.route('/system/cleanup', methods=['POST'])
@sudo_required
def system_cleanup():
    body = body_or_empty()
    removed = admin_system.cleanup_run(g.admin_ctx, body.get('items'),
                                       body.get('days', admin_system.ABANDONED_DEFAULT_DAYS), reason_of(body))
    return json_ok(removed=removed)


@admin_bp.route('/system/tunnel/restart', methods=['POST'])
@sudo_required
def system_tunnel_restart():
    with admin.action(g.admin_ctx, 'system.tunnel_restart', 'system', None, reason=reason_of(body_or_empty())):
        pass
    if not tunnel.restart_tunnel():
        raise AdminError('A tunnel nem indítható újra.', 409)
    return json_ok(**tunnel.status())


@admin_bp.route('/system/smtp-test', methods=['POST'])
@danger
def system_smtp_test():
    """Teszt e-mail magamnak (a küldés eredményével)."""
    with auth.transaction() as conn:
        admin.record(conn, g.admin_ctx, 'system.smtp_test', 'system', None)
    ok, error = email_service.send_plain_email(g.admin_user['email'], 'Magyar Scrabble — teszt e-mail',
                                               'Ez egy teszt e-mail az admin panelről. Az SMTP működik.', wait=True)
    if error == 'smtp_not_configured':
        return json_ok(sent=False, console=True)
    if not ok:
        raise AdminError('Az e-mail küldése nem sikerült.', 502, code=error)
    return json_ok(sent=True, console=False)


# ===== Levelező szerver (SMTP) =====

@admin_bp.route('/system/mail', methods=['GET'])
def system_mail_get():
    """A levelező szerver beállítása (a jelszó nélkül) és a környezeti alapérték."""
    return json_ok(mail=admin_mail.view())


@admin_bp.route('/system/mail', methods=['PATCH'])
@sudo_required
def system_mail_update():
    """A levelező szerver beállítása: kiszolgáló, port, titkosítás, bejelentkezés, feladó. Újraindítás nélkül hat."""
    return json_ok(mail=admin_mail.update(g.admin_ctx, body_or_empty()))


@admin_bp.route('/system/mail', methods=['DELETE'])
@sudo_required
def system_mail_reset():
    """A mentett beállítás törlése: a környezeti változók (`SMTP_*`) értéke lép érvénybe."""
    return json_ok(mail=admin_mail.reset(g.admin_ctx, reason_of(body_or_empty())))


@admin_bp.route('/system/mail/check', methods=['POST'])
@sudo_required
def system_mail_check():
    """Kapcsolat-próba a megadott (még nem mentett) beállítással; `send: true` esetén teszt levéllel a saját címedre."""
    return json_ok(**admin_mail.check(g.admin_ctx, body_or_empty(), g.admin_user['email']))


@admin_bp.route('/system/push-test', methods=['POST'])
@danger
def system_push_test():
    """Teszt push üzenet magamnak."""
    return json_ok(**admin_comm.push_test(g.admin_ctx, g.admin_user['id']))


# ===== Frissítés GitHubról és újraindítás =====

@admin_bp.route('/system/update', methods=['GET'])
def system_update_status():
    """A tár állapota hálózat nélkül: ág, commit, távoli cím, helyi módosítások, újraindítás szükséges-e."""
    return json_ok(update=admin_update.status())


@admin_bp.route('/system/update/check', methods=['POST'])
@danger
def system_update_check():
    """Lekéri az ágakat a GitHubról (`git fetch`), és megmondja, mi változna: `{branch}` (elhagyható)."""
    body = body_or_empty()
    return json_ok(update=admin_update.check(body.get('branch'), fetch=body.get('fetch') is not False))


@admin_bp.route('/system/update/apply', methods=['POST'])
@sudo_required
def system_update_apply():
    """Frissítés a GitHub egy ágára (előretekerés): `{branch, reason, install?, restart?}`."""
    body = body_or_empty()
    return json_ok(update=admin_update.apply(g.admin_ctx, body.get('branch'), reason_of(body),
                                             install=body.get('install') is True, restart=body.get('restart') is True))


@admin_bp.route('/system/restart', methods=['POST'])
@sudo_required
def system_restart():
    """A szerver újraindítása (a folyamat önmagára cserélése); a kapcsolatok megszakadnak, a kliensek újracsatlakoznak."""
    return json_ok(**admin_update.restart(g.admin_ctx, reason_of(body_or_empty())))
