"""Admin panel: játékok — archívum, levelezős játékok, ranglista és értékszám (ELO újraszámolás, gyanús minták).

Az archívum a `saved_games` táblából dolgozik. Az érvénytelenítés (`status = 'voided'`) csak jelölés: a játék kikerül
a ranglistából és az értékszámításból, de megmarad, és visszavonható. Az értékszám-újraszámolás a befejezett, nem
érvénytelenített, robot nélküli játékokból épül fel időrendben (`elo.rating_changes`), a kézi módosításokkal együtt.
"""
import json
from datetime import timedelta
from itertools import combinations

import admin
import admin_live
import admin_users
import auth
import elo
from admin import AdminError

GAME_STATUSES = ('active', 'finished', 'abandoned', 'voided')
GAME_SORTS = {'id': 'sg.id', 'created_at': 'sg.created_at', 'updated_at': 'sg.updated_at'}
MAX_CHANGES_SHOWN = 200
SUSPICIOUS_PAIR_GAMES = 5            # ennyi közös játék után jelzünk egyoldalú párosítást
SUSPICIOUS_PAIR_SHARE = 0.8          # a nagyobbik fél győzelmi aránya
FAST_PASS_MAX_MOVES = 16
FAST_PASS_SHARE = 0.8
FAST_PASS_MIN_MOVES = 6
EFFICIENCY_MIN_GAMES = 3
EFFICIENCY_THRESHOLD = 95.0
ASYNC_EXTEND_HOURS = (6, 12, 24, 48, 72, 168)


# ===== Archívum =====

def _players_of(rows):
    players = {}
    for r in rows:
        players.setdefault(r['game_id'], []).append({
            'name': r['player_name'], 'user_id': r['user_id'], 'score': r['final_score'],
            'winner': bool(r['is_winner']), 'rating_before': r['rating_before'], 'rating_after': r['rating_after']})
    return players


def list_games(args):
    """Az archívum szűrve (status, since / until, user, bots, async, suspicious, q) és lapozva."""
    where, params = [], []
    status = args.get('status')
    if status:
        if status not in GAME_STATUSES:
            raise AdminError('Érvénytelen szűrő.', 400)
        where.append('sg.status = ?')
        params.append(status)
    if args.get('since'):
        where.append('sg.created_at >= ?')
        params.append(admin._parse_bound(args['since'])[0])
    if args.get('until'):
        stamp, exclusive = admin._parse_bound(args['until'], end=True)
        where.append('sg.created_at < ?' if exclusive else 'sg.created_at <= ?')
        params.append(stamp)
    user = (args.get('user') or '').strip()
    if user:
        if user.isdigit():
            where.append('EXISTS (SELECT 1 FROM game_players p WHERE p.game_id = sg.id AND p.user_id = ?)')
            params.append(int(user))
        else:
            where.append("EXISTS (SELECT 1 FROM game_players p WHERE p.game_id = sg.id AND p.player_name LIKE ? ESCAPE '\\')")
            params.append(admin._like(user))
    for key, column in (('bots', 'sg.has_bots'), ('async', 'sg.is_async')):
        if args.get(key) in ('0', '1'):
            where.append(f'{column} = ?')
            params.append(int(args[key]))
    if args.get('suspicious') in ('1', 'true'):
        ids = sorted(suspicious_game_ids())
        where.append('sg.id IN (%s)' % ','.join('?' * len(ids)) if ids else '0')
        params.extend(ids)
    q = (args.get('q') or '').strip()
    if q:
        clause = "sg.room_name LIKE ? ESCAPE '\\'"
        params.append(admin._like(q))
        if q.isdigit():
            clause += ' OR sg.id = ?'
            params.append(int(q))
        where.append('(' + clause + ')')
    limit, offset = admin.page_args(args)
    clause = ('WHERE ' + ' AND '.join(where)) if where else ''
    order = admin.order_by(args.get('sort'), args.get('order', 'desc'), GAME_SORTS, 'id')
    with auth.transaction() as conn:
        total = conn.execute(f'SELECT COUNT(*) FROM saved_games sg {clause}', params).fetchone()[0]
        rows = conn.execute(
            'SELECT sg.id, sg.room_name, sg.status, sg.has_bots, sg.is_async, sg.challenge_mode, sg.created_at, '
            'sg.updated_at, sg.share_token IS NOT NULL AS shared, '
            '(SELECT COUNT(*) FROM game_moves m WHERE m.game_id = sg.id) AS moves, '
            'EXISTS (SELECT 1 FROM game_analysis a WHERE a.game_id = sg.id) AS analysed '
            f'FROM saved_games sg {clause} ORDER BY {order}, sg.id DESC LIMIT ? OFFSET ?',
            params + [limit, offset]).fetchall()
        ids = [r['id'] for r in rows]
        players = {}
        if ids:
            players = _players_of(conn.execute(
                'SELECT game_id, player_name, user_id, final_score, is_winner, rating_before, rating_after '
                f"FROM game_players WHERE game_id IN ({','.join('?' * len(ids))}) ORDER BY id", ids).fetchall())
    items = []
    for r in rows:
        items.append({'id': r['id'], 'name': r['room_name'], 'status': r['status'], 'bots': bool(r['has_bots']),
                      'async': bool(r['is_async']), 'challenge': bool(r['challenge_mode']), 'moves': r['moves'],
                      'created_at': r['created_at'], 'updated_at': r['updated_at'], 'shared': bool(r['shared']),
                      'analysed': bool(r['analysed']), 'players': players.get(r['id'], []),
                      'winners': [p['name'] for p in players.get(r['id'], []) if p['winner']]})
    return {'items': items, 'total': total, 'limit': limit, 'offset': offset}


