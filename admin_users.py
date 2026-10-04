"""Admin panel: felhasználók — lista, részletek és a felhasználói műveletek.

A modul a Flask kéréstől és a futó szerver állapotától független (az online állapotot és a socket-műveleteket a
hívó adja / végzi a tranzakció UTÁN). Minden módosító művelet `admin.action`-nel fut: kötelező indoklás, a napló
a művelettel egy tranzakcióban íródik. Védelmek: admin fiók nem tiltható ki, nem törölhető, az e-mail címe és a
jelszava nem módosítható a panelről (az adminság csak a szerver környezeti változójával változik).
"""
import json
import re
import secrets
import string
from datetime import timedelta
from urllib.parse import urlsplit

from werkzeug.security import generate_password_hash

import achievements
import admin
import auth
import config
from admin import AdminError

NAME_RE = re.compile(r'^[\w\sáéíóöőúüűÁÉÍÓÖŐÚÜŰ._-]{1,20}$', re.UNICODE)
EMAIL_RE = re.compile(r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$')
MAX_NOTE_LEN = 1000
MIN_RATING, MAX_RATING = 100, 4000
LIST_SORTS = {
    'id': 'u.id', 'name': 'py_fold(u.display_name)', 'email': 'u.email_lower', 'created_at': 'u.created_at',
    'last_login_at': 'COALESCE(u.last_login_at, \'\')', 'games_played': 'u.games_played', 'rating': 'u.rating',
}
STATUSES = ('active', 'banned', 'muted', 'deleted', 'review_blocked')
DELETED_NAME = 'Törölt felhasználó #{id}'
DELETED_EMAIL = 'deleted-{id}@deleted.invalid'
TEMP_PASSWORD_LENGTH = 12
_USER_AGENT_SHOW = 120


# ===== Közös =====

def _is_admin_email(email):
    return isinstance(email, str) and email.strip().lower() in config.ADMIN_EMAILS


def _row_status(row):
    if row['deleted_at']:
        return 'deleted'
    if auth.ban_from_row(row):
        return 'banned'
    if auth._until_active(row['chat_muted_until']):
        return 'muted'
    return 'active'


def _user_summary(row, online_ids=frozenset()):
    ban = auth.ban_from_row(row)
    return {
        'id': row['id'], 'display_name': row['display_name'], 'email': row['email'],
        'created_at': row['created_at'], 'last_login_at': row['last_login_at'],
        'online': row['id'] in online_ids, 'games_played': row['games_played'], 'games_won': row['games_won'],
        'rating': row['rating'], 'rated_games': row['rated_games'], 'status': _row_status(row),
        'banned': bool(ban), 'ban': ban, 'muted': auth._until_active(row['chat_muted_until']),
        'muted_until': row['chat_muted_until'] if auth._until_active(row['chat_muted_until']) else None,
        'review_blocked': bool(row['review_blocked']), 'deleted': bool(row['deleted_at']),
        'is_admin': _is_admin_email(row['email_lower']),
        'push_devices': row['push_devices'] if 'push_devices' in row.keys() else None,
    }


def get_row(conn, user_id):
    """A felhasználó sora, vagy 404."""
    row = conn.execute('SELECT * FROM users WHERE id = ?', (user_id,)).fetchone()
    if row is None:
        raise AdminError('Felhasználó nem található.', 404)
    return row


def _require_manageable(row, deleted_ok=False):
    """Admin és törölt fiókon nem végezhető romboló / személyes művelet."""
    if _is_admin_email(row['email_lower']):
        raise AdminError('Admin fiókon ez a művelet nem végezhető.', 403)
    if row['deleted_at'] and not deleted_ok:
        raise AdminError('A fiók törölve van.', 409)


def sanitize_name(name):
    if not isinstance(name, str):
        return None
    name = name.strip()
    return name if name and len(name) <= 20 and NAME_RE.match(name) else None


# ===== Lista =====

def list_users(args, online_ids=frozenset(), limit=None, offset=None):
    """Szűrt, rendezett, lapozott felhasználólista. `args`: q, status, online, admin, from, to, min_games,
    inactive_days, sort, order, limit, offset."""
    where, params = [], []
    q = (args.get('q') or '').strip()
    if q:
        clauses = ['py_fold(u.display_name) LIKE ? ESCAPE \'\\\'', 'u.email_lower LIKE ? ESCAPE \'\\\'']
        params.extend([admin.like_contains(q), admin._like(q.lower())])
        if q.isdigit():
            clauses.append('u.id = ?')
            params.append(int(q))
        where.append('(' + ' OR '.join(clauses) + ')')
    status = args.get('status')
    if status:
        if status not in STATUSES:
            raise AdminError('Érvénytelen szűrő.', 400)
        now = auth.format_ts(auth.utcnow())
        where.append({
            'active': "u.deleted_at IS NULL AND COALESCE(u.banned_until, '') <= ? "
                      "AND COALESCE(u.chat_muted_until, '') <= ?",
            'banned': "u.deleted_at IS NULL AND COALESCE(u.banned_until, '') > ?",
            'muted': "u.deleted_at IS NULL AND COALESCE(u.chat_muted_until, '') > ?",
            'deleted': 'u.deleted_at IS NOT NULL',
            'review_blocked': 'u.review_blocked = 1',
        }[status])
        params.extend({'active': [now, now], 'banned': [now], 'muted': [now]}.get(status, []))
    if args.get('online') in ('1', 'true'):
        ids = sorted(online_ids)
        where.append('u.id IN (%s)' % ','.join('?' * len(ids)) if ids else '0')
        params.extend(ids)
    if args.get('admin') in ('1', 'true'):
        emails = sorted(config.ADMIN_EMAILS)
        where.append('u.email_lower IN (%s)' % ','.join('?' * len(emails)) if emails else '0')
        params.extend(emails)
    if args.get('from'):
        stamp, _ = admin._parse_bound(args['from'])
        where.append('u.created_at >= ?')
        params.append(stamp)
    if args.get('to'):
        stamp, exclusive = admin._parse_bound(args['to'], end=True)
        where.append('u.created_at < ?' if exclusive else 'u.created_at <= ?')
        params.append(stamp)
    if args.get('min_games'):
        where.append('u.games_played >= ?')
        params.append(admin._int_arg(args['min_games'], 0, 0, 10 ** 6))
    if args.get('inactive_days'):
        days = admin._int_arg(args['inactive_days'], 0, 1, 3650)
        where.append("COALESCE(u.last_login_at, u.created_at) < ?")
        params.append(auth.format_ts(auth.utcnow() - timedelta(days=days)))

    if limit is None or offset is None:
        limit, offset = admin.page_args(args)
    clause = ('WHERE ' + ' AND '.join(where)) if where else ''
    order = admin.order_by(args.get('sort'), args.get('order', 'desc' if args.get('sort') in (None, '') else 'asc'),
                           LIST_SORTS, 'id')
    with auth.transaction() as conn:
        admin.register_sql_helpers(conn)
        total = conn.execute(f'SELECT COUNT(*) FROM users u {clause}', params).fetchone()[0]
        rows = conn.execute(
            'SELECT u.*, (SELECT COUNT(*) FROM push_subscriptions p WHERE p.user_id = u.id) AS push_devices '
            f'FROM users u {clause} ORDER BY {order}, u.id DESC LIMIT ? OFFSET ?',
            params + [limit, offset]).fetchall()
    return {'items': [_user_summary(r, online_ids) for r in rows], 'total': total, 'limit': limit,
            'offset': offset}


USER_CSV_FIELDS = ('id', 'display_name', 'email', 'created_at', 'last_login_at', 'games_played', 'games_won',
                   'rating', 'status', 'push_devices')


def search_brief(q, limit=8):
    """Rövid találatok a globális keresőhöz (név / e-mail / id)."""
    q = (q or '').strip()
    if not q:
        return []
    clauses = ['py_fold(display_name) LIKE ? ESCAPE \'\\\'', 'email_lower LIKE ? ESCAPE \'\\\'']
    params = [admin.like_contains(q), admin._like(q.lower())]
    if q.isdigit():
        clauses.append('id = ?')
        params.append(int(q))
    with auth.transaction() as conn:
        admin.register_sql_helpers(conn)
        rows = conn.execute(
            f"SELECT id, display_name, email, deleted_at FROM users WHERE {' OR '.join(clauses)} "
            'ORDER BY id DESC LIMIT ?', params + [limit]).fetchall()
    return [{'id': r['id'], 'display_name': r['display_name'], 'email': r['email'],
             'deleted': bool(r['deleted_at'])} for r in rows]


# ===== Részletek =====

def _host(endpoint):
    try:
        return urlsplit(endpoint).netloc or '?'
    except ValueError:
        return '?'


def _rating_history(conn, user_id):
    history = []
    for r in conn.execute(
            'SELECT sg.updated_at AS at, gp.rating_before AS before, gp.rating_after AS after, gp.game_id AS game_id '
            'FROM game_players gp JOIN saved_games sg ON sg.id = gp.game_id '
            'WHERE gp.user_id = ? AND gp.rating_after IS NOT NULL ORDER BY sg.updated_at, gp.id', (user_id,)):
        history.append({'at': r['at'], 'before': r['before'], 'after': r['after'], 'kind': 'game',
                        'game_id': r['game_id']})
    for r in conn.execute('SELECT created_at, rating_before, rating_after, reason FROM rating_adjustments '
                          'WHERE user_id = ?', (user_id,)):
        history.append({'at': r['created_at'], 'before': r['rating_before'], 'after': r['rating_after'],
                        'kind': 'adjustment', 'reason': r['reason']})
    history.sort(key=lambda item: item['at'] or '')
    return history


def user_detail(ctx, user_id, online_info=None):
    """A felhasználó teljes képe (személyes adat: a megtekintés naplózódik `view.user` néven)."""
    with auth.transaction() as conn:
        row = get_row(conn, user_id)
        summary = _user_summary(row, frozenset({user_id}) if online_info else frozenset())
        account = dict(summary)
        account.update({
            'last_login_ip': row['last_login_ip'], 'deleted_at': row['deleted_at'],
            'total_score': row['total_score'], 'ban_reason': row['ban_reason'],
            'win_rate': round(row['games_won'] / row['games_played'] * 100, 1) if row['games_played'] else 0,
            'avg_score': round(row['total_score'] / row['games_played'], 1) if row['games_played'] else 0,
        })
        sessions = [{'id': r['id'], 'created_at': r['created_at'], 'expires_at': r['expires_at'], 'ip': r['ip'],
                     'user_agent': (r['user_agent'] or '')[:_USER_AGENT_SHOW], 'last_seen': r['last_seen']}
                    for r in conn.execute('SELECT * FROM sessions WHERE user_id = ? ORDER BY id DESC', (user_id,))]
        push = [{'id': r['id'], 'lang': r['lang'], 'created_at': r['created_at'], 'host': _host(r['endpoint'])}
                for r in conn.execute('SELECT id, lang, created_at, endpoint FROM push_subscriptions '
                                      'WHERE user_id = ? ORDER BY id', (user_id,))]
        pending_in = [dict(r) for r in conn.execute(
            'SELECT u.id, u.display_name, f.created_at FROM friendships f JOIN users u ON u.id = f.user_id '
            "WHERE f.friend_id = ? AND f.status = 'pending'", (user_id,))]
        pending_out = [dict(r) for r in conn.execute(
            'SELECT u.id, u.display_name, f.created_at FROM friendships f JOIN users u ON u.id = f.friend_id '
            "WHERE f.user_id = ? AND f.status = 'pending'", (user_id,))]
        friends = [dict(r) for r in conn.execute(
            'SELECT u.id, u.display_name FROM friendships f JOIN users u '
            'ON (u.id = f.user_id OR u.id = f.friend_id) AND u.id != ? '
            "WHERE (f.user_id = ? OR f.friend_id = ?) AND f.status = 'accepted' ORDER BY u.display_name",
            (user_id, user_id, user_id))]
        badges = [dict(r) for r in conn.execute(
            'SELECT badge, game_id, earned_at FROM achievements WHERE user_id = ? ORDER BY earned_at, rowid',
            (user_id,))]
        daily = [dict(r) for r in conn.execute(
            'SELECT puzzle_date, best_score, attempts, revealed FROM daily_scores WHERE user_id = ? '
            'ORDER BY puzzle_date DESC LIMIT 60', (user_id,))]
        review = conn.execute(
            'SELECT COUNT(*) AS total, COALESCE(SUM(1 - verdict), 0) AS invalid FROM word_reviews WHERE user_id = ?',
            (user_id,)).fetchone()
        reviews = {
            'total': review['total'], 'invalid': review['invalid'],
            'invalid_share': round(review['invalid'] / review['total'] * 100, 1) if review['total'] else 0,
            'recent': [{'word': r['word'], 'valid': bool(r['verdict']), 'at': r['created_at']} for r in conn.execute(
                'SELECT word, verdict, created_at FROM word_reviews WHERE user_id = ? '
                'ORDER BY created_at DESC, rowid DESC LIMIT 50', (user_id,))],
        }
        games = [dict(r) for r in conn.execute(
            'SELECT sg.id AS id, sg.room_name AS room_name, sg.status AS status, sg.is_async AS is_async, '
            'sg.has_bots AS has_bots, sg.created_at AS created_at, sg.updated_at AS updated_at, '
            'gp.final_score AS score, gp.is_winner AS is_winner, gp.player_name AS player_name '
            'FROM game_players gp JOIN saved_games sg ON sg.id = gp.game_id WHERE gp.user_id = ? '
            'ORDER BY sg.id DESC LIMIT 200', (user_id,))]
        notes = [dict(r) for r in conn.execute(
            'SELECT n.id, n.note, n.created_at, n.admin_user_id, u.display_name AS admin_name '
            'FROM user_admin_notes n LEFT JOIN users u ON u.id = n.admin_user_id '
            'WHERE n.user_id = ? ORDER BY n.id DESC', (user_id,))]
        logins = [dict(r) for r in conn.execute(
            'SELECT id, ip, user_agent, success, reason, created_at FROM login_events WHERE user_id = ? '
            'ORDER BY id DESC LIMIT 20', (user_id,))]
        rating_history = _rating_history(conn, user_id)
        history = admin.query_audit(target_type='user', target_id=str(user_id), limit=30)['items']
        admin.record(conn, ctx, 'view.user', 'user', user_id)
    for entry in logins:
        entry['user_agent'] = (entry['user_agent'] or '')[:_USER_AGENT_SHOW]
    return {
        'user': account, 'sessions': sessions, 'push': push, 'friends': friends, 'pending_in': pending_in,
        'pending_out': pending_out, 'badges': badges, 'daily': daily, 'reviews': reviews, 'games': games,
        'notes': notes, 'logins': logins, 'rating_history': rating_history, 'admin_history': history,
        'available_badges': list(achievements.BADGES),
    }


def profile_view(ctx, user_id):
    """A profil oldal tartalma, ahogy a felhasználó látja (csak olvasható; naplózva)."""
    with auth.transaction() as conn:
        row = get_row(conn, user_id)
        admin.record(conn, ctx, 'view.user_profile', 'user', user_id)
    history = auth.get_user_game_history(user_id)
    played, won = row['games_played'], row['games_won']
    return {
        'display_name': row['display_name'], 'badges': auth.get_user_achievements(user_id),
        'stats': {'games_played': played, 'games_won': won,
                  'win_rate': round(won / played * 100, 1) if played else 0,
                  'avg_score': round(row['total_score'] / played, 1) if played else 0,
                  'total_score': row['total_score'], 'rating': row['rating'], 'rated_games': row['rated_games']},
        'history': [{'game_id': h['game_id'], 'room_name': h['room_name'], 'created_at': h['created_at'],
                     'final_score': h['final_score'], 'is_winner': bool(h['is_winner']),
                     'rating_change': (h['rating_after'] - h['rating_before'])
                     if h['rating_after'] is not None and h['rating_before'] is not None else None,
                     'opponents': h['opponents']} for h in history],
    }


# ===== Átnevezés =====

def _patch_state_names(state_json, old, new):
    """A mentett állapotban a játékos nevének cseréje; None, ha nem volt mit cserélni."""
    try:
        data = json.loads(state_json)
    except (TypeError, ValueError):
        return None
    changed = False
    for player in data.get('players', []):
        if player.get('name') == old:
            player['name'] = new
            changed = True
    names = data.get('winner_names')
    if isinstance(names, list) and old in names:
        data['winner_names'] = [new if n == old else n for n in names]
        changed = True
    return json.dumps(data, ensure_ascii=False) if changed else None


def rename_in_games(conn, user_id, old, new, only_active=True):
    """A felhasználó nevének átírása a játékaiban (`game_players`, lépésnapló, tulajdonos, mentett állapot).
    `only_active`: csak a folyamatban lévő játékokban (átnevezés); különben mindegyikben (anonimizálás)."""
    status_clause = "AND sg.status = 'active'" if only_active else ''
    games = [r['game_id'] for r in conn.execute(
        'SELECT DISTINCT gp.game_id AS game_id FROM game_players gp JOIN saved_games sg ON sg.id = gp.game_id '
        f'WHERE gp.user_id = ? {status_clause}', (user_id,))]
    for game_id in games:
        conn.execute('UPDATE game_players SET player_name = ? WHERE game_id = ? AND user_id = ?',
                     (new, game_id, user_id))
        conn.execute('UPDATE game_moves SET player_name = ? WHERE game_id = ? AND player_name = ?',
                     (new, game_id, old))
        conn.execute('UPDATE saved_games SET owner_name = ? WHERE id = ? AND owner_name = ?', (new, game_id, old))
        state_json = conn.execute('SELECT state_json FROM saved_games WHERE id = ?', (game_id,)).fetchone()[0]
        patched = _patch_state_names(state_json, old, new)
        if patched is not None:
            conn.execute('UPDATE saved_games SET state_json = ? WHERE id = ?', (patched, game_id))
    return games


def rename(ctx, user_id, new_name, reason):
    """Megjelenítési név módosítása a regisztrációval azonos szabályokkal. Visszatér: (régi, új, érintett játékok)."""
    name = sanitize_name(new_name)
    if name is None:
        raise AdminError('Érvénytelen megjelenítési név (1-20 karakter, betűk és számok).', 400, field='display_name')
    with admin.action(ctx, 'user.rename', 'user', user_id, reason=reason) as act:
        row = get_row(act.conn, user_id)
        _require_manageable(row)
        if row['display_name'] == name:
            raise AdminError('A név nem változott.', 409, field='display_name')
        games = rename_in_games(act.conn, user_id, row['display_name'], name, only_active=True)
        act.conn.execute('UPDATE users SET display_name = ? WHERE id = ?', (name, user_id))
        act.details['before'] = {'display_name': row['display_name']}
        act.details['after'] = {'display_name': name}
        act.details['games'] = games
    return row['display_name'], name, games


# ===== E-mail, jelszó =====

def change_email(ctx, user_id, new_email, reason):
    """E-mail cím módosítása (egyediség-ellenőrzéssel). Visszatér: a régi cím (az értesítéshez)."""
    email = new_email.strip() if isinstance(new_email, str) else ''
    if not EMAIL_RE.match(email) or len(email) > 254:
        raise AdminError('Érvénytelen email cím.', 400, field='email')
    lowered = email.lower()
    if lowered in config.ADMIN_EMAILS:
        raise AdminError('Ez a cím védett, más fiók nem veheti fel.', 403, field='email')
    with admin.action(ctx, 'user.email', 'user', user_id, reason=reason) as act:
        row = get_row(act.conn, user_id)
        _require_manageable(row)
        if lowered == row['email_lower']:
            raise AdminError('Az e-mail cím nem változott.', 409, field='email')
        if act.conn.execute('SELECT 1 FROM users WHERE email_lower = ?', (lowered,)).fetchone():
            raise AdminError('Ez az email cím már regisztrálva van.', 409, field='email')
        act.conn.execute('UPDATE users SET email = ?, email_lower = ? WHERE id = ?', (email, lowered, user_id))
        act.details['before'] = {'email': row['email']}
        act.details['after'] = {'email': email}
    return row['email']


def temp_password():
    alphabet = string.ascii_letters + string.digits
    return ''.join(secrets.choice(alphabet) for _ in range(TEMP_PASSWORD_LENGTH))


def reset_password(ctx, user_id, reason):
    """Ideiglenes jelszó beállítása, minden munkamenet érvénytelenítése. Visszatér: (jelszó, e-mail cím, sessionök).
    A jelszó a naplóba nem kerül."""
    password = temp_password()
    with admin.action(ctx, 'user.reset_password', 'user', user_id, reason=reason) as act:
        row = get_row(act.conn, user_id)
        _require_manageable(row)
        act.conn.execute('UPDATE users SET password_hash = ? WHERE id = ?',
                         (generate_password_hash(password, method='pbkdf2:sha256:260000'), user_id))
        killed = act.conn.execute('DELETE FROM sessions WHERE user_id = ?', (user_id,)).rowcount
        act.details['sessions_closed'] = killed
    return password, row['email'], killed


def logout_all(ctx, user_id, reason):
    """Kijelentkeztetés mindenhonnan: a munkamenetek törlése. A socketek bontása a hívó dolga (a tranzakció után)."""
    with admin.action(ctx, 'user.logout_all', 'user', user_id, reason=reason) as act:
        row = get_row(act.conn, user_id)
        _require_manageable(row)
        killed = act.conn.execute('DELETE FROM sessions WHERE user_id = ?', (user_id,)).rowcount
        act.details['sessions_closed'] = killed
    return killed


# ===== Kitiltás, némítás, tiltások =====

def ban(ctx, user_id, until, reason):
    """Kitiltás időtartammal (1h / 1d / 7d / 30d / pontos időpont / None = végleges). Visszatér: a lejárat."""
    stamp = admin.parse_until(until)
    with admin.action(ctx, 'user.ban', 'user', user_id, reason=reason) as act:
        row = get_row(act.conn, user_id)
        _require_manageable(row)
        act.conn.execute('UPDATE users SET banned_until = ?, ban_reason = ? WHERE id = ?',
                         (stamp, admin.normalize_reason(reason), user_id))
        killed = act.conn.execute('DELETE FROM sessions WHERE user_id = ?', (user_id,)).rowcount
        act.details['before'] = {'banned_until': row['banned_until'], 'ban_reason': row['ban_reason']}
        act.details['after'] = {'banned_until': stamp, 'permanent': stamp == auth.PERMANENT_UNTIL}
        act.details['sessions_closed'] = killed
    return stamp


def unban(ctx, user_id, reason):
    with admin.action(ctx, 'user.unban', 'user', user_id, reason=reason) as act:
        row = get_row(act.conn, user_id)
        if not row['banned_until']:
            raise AdminError('A felhasználó nincs kitiltva.', 409)
        act.conn.execute('UPDATE users SET banned_until = NULL, ban_reason = NULL WHERE id = ?', (user_id,))
        act.details['before'] = {'banned_until': row['banned_until'], 'ban_reason': row['ban_reason']}
        act.details['after'] = {'banned_until': None}


def mute(ctx, user_id, until, reason):
    """Chat némítás időtartammal. Visszatér: a lejárat."""
    stamp = admin.parse_until(until)
    with admin.action(ctx, 'user.mute', 'user', user_id, reason=reason) as act:
        row = get_row(act.conn, user_id)
        _require_manageable(row)
        act.conn.execute('UPDATE users SET chat_muted_until = ? WHERE id = ?', (stamp, user_id))
        act.details['before'] = {'chat_muted_until': row['chat_muted_until']}
        act.details['after'] = {'chat_muted_until': stamp}
    return stamp


def unmute(ctx, user_id, reason):
    with admin.action(ctx, 'user.unmute', 'user', user_id, reason=reason) as act:
        row = get_row(act.conn, user_id)
        if not row['chat_muted_until']:
            raise AdminError('A felhasználó nincs némítva.', 409)
        act.conn.execute('UPDATE users SET chat_muted_until = NULL WHERE id = ?', (user_id,))
        act.details['before'] = {'chat_muted_until': row['chat_muted_until']}
        act.details['after'] = {'chat_muted_until': None}


def set_review_blocked(ctx, user_id, blocked, reason):
    """Szótár-építő tiltása / engedélyezése. A letiltott szavazatai nem számítanak (a kizárásokat újra kell számolni)."""
    blocked = admin.bool_field(blocked, 'blocked')
    with admin.action(ctx, 'user.review_block' if blocked else 'user.review_unblock', 'user', user_id,
                      reason=reason) as act:
        row = get_row(act.conn, user_id)
        _require_manageable(row)
        act.conn.execute('UPDATE users SET review_blocked = ? WHERE id = ?', (1 if blocked else 0, user_id))
        act.details['before'] = {'review_blocked': bool(row['review_blocked'])}
        act.details['after'] = {'review_blocked': blocked}


def revert_reviews(ctx, user_id, since, reason):
    """A felhasználó szótár-építős szavazatainak törlése (mind, vagy egy időponttól). Visszatér: a törölt szavak."""
    stamp = admin._parse_bound(since)[0] if since else None
    with admin.action(ctx, 'user.reviews_revert', 'user', user_id, reason=reason) as act:
        row = get_row(act.conn, user_id)
        _require_manageable(row)
        sql, params = 'FROM word_reviews WHERE user_id = ?', [user_id]
        if stamp:
            sql += ' AND created_at >= ?'
            params.append(stamp)
        words = [r['word'] for r in act.conn.execute('SELECT word ' + sql, params)]
        act.conn.execute('DELETE ' + sql, params)
        act.details['count'] = len(words)
        act.details['since'] = stamp
        act.details['words'] = words[:200]
    return words


# ===== Értékszám, statisztika, kitüntetés =====

def set_rating(ctx, user_id, rating, reason):
    """Kézi értékszám-módosítás: külön sor az értékszám-előzményben (`rating_adjustments`)."""
    rating = admin.int_field(rating, 'rating', MIN_RATING, MAX_RATING)
    with admin.action(ctx, 'user.rating', 'user', user_id, reason=reason) as act:
        row = get_row(act.conn, user_id)
        _require_manageable(row)
        if row['rating'] == rating:
            raise AdminError('Az értékszám nem változott.', 409, field='rating')
        act.conn.execute('UPDATE users SET rating = ? WHERE id = ?', (rating, user_id))
        act.conn.execute(
            'INSERT INTO rating_adjustments (user_id, admin_user_id, rating_before, rating_after, reason) '
            'VALUES (?, ?, ?, ?, ?)', (user_id, ctx.admin_user_id, row['rating'], rating, admin.normalize_reason(reason)))
        act.details['before'] = {'rating': row['rating']}
        act.details['after'] = {'rating': rating}


def compute_counters(conn, user_id):
    """A számlálók a `game_players`-ből (befejezett játékok): (játszott, nyert, összpont)."""
    row = conn.execute(
        'SELECT COUNT(*) AS played, COALESCE(SUM(gp.is_winner), 0) AS won, COALESCE(SUM(gp.final_score), 0) AS score '
        "FROM game_players gp JOIN saved_games sg ON sg.id = gp.game_id AND sg.status = 'finished' "
        'WHERE gp.user_id = ?', (user_id,)).fetchone()
    return row['played'], row['won'], row['score']


def recompute_user_counters(conn, user_id):
    """Egy felhasználó számlálóinak újraszámolása. Visszatér: ((előtte), (utána))."""
    row = conn.execute('SELECT games_played, games_won, total_score FROM users WHERE id = ?', (user_id,)).fetchone()
    played, won, score = compute_counters(conn, user_id)
    conn.execute('UPDATE users SET games_played = ?, games_won = ?, total_score = ? WHERE id = ?',
                 (played, won, score, user_id))
    return ((row['games_played'], row['games_won'], row['total_score']), (played, won, score))


def recompute_stats(ctx, user_id, reason):
    with admin.action(ctx, 'user.recompute_stats', 'user', user_id, reason=reason) as act:
        row = get_row(act.conn, user_id)
        _require_manageable(row)
        before, after = recompute_user_counters(act.conn, user_id)
        act.details['before'] = dict(zip(('games_played', 'games_won', 'total_score'), before))
        act.details['after'] = dict(zip(('games_played', 'games_won', 'total_score'), after))


def set_badge(ctx, user_id, badge, grant, reason):
    """Kitüntetés adása / elvétele a `BADGES` listából."""
    grant = admin.bool_field(grant, 'grant')
    if badge not in achievements.BADGES:
        raise AdminError('Ismeretlen kitüntetés.', 400, field='badge')
    with admin.action(ctx, 'user.badge_grant' if grant else 'user.badge_revoke', 'user', user_id,
                      reason=reason) as act:
        row = get_row(act.conn, user_id)
        _require_manageable(row)
        have = act.conn.execute('SELECT 1 FROM achievements WHERE user_id = ? AND badge = ?',
                                (user_id, badge)).fetchone()
        if grant and have:
            raise AdminError('A felhasználónak már megvan ez a kitüntetés.', 409, field='badge')
        if not grant and not have:
            raise AdminError('A felhasználónak nincs ilyen kitüntetése.', 409, field='badge')
        if grant:
            act.conn.execute('INSERT INTO achievements (user_id, badge) VALUES (?, ?)', (user_id, badge))
        else:
            act.conn.execute('DELETE FROM achievements WHERE user_id = ? AND badge = ?', (user_id, badge))
        act.details['badge'] = badge


# ===== Push, jegyzet =====

def delete_push_device(ctx, user_id, device_id, reason):
    with admin.action(ctx, 'user.push_delete', 'user', user_id, reason=reason) as act:
        get_row(act.conn, user_id)
        removed = act.conn.execute('DELETE FROM push_subscriptions WHERE id = ? AND user_id = ?',
                                   (device_id, user_id)).rowcount
        if not removed:
            raise AdminError('Az eszköz nem található.', 404)
        act.details['device_id'] = device_id


def add_note(ctx, user_id, note):
    """Belső jegyzet a felhasználóhoz (a jegyzet maga az indoklás: külön indoklás nem kell)."""
    text = admin.clean_text(note, 'note', MAX_NOTE_LEN)
    with auth.transaction() as conn:
        get_row(conn, user_id)
        cursor = conn.execute('INSERT INTO user_admin_notes (user_id, admin_user_id, note) VALUES (?, ?, ?)',
                              (user_id, ctx.admin_user_id, text))
        admin.record(conn, ctx, 'user.note_add', 'user', user_id, {'note_id': cursor.lastrowid, 'note': text})
        return cursor.lastrowid


def delete_note(ctx, user_id, note_id):
    with auth.transaction() as conn:
        row = conn.execute('SELECT note FROM user_admin_notes WHERE id = ? AND user_id = ?',
                           (note_id, user_id)).fetchone()
        if row is None:
            raise AdminError('A jegyzet nem található.', 404)
        conn.execute('DELETE FROM user_admin_notes WHERE id = ?', (note_id,))
        admin.record(conn, ctx, 'user.note_delete', 'user', user_id, {'note_id': note_id, 'note': row['note']})


# ===== Export (GDPR) =====

def export_user(ctx, user_id):
    """A felhasználó összes adata JSON-ban: fiók, játékok, lépések, szavazatok, kitüntetések, barátok, napi eredmények."""
    with auth.transaction() as conn:
        row = get_row(conn, user_id)
        account = {key: row[key] for key in ('id', 'email', 'display_name', 'created_at', 'games_played',
                                             'games_won', 'total_score', 'rating', 'rated_games', 'last_login_at',
                                             'last_login_ip', 'banned_until', 'ban_reason', 'chat_muted_until',
                                             'review_blocked', 'deleted_at')}
        games = []
        for g in conn.execute(
                'SELECT sg.id, sg.room_name, sg.status, sg.created_at, sg.updated_at, sg.is_async, sg.has_bots, '
                'gp.player_name, gp.final_score, gp.is_winner, gp.rating_before, gp.rating_after '
                'FROM game_players gp JOIN saved_games sg ON sg.id = gp.game_id WHERE gp.user_id = ? ORDER BY sg.id',
                (user_id,)):
            entry = dict(g)
            entry['moves'] = [dict(m) for m in conn.execute(
                'SELECT move_number, player_name, action_type, details_json, created_at FROM game_moves '
                'WHERE game_id = ? AND player_name = ? ORDER BY move_number', (g['id'], g['player_name']))]
            games.append(entry)
        data = {
            'account': account, 'games': games,
            'word_reviews': [dict(r) for r in conn.execute(
                'SELECT word, verdict, created_at FROM word_reviews WHERE user_id = ? ORDER BY created_at',
                (user_id,))],
            'achievements': [dict(r) for r in conn.execute(
                'SELECT badge, game_id, earned_at FROM achievements WHERE user_id = ?', (user_id,))],
            'friends': [dict(r) for r in conn.execute(
                'SELECT u.id, u.display_name, f.status, f.created_at FROM friendships f JOIN users u '
                'ON (u.id = f.user_id OR u.id = f.friend_id) AND u.id != ? '
                'WHERE f.user_id = ? OR f.friend_id = ?', (user_id, user_id, user_id))],
            'daily_scores': [dict(r) for r in conn.execute(
                'SELECT puzzle_date, best_score, attempts, revealed FROM daily_scores WHERE user_id = ?',
                (user_id,))],
            'rating_adjustments': [dict(r) for r in conn.execute(
                'SELECT rating_before, rating_after, reason, created_at FROM rating_adjustments WHERE user_id = ?',
                (user_id,))],
            'login_events': [dict(r) for r in conn.execute(
                'SELECT ip, user_agent, success, reason, created_at FROM login_events WHERE user_id = ? '
                'ORDER BY id DESC LIMIT 500', (user_id,))],
            'push_devices': [{'lang': r['lang'], 'host': _host(r['endpoint']), 'created_at': r['created_at']}
                             for r in conn.execute('SELECT * FROM push_subscriptions WHERE user_id = ?', (user_id,))],
        }
        admin.record(conn, ctx, 'view.user_export', 'user', user_id)
    return data


# ===== Törlés / anonimizálás =====

def delete_user(ctx, user_id, mode, confirm_name, reason):
    """Fiók törlése. `anonymize` (alapértelmezett): név → „Törölt felhasználó #id”, e-mail és jelszó törölve, a
    játékok és az értékszám-előzmények megmaradnak. `delete`: a fiók sora is törlődik. A célpont nevének
    begépelése kötelező. A név a játékokban is átíródik (névtelenítés). Visszatér: az érintett (régi) név."""
    if mode not in ('anonymize', 'delete'):
        raise AdminError('Érvénytelen kérés.', 400, field='mode')
    with admin.action(ctx, 'user.delete' if mode == 'delete' else 'user.anonymize', 'user', user_id,
                      reason=reason) as act:
        row = get_row(act.conn, user_id)
        _require_manageable(row)
        if not isinstance(confirm_name, str) or confirm_name.strip() != row['display_name']:
            raise AdminError('A megerősítő név nem egyezik.', 400, field='confirm_name')
        placeholder = DELETED_NAME.format(id=user_id)
        rename_in_games(act.conn, user_id, row['display_name'], placeholder, only_active=False)
        conn = act.conn
        conn.execute('DELETE FROM sessions WHERE user_id = ?', (user_id,))
        conn.execute('DELETE FROM push_subscriptions WHERE user_id = ?', (user_id,))
        conn.execute('DELETE FROM friendships WHERE user_id = ? OR friend_id = ?', (user_id, user_id))
        conn.execute('DELETE FROM word_reviews WHERE user_id = ?', (user_id,))
        conn.execute('DELETE FROM user_admin_notes WHERE user_id = ?', (user_id,))
        conn.execute('DELETE FROM chat_log WHERE user_id = ?', (user_id,))
        conn.execute('DELETE FROM login_events WHERE user_id = ?', (user_id,))
        if mode == 'delete':
            conn.execute('DELETE FROM users WHERE id = ?', (user_id,))
        else:
            conn.execute(
                'UPDATE users SET display_name = ?, email = ?, email_lower = ?, password_hash = ?, '
                'reconnect_token = NULL, banned_until = NULL, ban_reason = NULL, chat_muted_until = NULL, '
                'last_login_ip = NULL, deleted_at = ? WHERE id = ?',
                (placeholder, DELETED_EMAIL.format(id=user_id), DELETED_EMAIL.format(id=user_id), '',
                 auth.format_ts(auth.utcnow()), user_id))
        act.details['mode'] = mode
        act.details['before'] = {'display_name': row['display_name'], 'email': row['email']}
        act.details['after'] = {'display_name': placeholder if mode == 'anonymize' else None}
    return row['display_name']
