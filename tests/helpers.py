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
