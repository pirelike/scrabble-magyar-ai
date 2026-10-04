"""Admin panel: moderáció — tiltott szavak, chat napló, bejelentések, nevek átnézése.

A játékszerver (`server.py`) is használja: a tiltott szavak szűrése (`contains_banned`, `filter_chat`), a chat
tartós naplózása (`log_chat`) és a játékosok bejelentései (`create_report`) ide futnak be.
"""
import json

import admin
import auth
import config
import settings
from admin import AdminError

MAX_REPORT_REASON = 300
MAX_REPORT_MESSAGE = 200
SNAPSHOT_CHAT_LINES = 15
REPORT_KINDS = ('player', 'chat')
REPORT_STATUSES = ('new', 'handled', 'rejected')
MIN_BANNED_WORD, MAX_BANNED_WORD = 2, 40
CHAT_LOG_LIMIT_MAX = 500

# ===== Tiltott szavak =====

_banned_cache = {'path': None, 'words': ()}


def invalidate_banned_words():
    _banned_cache['path'] = None


def banned_words():
    """A tiltott szavak (kis-nagybetű és ékezet nélküli alakban), gyorsítótárazva."""
    if _banned_cache['path'] != auth.DB_PATH:
        try:
            with auth.transaction() as conn:
                words = tuple(r['word'] for r in conn.execute('SELECT word FROM banned_words ORDER BY word'))
        except Exception:     # séma nélküli adatbázis
            words = ()
        _banned_cache.update(path=auth.DB_PATH, words=words)
    return _banned_cache['words']


def _fold_with_map(text):
    """A szöveg ékezet- és kisbetű-független alakja + minden karakterének eredeti helye."""
    chars, index = [], []
    for i, ch in enumerate(text):
        for folded in admin.fold(ch):
            chars.append(folded)
            index.append(i)
    return ''.join(chars), index


def find_banned(text):
    """A szövegben talált tiltott szavak helyei: [(kezdet, vég)] az eredeti szövegben (átfedés nélkül)."""
    words = banned_words()
    if not words or not isinstance(text, str):
        return []
    folded, index = _fold_with_map(text)
    spans = []
    for word in words:
        start = folded.find(word)
        while start != -1:
            spans.append((index[start], index[start + len(word) - 1] + 1))
            start = folded.find(word, start + 1)
    spans.sort()
    merged = []
    for start, end in spans:
        if merged and start <= merged[-1][1]:
            merged[-1] = (merged[-1][0], max(end, merged[-1][1]))
        else:
            merged.append((start, end))
    return merged


def contains_banned(text):
    return bool(find_banned(text))


def mask_banned(text):
    """A tiltott szavak kicsillagozva. Visszatér: (szöveg, talált-e)."""
    spans = find_banned(text)
    if not spans:
        return text, False
    chars = list(text)
    for start, end in spans:
        for i in range(start, end):
            chars[i] = '*'
    return ''.join(chars), True


def filter_chat(message):
    """A chat üzenet szűrése a beállított kezelés szerint. Visszatér: (üzenet vagy None = eldobva, jelölt-e)."""
    masked, flagged = mask_banned(message)
    if not flagged:
        return message, False
    if settings.get('banned_word_action', 'mask') == 'drop':
        return None, True
    return masked, True


def list_banned_words():
    with auth.transaction() as conn:
        rows = conn.execute('SELECT w.word, w.created_at, u.display_name AS admin_name FROM banned_words w '
                            'LEFT JOIN users u ON u.id = w.created_by ORDER BY w.word').fetchall()
    return [dict(r) for r in rows]


def add_banned_word(ctx, word, reason):
    text = admin.fold(admin.clean_text(word, 'word', MAX_BANNED_WORD))
    if len(text) < MIN_BANNED_WORD:
        raise AdminError('A tiltott szó legalább 2 karakter legyen.', 400, field='word')
    with admin.action(ctx, 'mod.word_add', 'word', text, reason=reason) as act:
        if act.conn.execute('SELECT 1 FROM banned_words WHERE word = ?', (text,)).fetchone():
            raise AdminError('Ez a szó már szerepel a listán.', 409, field='word')
        act.conn.execute('INSERT INTO banned_words (word, created_by) VALUES (?, ?)', (text, ctx.admin_user_id))
    invalidate_banned_words()
    return text


