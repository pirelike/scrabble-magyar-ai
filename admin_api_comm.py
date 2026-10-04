"""Admin API: kommunikáció (közlemény, karbantartási mód, push, e-mail), beállítások és statisztika."""
from flask import g, request

import admin
import admin_comm
import admin_live
import admin_stats
import auth
import settings
from admin import AdminError
from admin_routes import (
    admin_bp, body_or_empty, csv_response, danger, json_ok, reason_of, sudo_required, wants_csv,
)


def _broadcast_announcements():
    """A közlemények élő frissítése minden kliensnek (a szerver `announcement` eseményével)."""
    admin_live.srv().broadcast_announcements()


# ===== Közlemények =====

@admin_bp.route('/announcements', methods=['GET'])
def announcements_list():
    return json_ok(items=admin_comm.list_announcements(), maintenance=admin_comm.maintenance_state())


@admin_bp.route('/announcements', methods=['POST'])
def announcements_create():
    new_id = admin_comm.create_announcement(g.admin_ctx, body_or_empty())
    _broadcast_announcements()
    return json_ok(id=new_id)


@admin_bp.route('/announcements/<int:announcement_id>', methods=['PATCH'])
def announcements_update(announcement_id):
    admin_comm.update_announcement(g.admin_ctx, announcement_id, body_or_empty())
    _broadcast_announcements()
    return json_ok()


@admin_bp.route('/announcements/<int:announcement_id>', methods=['DELETE'])
def announcements_revoke(announcement_id):
    admin_comm.revoke_announcement(g.admin_ctx, announcement_id)
    _broadcast_announcements()
    return json_ok()


# ===== Karbantartási mód =====

@admin_bp.route('/maintenance', methods=['GET'])
def maintenance_get():
    return json_ok(**admin_comm.maintenance_state())


@admin_bp.route('/maintenance', methods=['POST'])
@danger
def maintenance_set():
    state = admin_comm.set_maintenance(g.admin_ctx, body_or_empty())
    _broadcast_announcements()
    return json_ok(**state)


# ===== Push és e-mail =====

@admin_bp.route('/push/preview', methods=['POST'])
def push_preview():
    return json_ok(**admin_comm.push_preview(body_or_empty(), admin_live.online_user_ids()))


@admin_bp.route('/push', methods=['POST'])
@danger
def push_send():
    return json_ok(**admin_comm.push_send(g.admin_ctx, body_or_empty(), admin_live.online_user_ids()))


@admin_bp.route('/email', methods=['POST'])
@danger
def email_send():
    body = body_or_empty()
    user_id = body.get('user_id')
    if isinstance(user_id, bool) or not isinstance(user_id, int):
        raise AdminError('Érvénytelen kérés.', 400, field='user_id')
    return json_ok(**admin_comm.email_user(g.admin_ctx, user_id, body))


@admin_bp.route('/email/preview', methods=['GET'])
def email_preview():
    return json_ok(**admin_comm.email_preview(request.args.get('group')))


@admin_bp.route('/email/bulk', methods=['POST'])
@sudo_required
def email_bulk():
    return json_ok(**admin_comm.email_bulk(g.admin_ctx, body_or_empty()))


# ===== Beállítások (funkciókapcsolók) =====

@admin_bp.route('/settings', methods=['GET'])
def settings_get():
    items = settings.describe()
    ids = sorted({i['changed_by'] for i in items if i['changed_by']})
    names = {}
    if ids:
        with auth.transaction() as conn:
            names = {r['id']: r['display_name'] for r in conn.execute(
                f"SELECT id, display_name FROM users WHERE id IN ({','.join('?' * len(ids))})", ids)}
    for item in items:
        item['changed_by_name'] = names.get(item['changed_by'])
    return json_ok(items=items, groups=list(settings.GROUPS))


@admin_bp.route('/settings', methods=['PATCH'])
@danger
def settings_patch():
    """Egy vagy több beállítás módosítása: `{changes: {kulcs: érték | null}, reason}`. A `null` visszaállít az alapra.
    Minden változás a naplóba kerül (előtte / utána)."""
    body = body_or_empty()
    changes = body.get('changes')
    if not isinstance(changes, dict) or not changes:
        raise AdminError('Érvénytelen kérés.', 400)
    server = admin_live.srv()
    before, after = {}, {}
    with admin.action(g.admin_ctx, 'settings.update', 'setting', ','.join(sorted(changes)), reason=reason_of(body)) as act:
        for key, value in changes.items():
            spec = settings.definition(key)
            if spec is None or spec.get('hidden'):
                raise AdminError('Ismeretlen beállítás.', 400, field=key)
            before[key] = settings.get(key)
            try:
                settings.store(act.conn, key, value, g.admin_ctx.admin_user_id)
                after[key] = settings.default_of(key) if value is None else settings.validate(key, value)
            except settings.SettingError as exc:
                raise AdminError(str(exc), 400, field=key) from None
        act.details['before'] = before
        act.details['after'] = after
    server.apply_runtime_settings()
    return json_ok(changed=sorted(changes))


# ===== Statisztika =====

@admin_bp.route('/stats', methods=['GET'])
def stats_get():
    data = admin_stats.stats(request.args.get('metric'), request.args.get('range', 30))
    if wants_csv():
        table = data['table']
        rows = [dict(zip(table['columns'], row)) for row in table['rows']]
        return csv_response(f"admin-stats-{data['metric']}.csv", table['columns'], rows)
    return json_ok(**data)
