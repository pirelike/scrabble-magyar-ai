"""Admin panel: biztonság — belépési napló, IP tiltás, forgalomkorlát állapota, ellenőrző kódok, munkamenetek.

A modul a Flask kéréstől független; a forgalomkorlátozót (`RateLimiter`) a hívó adja át.
"""
import ipaddress
from datetime import timedelta
import time

import admin
import auth
import config
from admin import AdminError

FAILED_IP_THRESHOLD = 5          # ennyi sikertelen belépés / IP / 24 óra → gyanús
FAILED_ACCOUNT_THRESHOLD = 3     # ennyi sikertelen belépés / fiók / 24 óra → gyanús
MANY_ACCOUNTS_THRESHOLD = 3      # ennyi különböző fiók ugyanarról az IP-ről / 24 óra → gyanús
LOGIN_SORT_LIMIT = 200
BAN_REASON_MAX = 300

# ===== IP tiltás =====

_cache = {'path': None, 'loaded': 0.0, 'bans': []}
_CACHE_SECONDS = 30


def invalidate_bans():
    _cache['path'] = None


def parse_network(text):
    """IP-cím vagy CIDR → hálózat (AdminError, ha érvénytelen)."""
    try:
        return ipaddress.ip_network((text or '').strip(), strict=False)
    except ValueError:
        raise AdminError('Érvénytelen IP-cím vagy hálózat.', 400, field='ip') from None


def _load_bans():
    now = time.time()
    if _cache['path'] == auth.DB_PATH and now - _cache['loaded'] < _CACHE_SECONDS:
        return _cache['bans']
    stamp = auth.format_ts(auth.utcnow())
    bans = []
    try:
        with auth.transaction() as conn:
            rows = conn.execute('SELECT id, ip, reason, expires_at FROM ip_bans WHERE expires_at IS NULL '
                                'OR expires_at > ?', (stamp,)).fetchall()
    except Exception:      # séma nélküli adatbázis: nincs tiltás
        rows = []
    for row in rows:
        try:
            bans.append((ipaddress.ip_network(row['ip'], strict=False), row['id'], row['reason']))
        except ValueError:
            continue
    _cache.update(path=auth.DB_PATH, loaded=now, bans=bans)
    return bans


def is_ip_banned(ip):
    """Igaz, ha a kliens IP-je egy érvényes tiltás alá esik (gyorsítótárazva; a változás azonnal érvényesül)."""
    bans = _load_bans()
    if not bans:
        return False
    try:
        address = ipaddress.ip_address((ip or '').strip())
    except ValueError:
        return False
    mapped = getattr(address, 'ipv4_mapped', None)
    candidates = (address, mapped) if mapped else (address,)
    return any(c in net for net, _id, _reason in bans for c in candidates if c.version == net.version)


def list_ip_bans():
    stamp = auth.format_ts(auth.utcnow())
    with auth.transaction() as conn:
        rows = conn.execute(
            'SELECT b.*, u.display_name AS admin_name FROM ip_bans b LEFT JOIN users u ON u.id = b.created_by '
            'ORDER BY b.id DESC').fetchall()
    return [{'id': r['id'], 'ip': r['ip'], 'reason': r['reason'], 'expires_at': r['expires_at'],
             'created_at': r['created_at'], 'created_by': r['created_by'], 'admin_name': r['admin_name'],
             'active': r['expires_at'] is None or r['expires_at'] > stamp} for r in rows]


def add_ip_ban(ctx, ip, until, reason, own_ip=''):
    """IP vagy hálózat tiltása (lejárattal vagy véglegesen). A saját IP-d nem tiltható ki."""
    network = parse_network(ip)
    try:
        own = ipaddress.ip_address((own_ip or '').strip())
    except ValueError:
        own = None
    if own is not None and own.version == network.version and own in network:
        raise AdminError('Saját IP-címedet nem tilthatod ki.', 409, field='ip')
    expires = None if until in (None, 'permanent') else admin.parse_until(until, permanent_ok=False)
    with admin.action(ctx, 'security.ip_ban', 'ip', str(network), reason=reason,
                      details={'until': expires}) as act:
        cursor = act.conn.execute('INSERT INTO ip_bans (ip, reason, expires_at, created_by) VALUES (?, ?, ?, ?)',
                                  (str(network), admin.normalize_reason(reason)[:BAN_REASON_MAX], expires,
                                   ctx.admin_user_id))
        act.details['ban_id'] = cursor.lastrowid
    invalidate_bans()
    return cursor.lastrowid


def remove_ip_ban(ctx, ban_id, reason):
    with admin.action(ctx, 'security.ip_unban', 'ip', None, reason=reason) as act:
        row = act.conn.execute('SELECT ip, expires_at FROM ip_bans WHERE id = ?', (ban_id,)).fetchone()
        if row is None:
            raise AdminError('A tiltás nem található.', 404)
        act.conn.execute('DELETE FROM ip_bans WHERE id = ?', (ban_id,))
        act.details['ip'] = row['ip']
        act.details['ban_id'] = ban_id
    invalidate_bans()


