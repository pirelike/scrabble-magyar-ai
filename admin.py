"""Admin panel logika: napló (audit), admin munkamenet (tétlenség, sudo mód) és a napló lekérdezése.

A modul a Flask kéréstől független (a HTTP réteg az `admin_routes.py`-ban van), így közvetlenül
tesztelhető. Alapelvek (lásd `docs/ADMIN_PANEL.md`):
  * minden módosító admin művelet naplózva van, kötelező indoklással, a művelettel EGY tranzakcióban:
    ha a napló nem írható, a művelet sem történik meg (`action`);
  * a napló csak hozzáfűzhető: a táblán adatbázis-trigger tiltja a módosítást és a törlést;
  * a személyes adat megtekintése is naplózódik (`view.*`), indoklás nélkül (`record`).
"""
import csv
import io
import ipaddress
import json
import unicodedata
from contextlib import contextmanager
from dataclasses import dataclass
from datetime import datetime, timedelta

import auth
import config

MIN_REASON_LEN = 3
MAX_REASON_LEN = 500
MAX_USER_AGENT_LEN = 300
# A tétlenségi időzítő frissítése nem minden kérésnél ír az adatbázisba, csak ennyi másodpercenként
TOUCH_INTERVAL_SECONDS = 15

AUDIT_DEFAULT_LIMIT = 50
AUDIT_MAX_LIMIT = 200
AUDIT_EXPORT_MAX = 10000
AUDIT_FILTER_MAX_LEN = 100
AUDIT_CSV_FIELDS = ('id', 'created_at', 'admin_user_id', 'admin_name', 'action', 'target_type',
                    'target_id', 'ip', 'user_agent', 'reason', 'details_json')


class AdminError(Exception):
    """Az admin művelet nem hajtható végre; a HTTP réteg `status` kóddal és magyar üzenettel válaszol."""

    def __init__(self, message, status=400, **extra):
        super().__init__(message)
        self.message = message
        self.status = status
        self.extra = extra


@dataclass(frozen=True)
class AdminContext:
    """Ki és honnan végzi a műveletet (a napló sorába kerül)."""
    admin_user_id: int
    ip: str = ''
    user_agent: str = ''


# ===== IP-engedélylista =====

def ip_allowed(ip, networks=None):
    """Igaz, ha a kliens IP-je szerepel az engedélylistán. Üres lista → bármelyik IP engedélyezett."""
    networks = config.ADMIN_IP_ALLOWLIST if networks is None else networks
    if not networks:
        return True
    try:
        address = ipaddress.ip_address((ip or '').strip())
    except ValueError:
        return False
    # IPv4-ba képezett IPv6 cím (::ffff:1.2.3.4) az IPv4 hálózatokra is illeszkedjen
    mapped = getattr(address, 'ipv4_mapped', None)
    candidates = (address, mapped) if mapped else (address,)
    return any(candidate in network for network in networks for candidate in candidates
               if candidate.version == network.version)


# ===== Napló =====

def normalize_reason(reason):
    """Az indoklás tisztítása és ellenőrzése. Módosító admin műveletnél kötelező."""
    text = reason.strip() if isinstance(reason, str) else ''
    if len(text) < MIN_REASON_LEN:
        raise AdminError('Az indoklás megadása kötelező (legalább 3 karakter).', 400, field='reason')
    if len(text) > MAX_REASON_LEN:
        raise AdminError('Az indoklás legfeljebb 500 karakter lehet.', 400, field='reason')
    return text


def record(conn, ctx, action, target_type=None, target_id=None, details=None):
    """Egy naplósor beírása a megadott kapcsolaton (tehát a hívó tranzakciójában). Visszaadja a sor id-ját."""
    details_json = json.dumps(details, ensure_ascii=False, default=str) if details else None
    cursor = conn.execute(
        'INSERT INTO admin_audit (admin_user_id, action, target_type, target_id, details_json, ip, user_agent) '
        'VALUES (?, ?, ?, ?, ?, ?, ?)',
        (ctx.admin_user_id, action, target_type,
         None if target_id is None else str(target_id), details_json,
         ctx.ip or None, (ctx.user_agent or '')[:MAX_USER_AGENT_LEN] or None))
    return cursor.lastrowid