GAME_CSV_FIELDS = ('id', 'name', 'status', 'bots', 'async', 'moves', 'created_at', 'updated_at', 'players', 'winners')


def game_detail(ctx, game_id):
    """Egy játék részletei: adatok, játékosok (értékszám-változással), lépésnapló (kezekkel), elemzés összegzése.
    A kezek miatt a megtekintés naplózva: `view.game`."""
    with auth.transaction() as conn:
        row = conn.execute('SELECT * FROM saved_games WHERE id = ?', (game_id,)).fetchone()
        if row is None:
            raise AdminError('A játék nem található.', 404)
        players = [dict(r) for r in conn.execute(
            'SELECT player_name AS name, user_id, final_score AS score, is_winner AS winner, rating_before, '
            'rating_after FROM game_players WHERE game_id = ? ORDER BY id', (game_id,))]
        moves = []
        for m in conn.execute('SELECT move_number, player_name, action_type, details_json, created_at '
                              'FROM game_moves WHERE game_id = ? ORDER BY move_number', (game_id,)):
            try:
                details = json.loads(m['details_json'] or '{}')
            except ValueError:
                details = {}
            moves.append({'n': m['move_number'], 'player': m['player_name'], 'type': m['action_type'],
                          'score': details.get('score'), 'words': details.get('words', []),
                          'rack': details.get('rack'), 'admin': bool(details.get('admin')),
                          'at': m['created_at']})
        analysis_row = conn.execute('SELECT version, result_json, created_at FROM game_analysis WHERE game_id = ?',
                                    (game_id,)).fetchone()
        admin.record(conn, ctx, 'view.game', 'game', game_id)
    analysis = None
    if analysis_row:
        try:
            analysis = {'version': analysis_row['version'], 'created_at': analysis_row['created_at'],
                        'players': json.loads(analysis_row['result_json']).get('players', [])}
        except ValueError:
            analysis = None
    for p in players:
        p['winner'] = bool(p['winner'])
        p['rating_change'] = (p['rating_after'] - p['rating_before']
                              if p['rating_after'] is not None and p['rating_before'] is not None else None)
    history = admin.query_audit(target_type='game', target_id=str(game_id), limit=30)['items']
    return {
        'id': row['id'], 'name': row['room_name'], 'status': row['status'], 'room_id': row['room_id'],
        'bots': bool(row['has_bots']), 'async': bool(row['is_async']), 'challenge': bool(row['challenge_mode']),
        'owner': row['owner_name'], 'created_at': row['created_at'], 'updated_at': row['updated_at'],
        'share_token': row['share_token'], 'players': players, 'moves': moves, 'analysis': analysis,
        'admin_history': history,
    }


def game_moves(game_id):
    """A lépésnapló a tábla-pillanatképekkel (a visszajátszáshoz)."""
    with auth.transaction() as conn:
        if conn.execute('SELECT 1 FROM saved_games WHERE id = ?', (game_id,)).fetchone() is None:
            raise AdminError('A játék nem található.', 404)
        rows = conn.execute('SELECT move_number, player_name, action_type, details_json, board_snapshot_json '
                            'FROM game_moves WHERE game_id = ? ORDER BY move_number', (game_id,)).fetchall()
    moves = []
    for r in rows:
        try:
            details = json.loads(r['details_json'] or '{}')
            board = json.loads(r['board_snapshot_json']) if r['board_snapshot_json'] else None
        except ValueError:
            details, board = {}, None
        moves.append({'n': r['move_number'], 'player': r['player_name'], 'type': r['action_type'],
                      'score': details.get('score'), 'words': details.get('words', []),
                      'tiles': details.get('tiles', []), 'board': board})
    return moves


