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


# ===== Admin panel =====

ADMIN_EMAIL = 'boss@example.com'
ADMIN_PASSWORD = 'secret12'
ADMIN_HEADERS = {'X-Admin-Request': '1', 'Origin': 'http://localhost'}


class AdminApi:
    """Az admin API hívása tesztből: az őr elvárta fejlécekkel és a bejelentkezett admin sessionjével."""

    def __init__(self, token, user_id):
        self.token = token
        self.user_id = user_id
        self.client = client_with_session(token)

    def get(self, path, **kwargs):
        return self.client.get(path, **kwargs)

    def post(self, path, json=None):
        return self.client.post(path, json={} if json is None else json, headers=ADMIN_HEADERS)

    def patch(self, path, json=None):
        return self.client.patch(path, json={} if json is None else json, headers=ADMIN_HEADERS)

    def delete(self, path, json=None):
        return self.client.delete(path, json={} if json is None else json, headers=ADMIN_HEADERS)

    def sudo(self):
        response = self.post('/api/admin/sudo', {'password': ADMIN_PASSWORD})
        assert response.status_code == 200, response.get_data(as_text=True)

    def ctx(self):
        import admin
        return admin.AdminContext(self.user_id, '203.0.113.7', 'Teszt-Böngésző/1.0')


def make_user(email, name='Játékos'):
    """Regisztrált felhasználó (id, session token)."""
    return create_user_with_session(email, name)


def guest_client(name='Vendég'):
    """Socket.IO tesztkliens vendégként (a névvel azonosított)."""
    import server
    client = server.socketio.test_client(server.app)
    client.emit('set_name', {'name': name, 'is_guest': True, 'user_id': None})
    client.get_received()
    return client


def registered_client(user_id, name):
    """Socket.IO tesztkliens regisztrált felhasználóként (aláírt tokennel)."""
    import server
    client = server.socketio.test_client(server.app)
    client.emit('set_name', registered_set_name_payload(user_id, name))
    client.get_received()
    return client


def live_room(owner, others=(), **options):
    """Szoba létrehozása és indítása. Visszatér: a szoba (a `server.state.rooms` utolsó eleme)."""
    import server
    owner.emit('create_room', {'name': options.pop('name', 'Teszt szoba'), 'max_players': 4, **options})
    owner.get_received()
    room = list(server.state.rooms.values())[-1]
    for other in others:
        other.emit('join_room', {'code': room.join_code})
        other.get_received()
    owner.emit('start_game')
    owner.get_received()
    for other in others:
        other.get_received()
    return room


def finished_game(room_id, results, has_bots=False, resigned=(), moves=0):
    """Befejezett játék az adatbázisban (az értékszámot és a statisztikát is rögzíti).

    results: [(user_id vagy None, név, pont)]; a legtöbb pontot elérő nyer. Visszatér: a játék azonosítója."""
    import json
    import auth
    best = max(score for _uid, _name, score in results)
    players = [{'player_name': name, 'user_id': uid, 'final_score': score, 'is_winner': score == best,
                'resigned': name in resigned} for uid, name, score in results]
    state = {'players': [{'name': name, 'resigned': name in resigned, 'is_bot': False} for _u, name, _s in results],
             'finished': True}
    game_id = auth.finish_game(room_id, json.dumps(state), players, room_name=f'Játék {room_id}', has_bots=has_bots)
    for number in range(1, moves + 1):
        auth.add_game_move(game_id, number, results[(number - 1) % len(results)][1], 'pass',
                           json.dumps({'rack': ['A'], 'score': 0}), json.dumps({}))
    return game_id