class _Action:
    """Az `action` blokk kezelője: a nyitott kapcsolat és a naplóba kerülő részletek (előtte / utána...)."""

    def __init__(self, conn, details):
        self.conn = conn
        self.details = details


@contextmanager
def action(ctx, name, target_type=None, target_id=None, reason=None, details=None, require_reason=True):
    """Módosító admin művelet naplózva, EGY tranzakcióban.

        with admin.action(ctx, 'user.ban', 'user', 42, reason=reason) as act:
            act.conn.execute('UPDATE users ...')
            act.details['before'] = ...

    Indoklás nélkül (túl rövid) a művelet el sem indul (`AdminError`). A naplósor a blokk végén íródik be
    ugyanabba a tranzakcióba, ezért ha a blokk vagy a naplózás hibázik, minden visszagördül. A memóriabeli
    mellékhatásokat (socket bontás, szoba állapota) a blokk UTÁN kell elvégezni.

    `require_reason=False`: az olyan műveleteknél, ahol a tartalom maga az indoklás (közlemény, push, e-mail): az
    indoklás elhagyható, de ha van, kötelezően ellenőrzött és naplózott.
    """
    clean_reason = normalize_reason(reason) if (require_reason or reason) else None
    with auth.transaction() as conn:
        act_details = {'reason': clean_reason} if clean_reason else {}
        act_details.update({k: v for k, v in (details or {}).items() if k != 'reason'})
        act = _Action(conn, act_details)
        yield act
        record(conn, ctx, name, target_type, target_id, act.details)


# ===== Admin munkamenet: tétlenség és sudo mód =====

def _seconds_until(stamp, now=None):
    moment = auth.parse_ts(stamp)
    if moment is None:
        return 0
    return max(0, int((moment - (now or auth.utcnow())).total_seconds()))


@dataclass(frozen=True)
class SessionStatus:
    """Az admin munkamenet állapota (a panel fejlécéhez és az őrhöz)."""
    idle_minutes: int
    idle_remaining: int      # mp; 0 → lejárt, jelszó kell
    sudo_minutes: int
    sudo_remaining: int      # mp; 0 → nincs érvényes sudo
    seen_at: object = None   # az utolsó admin-kérés ideje (datetime) vagy None

    @property
    def idle_expired(self):
        return self.idle_remaining <= 0

    def as_dict(self):
        return {
            'idle_minutes': self.idle_minutes,
            'idle_remaining': self.idle_remaining,
            'idle_expired': self.idle_expired,
            'sudo_minutes': self.sudo_minutes,
            'sudo_remaining': self.sudo_remaining,
        }


def session_status(token):
    """A munkamenet admin állapota; ha nincs ilyen session, None.

    A tétlenség az utolsó admin-kérésből számolódik; ha még nem volt ilyen (nincs időbélyeg), lejártnak számít.
    """
    sess = auth.get_admin_session(token)
    if sess is None:
        return None
    now = auth.utcnow()
    seen = auth.parse_ts(sess['admin_seen_at'])
    idle_limit = config.ADMIN_SESSION_IDLE_MINUTES * 60
    idle_remaining = 0 if seen is None else max(0, int(idle_limit - (now - seen).total_seconds()))
    return SessionStatus(
        idle_minutes=config.ADMIN_SESSION_IDLE_MINUTES,
        idle_remaining=idle_remaining,
        sudo_minutes=config.ADMIN_SUDO_MINUTES,
        # Tétlenség után a sudo sem él (az újraigazolás is lezárja)
        sudo_remaining=0 if idle_remaining <= 0 else _seconds_until(sess['sudo_until'], now),
        seen_at=seen,
    )