def _game_row(conn, game_id):
    row = conn.execute('SELECT * FROM saved_games WHERE id = ?', (game_id,)).fetchone()
    if row is None:
        raise AdminError('A játék nem található.', 404)
    return row


def _participants(conn, game_id):
    return [r['user_id'] for r in conn.execute(
        'SELECT DISTINCT user_id FROM game_players WHERE game_id = ? AND user_id IS NOT NULL', (game_id,))]


def _refresh_after_status_change(conn, user_ids):
    """Státuszváltozás után: a résztvevők számlálói és az (összes érintett) értékszám újraszámolása."""
    for uid in user_ids:
        admin_users.recompute_user_counters(conn, uid)
    return recompute_ratings(conn, apply=True)


def void_game(ctx, game_id, reason, revoke_badges=False):
    """Érvénytelenítés: kikerül a ranglistából, a résztvevők értékszáma és számlálói újraszámolódnak; a kitüntetések
    csak kérésre vesznek el. Visszatér: az értékszám-újraszámolás összegzése."""
    revoke_badges = admin.bool_field(revoke_badges, 'revoke_badges')
    with admin.action(ctx, 'game.void', 'game', game_id, reason=reason) as act:
        row = _game_row(act.conn, game_id)
        if row['status'] != 'finished':
            raise AdminError('Csak befejezett játék érvényteleníthető.', 409)
        act.conn.execute("UPDATE saved_games SET status = 'voided' WHERE id = ?", (game_id,))
        users = _participants(act.conn, game_id)
        if revoke_badges:
            act.details['badges_revoked'] = act.conn.execute(
                'DELETE FROM achievements WHERE game_id = ?', (game_id,)).rowcount
        summary = _refresh_after_status_change(act.conn, users)
        act.details['before'] = {'status': 'finished'}
        act.details['after'] = {'status': 'voided'}
        act.details['rating_changes'] = summary['changed']
    return summary


def unvoid_game(ctx, game_id, reason):
    with admin.action(ctx, 'game.unvoid', 'game', game_id, reason=reason) as act:
        row = _game_row(act.conn, game_id)
        if row['status'] != 'voided':
            raise AdminError('A játék nincs érvénytelenítve.', 409)
        act.conn.execute("UPDATE saved_games SET status = 'finished' WHERE id = ?", (game_id,))
        summary = _refresh_after_status_change(act.conn, _participants(act.conn, game_id))
        act.details['before'] = {'status': 'voided'}
        act.details['after'] = {'status': 'finished'}
        act.details['rating_changes'] = summary['changed']
    return summary


def unshare_game(ctx, game_id, reason):
    with admin.action(ctx, 'game.unshare', 'game', game_id, reason=reason) as act:
        row = _game_row(act.conn, game_id)
        if not row['share_token']:
            raise AdminError('A játéknak nincs megosztási linkje.', 409)
        act.conn.execute('UPDATE saved_games SET share_token = NULL WHERE id = ?', (game_id,))


def set_game_status(ctx, game_id, status, reason, live_game_ids=frozenset()):
    """Mentett játék állapotának módosítása: `abandoned` ↔ `active`."""
    if status not in ('abandoned', 'active'):
        raise AdminError('Érvénytelen állapot.', 400, field='status')
    with admin.action(ctx, 'game.status', 'game', game_id, reason=reason) as act:
        row = _game_row(act.conn, game_id)
        if row['status'] not in ('abandoned', 'active'):
            raise AdminError('Ez a játék állapota nem módosítható.', 409)
        if row['status'] == status:
            raise AdminError('A játék már ebben az állapotban van.', 409)
        if game_id in live_game_ids:
            raise AdminError('A játék éppen fut, előbb oszlasd fel a szobát.', 409)
        act.conn.execute('UPDATE saved_games SET status = ? WHERE id = ?', (status, game_id))
        act.details['before'] = {'status': row['status']}
        act.details['after'] = {'status': status}