# ===== Belépési napló =====

def _suspicious_sets(conn, since):
    ips = {r['ip'] for r in conn.execute(
        'SELECT ip FROM login_events WHERE success = 0 AND created_at >= ? AND ip IS NOT NULL '
        'GROUP BY ip HAVING COUNT(*) >= ?', (since, FAILED_IP_THRESHOLD))}
    accounts = {r['email'] for r in conn.execute(
        "SELECT email FROM login_events WHERE success = 0 AND created_at >= ? AND email != '' "
        'GROUP BY email HAVING COUNT(*) >= ?', (since, FAILED_ACCOUNT_THRESHOLD))}
    spread = {r['ip'] for r in conn.execute(
        "SELECT ip FROM login_events WHERE created_at >= ? AND ip IS NOT NULL AND email != '' "
        'GROUP BY ip HAVING COUNT(DISTINCT email) >= ?', (since, MANY_ACCOUNTS_THRESHOLD))}
    return ips, accounts, spread


def list_logins(args):
    """Belépési kísérletek szűrve (user / ip / success / since / until / q) és lapozva; a gyanús mintákat
    (sok sikertelen próbálkozás egy IP-ről / fiókra, egy IP sok fiókkal) a sorok jelölik."""
    where, params = [], []
    user = (args.get('user') or '').strip()
    if user:
        if user.isdigit():
            where.append('e.user_id = ?')
            params.append(int(user))
        else:
            where.append("e.email LIKE ? ESCAPE '\\'")
            params.append(admin._like(user.lower()))
    ip = (args.get('ip') or '').strip()
    if ip:
        where.append("e.ip LIKE ? ESCAPE '\\'")
        params.append(admin._like(ip))
    if args.get('success') in ('0', '1'):
        where.append('e.success = ?')
        params.append(int(args['success']))
    if args.get('since'):
        where.append('e.created_at >= ?')
        params.append(admin._parse_bound(args['since'])[0])
    if args.get('until'):
        stamp, exclusive = admin._parse_bound(args['until'], end=True)
        where.append('e.created_at < ?' if exclusive else 'e.created_at <= ?')
        params.append(stamp)
    q = (args.get('q') or '').strip()
    if q:
        where.append("(e.email LIKE ? ESCAPE '\\' OR e.ip LIKE ? ESCAPE '\\' OR e.user_agent LIKE ? ESCAPE '\\' "
                     "OR e.reason LIKE ? ESCAPE '\\')")
        params.extend([admin._like(q)] * 4)
    if args.get('flagged') in ('1', 'true'):
        pass     # a jelölés a sorok kiegészítése; a szűrés lent, Pythonban történik
    limit, offset = admin.page_args(args)
    clause = ('WHERE ' + ' AND '.join(where)) if where else ''
    since = auth.format_ts(auth.utcnow() - timedelta(hours=24))
    with auth.transaction() as conn:
        ips, accounts, spread = _suspicious_sets(conn, since)
        total = conn.execute(f'SELECT COUNT(*) FROM login_events e {clause}', params).fetchone()[0]
        rows = conn.execute(
            f'SELECT e.*, u.display_name AS display_name FROM login_events e LEFT JOIN users u ON u.id = e.user_id '
            f'{clause} ORDER BY e.id DESC LIMIT ? OFFSET ?', params + [limit, offset]).fetchall()
    items = []
    for r in rows:
        flags = []
        if not r['success'] and r['ip'] in ips:
            flags.append('ip_failures')
        if not r['success'] and r['email'] in accounts:
            flags.append('account_failures')
        if r['ip'] in spread:
            flags.append('many_accounts')
        items.append({'id': r['id'], 'user_id': r['user_id'], 'display_name': r['display_name'],
                      'email': r['email'], 'ip': r['ip'], 'user_agent': (r['user_agent'] or '')[:160],
                      'success': bool(r['success']), 'reason': r['reason'], 'created_at': r['created_at'],
                      'flags': flags})
    return {'items': items, 'total': total, 'limit': limit, 'offset': offset,
            'suspicious': {'ips': sorted(ips), 'accounts': sorted(accounts), 'spread_ips': sorted(spread)}}


LOGIN_CSV_FIELDS = ('id', 'created_at', 'success', 'email', 'user_id', 'ip', 'user_agent', 'reason')


# ===== Ellenőrző kódok =====

def list_codes(ctx):
    """A függő regisztrációs kódok (SMTP nélkül itt látszik a kód). A megtekintés naplózva: `view.codes`."""
    stamp = auth.format_ts(auth.utcnow())
    with auth.transaction() as conn:
        rows = conn.execute(
            'SELECT id, email, code, created_at, expires_at, attempts FROM verification_codes '
            'WHERE used = 0 AND expires_at > ? ORDER BY id DESC', (stamp,)).fetchall()
        admin.record(conn, ctx, 'view.codes', 'verification', None, {'count': len(rows)})
    return [dict(r) for r in rows]