def remove_banned_word(ctx, word, reason):
    text = admin.fold(admin.clean_text(word, 'word', MAX_BANNED_WORD))
    with admin.action(ctx, 'mod.word_remove', 'word', text, reason=reason) as act:
        if not act.conn.execute('DELETE FROM banned_words WHERE word = ?', (text,)).rowcount:
            raise AdminError('A szó nem szerepel a listán.', 404)
    invalidate_banned_words()


# ===== Chat napló =====

def log_chat(room, user_id, name, message):
    """A chat üzenet tartós naplózása (csak ha a beállítás be van kapcsolva; a hiba nem akadályozza a chatet)."""
    if not settings.get('chat_log_enabled', False):
        return
    try:
        with auth.transaction() as conn:
            conn.execute('INSERT INTO chat_log (room_id, room_name, user_id, name, message) VALUES (?, ?, ?, ?, ?)',
                         (room.id, room.name, user_id, name, message))
    except Exception as exc:
        print(f'[chat] A chat napló nem írható: {exc}')


def query_chat_log(ctx, args):
    """A tartósan naplózott chat üzenetek (szűrés: q, room, user, since, until). Naplózva: `view.chat`."""
    where, params = [], []
    q = (args.get('q') or '').strip()
    if q:
        where.append("c.message LIKE ? ESCAPE '\\'")
        params.append(admin._like(q))
    if args.get('room'):
        where.append("(c.room_id = ? OR c.room_name LIKE ? ESCAPE '\\')")
        params.extend([args['room'], admin._like(args['room'])])
    user = (args.get('user') or '').strip()
    if user:
        if user.isdigit():
            where.append('c.user_id = ?')
            params.append(int(user))
        else:
            where.append("c.name LIKE ? ESCAPE '\\'")
            params.append(admin._like(user))
    if args.get('since'):
        where.append('c.created_at >= ?')
        params.append(admin._parse_bound(args['since'])[0])
    if args.get('until'):
        stamp, exclusive = admin._parse_bound(args['until'], end=True)
        where.append('c.created_at < ?' if exclusive else 'c.created_at <= ?')
        params.append(stamp)
    limit, offset = admin.page_args(args, default=100, maximum=CHAT_LOG_LIMIT_MAX)
    clause = ('WHERE ' + ' AND '.join(where)) if where else ''
    with auth.transaction() as conn:
        total = conn.execute(f'SELECT COUNT(*) FROM chat_log c {clause}', params).fetchone()[0]
        rows = conn.execute(f'SELECT c.* FROM chat_log c {clause} ORDER BY c.id DESC LIMIT ? OFFSET ?',
                            params + [limit, offset]).fetchall()
        admin.record(conn, ctx, 'view.chat', 'chat', None,
                     {'filters': {k: v for k, v in args.items() if v and k in ('q', 'room', 'user', 'since', 'until')}})
    items = [dict(r) for r in rows]
    for item in items:
        item['flagged'] = contains_banned(item['message'])
    return {'items': items, 'total': total, 'limit': limit, 'offset': offset}


# ===== Bejelentések =====

def create_report(reporter_id, reporter_name, reported_user_id, reported_name, kind, message, reason, room):
    """Egy bejelentés rögzítése a bejelentett tartalom pillanatképével. Visszatér: az azonosító, vagy None, ha
    a bejelentés érvénytelen."""
    if kind not in REPORT_KINDS:
        return None
    reason = reason.strip() if isinstance(reason, str) else ''
    if len(reason) > MAX_REPORT_REASON:
        return None
    text = message.strip() if isinstance(message, str) else ''
    if kind == 'chat' and (not text or len(text) > MAX_REPORT_MESSAGE):
        return None
    if kind == 'player':
        text = ''
    snapshot = {
        'players': [{'name': p.name, 'is_bot': p.is_bot, 'score': p.score} for p in room.game.players],
        'chat': room.chat_messages[-SNAPSHOT_CHAT_LINES:],
        'turn': room.game.turn_number,
    }
    with auth.transaction() as conn:
        cursor = conn.execute(
            'INSERT INTO reports (reporter_user_id, reporter_name, reported_user_id, reported_name, kind, message, '
            'reason, room_id, room_name, snapshot_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)',
            (reporter_id, reporter_name, reported_user_id, reported_name, kind, text, reason, room.id, room.name,
             json.dumps(snapshot, ensure_ascii=False)))
        return cursor.lastrowid


def notify_admins_of_report(room_name):
    """Push értesítés az adminoknak egy új bejelentésről (háttérben; push híján nem történik semmi)."""
    import push_service
    for email in sorted(config.ADMIN_EMAILS):
        user = auth.get_user_by_email(email)
        if user:
            push_service.notify_background(user['id'], 'report', room=room_name)


