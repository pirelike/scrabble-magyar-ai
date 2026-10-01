"""Rövid életű, aláírt token a Socket.IO identitás igazolásához.

A socket a bejelentkezés előtt csatlakozik, ezért a handshake cookie-jai nem
tartalmazzák a session sütit. A kliens ezért a (cookie-val védett) HTTP végponton
kér egy tokent, és azt küldi a `set_name` eseményben — így a szerver nem a kliens
által megadott `user_id`-ban bízik.
"""
from itsdangerous import BadSignature, SignatureExpired, URLSafeTimedSerializer

_SALT = 'socket-auth'
TOKEN_MAX_AGE = 300  # másodperc


def _serializer(secret_key):
    return URLSafeTimedSerializer(secret_key, salt=_SALT)


def create_socket_token(secret_key, user_id):
    """Aláírt token a megadott felhasználóhoz."""
    return _serializer(secret_key).dumps({'uid': int(user_id)})


def verify_socket_token(secret_key, token, max_age=TOKEN_MAX_AGE):
    """Visszaadja a token user_id-ját, vagy None-t, ha érvénytelen / lejárt."""
    if not isinstance(token, str) or not token:
        return None
    try:
        payload = _serializer(secret_key).loads(token, max_age=max_age)
    except (BadSignature, SignatureExpired):
        return None
    uid = payload.get('uid') if isinstance(payload, dict) else None
    return uid if isinstance(uid, int) else None
