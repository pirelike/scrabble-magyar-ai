"""Admin panel: a levelező szerver (SMTP) beállítása, kapcsolat-próbája és visszaállítása.

A tárolás és a most érvényes beállítás a `mail_config.py`-ban van; itt az ellenőrzés, a naplózás és a próba. A
jelszó soha nem kerül a válaszba és a naplóba (csak az, hogy változott-e). A módosítás a beállítást azonnal, újraindítás
nélkül érvényesíti: az `email_service` minden küldésnél a friss értéket olvassa.
"""
import ipaddress
import re

import auth
import email_service
import mail_config
from admin import AdminError, action, bool_field, clean_text, int_field, record

_HOST_RE = re.compile(r'^(?=.{1,253}$)[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?(\.[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?)*$')
_EMAIL_RE = re.compile(r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$')
MAX_HOST = 253
MAX_USERNAME = 254
MAX_PASSWORD = 256
MAX_FROM_NAME = 80


def _single_line(text, field):
    """Fejléc-injektálás ellen: sortörés és vezérlőkarakter nem lehet a mezőben."""
    if any(ord(ch) < 32 or ord(ch) == 127 for ch in text):
        raise AdminError('Érvénytelen karakter a mezőben.', 400, field=field)
    return text


def _host(value):
    host = _single_line(clean_text(value, 'host', MAX_HOST), 'host')
    if _HOST_RE.match(host):
        return host
    try:
        ipaddress.ip_address(host.strip('[]'))      # IPv6 cím
        return host.strip('[]')
    except ValueError:
        raise AdminError('Érvénytelen kiszolgáló.', 400, field='host') from None


def build(data):
    """A kérésből ellenőrzött, teljes beállítás. A jelszó hiányában (`null`) a most érvényes marad (ha a kiszolgáló és
    a felhasználónév nem változott); felhasználónév nélkül nincs jelszó."""
    if not isinstance(data, dict):
        raise AdminError('Érvénytelen kérés.', 400)
    current = mail_config.current()
    host = _host(data.get('host'))
    port = int_field(data.get('port'), 'port', 1, 65535)
    security = data.get('security')
    if security not in mail_config.SECURITY_MODES:
        raise AdminError('Érvénytelen titkosítási mód.', 400, field='security')
    username = _single_line(clean_text(data.get('username'), 'username', MAX_USERNAME, required=False), 'username')
    password = data.get('password')
    if password is not None and (not isinstance(password, str) or len(password) > MAX_PASSWORD):
        raise AdminError('Érvénytelen jelszó.', 400, field='password')
    if not username:
        password = ''
    elif password is None:
        # Nincs megadva: a mostani marad, de csak ugyanahhoz a kiszolgálóhoz és felhasználóhoz (a mentett jelszó
        # így nem küldhető el egy másik címre anélkül, hogy valaki újra megadná)
        if host != current['host'] or username != current['username']:
            raise AdminError('A kiszolgáló vagy a felhasználónév változott: add meg újra a jelszót.', 400, field='password')
        password = current['password']
    else:
        password = _single_line(password, 'password')
    if username and not password:
        raise AdminError('A felhasználónévhez jelszó is kell.', 400, field='password')
    from_address = _single_line(clean_text(data.get('from_address'), 'from_address', 254), 'from_address')
    if not _EMAIL_RE.match(from_address):
        raise AdminError('Érvénytelen feladó e-mail cím.', 400, field='from_address')
    from_name = _single_line(clean_text(data.get('from_name'), 'from_name', MAX_FROM_NAME, required=False), 'from_name')
    verify_tls = bool_field(data.get('verify_tls', True), 'verify_tls')
    return {'host': host, 'port': port, 'security': security, 'username': username, 'password': password,
            'from_address': from_address, 'from_name': from_name, 'verify_tls': verify_tls}


def _audit_view(cfg):
    """A beállítás naplózható (jelszó nélküli) képe."""
    return {'host': cfg['host'], 'port': cfg['port'], 'security': cfg['security'], 'username': cfg['username'],
            'password_set': bool(cfg['password']), 'from_address': cfg['from_address'],
            'from_name': cfg['from_name'], 'verify_tls': bool(cfg['verify_tls']), 'source': cfg.get('source')}


def view():
    """A felületnek szánt beállítás (jelszó nélkül), a módosító nevével."""
    data = mail_config.public_view()
    data['updated_by_name'] = None
    if data['updated_by']:
        with auth.transaction() as conn:
            row = conn.execute('SELECT display_name FROM users WHERE id = ?', (data['updated_by'],)).fetchone()
        data['updated_by_name'] = row['display_name'] if row else None
    return data


def update(ctx, data):
    """A levelező szerver beállításának mentése (kötelező indoklással, naplózva)."""
    cfg = build(data)
    before = mail_config.current()
    with action(ctx, 'system.mail_update', 'system', 'mail', reason=data.get('reason')) as act:
        mail_config.save(act.conn, cfg, ctx.admin_user_id)
        act.details['before'] = _audit_view(before)
        act.details['after'] = _audit_view({**cfg, 'source': 'database'})
        act.details['password_changed'] = cfg['password'] != before['password']
    return view()


def reset(ctx, reason):
    """A mentett beállítás törlése: a környezeti változók értéke lép érvénybe."""
    if mail_config.stored() is None:
        raise AdminError('Nincs mentett beállítás: a környezeti változók érvényesek.', 409)
    before = mail_config.current()
    with action(ctx, 'system.mail_reset', 'system', 'mail', reason=reason) as act:
        mail_config.clear(act.conn)
        act.details['before'] = _audit_view(before)
    return view()


def check(ctx, data, recipient=None):
    """Kapcsolat-próba a megadott (még nem mentett) beállítással; `recipient` esetén teszt levéllel. A próba a
    naplóba kerül (indoklás nélkül, jelszó nélkül). Visszatér: {ok, code, sent_to}."""
    cfg = build(data)
    send_to = recipient if data.get('send') is True and recipient else None
    ok, code = email_service.check_connection(cfg, send_to)
    with auth.transaction() as conn:
        record(conn, ctx, 'system.mail_check', 'system', 'mail',
               {'host': cfg['host'], 'port': cfg['port'], 'security': cfg['security'], 'sent': bool(send_to),
                'ok': ok, 'code': code})
    return {'ok': ok, 'code': code, 'sent_to': send_to if ok else None}