def touch(token, status=None):
    """Tétlenségi időzítő frissítése egy sikeres admin kérés után (ritkított írással)."""
    status = status if status is not None else session_status(token)
    if status is None:
        return
    if status.seen_at is not None \
            and (auth.utcnow() - status.seen_at).total_seconds() < TOUCH_INTERVAL_SECONDS:
        return
    auth.touch_admin_session(token)


def sudo_active(token):
    """Igaz, ha a munkamenetnek érvényes (nem lejárt) sudo megerősítése van."""
    status = session_status(token)
    return bool(status and status.sudo_remaining > 0)


def reauth(ctx, token):
    """Jelszavas újraigazolás tétlenség után: az időzítő újraindul, a sudo lezárul. Naplózva."""
    with auth.transaction() as conn:
        auth.touch_admin_session(token, conn=conn)
        conn.execute('UPDATE sessions SET sudo_until = NULL WHERE token = ?', (token,))
        record(conn, ctx, 'admin.reauth')


def grant_sudo(ctx, token):
    """Sudo mód megnyitása `ADMIN_SUDO_MINUTES` percre. Naplózva, egy tranzakcióban. Visszaadja a lejáratot."""
    minutes = config.ADMIN_SUDO_MINUTES
    until = auth.utcnow() + timedelta(minutes=minutes)
    with auth.transaction() as conn:
        auth.set_admin_sudo(token, until, conn=conn)
        record(conn, ctx, 'admin.sudo', details={'minutes': minutes})
    return until


def end_sudo(ctx, token):
    """Sudo mód lezárása. Naplózva."""
    with auth.transaction() as conn:
        auth.set_admin_sudo(token, None, conn=conn)
        record(conn, ctx, 'admin.sudo_end')


def record_failed_password(ctx, name):
    """Rossz jelszó megadása (`admin.sudo_failed` / `admin.reauth_failed`): csak napló, nincs más hatása."""
    with auth.transaction() as conn:
        record(conn, ctx, name)


# ===== Közös segédek a listákhoz és az időtartamokhoz =====

def fold(text):
    """Kis-nagybetű és ékezet nélküli alak a kereséshez (a magyar ő / ű is: o / u)."""
    if not isinstance(text, str):
        return text
    decomposed = unicodedata.normalize('NFD', text.lower())
    return ''.join(ch for ch in decomposed if not unicodedata.combining(ch))


def register_sql_helpers(conn):
    """A `py_fold` SQL függvény a kapcsolaton: ékezet- és kisbetű-független LIKE keresésekhez."""
    conn.create_function('py_fold', 1, fold)


def like_contains(text):
    """LIKE minta a (kisbetűs, ékezet nélküli) szöveg tartalmazására; ESCAPE '\\'."""
    return _like(fold(text))


def page_args(args, default=50, maximum=200):
    """`limit` / `offset` a kérésből, korlátok között."""
    return (_int_arg(args.get('limit'), default, 1, maximum), _int_arg(args.get('offset'), 0, 0, 10 ** 9))


def order_by(sort, order, allowed, default):
    """ORDER BY rész egy fehérlistából (a rendezés oszlopa sosem jön közvetlenül a felhasználótól).

    allowed: {kulcs: SQL-kifejezés}; az `order` 'asc' / 'desc'. Visszatér: 'kifejezés ASC|DESC'."""
    column = allowed.get(sort) or allowed[default]
    return f"{column} {'ASC' if str(order).lower() == 'asc' else 'DESC'}"


_RELATIVE_UNTIL = {'1h': timedelta(hours=1), '1d': timedelta(days=1), '7d': timedelta(days=7),
                   '30d': timedelta(days=30)}


