"""Közös segédfüggvények a tesztekhez."""


def registered_set_name_payload(user_id, name=None):
    """set_name payload regisztrált felhasználóhoz, érvényes aláírt tokennel.

    A szerver a kliens által küldött user_id-ban önmagában nem bízik, ezért kell token.
    """
    import server
    from socket_auth import create_socket_token
    return {
        'name': name or f'User{user_id}',
        'is_guest': False,
        'user_id': user_id,
        'auth_token': create_socket_token(server.app.config['SECRET_KEY'], user_id),
    }


def verify_email(email):
    """Email cím kóddal való megerősítése (a regisztrációhoz ez kell)."""
    import auth
    code = auth.create_verification_code(email)
    ok, msg = auth.verify_code(email, code)
    assert ok, msg


def create_user_with_session(email, name='Tester', password='secret12'):
    """Regisztrált felhasználó és érvényes session token (a HTTP bejelentkezés és a forgalomkorlát megkerülésével)."""
    import auth
    ok, user_id = auth.create_user(email, name, password)
    assert ok, user_id
    return user_id, auth.create_session(user_id)


def client_with_session(token):
    """Flask tesztkliens a megadott session cookie-val (token=None → névtelen)."""
    from server import app
    client = app.test_client()
    if token:
        client.set_cookie('session_token', token)
    return client
