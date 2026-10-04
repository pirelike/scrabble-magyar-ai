"""Admin API: élő szobák, játékarchívum, levelezős játékok, ranglista / értékszám, napi feladvány.

`/api/admin/rooms`, `/games`, `/async`, `/ratings`, `/daily`. Az őr és a forgalomkorlát minden útvonalra érvényes.
"""
import json

from flask import g, make_response, request

import admin
import admin_games
import admin_live
import admin_stats
import analysis
import auth
import routes as main_routes
from admin import AdminError
from admin_routes import (
    _json_error, admin_bp, body_or_empty, csv_response, danger, flat, json_ok, reason_of, sudo_required, wants_csv,
)

# A legsúlyosabb szoba-beavatkozásokhoz friss jelszó-megerősítés kell
SUDO_ROOM_ACTIONS = frozenset({'end', 'void', 'disband'})


def _live_game_ids():
    return {room.db_game_id for room in admin_live.srv().state.rooms.values() if room.db_game_id}


# ===== Szobák =====

@admin_bp.route('/rooms', methods=['GET'])
def rooms_list():
    data = admin_live.list_rooms(request.args)
    if wants_csv():
        rows = [{'code': r['code'], 'id': r['id'], 'name': r['name'], 'kind': r['kind'], 'status': r['status'],
                 'owner': r['owner'], 'players': flat([p['name'] for p in r['players']]),
                 'spectators': r['spectators'], 'idle_seconds': r['idle_seconds'], 'stuck': r['stuck']}
                for r in data['items']]
        return csv_response('admin-rooms.csv', ('code', 'id', 'name', 'kind', 'status', 'owner', 'players',
                                                'spectators', 'idle_seconds', 'stuck'), rows)
    return json_ok(**data)


@admin_bp.route('/rooms/<key>', methods=['GET'])
def rooms_detail(key):
    return json_ok(room=admin_live.room_detail(g.admin_ctx, key))


@admin_bp.route('/rooms/<key>/racks', methods=['GET'])
def rooms_racks(key):
    """A játékosok keze: csak kifejezett kérésre, naplózva (`view.racks`)."""
    return json_ok(racks=admin_live.room_racks(g.admin_ctx, key))


@admin_bp.route('/rooms/<key>/action', methods=['POST'])
@danger
def rooms_action(key):
    """Beavatkozás: message, kick, transfer, skip, extend, pause, resume, resolve_vote, reschedule_bot, save, end, void,
    disband. A legsúlyosabbakhoz (end, void, disband) sudo kell."""
    body = body_or_empty()
    if body.get('action') in SUDO_ROOM_ACTIONS and not admin.sudo_active(g.admin_token):
        return _json_error(401, 'Ehhez a művelethez jelszó-megerősítés (sudo) szükséges.', sudo_required=True)
    return json_ok(**admin_live.room_action(g.admin_ctx, key, body))


# ===== Játékarchívum =====

@admin_bp.route('/games', methods=['GET'])
def games_list():
    if wants_csv():
        data = admin_games.list_games({**request.args.to_dict(), 'limit': '200', 'offset': '0'})
        rows = [{**item, 'players': flat([f"{p['name']} ({p['score']})" for p in item['players']]),
                 'winners': flat(item['winners'])} for item in data['items']]
        return csv_response('admin-games.csv', admin_games.GAME_CSV_FIELDS, rows)
    return json_ok(**admin_games.list_games(request.args))


@admin_bp.route('/games/<int:game_id>', methods=['GET'])
def games_detail(game_id):
    return json_ok(game=admin_games.game_detail(g.admin_ctx, game_id))


@admin_bp.route('/games/<int:game_id>/moves', methods=['GET'])
def games_moves(game_id):
    """A lépésnapló a tábla-pillanatképekkel (a visszajátszáshoz)."""
    return json_ok(moves=admin_games.game_moves(game_id))


@admin_bp.route('/games/<int:game_id>/export', methods=['GET'])
def games_export(game_id):
    data = admin_games.export_game(g.admin_ctx, game_id)
    response = make_response(json.dumps(data, ensure_ascii=False, indent=2))
    response.headers['Content-Type'] = 'application/json; charset=utf-8'
    response.headers['Content-Disposition'] = f'attachment; filename="game-{game_id}.json"'
    return response