def parse_until(value, permanent_ok=True, now=None):
    """Lejárat megadása: relatív érték (1h / 1d / 7d / 30d), pontos időpont, vagy None / 'permanent' (végleges).

    Visszatér: adatbázis-időbélyeg (a végleges a `auth.PERMANENT_UNTIL`)."""
    now = now or auth.utcnow()
    if value is None or value == 'permanent':
        if not permanent_ok:
            raise AdminError('Érvénytelen időtartam.', 400, field='until')
        return auth.PERMANENT_UNTIL
    if not isinstance(value, str):
        raise AdminError('Érvénytelen időtartam.', 400, field='until')
    if value in _RELATIVE_UNTIL:
        return auth.format_ts(now + _RELATIVE_UNTIL[value])
    stamp, _ = _parse_bound(value)
    if auth.parse_ts(stamp) <= now:
        raise AdminError('A lejárat nem lehet a múltban.', 400, field='until')
    return stamp


def clean_text(value, field, maximum, required=True):
    """Szöveges mező tisztítása (szóközök levágva, hossz ellenőrizve)."""
    text = value.strip() if isinstance(value, str) else ''
    if required and not text:
        raise AdminError('A mező kitöltése kötelező.', 400, field=field)
    if len(text) > maximum:
        raise AdminError('A szöveg túl hosszú.', 400, field=field)
    return text


def bool_field(value, field):
    if not isinstance(value, bool):
        raise AdminError('Érvénytelen kérés.', 400, field=field)
    return value


def int_field(value, field, low, high):
    if isinstance(value, bool) or not isinstance(value, int) or not low <= value <= high:
        raise AdminError('Érvénytelen szám.', 400, field=field)
    return value


# ===== Napló lekérdezése =====

def _like(text):
    """LIKE minta a szöveg tartalmazására (a joker karakterek elmenekítve; ESCAPE '\\')."""
    escaped = text.replace('\\', '\\\\').replace('%', '\\%').replace('_', '\\_')
    return f'%{escaped}%'


def _prefix_like(text):
    escaped = text.replace('\\', '\\\\').replace('%', '\\%').replace('_', '\\_')
    return f'{escaped}%'


def _parse_bound(value, end=False):
    """Dátum (ÉÉÉÉ-HH-NN) vagy időpont (ÉÉÉÉ-HH-NN ÓÓ:PP[:MM]) → adatbázis-időbélyeg. `end`: a nap végéig."""
    text = value.strip().replace('T', ' ')
    for fmt, date_only in (('%Y-%m-%d', True), ('%Y-%m-%d %H:%M', False), ('%Y-%m-%d %H:%M:%S', False)):
        try:
            moment = datetime.strptime(text, fmt)
        except ValueError:
            continue
        if date_only and end:
            moment += timedelta(days=1)  # a nap vége: a következő nap kezdetéig (kizárólag)
        return auth.format_ts(moment), (date_only and end)
    raise AdminError('Érvénytelen dátum (ÉÉÉÉ-HH-NN formátum kell).', 400)


def _text_filter(value):
    if value is None:
        return None
    if not isinstance(value, str):
        raise AdminError('Érvénytelen szűrő.', 400)
    value = value.strip()
    if len(value) > AUDIT_FILTER_MAX_LEN:
        raise AdminError('A szűrő túl hosszú.', 400)
    return value or None


def _int_arg(value, default, low, high):
    try:
        number = int(value)
    except (TypeError, ValueError):
        return default
    return max(low, min(high, number))


def _audit_item(row):
    try:
        details = json.loads(row['details_json']) if row['details_json'] else None
    except ValueError:
        details = None
    reason = details.get('reason') if isinstance(details, dict) else None
    return {
        'id': row['id'],
        'created_at': row['created_at'],
        'admin_user_id': row['admin_user_id'],
        'admin_name': row['admin_name'],
        'action': row['action'],
        'target_type': row['target_type'],
        'target_id': row['target_id'],
        'ip': row['ip'],
        'user_agent': row['user_agent'],
        'reason': reason if isinstance(reason, str) else None,
        'details': details,
    }


