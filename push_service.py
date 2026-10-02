"""Web Push értesítések: VAPID kulcsok, feliratkozások és a „Te jössz!” üzenet.

A kulcspár a `VAPID_PRIVATE_KEY` környezeti változóból jön (PEM, vagy a `web-push` eszköz base64url
formája; a nyilvános kulcs ebből számolódik), ennek híján az első induláskor készül és az
adatbázisban (`app_settings`) marad, hogy
a feliratkozások újraindítás után is érvényesek legyenek. Ha a `pywebpush` nincs telepítve, a
funkció kikapcsol (`is_available()` hamis), a játék többi része érintetlen.
"""
import base64
import json
import os

import auth
from config import SMTP_FROM

try:
    from cryptography.hazmat.primitives import serialization
    from py_vapid import Vapid
    from pywebpush import WebPushException, webpush
    _LIBS = True
except ImportError:   # pragma: no cover - a környezet dönti el
    _LIBS = False

TTL_SECONDS = 6 * 3600   # ennyi ideig őrzi a push szolgáltatás a kézbesítetlen üzenetet

MESSAGES = {
    'hu': {'title': 'Magyar Scrabble', 'turn': 'Te jössz! · {room}',
           'invite': 'Meghívtak egy levelezős játékba: {room}'},
    'en': {'title': 'Hungarian Scrabble', 'turn': "It's your turn! · {room}",
           'invite': "You've been invited to a correspondence game: {room}"},
}
DEFAULT_LANG = 'hu'

_keys = None          # (Vapid, nyilvános kulcs base64url) gyorsítótár
_spawn = None         # háttérfeladat-indító (a szerver állítja be)


def is_available():
    return _LIBS


def init(spawn):
    """A háttérfeladat-indító beállítása (pl. `socketio.start_background_task`)."""
    global _spawn
    _spawn = spawn


def subject():
    """A VAPID `sub` mezője (mailto: vagy https: cím)."""
    configured = os.environ.get('VAPID_SUBJECT', '')
    if configured:
        return configured
    return f'mailto:{SMTP_FROM}' if SMTP_FROM else 'mailto:admin@localhost.localdomain'


def _b64url(raw):
    return base64.urlsafe_b64encode(raw).rstrip(b'=').decode()


def _public_b64(vapid):
    raw = vapid.public_key.public_bytes(serialization.Encoding.X962,
                                        serialization.PublicFormat.UncompressedPoint)
    return _b64url(raw)


def _key_from_env():
    """A `VAPID_PRIVATE_KEY` kulcs (PEM, vagy a `web-push` / `vapid` eszközök base64url formája),
    vagy None, ha nincs megadva / nem olvasható."""
    configured = os.environ.get('VAPID_PRIVATE_KEY', '').replace('\\n', '\n').strip()
    if not configured:
        return None
    try:
        if configured.startswith('-----'):
            return Vapid.from_pem(configured.encode())
        return Vapid.from_string(configured)
    except Exception as e:
        print(f"[push] A VAPID_PRIVATE_KEY nem olvasható, a tárolt kulcsot használom: {e}")
        return None


def _load_keys():
    """(Vapid, nyilvános kulcs): környezetből, az adatbázisból, vagy újonnan generálva.
    A generált kulcs az adatbázisba kerül (a környezetből jövő sosem írja felül)."""
    vapid = _key_from_env()
    if vapid is None:
        pem = auth.get_setting('vapid_private_pem') or ''
        if pem.startswith('-----'):
            vapid = Vapid.from_pem(pem.encode())
        else:
            vapid = Vapid()
            vapid.generate_keys()
            auth.set_setting('vapid_private_pem', vapid.private_pem().decode())
    return vapid, _public_b64(vapid)


def get_keys():
    global _keys
    if _keys is None:
        _keys = _load_keys()
    return _keys


def public_key():
    """A böngészőnek szóló nyilvános kulcs (applicationServerKey, base64url)."""
    return get_keys()[1]


def build_message(kind, lang=None, **params):
    """A push üzenet tartalma a felhasználó nyelvén: {'title', 'body', 'tag', 'url'}."""
    texts = MESSAGES.get(lang) or MESSAGES[DEFAULT_LANG]
    room = params.get('room') or ''
    return {
        'title': texts['title'],
        'body': texts[kind].format(room=room),
        'tag': f"{kind}-{params.get('room_id', '')}",
        'url': '/',
    }


def _send_one(subscription, payload):
    """Egy feliratkozás értesítése. Visszatér: 'ok' | 'gone' (a feliratkozás megszűnt) | 'error'."""
    vapid, _public = get_keys()
    info = {'endpoint': subscription['endpoint'],
            'keys': {'p256dh': subscription['p256dh'], 'auth': subscription['auth']}}
    try:
        webpush(info, data=json.dumps(payload, ensure_ascii=False), vapid_private_key=vapid,
                vapid_claims={'sub': subject()}, ttl=TTL_SECONDS, timeout=10)
        return 'ok'
    except WebPushException as e:
        status = getattr(getattr(e, 'response', None), 'status_code', None)
        if status in (404, 410):
            return 'gone'
        print(f"[push] Sikertelen küldés ({status}): {e}")
        return 'error'
    except Exception as e:   # hálózati hiba stb.: a játékot nem érinti
        print(f"[push] Hiba a küldésnél: {e}")
        return 'error'


def notify_user(user_id, kind, **params):
    """A felhasználó minden feliratkozott eszközének értesítése; a megszűnt feliratkozások törlődnek.
    Visszatér: a sikeresen küldött értesítések száma."""
    if not is_available():
        return 0
    sent = 0
    for sub in auth.get_push_subscriptions(user_id):
        payload = build_message(kind, sub['lang'], **params)
        outcome = _send_one(sub, payload)
        if outcome == 'ok':
            sent += 1
        elif outcome == 'gone':
            auth.delete_push_subscription(sub['endpoint'])
    return sent


def notify_turn(user_id, room_name, room_id, kind='turn'):
    """„Te jössz!” (vagy meghívó) értesítés háttérben (nem blokkolja a játékmenetet)."""
    if not is_available() or not auth.count_push_subscriptions(user_id):
        return False
    task = lambda: notify_user(user_id, kind, room=room_name, room_id=room_id)   # noqa: E731
    if _spawn is not None:
        _spawn(task)
    else:
        task()
    return True
