"""Levelező szerver (SMTP) beállításai.

A ténylegesen használt beállítás kétféle forrásból jöhet:

* az admin panelen mentett felülbírálat (`app_settings`, `mail.smtp` kulcs, JSON) — ez az erősebb, újraindítás nélkül hat;
* ennek híján a környezeti változókból olvasott `config.SMTP_*` értékek (a korábbi viselkedés: STARTTLS).

A jelszó a felülbírálatban az adatbázisban van (mint a push VAPID kulcsa), ezért sosem kerül ki a szerverről: a
`public_view()` csak azt mondja meg, hogy be van-e állítva. Az érvényesítés és a naplózás az `admin_mail.py`-ban van; ez
a modul csak tárol és kiszámolja a most érvényes beállítást. Hívási időben olvassa a `config` modult, így a tesztek
(és a futó szerver) mindig a friss értékeket látják.
"""
import json

import auth
import config

KEY = 'mail.smtp'
SECURITY_MODES = ('starttls', 'ssl', 'none')
DEFAULT_PORTS = {'starttls': 587, 'ssl': 465, 'none': 25}
STORED_FIELDS = ('host', 'port', 'security', 'username', 'password', 'from_address', 'from_name', 'verify_tls')


def environment():
    """A környezeti változókból jövő beállítás (a jelszóval együtt; csak szerveroldali használatra)."""
    return {
        'host': config.SMTP_HOST, 'port': config.SMTP_PORT, 'security': 'starttls',
        'username': config.SMTP_USER, 'password': config.SMTP_PASSWORD,
        'from_address': config.SMTP_FROM, 'from_name': '', 'verify_tls': True,
    }


def stored():
    """A mentett felülbírálat vagy None (nincs, nem olvasható, vagy hiányos a sorban lévő JSON)."""
    try:
        raw = auth.get_setting(KEY)
    except Exception:       # nincs még adatbázis / séma: a környezeti beállítás számít
        return None
    if not raw:
        return None
    try:
        data = json.loads(raw)
    except ValueError:
        return None
    if not isinstance(data, dict) or any(field not in data for field in STORED_FIELDS):
        return None
    if data['security'] not in SECURITY_MODES or not isinstance(data['port'], int):
        return None
    return data


def _complete(cfg):
    """Elég-e a beállítás a küldéshez: kiszolgáló, feladó, és ha van felhasználónév, jelszó is."""
    return bool(cfg['host'] and cfg['port'] and cfg['from_address'] and (not cfg['username'] or cfg['password']))


def current():
    """A most érvényes beállítás (jelszóval!) és a `source` ('database' | 'environment'), `configured`."""
    saved = stored()
    if saved is None:
        cfg = environment()
        # A korábbi szabály marad: az alapértelmezett kiszolgáló önmagában nem jelent beállított levelezést
        cfg.update(source='environment', configured=bool(config.SMTP_CONFIGURED))
        return cfg
    cfg = {field: saved[field] for field in STORED_FIELDS}
    cfg.update(source='database', configured=_complete(cfg))
    return cfg


def is_configured():
    return current()['configured']


def from_address():
    """A feladó címe (pl. a push VAPID `mailto:` címéhez), vagy üres szöveg."""
    return current()['from_address'] or ''


def save(conn, cfg, admin_id):
    """A felülbírálat beírása a hívó tranzakciójában. `cfg` a `STORED_FIELDS` mezőit tartalmazza (ellenőrizve)."""
    payload = {field: cfg[field] for field in STORED_FIELDS}
    payload.update(updated_by=admin_id, updated_at=auth.format_ts(auth.utcnow()))
    conn.execute('INSERT INTO app_settings (key, value) VALUES (?, ?) '
                 'ON CONFLICT(key) DO UPDATE SET value = excluded.value',
                 (KEY, json.dumps(payload, ensure_ascii=False)))


def clear(conn):
    """A felülbírálat törlése: a környezeti beállítás lép érvénybe. Visszatér: volt-e mit törölni."""
    return conn.execute('DELETE FROM app_settings WHERE key = ?', (KEY,)).rowcount > 0


def _public(cfg):
    return {
        'host': cfg['host'], 'port': cfg['port'], 'security': cfg['security'], 'username': cfg['username'],
        'password_set': bool(cfg['password']), 'from_address': cfg['from_address'], 'from_name': cfg['from_name'],
        'verify_tls': bool(cfg['verify_tls']),
    }


def public_view():
    """A beállítás a felületnek: jelszó nélkül. `environment`: a környezeti alapérték (a „visszaállítás” célja)."""
    cfg = current()
    saved = stored()
    view = _public(cfg)
    view.update(source=cfg['source'], configured=cfg['configured'],
                updated_by=saved['updated_by'] if saved else None, updated_at=saved['updated_at'] if saved else None)
    env = environment()
    view['environment'] = {**_public(env), 'configured': bool(config.SMTP_CONFIGURED)}
    view['defaults'] = dict(DEFAULT_PORTS)
    return view