def query_audit(admin=None, action=None, target_type=None, target_id=None, since=None, until=None, q=None,
                limit=AUDIT_DEFAULT_LIMIT, offset=0, max_limit=AUDIT_MAX_LIMIT):
    """Naplósorok szűrve, a legújabbal kezdve. Visszaad: {items, total, limit, offset}.

    admin: id (szám) vagy név / e-mail részlete · action: előtag (pl. `user.` az összes felhasználói műveletre)
    · target_type / target_id: pontos egyezés · since / until: dátum vagy időpont (az `until` dátum a nap végéig)
    · q: szabad szöveg az akcióban, a célpontban, az IP-ben és a részletekben · limit: legfeljebb `max_limit`.
    """
    where, params = [], []
    admin = _text_filter(admin)
    if admin:
        if admin.isdigit():
            where.append('a.admin_user_id = ?')
            params.append(int(admin))
        else:
            where.append("(u.display_name LIKE ? ESCAPE '\\' OR u.email_lower LIKE ? ESCAPE '\\')")
            params.extend([_like(admin), _like(admin.lower())])
    action = _text_filter(action)
    if action:
        where.append("a.action LIKE ? ESCAPE '\\'")
        params.append(_prefix_like(action))
    target_type = _text_filter(target_type)
    if target_type:
        where.append('a.target_type = ?')
        params.append(target_type)
    target_id = _text_filter(target_id)
    if target_id:
        where.append('a.target_id = ?')
        params.append(target_id)
    since = _text_filter(since)
    if since:
        stamp, _ = _parse_bound(since)
        where.append('a.created_at >= ?')
        params.append(stamp)
    until = _text_filter(until)
    if until:
        stamp, exclusive = _parse_bound(until, end=True)
        where.append('a.created_at < ?' if exclusive else 'a.created_at <= ?')
        params.append(stamp)
    q = _text_filter(q)
    if q:
        pattern = _like(q)
        where.append("(a.action LIKE ? ESCAPE '\\' OR a.target_id LIKE ? ESCAPE '\\' "
                     "OR a.ip LIKE ? ESCAPE '\\' OR a.details_json LIKE ? ESCAPE '\\')")
        params.extend([pattern] * 4)

    limit = _int_arg(limit, AUDIT_DEFAULT_LIMIT, 1, max_limit)
    offset = _int_arg(offset, 0, 0, 10 ** 9)
    clause = ('WHERE ' + ' AND '.join(where)) if where else ''
    source = 'FROM admin_audit a LEFT JOIN users u ON u.id = a.admin_user_id ' + clause
    with auth.transaction() as conn:
        total = conn.execute('SELECT COUNT(*) ' + source, params).fetchone()[0]
        rows = conn.execute(
            'SELECT a.*, u.display_name AS admin_name ' + source + ' ORDER BY a.id DESC LIMIT ? OFFSET ?',
            params + [limit, offset]).fetchall()
    return {'items': [_audit_item(row) for row in rows], 'total': total, 'limit': limit, 'offset': offset}


def _csv_cell(value):
    """Táblázatkezelőben megnyitva ne fusson le képlet: a `=`, `+`, `-`, `@` kezdetű cellát idézőjel előzi meg."""
    if value is None:
        return ''
    text = str(value)
    if text and text[0] in ('=', '+', '-', '@', '\t', '\r'):
        return "'" + text
    return text


def audit_csv(items):
    """A naplósorok CSV-ben (UTF-8 BOM nélkül; a hívó adja a fejlécet)."""
    out = io.StringIO()
    writer = csv.writer(out, lineterminator='\r\n')
    writer.writerow(AUDIT_CSV_FIELDS)
    for item in items:
        details_json = json.dumps(item['details'], ensure_ascii=False) if item['details'] is not None else ''
        writer.writerow([_csv_cell(item.get(field)) if field != 'details_json' else _csv_cell(details_json)
                         for field in AUDIT_CSV_FIELDS])
    return out.getvalue()