def _report_item(row, detail=False):
    item = {
        'id': row['id'], 'status': row['status'], 'kind': row['kind'], 'created_at': row['created_at'],
        'reporter_user_id': row['reporter_user_id'], 'reporter_name': row['reporter_name'],
        'reported_user_id': row['reported_user_id'], 'reported_name': row['reported_name'],
        'message': row['message'], 'reason': row['reason'], 'room_id': row['room_id'], 'room_name': row['room_name'],
        'handled_by': row['handled_by'], 'handled_at': row['handled_at'], 'handler_note': row['handler_note'],
    }
    if detail:
        try:
            item['snapshot'] = json.loads(row['snapshot_json'] or 'null')
        except ValueError:
            item['snapshot'] = None
    return item


def list_reports(args):
    where, params = [], []
    status = args.get('status')
    if status:
        if status not in REPORT_STATUSES:
            raise AdminError('Érvénytelen szűrő.', 400)
        where.append('status = ?')
        params.append(status)
    limit, offset = admin.page_args(args)
    clause = ('WHERE ' + ' AND '.join(where)) if where else ''
    with auth.transaction() as conn:
        total = conn.execute(f'SELECT COUNT(*) FROM reports {clause}', params).fetchone()[0]
        new_count = conn.execute("SELECT COUNT(*) FROM reports WHERE status = 'new'").fetchone()[0]
        rows = conn.execute(f'SELECT * FROM reports {clause} ORDER BY id DESC LIMIT ? OFFSET ?',
                            params + [limit, offset]).fetchall()
    return {'items': [_report_item(r) for r in rows], 'total': total, 'new': new_count, 'limit': limit,
            'offset': offset}


def get_report(ctx, report_id):
    """Egy bejelentés a pillanatképpel (a megtekintés naplózva: `view.report`)."""
    with auth.transaction() as conn:
        row = conn.execute('SELECT * FROM reports WHERE id = ?', (report_id,)).fetchone()
        if row is None:
            raise AdminError('A bejelentés nem található.', 404)
        admin.record(conn, ctx, 'view.report', 'report', report_id)
    return _report_item(row, detail=True)


def update_report(ctx, report_id, status, note):
    """A bejelentés kezelése: új / kezelt / elutasított állapot, indoklással."""
    if status not in REPORT_STATUSES:
        raise AdminError('Érvénytelen állapot.', 400, field='status')
    with admin.action(ctx, 'mod.report_update', 'report', report_id, reason=note) as act:
        row = act.conn.execute('SELECT status FROM reports WHERE id = ?', (report_id,)).fetchone()
        if row is None:
            raise AdminError('A bejelentés nem található.', 404)
        act.conn.execute(
            'UPDATE reports SET status = ?, handled_by = ?, handled_at = ?, handler_note = ? WHERE id = ?',
            (status, ctx.admin_user_id if status != 'new' else None,
             auth.format_ts(auth.utcnow()) if status != 'new' else None, admin.normalize_reason(note), report_id))
        act.details['before'] = {'status': row['status']}
        act.details['after'] = {'status': status}


# ===== Nevek átnézése =====

def recent_names(args):
    """Legutóbb regisztrált és átnevezett nevek (tiltott szóra jelölve): az átnevezés a felhasználó oldaláról
    egy kattintással elvégezhető."""
    limit = admin._int_arg(args.get('limit'), 50, 1, 200)
    items = []
    with auth.transaction() as conn:
        for r in conn.execute('SELECT id, display_name, created_at FROM users WHERE deleted_at IS NULL '
                              'ORDER BY id DESC LIMIT ?', (limit,)):
            items.append({'user_id': r['id'], 'display_name': r['display_name'], 'at': r['created_at'],
                          'kind': 'registered', 'flagged': contains_banned(r['display_name'])})
        for r in conn.execute("SELECT a.target_id, a.created_at, a.details_json FROM admin_audit a "
                              "WHERE a.action = 'user.rename' ORDER BY a.id DESC LIMIT ?", (limit,)):
            try:
                after = (json.loads(r['details_json']) or {}).get('after', {}).get('display_name')
            except ValueError:
                after = None
            if after and (r['target_id'] or '').isdigit():
                items.append({'user_id': int(r['target_id']), 'display_name': after, 'at': r['created_at'],
                              'kind': 'renamed', 'flagged': contains_banned(after)})
    items.sort(key=lambda i: i['at'] or '', reverse=True)
    return {'items': items[:limit]}