def export_game(ctx, game_id):
    """A játék állapota + lépései JSON-ban (naplózva)."""
    with auth.transaction() as conn:
        row = _game_row(conn, game_id)
        data = {
            'game': {k: row[k] for k in ('id', 'room_id', 'room_name', 'status', 'challenge_mode', 'owner_name',
                                         'has_bots', 'is_async', 'created_at', 'updated_at')},
            'state': json.loads(row['state_json']) if row['state_json'] else None,
            'players': [dict(r) for r in conn.execute(
                'SELECT player_name, user_id, final_score, is_winner, rating_before, rating_after '
                'FROM game_players WHERE game_id = ?', (game_id,))],
            'moves': [dict(r) for r in conn.execute(
                'SELECT move_number, player_name, action_type, details_json, board_snapshot_json, created_at '
                'FROM game_moves WHERE game_id = ? ORDER BY move_number', (game_id,))],
        }
        admin.record(conn, ctx, 'view.game_export', 'game', game_id)
    return data


def delete_game(ctx, game_id, reason, confirm_finished=False, live_game_ids=frozenset()):
    """Végleges törlés. Befejezett (ranglistás) játékot csak külön megerősítéssel (`confirm_finished`) lehet törölni;
    a törlés után a résztvevők számlálói és az értékszámok újraszámolódnak."""
    confirm_finished = admin.bool_field(confirm_finished, 'confirm_finished')
    with admin.action(ctx, 'game.delete', 'game', game_id, reason=reason) as act:
        row = _game_row(act.conn, game_id)
        if game_id in live_game_ids:
            raise AdminError('A játék éppen fut, előbb oszlasd fel a szobát.', 409)
        if row['status'] == 'finished' and not confirm_finished:
            raise AdminError('Befejezett játék törléséhez külön megerősítés kell.', 409, needs_confirm=True)
        users = _participants(act.conn, game_id)
        names = [r['player_name'] for r in act.conn.execute(
            'SELECT player_name FROM game_players WHERE game_id = ?', (game_id,))]
        act.conn.execute('DELETE FROM achievements WHERE game_id = ?', (game_id,))
        act.conn.execute('DELETE FROM saved_games WHERE id = ?', (game_id,))
        summary = _refresh_after_status_change(act.conn, users) if row['status'] in ('finished', 'voided') else None
        act.details['before'] = {'name': row['room_name'], 'status': row['status'], 'players': names}
        act.details['rating_changes'] = summary['changed'] if summary else 0
    return summary


def old_abandoned_preview(days):
    """A régi (`abandoned`, `days` napnál régebbi) mentések, a törlés előnézete."""
    days = admin.int_field(days, 'days', 1, 3650)
    cutoff = auth.format_ts(auth.utcnow() - timedelta(days=days))
    with auth.transaction() as conn:
        rows = conn.execute("SELECT id, room_name, updated_at FROM saved_games WHERE status = 'abandoned' "
                            'AND updated_at < ? ORDER BY updated_at LIMIT 500', (cutoff,)).fetchall()
        total = conn.execute("SELECT COUNT(*) FROM saved_games WHERE status = 'abandoned' AND updated_at < ?",
                             (cutoff,)).fetchone()[0]
    return {'total': total, 'items': [dict(r) for r in rows], 'days': days}


def old_abandoned_delete(ctx, days, reason, live_game_ids=frozenset()):
    days = admin.int_field(days, 'days', 1, 3650)
    cutoff = auth.format_ts(auth.utcnow() - timedelta(days=days))
    with admin.action(ctx, 'game.bulk_delete', 'game', None, reason=reason, details={'days': days}) as act:
        ids = [r['id'] for r in act.conn.execute(
            "SELECT id FROM saved_games WHERE status = 'abandoned' AND updated_at < ?", (cutoff,))]
        ids = [i for i in ids if i not in live_game_ids]
        for i in ids:
            act.conn.execute('DELETE FROM saved_games WHERE id = ?', (i,))
        act.details['deleted'] = len(ids)
        act.details['ids'] = ids[:500]
    return len(ids)


# ===== Értékszám újraszámolása =====

def _state_flags(state_json):
    """{játékosnév: feladta-e} a mentett állapotból."""
    try:
        data = json.loads(state_json)
    except (TypeError, ValueError):
        return {}
    return {p.get('name'): bool(p.get('resigned')) for p in data.get('players', [])}


def _timeline(conn):
    """A befejezett, nem érvénytelenített, robot nélküli játékok és a kézi módosítások időrendben."""
    events = []
    for g in conn.execute("SELECT id, updated_at, state_json FROM saved_games WHERE status = 'finished' "
                          'AND has_bots = 0 ORDER BY updated_at, id'):
        events.append((g['updated_at'] or '', 0, g['id'], g))
    for a in conn.execute('SELECT id, user_id, rating_before, rating_after, created_at FROM rating_adjustments '
                          'ORDER BY created_at, id'):
        events.append((a['created_at'] or '', 1, a['id'], a))
    events.sort(key=lambda e: (e[0], e[1], e[2]))
    return events


