"""Admin API: áttekintés, globális kereső és felhasználók (`/api/admin/overview`, `/search`, `/users...`).

Az őr (`admin_routes._guard`) minden útvonalra érvényes; a romboló műveletekhez sudo kell (`sudo_required`).
"""
from flask import g, request

import admin
import admin_live
import admin_users
import auth
import config
import email_service
import word_review
from admin import AdminError
from admin_routes import (
    _json_error, admin_bp, csv_response, danger, json_ok, reason_of, sudo_required, wants_csv, body_or_empty,
)


# ===== Áttekintés, grafikonok, kereső =====

@admin_bp.route('/overview', methods=['GET'])
def overview():
    """Élő számlálók, mai adatok, szerver és szolgáltatások állapota, figyelmeztetések."""
    return json_ok(**admin_live.overview())


@admin_bp.route('/charts', methods=['GET'])
def charts():
    """Napi bontású grafikonok (alapértelmezés: 30 nap)."""
    return json_ok(**admin_live.charts(request.args.get('days', 30, type=int)))


@admin_bp.route('/search', methods=['GET'])
def global_search():
    """Globális kereső: felhasználó (név / e-mail / id), szoba (kód / név / id), játék (id / név), szó."""
    q = (request.args.get('q') or '').strip()
    if len(q) > 60:
        raise AdminError('A szűrő túl hosszú.', 400)
    result = {'users': [], 'rooms': [], 'games': [], 'words': []}
    if not q:
        return json_ok(**result)
    result['users'] = admin_users.search_brief(q)
    lowered = q.lower()
    for room in list(admin_live.srv().state.rooms.values()):
        if lowered in room.join_code or lowered == room.id.lower() or lowered in room.name.lower():
            result['rooms'].append({'id': room.id, 'code': room.join_code, 'name': room.name})
            if len(result['rooms']) >= 8:
                break
    with auth.transaction() as conn:
        clauses, params = ["room_name LIKE ? ESCAPE '\\'"], [admin._like(q)]
        if q.isdigit():
            clauses.append('id = ?')
            params.append(int(q))
        rows = conn.execute(f"SELECT id, room_name, status FROM saved_games WHERE {' OR '.join(clauses)} "
                            'ORDER BY id DESC LIMIT 8', params).fetchall()
    result['games'] = [{'id': r['id'], 'name': r['room_name'], 'status': r['status']} for r in rows]
    from tiles import tokenize_word
    word = q.upper()
    if 2 <= len(word) <= 15 and tokenize_word(word) is not None:
        result['words'].append({'word': word})
    return json_ok(**result)


# ===== Felhasználók =====

@admin_bp.route('/users', methods=['GET'])
def users_list():
    online = admin_live.online_user_ids()
    if wants_csv():
        data = admin_users.list_users(request.args, online, limit=10000, offset=0)
        return csv_response('admin-users.csv', admin_users.USER_CSV_FIELDS, data['items'])
    return json_ok(**admin_users.list_users(request.args, online))


@admin_bp.route('/users/<int:user_id>', methods=['GET'])
def users_detail(user_id):
    live = admin_live.user_live(user_id)
    data = admin_users.user_detail(g.admin_ctx, user_id, online_info=live['online'])
    data['live'] = live
    data['is_self'] = user_id == g.admin_user['id']
    return json_ok(**data)


@admin_bp.route('/users/<int:user_id>/profile', methods=['GET'])
def users_profile(user_id):
    """A profil oldal tartalma, ahogy a felhasználó látja (csak olvasható nézet; megszemélyesítés nincs)."""
    return json_ok(profile=admin_users.profile_view(g.admin_ctx, user_id))


@admin_bp.route('/users/<int:user_id>', methods=['PATCH'])
def users_patch(user_id):
    """Név és / vagy e-mail módosítása. Az e-mail módosításához sudo kell; a régi címre értesítés megy."""
    body = body_or_empty()
    wants_name = 'display_name' in body
    wants_email = 'email' in body
    if not (wants_name or wants_email):
        return _json_error(400, 'Érvénytelen kérés.')
    if wants_email and not admin.sudo_active(g.admin_token):
        return _json_error(401, 'Ehhez a művelethez jelszó-megerősítés (sudo) szükséges.', sudo_required=True)
    result = {}
    if wants_name:
        new_name = admin_users.sanitize_name(body.get('display_name'))
        if new_name and admin_live.rename_conflicts(user_id, new_name):
            return _json_error(409, 'Már van ilyen nevű játékos abban a szobában, ahol a felhasználó játszik.')
        old, new, games = admin_users.rename(g.admin_ctx, user_id, body.get('display_name'), reason_of(body))
        admin_live.rename_live(user_id, old, new)
        result['display_name'] = new
    if wants_email:
        old_email = admin_users.change_email(g.admin_ctx, user_id, body.get('email'), reason_of(body))
        ok, _err = email_service.send_plain_email(
            old_email, 'Magyar Scrabble — megváltozott az e-mail címed',
            'Az e-mail címedet egy moderátor megváltoztatta. Ha ez nem várt, vedd fel velünk a kapcsolatot.')
        result['email'] = body.get('email').strip()
        result['notified'] = bool(ok)
    return json_ok(**result)


@admin_bp.route('/users/<int:user_id>/ban', methods=['POST'])
@sudo_required
def users_ban(user_id):
    body = body_or_empty()
    until = admin_users.ban(g.admin_ctx, user_id, body.get('until'), reason_of(body))
    reason = admin.normalize_reason(reason_of(body))
    kicked = admin_live.kick_user(user_id, 'account_banned',
                                  {'reason': reason, 'until': None if until == auth.PERMANENT_UNTIL else until})
    return json_ok(until=None if until == auth.PERMANENT_UNTIL else until, kicked=kicked)