@admin_bp.route('/games/<int:game_id>/void', methods=['POST'])
@danger
def games_void(game_id):
    body = body_or_empty()
    summary = admin_games.void_game(g.admin_ctx, game_id, reason_of(body), body.get('revoke_badges', False))
    return json_ok(rating_changes=summary['changed'])


@admin_bp.route('/games/<int:game_id>/unvoid', methods=['POST'])
@danger
def games_unvoid(game_id):
    summary = admin_games.unvoid_game(g.admin_ctx, game_id, reason_of(body_or_empty()))
    return json_ok(rating_changes=summary['changed'])


@admin_bp.route('/games/<int:game_id>/unshare', methods=['POST'])
def games_unshare(game_id):
    admin_games.unshare_game(g.admin_ctx, game_id, reason_of(body_or_empty()))
    return json_ok()


@admin_bp.route('/games/<int:game_id>/status', methods=['POST'])
def games_status(game_id):
    body = body_or_empty()
    admin_games.set_game_status(g.admin_ctx, game_id, body.get('status'), reason_of(body), _live_game_ids())
    return json_ok()


@admin_bp.route('/games/<int:game_id>/reanalyze', methods=['POST'])
@danger
def games_reanalyze(game_id):
    """Az elemzés újrafuttatása: a gyorsítótár törlése és az elemzés elindítása a háttérben."""
    with admin.action(g.admin_ctx, 'game.reanalyze', 'game', game_id, reason=reason_of(body_or_empty())) as act:
        row = act.conn.execute('SELECT status FROM saved_games WHERE id = ?', (game_id,)).fetchone()
        if row is None:
            raise AdminError('A játék nem található.', 404)
        if row['status'] != 'finished':
            raise AdminError('Csak befejezett játék elemezhető.', 409)
        moves = auth.get_game_moves(game_id)
        turns = analysis.analyzable_turns(moves)
        if not turns:
            raise AdminError('A játékhoz nincs rögzített kéz, nem elemezhető.', 409)
        act.conn.execute('DELETE FROM game_analysis WHERE game_id = ?', (game_id,))
        act.details['turns'] = len(turns)
    main_routes._analysis_jobs[game_id] = {'done': 0, 'total': len(turns)}
    main_routes._socketio.start_background_task(main_routes._run_analysis, game_id, moves)
    return json_ok(total=len(turns))


@admin_bp.route('/games/<int:game_id>', methods=['DELETE'])
@sudo_required
def games_delete(game_id):
    body = body_or_empty()
    summary = admin_games.delete_game(g.admin_ctx, game_id, reason_of(body), body.get('confirm_finished', False),
                                      _live_game_ids())
    return json_ok(rating_changes=summary['changed'] if summary else 0)


@admin_bp.route('/games/cleanup', methods=['GET'])
def games_cleanup_preview():
    return json_ok(**admin_games.old_abandoned_preview(request.args.get('days', 30, type=int)))


@admin_bp.route('/games/cleanup', methods=['POST'])
@sudo_required
def games_cleanup():
    body = body_or_empty()
    deleted = admin_games.old_abandoned_delete(g.admin_ctx, body.get('days', 30), reason_of(body), _live_game_ids())
    return json_ok(deleted=deleted)


# ===== Levelezős játékok =====

@admin_bp.route('/async', methods=['GET'])
def async_list():
    return json_ok(**admin_games.list_async())


@admin_bp.route('/async/sweep', methods=['POST'])
@danger
def async_sweep():
    return json_ok(expired=admin_games.async_sweep(g.admin_ctx))


@admin_bp.route('/async/<int:game_id>/extend', methods=['POST'])
def async_extend(game_id):
    body = body_or_empty()
    return json_ok(deadline=admin_games.async_extend(g.admin_ctx, game_id, body.get('hours'), reason_of(body)))


@admin_bp.route('/async/<int:game_id>/expire', methods=['POST'])
def async_expire(game_id):
    return json_ok(player=admin_games.async_expire(g.admin_ctx, game_id, reason_of(body_or_empty())))