def compute_ratings(conn):
    """A teljes értékszám-számítás a nulláról. Visszatér: ({user_id: (értékszám, értékelt játékok)},
    {(game_id, user_id): (előtte, utána)})."""
    state = {r['id']: (elo.INITIAL_RATING, 0) for r in conn.execute('SELECT id FROM users')}
    per_game = {}
    for _stamp, kind, _id, row in _timeline(conn):
        if kind == 1:
            if row['user_id'] in state:
                rating, games = state[row['user_id']]
                state[row['user_id']] = (rating + (row['rating_after'] - row['rating_before']), games)
            continue
        flags = _state_flags(row['state_json'])
        ranked = {}
        for p in conn.execute('SELECT player_name, user_id, final_score FROM game_players '
                              'WHERE game_id = ? AND user_id IS NOT NULL ORDER BY id', (row['id'],)):
            uid = p['user_id']
            if uid in state and uid not in ranked:
                score = -10 ** 9 if flags.get(p['player_name']) else p['final_score']
                ranked[uid] = (state[uid][0], state[uid][1], score)
        if len(ranked) < 2:
            continue
        changes = elo.rating_changes([(uid, r, g, s) for uid, (r, g, s) in ranked.items()])
        for uid, (before, games, _score) in ranked.items():
            after = before + changes[uid]
            state[uid] = (after, games + 1)
            per_game[(row['id'], uid)] = (before, after)
    return state, per_game


def recompute_ratings(conn, apply=False):
    """A teljes ELO újraszámolás. `apply` hamis: csak előnézet (ki mennyit változna). Igaz: a felhasználók és a
    `game_players` értékei felülíródnak. Visszatér: {'changed', 'unchanged', 'items': [...]}."""
    state, per_game = compute_ratings(conn)
    names = {r['id']: r['display_name'] for r in conn.execute('SELECT id, display_name FROM users')}
    current = {r['id']: (r['rating'], r['rated_games']) for r in
               conn.execute('SELECT id, rating, rated_games FROM users')}
    items, unchanged = [], 0
    for uid, (rating, games) in state.items():
        cur_rating, cur_games = current[uid]
        if (rating, games) == (cur_rating, cur_games):
            unchanged += 1
            continue
        items.append({'user_id': uid, 'display_name': names.get(uid), 'current': cur_rating, 'new': rating,
                      'delta': rating - cur_rating, 'rated_games': cur_games, 'new_rated_games': games})
    items.sort(key=lambda i: abs(i['delta']), reverse=True)
    if apply:
        for uid, (rating, games) in state.items():
            if (rating, games) != current[uid]:
                conn.execute('UPDATE users SET rating = ?, rated_games = ? WHERE id = ?', (rating, games, uid))
        conn.execute('UPDATE game_players SET rating_before = NULL, rating_after = NULL '
                     'WHERE rating_before IS NOT NULL OR rating_after IS NOT NULL')
        for (game_id, uid), (before, after) in per_game.items():
            conn.execute('UPDATE game_players SET rating_before = ?, rating_after = ? WHERE game_id = ? AND user_id = ?',
                         (before, after, game_id, uid))
    return {'changed': len(items), 'unchanged': unchanged, 'items': items[:MAX_CHANGES_SHOWN]}


def ratings_dry_run():
    with auth.transaction() as conn:
        return recompute_ratings(conn, apply=False)


def ratings_apply(ctx, reason):
    """Az újraszámolt értékszámok alkalmazása (naplózva)."""
    with admin.action(ctx, 'ratings.recompute', 'setting', None, reason=reason) as act:
        summary = recompute_ratings(act.conn, apply=True)
        act.details['changed'] = summary['changed']
        act.details['top_changes'] = summary['items'][:20]
    return summary


# ===== Ranglista és gyanús minták =====

def leaderboard(metric, limit):
    """A nyilvánossal azonos ranglista, a minimum játékszám nélkül."""
    if metric not in auth.LEADERBOARD_METRICS:
        raise AdminError('Ismeretlen rangsor.', 400)
    entries, _me = auth.get_leaderboard(metric, limit, None, ignore_min=True)
    return entries


def _human_games(conn):
    """{game_id: [(user_id, név, pont, nyert)]} a befejezett, robot nélküli játékokból (csak regisztráltakkal)."""
    games = {}
    for r in conn.execute(
            'SELECT gp.game_id, gp.user_id, gp.player_name, gp.final_score, gp.is_winner FROM game_players gp '
            "JOIN saved_games sg ON sg.id = gp.game_id AND sg.status = 'finished' AND sg.has_bots = 0 "
            'WHERE gp.user_id IS NOT NULL ORDER BY gp.game_id, gp.id'):
        games.setdefault(r['game_id'], []).append((r['user_id'], r['player_name'], r['final_score'], bool(r['is_winner'])))
    return games