def invalidate_code(ctx, code_id, reason):
    with admin.action(ctx, 'security.code_invalidate', 'verification', code_id, reason=reason) as act:
        row = act.conn.execute('SELECT email FROM verification_codes WHERE id = ? AND used = 0',
                               (code_id,)).fetchone()
        if row is None:
            raise AdminError('A kód nem található.', 404)
        act.conn.execute('UPDATE verification_codes SET used = 1 WHERE id = ?', (code_id,))
        act.details['email'] = row['email']


# ===== Munkamenetek =====

def _session_rows(conn, where, params, token):
    rows = conn.execute(
        'SELECT s.id, s.user_id, s.created_at, s.expires_at, s.ip, s.user_agent, s.last_seen, s.admin_seen_at, '
        '(s.token = ?) AS current, u.display_name, u.email_lower '
        f'FROM sessions s JOIN users u ON u.id = s.user_id {where} ORDER BY COALESCE(s.last_seen, s.created_at) DESC '
        'LIMIT ?', [token or ''] + params + [500]).fetchall()
    return [{'id': r['id'], 'user_id': r['user_id'], 'display_name': r['display_name'], 'ip': r['ip'],
             'user_agent': (r['user_agent'] or '')[:160], 'created_at': r['created_at'], 'last_seen': r['last_seen'],
             'expires_at': r['expires_at'], 'current': bool(r['current']),
             'is_admin': r['email_lower'] in config.ADMIN_EMAILS} for r in rows]


def list_sessions(args, token=None):
    """Az aktív munkamenetek (a tokenek nélkül): q (név / e-mail / IP), `admin=1`: csak az adminoké."""
    where, params = ['s.expires_at > ?'], [auth.format_ts(auth.utcnow())]
    q = (args.get('q') or '').strip()
    if q:
        where.append("(u.display_name LIKE ? ESCAPE '\\' OR u.email_lower LIKE ? ESCAPE '\\' OR s.ip LIKE ? ESCAPE '\\')")
        params.extend([admin._like(q)] * 3)
    if args.get('admin') in ('1', 'true'):
        emails = sorted(config.ADMIN_EMAILS)
        where.append('u.email_lower IN (%s)' % ','.join('?' * len(emails)) if emails else '0')
        params.extend(emails)
    with auth.transaction() as conn:
        return _session_rows(conn, 'WHERE ' + ' AND '.join(where), params, token)


def revoke_session(ctx, session_id, reason, token=None):
    """Egy munkamenet érvénytelenítése. A saját (aktuális) munkameneted nem."""
    with admin.action(ctx, 'security.session_revoke', 'session', session_id, reason=reason) as act:
        row = act.conn.execute('SELECT user_id, token FROM sessions WHERE id = ?', (session_id,)).fetchone()
        if row is None:
            raise AdminError('A munkamenet nem található.', 404)
        if token and row['token'] == token:
            raise AdminError('A saját munkameneted nem zárható be.', 409)
        act.conn.execute('DELETE FROM sessions WHERE id = ?', (session_id,))
        act.details['user_id'] = row['user_id']
    return row['user_id']


def revoke_all_sessions(ctx, keep_token, reason):
    """Mindenki kijelentkeztetése (pl. a SECRET_KEY cseréje után): minden munkamenet törlése a sajátodon kívül.
    Visszatér: a törölt munkamenetek száma."""
    with admin.action(ctx, 'security.sessions_revoke_all', 'session', None, reason=reason) as act:
        removed = act.conn.execute('DELETE FROM sessions WHERE token != ?', (keep_token or '',)).rowcount
        act.details['removed'] = removed
    return removed


def close_other_admin_sessions(ctx, keep_token, reason):
    """Az adminok minden más munkamenetének bezárása (a sajátodon kívül)."""
    emails = sorted(config.ADMIN_EMAILS)
    if not emails:
        return 0
    with admin.action(ctx, 'security.admin_sessions_close', 'session', None, reason=reason) as act:
        removed = act.conn.execute(
            'DELETE FROM sessions WHERE token != ? AND user_id IN (SELECT id FROM users WHERE email_lower IN (%s))'
            % ','.join('?' * len(emails)), [keep_token or ''] + emails).rowcount
        act.details['removed'] = removed
    return removed


# ===== Forgalomkorlát =====

def rate_limit_state(limiter):
    return {'ips': limiter.limited_ips(), 'sids': limiter.limited_sids(), 'limits': limiter.effective_limits()}


def unblock_ip(ctx, limiter, ip, reason):
    address = (ip or '').strip()
    if not address:
        raise AdminError('Érvénytelen IP-cím vagy hálózat.', 400, field='ip')
    if address not in limiter._ip_history:
        raise AdminError('Ehhez az IP-hez nincs forgalomkorlát-előzmény.', 404)
    with admin.action(ctx, 'security.rate_unblock', 'ip', address, reason=reason):
        pass
    limiter.clear_ip(address)