@admin_bp.route('/users/<int:user_id>/unban', methods=['POST'])
def users_unban(user_id):
    admin_users.unban(g.admin_ctx, user_id, reason_of(body_or_empty()))
    return json_ok()


@admin_bp.route('/users/<int:user_id>/mute', methods=['POST'])
def users_mute(user_id):
    body = body_or_empty()
    return json_ok(until=admin_users.mute(g.admin_ctx, user_id, body.get('until'), reason_of(body)))


@admin_bp.route('/users/<int:user_id>/unmute', methods=['POST'])
def users_unmute(user_id):
    admin_users.unmute(g.admin_ctx, user_id, reason_of(body_or_empty()))
    return json_ok()


@admin_bp.route('/users/<int:user_id>/logout-all', methods=['POST'])
@danger
def users_logout_all(user_id):
    closed = admin_users.logout_all(g.admin_ctx, user_id, reason_of(body_or_empty()))
    return json_ok(sessions_closed=closed, disconnected=admin_live.kick_user(user_id))


@admin_bp.route('/users/<int:user_id>/reset-password', methods=['POST'])
@sudo_required
def users_reset_password(user_id):
    """Ideiglenes jelszó: e-mailben megy (SMTP-vel); SMTP nélkül az admin kapja meg egyszer a válaszban."""
    password, email, closed = admin_users.reset_password(g.admin_ctx, user_id, reason_of(body_or_empty()))
    emailed, _err = email_service.send_plain_email(
        email, 'Magyar Scrabble — ideiglenes jelszó',
        f'Az ideiglenes jelszavad: {password}\n\nBelépés után változtasd meg. A korábbi munkameneteid kijelentkeztek.')
    admin_live.kick_user(user_id)
    data = {'emailed': bool(emailed), 'sessions_closed': closed}
    if not config.SMTP_CONFIGURED:
        data['temp_password'] = password
    return json_ok(**data)


@admin_bp.route('/users/<int:user_id>/rating', methods=['POST'])
@sudo_required
def users_rating(user_id):
    body = body_or_empty()
    admin_users.set_rating(g.admin_ctx, user_id, body.get('rating'), reason_of(body))
    return json_ok()


@admin_bp.route('/users/<int:user_id>/badges', methods=['POST'])
def users_badges(user_id):
    body = body_or_empty()
    admin_users.set_badge(g.admin_ctx, user_id, body.get('badge'), body.get('grant'), reason_of(body))
    return json_ok()


@admin_bp.route('/users/<int:user_id>/recompute-stats', methods=['POST'])
def users_recompute_stats(user_id):
    admin_users.recompute_stats(g.admin_ctx, user_id, reason_of(body_or_empty()))
    return json_ok()


@admin_bp.route('/users/<int:user_id>/reviews/revert', methods=['POST'])
@sudo_required
def users_reviews_revert(user_id):
    body = body_or_empty()
    words = admin_users.revert_reviews(g.admin_ctx, user_id, body.get('since'), reason_of(body))
    word_review.refresh()
    return json_ok(count=len(words))


@admin_bp.route('/users/<int:user_id>/reviews/block', methods=['POST'])
def users_reviews_block(user_id):
    body = body_or_empty()
    admin_users.set_review_blocked(g.admin_ctx, user_id, body.get('blocked'), reason_of(body))
    word_review.refresh()
    return json_ok()


@admin_bp.route('/users/<int:user_id>/push-test', methods=['POST'])
def users_push_test(user_id):
    import admin_comm
    return json_ok(**admin_comm.push_test(g.admin_ctx, user_id))


@admin_bp.route('/users/<int:user_id>/push/<int:device_id>', methods=['DELETE'])
def users_push_delete(user_id, device_id):
    admin_users.delete_push_device(g.admin_ctx, user_id, device_id, reason_of(body_or_empty()))
    return json_ok()


@admin_bp.route('/users/<int:user_id>/notes', methods=['POST'])
def users_note_add(user_id):
    return json_ok(id=admin_users.add_note(g.admin_ctx, user_id, body_or_empty().get('note')))


@admin_bp.route('/users/<int:user_id>/notes/<int:note_id>', methods=['DELETE'])
def users_note_delete(user_id, note_id):
    admin_users.delete_note(g.admin_ctx, user_id, note_id)
    return json_ok()


@admin_bp.route('/users/<int:user_id>/export', methods=['GET'])
def users_export(user_id):
    """GDPR export: a felhasználó összes adata JSON-ban (letöltésként; naplózva)."""
    import json
    from flask import make_response
    data = admin_users.export_user(g.admin_ctx, user_id)
    response = make_response(json.dumps(data, ensure_ascii=False, indent=2))
    response.headers['Content-Type'] = 'application/json; charset=utf-8'
    response.headers['Content-Disposition'] = f'attachment; filename="user-{user_id}.json"'
    return response


@admin_bp.route('/users/<int:user_id>', methods=['DELETE'])
@sudo_required
def users_delete(user_id):
    """Fiók törlése / anonimizálása (a célpont nevének begépelése kötelező)."""
    body = body_or_empty()
    name = admin_users.delete_user(g.admin_ctx, user_id, body.get('mode', 'anonymize'), body.get('confirm_name'),
                                   reason_of(body))
    admin_live.kick_user(user_id)
    word_review.refresh()
    return json_ok(name=name)