def _user_ips(conn):
    ips = {}
    for r in conn.execute('SELECT user_id, ip FROM login_events WHERE success = 1 AND user_id IS NOT NULL '
                          'AND ip IS NOT NULL GROUP BY user_id, ip'):
        ips.setdefault(r['user_id'], set()).add(r['ip'])
    for r in conn.execute('SELECT id, last_login_ip FROM users WHERE last_login_ip IS NOT NULL'):
        ips.setdefault(r['id'], set()).add(r['last_login_ip'])
    return ips


def suspicious_patterns():
    """Gyanús minták (csak jelzés, automatikus büntetés nincs): egyoldalú párosítás, azonos IP-ről érkező
    ellenfelek, gyors passzolós játékok, kiugróan jó (a legjobb lépést rendszeresen rakó) játékosok."""
    with auth.transaction() as conn:
        names = {r['id']: r['display_name'] for r in conn.execute('SELECT id, display_name FROM users')}
        games = _human_games(conn)
        ips = _user_ips(conn)
        pair_games, pair_wins, shared_ip = {}, {}, {}
        for game_id, players in games.items():
            if len(players) == 2:
                (a, _na, _sa, wa), (b, _nb, _sb, wb) = players
                if a != b:
                    key = tuple(sorted((a, b)))
                    pair_games.setdefault(key, []).append(game_id)
                    wins = pair_wins.setdefault(key, {key[0]: 0, key[1]: 0})
                    if wa and not wb:
                        wins[a] += 1
                    elif wb and not wa:
                        wins[b] += 1
            for (ua, *_x), (ub, *_y) in combinations(players, 2):
                if ua != ub:
                    common = ips.get(ua, set()) & ips.get(ub, set())
                    if common:
                        shared_ip.setdefault(tuple(sorted((ua, ub))), {'games': [], 'ips': set()})
                        entry = shared_ip[tuple(sorted((ua, ub)))]
                        entry['games'].append(game_id)
                        entry['ips'] |= common
        one_sided = []
        for key, ids in pair_games.items():
            wins = pair_wins[key]
            top = max(wins.values())
            if len(ids) >= SUSPICIOUS_PAIR_GAMES and top / len(ids) >= SUSPICIOUS_PAIR_SHARE:
                one_sided.append({'users': [{'id': u, 'name': names.get(u), 'wins': wins[u]} for u in key],
                                  'games': len(ids), 'game_ids': ids[-20:]})
        one_sided.sort(key=lambda e: e['games'], reverse=True)
        same_ip = [{'users': [{'id': u, 'name': names.get(u)} for u in key], 'games': len(v['games']),
                    'game_ids': v['games'][-20:], 'ips': sorted(v['ips'])[:5]} for key, v in shared_ip.items()]
        same_ip.sort(key=lambda e: e['games'], reverse=True)
        fast = []
        for r in conn.execute(
                'SELECT m.game_id AS game_id, COUNT(*) AS total, SUM(CASE WHEN m.action_type = "pass" THEN 1 ELSE 0 END) '
                'AS passes FROM game_moves m JOIN saved_games sg ON sg.id = m.game_id '
                "WHERE sg.status = 'finished' AND sg.has_bots = 0 GROUP BY m.game_id "
                'HAVING total >= ? AND total <= ? AND passes * 1.0 / total >= ?',
                (FAST_PASS_MIN_MOVES, FAST_PASS_MAX_MOVES, FAST_PASS_SHARE)):
            fast.append({'game_id': r['game_id'], 'moves': r['total'], 'passes': r['passes'],
                         'players': [p[1] for p in games.get(r['game_id'], [])]})
        efficiency = _efficiency_outliers(conn, games, names)
    return {'one_sided_pairs': one_sided, 'same_ip': same_ip, 'fast_pass_games': fast, 'high_efficiency': efficiency,
            'thresholds': {'pair_games': SUSPICIOUS_PAIR_GAMES, 'pair_share': SUSPICIOUS_PAIR_SHARE,
                           'efficiency': EFFICIENCY_THRESHOLD, 'efficiency_games': EFFICIENCY_MIN_GAMES}}


