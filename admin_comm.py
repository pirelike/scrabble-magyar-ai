"""Admin panel: kommunikáció — közlemények (banner), karbantartási mód, push üzenetek, e-mail.

A közlemény és a karbantartási mód nyilvános oldalról is olvasható (`active_announcements`); a küldés (push, e-mail)
a felhasználók beállításait és az SMTP / push elérhetőségét tiszteletben tartja. A tartalom maga az indoklás, ezért
ezeknél a naplóbejegyzésnél az indoklás nem kötelező (a szöveg a naplóba kerül).
"""
import time
from datetime import timedelta

import admin
import auth
import config
import email_service
import push_service
import settings
from admin import AdminError

ANNOUNCEMENT_KINDS = ('info', 'warning', 'maintenance')
AUDIENCES = ('all', 'registered', 'guests')
MAX_TEXT = 500
MAX_TITLE = 80
MAX_EMAIL_SUBJECT = 150
MAX_EMAIL_BODY = 5000
MAX_PUSH_RECIPIENTS = 2000
MAX_EMAIL_RECIPIENTS = 500
BULK_EMAIL_COOLDOWN = 60            # mp: tömeges e-mailt ennyi időnként lehet küldeni
PUSH_GROUPS = ('online', 'recent', 'all')
EMAIL_GROUPS = ('recent', 'all')
RECENT_DAYS = 14
_bulk_state = {'last': 0.0}


# ===== Közlemények =====

def _clean_announcement(data):
    text_hu = admin.clean_text(data.get('text_hu'), 'text_hu', MAX_TEXT)
    text_en = admin.clean_text(data.get('text_en'), 'text_en', MAX_TEXT, required=False)
    kind = data.get('kind', 'info')
    audience = data.get('audience', 'all')
    if kind not in ANNOUNCEMENT_KINDS:
        raise AdminError('Érvénytelen típus.', 400, field='kind')
    if audience not in AUDIENCES:
        raise AdminError('Érvénytelen célcsoport.', 400, field='audience')
    starts = _stamp(data.get('starts_at'), 'starts_at')
    ends = _stamp(data.get('ends_at'), 'ends_at')
    if starts and ends and ends <= starts:
        raise AdminError('A vég nem lehet a kezdet előtt.', 400, field='ends_at')
    return {'text_hu': text_hu, 'text_en': text_en, 'kind': kind, 'audience': audience, 'starts_at': starts,
            'ends_at': ends}


def _stamp(value, field):
    if value in (None, ''):
        return None
    if not isinstance(value, str):
        raise AdminError('Érvénytelen dátum (ÉÉÉÉ-HH-NN formátum kell).', 400, field=field)
    try:
        return admin._parse_bound(value)[0]
    except AdminError as exc:
        exc.extra['field'] = field
        raise


def _status(row, now):
    if not row['active']:
        return 'revoked'
    if row['starts_at'] and row['starts_at'] > now:
        return 'scheduled'
    if row['ends_at'] and row['ends_at'] <= now:
        return 'expired'
    return 'active'


def _item(row, now):
    return {'id': row['id'], 'text_hu': row['text_hu'], 'text_en': row['text_en'], 'kind': row['kind'],
            'audience': row['audience'], 'starts_at': row['starts_at'], 'ends_at': row['ends_at'],
            'active': bool(row['active']), 'status': _status(row, now), 'created_by': row['created_by'],
            'created_at': row['created_at'], 'updated_at': row['updated_at']}


def list_announcements():
    now = auth.format_ts(auth.utcnow())
    with auth.transaction() as conn:
        rows = conn.execute('SELECT * FROM announcements ORDER BY id DESC LIMIT 200').fetchall()
    return [_item(r, now) for r in rows]


def create_announcement(ctx, data):
    clean = _clean_announcement(data)
    with admin.action(ctx, 'comm.announcement_create', 'announcement', None, reason=data.get('reason'),
                      details=dict(clean), require_reason=False) as act:
        cursor = act.conn.execute(
            'INSERT INTO announcements (text_hu, text_en, kind, audience, starts_at, ends_at, created_by) '
            'VALUES (?, ?, ?, ?, ?, ?, ?)', (clean['text_hu'], clean['text_en'], clean['kind'], clean['audience'],
                                            clean['starts_at'], clean['ends_at'], ctx.admin_user_id))
        act.details['announcement_id'] = cursor.lastrowid
    return cursor.lastrowid