@admin_bp.route('/async/<int:game_id>/resign', methods=['POST'])
@danger
def async_resign(game_id):
    body = body_or_empty()
    admin_games.async_resign(g.admin_ctx, game_id, body.get('player'), reason_of(body))
    return json_ok()


@admin_bp.route('/async/<int:game_id>/remind', methods=['POST'])
def async_remind(game_id):
    return json_ok(sent=admin_games.async_remind(g.admin_ctx, game_id))


@admin_bp.route('/async/<int:game_id>/void', methods=['POST'])
@danger
def async_void(game_id):
    admin_games.async_void(g.admin_ctx, game_id, reason_of(body_or_empty()))
    return json_ok()


@admin_bp.route('/async/<int:game_id>/load', methods=['POST'])
def async_load(game_id):
    admin_games.async_load(g.admin_ctx, game_id)
    return json_ok()


@admin_bp.route('/async/<int:game_id>/unload', methods=['POST'])
def async_unload(game_id):
    admin_games.async_unload(g.admin_ctx, game_id, reason_of(body_or_empty()))
    return json_ok()


# ===== Ranglista és értékszám =====

@admin_bp.route('/ratings', methods=['GET'])
def ratings_leaderboard():
    metric = request.args.get('metric', 'rating')
    limit = request.args.get('limit', 100, type=int)
    entries = admin_games.leaderboard(metric, limit)
    if wants_csv():
        fields = ('rank', 'user_id', 'display_name', 'games_played', 'games_won', 'win_rate', 'avg_score',
                  'best_score', 'rating', 'rated_games')
        return csv_response('admin-leaderboard.csv', fields, entries)
    return json_ok(metric=metric, entries=entries)


@admin_bp.route('/ratings/suspicious', methods=['GET'])
def ratings_suspicious():
    return json_ok(**admin_games.suspicious_patterns())


@admin_bp.route('/ratings/recompute', methods=['POST'])
@danger
def ratings_recompute():
    """Teljes ELO újraszámolás: `dry_run` (alapértelmezett) csak előnézet; az alkalmazáshoz sudo és indoklás kell."""
    body = body_or_empty()
    if body.get('dry_run', True) is not False:
        return json_ok(dry_run=True, **admin_games.ratings_dry_run())
    if not admin.sudo_active(g.admin_token):
        return _json_error(401, 'Ehhez a művelethez jelszó-megerősítés (sudo) szükséges.', sudo_required=True)
    return json_ok(dry_run=False, **admin_games.ratings_apply(g.admin_ctx, reason_of(body)))


# ===== Napi feladvány =====

@admin_bp.route('/daily', methods=['GET'])
def daily_day():
    return json_ok(**admin_stats.daily_day(request.args.get('date')))


@admin_bp.route('/daily/archive', methods=['GET'])
def daily_archive():
    return json_ok(items=admin_stats.daily_archive(request.args.get('days', 30, type=int)))


@admin_bp.route('/daily/preview', methods=['GET'])
def daily_preview():
    return json_ok(**admin_stats.daily_preview(request.args.get('date') or ''))


@admin_bp.route('/daily/<date>/regenerate', methods=['POST'])
@danger
def daily_regenerate(date):
    body = body_or_empty()
    result = admin_stats.daily_regenerate(g.admin_ctx, date, reason_of(body), body.get('reset_scores', False),
                                          sudo_ok=admin.sudo_active(g.admin_token))
    return json_ok(**result)


@admin_bp.route('/daily/<date>/scores/<int:user_id>', methods=['DELETE'])
@danger
def daily_delete_score(date, user_id):
    admin_stats.daily_delete_score(g.admin_ctx, date, user_id, reason_of(body_or_empty()))
    return json_ok()


@admin_bp.route('/daily/<date>/scores/<int:user_id>/reset-revealed', methods=['POST'])
def daily_reset_revealed(date, user_id):
    admin_stats.daily_reset_revealed(g.admin_ctx, date, user_id, reason_of(body_or_empty()))
    return json_ok()