def _efficiency_outliers(conn, games, names):
    """A játékelemzésekből: akinek az átlagos hatékonysága (a legjobb lépéshez mérve) rendszeresen kiugróan magas."""
    per_user = {}
    for r in conn.execute('SELECT game_id, result_json FROM game_analysis'):
        try:
            players = json.loads(r['result_json']).get('players', [])
        except ValueError:
            continue
        by_name = {p['player']: p for p in players}
        for user_id, name, _score, _win in games.get(r['game_id'], []):
            entry = by_name.get(name)
            if entry and entry.get('turns', 0) >= 8:
                per_user.setdefault(user_id, []).append(entry['efficiency'])
    result = []
    for user_id, values in per_user.items():
        average = sum(values) / len(values)
        if len(values) >= EFFICIENCY_MIN_GAMES and average >= EFFICIENCY_THRESHOLD:
            result.append({'user_id': user_id, 'name': names.get(user_id), 'games': len(values),
                           'average': round(average, 1)})
    result.sort(key=lambda e: e['average'], reverse=True)
    return result


def suspicious_game_ids():
    """A gyanús mintákban szereplő játékok azonosítói (az archívum „gyanús” szűrőjéhez)."""
    patterns = suspicious_patterns()
    ids = set()
    for entry in patterns['one_sided_pairs'] + patterns['same_ip']:
        ids.update(entry['game_ids'])
    ids.update(e['game_id'] for e in patterns['fast_pass_games'])
    return ids


# ===== Levelezős játékok =====

def _async_row(row, room, now):
    """Egy levelezős játék sora: a memóriában lévő szoba (ha betöltött), különben a mentett állapot."""
    try:
        state = json.loads(row['state_json'])
    except (TypeError, ValueError):
        state = {}
    if room is not None:
        game = room.game
        players = [{'name': p.name, 'score': p.score, 'timeouts': p.timeouts, 'resigned': p.resigned}
                   for p in game.players]
        current = game.current_player().name if game.players else None
        deadline, turn_hours, finished = game.turn_deadline, game.turn_hours, game.finished
    else:
        players = [{'name': p.get('name'), 'score': p.get('score', 0), 'timeouts': p.get('timeouts', 0),
                    'resigned': bool(p.get('resigned'))} for p in state.get('players', [])]
        index = state.get('current_player_idx', 0)
        current = players[index]['name'] if 0 <= index < len(players) else None
        deadline, turn_hours, finished = state.get('turn_deadline'), state.get('turn_hours', 0), state.get('finished')
    return {'id': row['id'], 'name': row['room_name'], 'players': players, 'current': current, 'deadline': deadline,
            'remaining': (deadline - now) if deadline else None, 'turn_hours': turn_hours, 'finished': bool(finished),
            'loaded': room is not None, 'room_id': row['room_id'], 'updated_at': row['updated_at'],
            'created_at': row['created_at'], 'moves': row['moves'], 'last_move_at': row['last_move_at'],
            'max_timeouts': max((p['timeouts'] for p in players), default=0)}


def list_async():
    """A folyamatban lévő levelezős játékok és a háttérfeladat (`_async_sweeper`) állapota."""
    import time
    import admin_system
    server = admin_live.srv()
    loaded = {room.db_game_id: room for room in server.state.rooms.values() if room.is_async}
    now = time.time()
    with auth.transaction() as conn:
        rows = conn.execute(
            "SELECT sg.id, sg.room_id, sg.room_name, sg.state_json, sg.updated_at, sg.created_at, "
            '(SELECT COUNT(*) FROM game_moves m WHERE m.game_id = sg.id) AS moves, '
            '(SELECT MAX(m.created_at) FROM game_moves m WHERE m.game_id = sg.id) AS last_move_at '
            "FROM saved_games sg WHERE sg.is_async = 1 AND sg.status = 'active' ORDER BY sg.updated_at DESC").fetchall()
    items = [_async_row(r, loaded.get(r['id']), now) for r in rows]
    sweeper = next((j for j in admin_system.jobs_status() if j['name'] == 'async_sweeper'), None)
    return {'items': items, 'sweeper': sweeper, 'loaded_count': len(loaded)}


def _async_game(game_id):
    """(sor, szoba): a levelezős játék sora, a szoba betöltve a memóriába (ha még nem volt)."""
    with auth.transaction() as conn:
        row = conn.execute("SELECT * FROM saved_games WHERE id = ? AND is_async = 1 AND status = 'active'",
                           (game_id,)).fetchone()
    if row is None:
        raise AdminError('Ez a levelezős játék nem található.', 404)
    server = admin_live.srv()
    room = server._async_room_for_db_game(dict(row))
    if room is None:
        raise AdminError('Ez a levelezős játék nem tölthető be.', 500)
    return row, room