def update_announcement(ctx, announcement_id, data):
    clean = _clean_announcement(data)
    active = data.get('active', True)
    active = admin.bool_field(active, 'active')
    with admin.action(ctx, 'comm.announcement_update', 'announcement', announcement_id, reason=data.get('reason'),
                      require_reason=False) as act:
        row = act.conn.execute('SELECT * FROM announcements WHERE id = ?', (announcement_id,)).fetchone()
        if row is None:
            raise AdminError('A közlemény nem található.', 404)
        act.conn.execute(
            'UPDATE announcements SET text_hu = ?, text_en = ?, kind = ?, audience = ?, starts_at = ?, ends_at = ?, '
            "active = ?, updated_at = datetime('now') WHERE id = ?",
            (clean['text_hu'], clean['text_en'], clean['kind'], clean['audience'], clean['starts_at'],
             clean['ends_at'], 1 if active else 0, announcement_id))
        act.details['before'] = {k: row[k] for k in ('text_hu', 'text_en', 'kind', 'audience', 'starts_at', 'ends_at',
                                                     'active')}
        act.details['after'] = {**clean, 'active': active}


def revoke_announcement(ctx, announcement_id):
    with admin.action(ctx, 'comm.announcement_revoke', 'announcement', announcement_id, require_reason=False) as act:
        row = act.conn.execute('SELECT active FROM announcements WHERE id = ?', (announcement_id,)).fetchone()
        if row is None:
            raise AdminError('A közlemény nem található.', 404)
        if not row['active']:
            raise AdminError('A közlemény már vissza van vonva.', 409)
        act.conn.execute("UPDATE announcements SET active = 0, updated_at = datetime('now') WHERE id = ?",
                         (announcement_id,))


def _utc_iso(stamp):
    return stamp.replace(' ', 'T') + 'Z' if stamp else None


def active_announcements(registered):
    """A most érvényes közlemények a nyilvános felületnek (a karbantartási móddal együtt). `registered`: a kérő
    bejelentkezett-e (célcsoport szerinti szűréshez)."""
    now = auth.format_ts(auth.utcnow())
    audiences = ('all', 'registered') if registered else ('all', 'guests')
    with auth.transaction() as conn:
        rows = conn.execute(
            'SELECT * FROM announcements WHERE active = 1 AND (starts_at IS NULL OR starts_at <= ?) '
            f"AND (ends_at IS NULL OR ends_at > ?) AND audience IN ({','.join('?' * len(audiences))}) "
            'ORDER BY id', [now, now, *audiences]).fetchall()
    items = [{'id': r['id'], 'kind': r['kind'], 'text_hu': r['text_hu'], 'text_en': r['text_en'] or r['text_hu'],
              'stamp': r['updated_at'], 'ends_at': _utc_iso(r['ends_at'])} for r in rows]
    maintenance = settings.maintenance()
    if maintenance:
        items.insert(0, {'id': 'maintenance', 'kind': 'maintenance', 'text_hu': maintenance.get('message_hu', ''),
                         'text_en': maintenance.get('message_en') or maintenance.get('message_hu', ''),
                         'stamp': maintenance.get('started_at', ''), 'ends_at': _utc_iso(maintenance.get('until'))})
    return items


# ===== Karbantartási mód =====

def maintenance_state():
    value = settings.get(settings.MAINTENANCE_KEY) or {}
    return {'enabled': bool(value.get('enabled')), 'active': settings.maintenance() is not None,
            'message_hu': value.get('message_hu', ''), 'message_en': value.get('message_en', ''),
            'until': value.get('until'), 'started_at': value.get('started_at')}


def set_maintenance(ctx, data):
    """A karbantartási mód be- és kikapcsolása. Bekapcsolva nem hozható létre új szoba / levelezős játék / napi
    próbálkozás, a futó játékok folytatódnak; az admin kivétel. Az `until` után magától véget ér."""
    enabled = admin.bool_field(data.get('enabled'), 'enabled')
    message_hu = admin.clean_text(data.get('message_hu'), 'message_hu', MAX_TEXT, required=enabled)
    message_en = admin.clean_text(data.get('message_en'), 'message_en', MAX_TEXT, required=False)
    until = _stamp(data.get('until'), 'until')
    if enabled and until and until <= auth.format_ts(auth.utcnow()):
        raise AdminError('A lejárat nem lehet a múltban.', 400, field='until')
    before = maintenance_state()
    value = {'enabled': enabled, 'message_hu': message_hu, 'message_en': message_en, 'until': until,
             'started_at': auth.format_ts(auth.utcnow()) if enabled else None}
    with admin.action(ctx, 'comm.maintenance', 'setting', settings.MAINTENANCE_KEY, reason=data.get('reason'),
                      require_reason=False) as act:
        settings.store(act.conn, settings.MAINTENANCE_KEY, value, ctx.admin_user_id)
        act.details['before'] = {k: before[k] for k in ('enabled', 'message_hu', 'until')}
        act.details['after'] = {'enabled': enabled, 'message_hu': message_hu, 'until': until}
    settings.invalidate()
    return maintenance_state()


# ===== Címzettek =====

def _recent_cutoff():
    return auth.format_ts(auth.utcnow() - timedelta(days=RECENT_DAYS))


def _group_users(group, online_ids, with_push=False):
    """A csoport felhasználói: [(id, név, e-mail)]. Törölt és kitiltott fiók nem címzett."""
    now = auth.format_ts(auth.utcnow())
    where = "u.deleted_at IS NULL AND COALESCE(u.banned_until, '') <= ?"
    params = [now]
    if group == 'recent':
        where += ' AND COALESCE(u.last_login_at, u.created_at) >= ?'
        params.append(_recent_cutoff())
    elif group == 'online':
        ids = sorted(online_ids)
        where += ' AND u.id IN (%s)' % ','.join('?' * len(ids)) if ids else ' AND 0'
        params.extend(ids)
    elif group != 'all':
        raise AdminError('Érvénytelen célcsoport.', 400, field='group')
    if with_push:
        where += ' AND EXISTS (SELECT 1 FROM push_subscriptions p WHERE p.user_id = u.id)'
    with auth.transaction() as conn:
        rows = conn.execute(f'SELECT u.id, u.display_name, u.email FROM users u WHERE {where} ORDER BY u.id',
                            params).fetchall()
    return [(r['id'], r['display_name'], r['email']) for r in rows]


def _user_row(user_id):
    with auth.transaction() as conn:
        row = conn.execute('SELECT id, display_name, email, deleted_at FROM users WHERE id = ?',
                           (user_id,)).fetchone()
    if row is None or row['deleted_at']:
        raise AdminError('Felhasználó nem található.', 404)
    return row


# ===== Push =====

def _clean_push(data):
    title = admin.clean_text(data.get('title'), 'title', MAX_TITLE)
    body = admin.clean_text(data.get('body'), 'body', MAX_TEXT)
    return title, body


def push_recipients(target, user_id=None, group=None, online_ids=frozenset()):
    """A push címzettjei: [(user_id, név)] — csak akinek van feliratkozott eszköze."""
    if target == 'user':
        row = _user_row(user_id)
        users = [(row['id'], row['display_name'], row['email'])] if auth.count_push_subscriptions(row['id']) else []
    elif target == 'group':
        users = _group_users(group, online_ids, with_push=True)
    else:
        raise AdminError('Érvénytelen célcsoport.', 400, field='target')
    return [(uid, name) for uid, name, _email in users][:MAX_PUSH_RECIPIENTS]


def push_preview(data, online_ids=frozenset()):
    """Előnézet: mit kapnának és hányan (a küldés nélkül)."""
    title, body = _clean_push(data)
    recipients = push_recipients(data.get('target'), data.get('user_id'), data.get('group'), online_ids)
    devices = sum(auth.count_push_subscriptions(uid) for uid, _n in recipients)
    return {'title': title, 'body': body, 'recipients': len(recipients), 'devices': devices,
            'available': push_service.is_available(), 'sample': [n for _u, n in recipients[:10]]}


def push_send(ctx, data, online_ids=frozenset()):
    """Push üzenet küldése egy felhasználónak vagy csoportnak. Csoportnál megerősítés (`confirm`) kell. Visszatér: az
    eredmény (sikeres / sikertelen / törölt feliratkozás)."""
    if not push_service.is_available():
        raise AdminError('A push értesítések ezen a szerveren nem érhetők el.', 503)
    title, body = _clean_push(data)
    if data.get('target') == 'group' and data.get('confirm') is not True:
        raise AdminError('Csoportos küldéshez megerősítés kell.', 400, needs_confirm=True)
    recipients = push_recipients(data.get('target'), data.get('user_id'), data.get('group'), online_ids)
    if not recipients:
        raise AdminError('Nincs címzett (nincs feliratkozott eszköz).', 409)
    with auth.transaction() as conn:
        admin.record(conn, ctx, 'comm.push', 'user' if data.get('target') == 'user' else 'group',
                     data.get('user_id') or data.get('group'),
                     {'title': title, 'body': body, 'recipients': len(recipients)})
    totals = {'sent': 0, 'removed': 0, 'failed': 0}
    for uid, _name in recipients:
        result = push_service.send_custom(uid, title, body)
        for key in totals:
            totals[key] += result[key]
    return {'recipients': len(recipients), **totals}