def async_extend(ctx, game_id, hours, reason):
    """A soron lévő határidejének meghosszabbítása `hours` órával (a lejárt határidő is a mostantól számolódik)."""
    import time
    if hours not in ASYNC_EXTEND_HOURS:
        raise AdminError('Érvénytelen időtartam.', 400, field='hours')
    _row, room = _async_game(game_id)
    game = room.game
    if game.finished or not game.turn_deadline:
        raise AdminError('Ennek a játéknak nincs határideje.', 409)
    new_deadline = max(game.turn_deadline, time.time()) + hours * 3600
    with admin.action(ctx, 'async.extend', 'game', game_id, reason=reason,
                      details={'hours': hours, 'before': {'deadline': game.turn_deadline},
                               'after': {'deadline': new_deadline}}):
        pass
    game.turn_deadline = new_deadline
    admin_live.srv()._emit_all_states(game, room.id)
    return new_deadline


def async_expire(ctx, game_id, reason):
    """A kör azonnali lejáratása: automatikus passz (három egymás utáni után a játékos feladja)."""
    _row, room = _async_game(game_id)
    game = room.game
    current = game.current_player()
    with admin.action(ctx, 'async.expire', 'game', game_id, reason=reason, details={'player': current.name}):
        pass
    if not game.expire_turn():
        raise AdminError('A kör most nem járatható le.', 409)
    server = admin_live.srv()
    server._emit_all_states(game, room.id)
    server._cleanup_finished_async(room.id)
    return current.name


def async_resign(ctx, game_id, player_name, reason):
    """Feladás valaki nevében (a játék véget ér, a feladó nem lehet győztes)."""
    _row, room = _async_game(game_id)
    game = room.game
    player = next((p for p in game.players if p.name == player_name and not p.is_bot), None)
    if player is None:
        raise AdminError('A játékos nem található.', 404, field='player')
    with admin.action(ctx, 'async.resign', 'game', game_id, reason=reason, details={'player': player_name}):
        pass
    ok, _msg = game.resign(player.id)
    if not ok:
        raise AdminError('A játék nem adható fel.', 409)
    server = admin_live.srv()
    server._emit_all_states(game, room.id)
    server._cleanup_finished_async(room.id)


def async_remind(ctx, game_id):
    """Push emlékeztető a soron lévőnek."""
    import push_service
    _row, room = _async_game(game_id)
    game = room.game
    current = game.current_player()
    user_id = room.known_user_ids.get(current.name) if current else None
    if not user_id:
        raise AdminError('A soron lévő játékos nem regisztrált.', 409)
    sent = push_service.notify_background(user_id, 'reminder', room=room.name, room_id=room.id)
    with auth.transaction() as conn:
        admin.record(conn, ctx, 'async.remind', 'game', game_id, {'player': current.name, 'sent': bool(sent)})
    return bool(sent)


def async_void(ctx, game_id, reason):
    """A folyamatban lévő levelezős játék érvénytelenítése: jelölés, a szoba megszűnik."""
    row, room = _async_game(game_id)
    with admin.action(ctx, 'async.void', 'game', game_id, reason=reason) as act:
        act.conn.execute("UPDATE saved_games SET status = 'voided' WHERE id = ?", (game_id,))
    server = admin_live.srv()
    room.manually_saved = True
    server._disband_active_room(room.id, 'A játék érvénytelenítve lett.')
    server.socketio.emit('rooms_list', server.state.get_rooms_list())


def async_unload(ctx, game_id, reason):
    """A játék kiürítése a memóriából (az adatbázisban megmarad; csak ha senki sincs a szobában)."""
    server = admin_live.srv()
    room = next((r for r in server.state.rooms.values() if r.is_async and r.db_game_id == game_id), None)
    if room is None:
        raise AdminError('A játék nincs betöltve.', 404)
    if room.game.has_connected_human() or room.spectators:
        raise AdminError('A szobában éppen vannak, nem üríthető ki.', 409)
    with admin.action(ctx, 'async.unload', 'game', game_id, reason=reason):
        pass
    server._cleanup_room(room.id)


def async_load(ctx, game_id):
    """A játék betöltése a memóriába."""
    _async_game(game_id)
    with auth.transaction() as conn:
        admin.record(conn, ctx, 'async.load', 'game', game_id)


def async_sweep(ctx):
    """A határidő-figyelő kézi indítása. Visszatér: a léptetett játékok száma."""
    server = admin_live.srv()
    with auth.transaction() as conn:
        admin.record(conn, ctx, 'async.sweep', 'system', None)
    return server._expire_async_turns()