def push_test(ctx, user_id):
    """Teszt üzenet a felhasználó összes eszközére."""
    if not push_service.is_available():
        raise AdminError('A push értesítések ezen a szerveren nem érhetők el.', 503)
    row = _user_row(user_id)
    if not auth.count_push_subscriptions(user_id):
        raise AdminError('A felhasználónak nincs feliratkozott eszköze.', 409)
    with auth.transaction() as conn:
        admin.record(conn, ctx, 'user.push_test', 'user', user_id)
    messages = push_service.MESSAGES
    result = {'sent': 0, 'removed': 0, 'failed': 0}
    for sub in auth.get_push_subscriptions(user_id):
        texts = messages.get(sub['lang']) or messages[push_service.DEFAULT_LANG]
        outcome = push_service._send_one(sub, {'title': texts['title'], 'body': texts['test'], 'tag': 'test',
                                               'url': '/'})
        if outcome == 'ok':
            result['sent'] += 1
        elif outcome == 'gone':
            auth.delete_push_subscription(sub['endpoint'])
            result['removed'] += 1
        else:
            result['failed'] += 1
    result['name'] = row['display_name']
    return result


# ===== E-mail =====

def _email_body(name, body):
    return f'Szia {name}!\n\n{body}\n\nÜdvözlettel,\nMagyar Scrabble'


def _clean_email(data):
    subject = admin.clean_text(data.get('subject'), 'subject', MAX_EMAIL_SUBJECT)
    body = admin.clean_text(data.get('body'), 'body', MAX_EMAIL_BODY)
    if '\n' in subject or '\r' in subject:
        raise AdminError('Érvénytelen tárgy.', 400, field='subject')
    return subject, body


def email_user(ctx, user_id, data):
    """E-mail egy felhasználónak (SMTP-n, sablonnal). SMTP nélkül a konzolra kerül. Visszatér: {'sent', 'console'}."""
    subject, body = _clean_email(data)
    row = _user_row(user_id)
    with auth.transaction() as conn:
        admin.record(conn, ctx, 'comm.email', 'user', user_id, {'subject': subject, 'body': body})
    ok, error = email_service.send_plain_email(row['email'], subject, _email_body(row['display_name'], body), wait=True)
    if error and error != 'smtp_not_configured':
        raise AdminError('Az e-mail küldése nem sikerült.', 502)
    return {'sent': ok, 'console': not config.SMTP_CONFIGURED}


def email_preview(group):
    if group not in EMAIL_GROUPS:
        raise AdminError('Érvénytelen célcsoport.', 400, field='group')
    users = _group_users(group, frozenset())
    return {'recipients': len(users), 'limit': MAX_EMAIL_RECIPIENTS, 'sample': [n for _i, n, _e in users[:10]]}


def email_bulk(ctx, data):
    """Tömeges e-mail: csak megerősítéssel (`confirm`), legfeljebb percenként egy, legfeljebb 500 címzett."""
    group = data.get('group')
    if group not in EMAIL_GROUPS:
        raise AdminError('Érvénytelen célcsoport.', 400, field='group')
    subject, body = _clean_email(data)
    if data.get('confirm') is not True:
        raise AdminError('Tömeges küldéshez megerősítés kell.', 400, needs_confirm=True)
    users = _group_users(group, frozenset())
    if not users:
        raise AdminError('Nincs címzett.', 409)
    if len(users) > MAX_EMAIL_RECIPIENTS:
        raise AdminError('Túl sok címzett egyszerre.', 400)
    now = time.time()
    if now - _bulk_state['last'] < BULK_EMAIL_COOLDOWN:
        raise AdminError('Tömeges e-mailt legfeljebb percenként egyet lehet küldeni.', 429)
    with auth.transaction() as conn:
        admin.record(conn, ctx, 'comm.email_bulk', 'group', group,
                     {'subject': subject, 'body': body, 'recipients': len(users)})
    _bulk_state['last'] = now
    sent = failed = 0
    for _uid, name, email in users:
        ok, error = email_service.send_plain_email(email, subject, _email_body(name, body))
        if ok:
            sent += 1
        elif error != 'smtp_not_configured':
            failed += 1
    return {'recipients': len(users), 'sent': sent, 'failed': failed, 'console': not config.SMTP_CONFIGURED}
