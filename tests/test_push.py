"""Web Push: VAPID kulcsok, feliratkozások, küldés és a „Te jössz!” értesítés."""
import base64
import json
from types import SimpleNamespace

import pytest

import auth
import push_service

ENDPOINT = 'https://push.example.com/send/abcdef123456'
P256DH = 'BNcRdreALRFXTkOOUHK1EtK2wtaz5Ry4YfYCA_0QTpQtUbVlUls0VJXg7A8u-Ts1XbjhazAkj7I99e8QcYP7DkM'
AUTH_KEY = 'tBHItJI5svbpez7KI4CCXg'


@pytest.fixture(autouse=True)
def _isolated(monkeypatch):
    import server
    monkeypatch.setattr(push_service, '_keys', None)
    monkeypatch.setattr(push_service, '_spawn', None)        # a küldés a hívó szálon, azonnal
    for var in ('VAPID_PRIVATE_KEY', 'VAPID_PUBLIC_KEY', 'VAPID_SUBJECT'):
        monkeypatch.delenv(var, raising=False)
    server.rate_limiter._ip_history.clear()
    server.rate_limiter._socket_history.clear()
    server.state.hidden_sids.clear()
    yield
    server.state.hidden_sids.clear()


@pytest.fixture
def sent(monkeypatch):
    """Rögzíti a `webpush` hívásait; a viselkedés a `behaviour` listával szabályozható."""
    class Calls(list):
        behaviour = {'status': None}

    calls = Calls()
    behaviour = calls.behaviour

    def fake(info, data=None, vapid_private_key=None, vapid_claims=None, ttl=0, timeout=None, **kwargs):
        calls.append({'info': info, 'data': json.loads(data), 'claims': vapid_claims, 'ttl': ttl})
        if behaviour['status']:
            error = push_service.WebPushException('boom')
            error.response = SimpleNamespace(status_code=behaviour['status'])
            raise error
        return SimpleNamespace(status_code=201)
    monkeypatch.setattr(push_service, 'webpush', fake)
    return calls


def _user(email='a@example.com', name='Anna'):
    return auth.create_user(email, name, 'pass123')[1]


class TestMessages:
    def test_hungarian_and_english(self):
        hu = push_service.build_message('turn', 'hu', room='Kedd esti', room_id='r1')
        en = push_service.build_message('turn', 'en', room='Tuesday', room_id='r1')
        assert hu['body'] == 'Te jössz! · Kedd esti' and hu['title'] == 'Magyar Scrabble'
        assert en['body'] == "It's your turn! · Tuesday"
        assert hu['url'] == '/' and hu['tag'] == 'turn-r1'

    def test_unknown_language_falls_back_to_hungarian(self):
        assert push_service.build_message('turn', 'de', room='X')['body'].startswith('Te jössz')
        assert push_service.build_message('turn', None, room='X')['body'].startswith('Te jössz')


class TestSubscriptions:
    def test_save_and_list(self):
        uid = _user()
        auth.save_push_subscription(uid, ENDPOINT, P256DH, AUTH_KEY, 'en')
        subs = auth.get_push_subscriptions(uid)
        assert subs == [{'endpoint': ENDPOINT, 'p256dh': P256DH, 'auth': AUTH_KEY, 'lang': 'en'}]
        assert auth.count_push_subscriptions(uid) == 1

    def test_resubscribing_updates_instead_of_duplicating(self):
        uid = _user()
        auth.save_push_subscription(uid, ENDPOINT, P256DH, AUTH_KEY, 'hu')
        auth.save_push_subscription(uid, ENDPOINT, P256DH, AUTH_KEY, 'en')
        assert auth.count_push_subscriptions(uid) == 1
        assert auth.get_push_subscriptions(uid)[0]['lang'] == 'en'

    def test_endpoint_moves_to_the_newest_account_on_the_device(self):
        a, b = _user('a@example.com', 'A'), _user('b@example.com', 'B')
        auth.save_push_subscription(a, ENDPOINT, P256DH, AUTH_KEY)
        auth.save_push_subscription(b, ENDPOINT, P256DH, AUTH_KEY)
        assert auth.count_push_subscriptions(a) == 0 and auth.count_push_subscriptions(b) == 1

    def test_delete_is_scoped_to_the_owner(self):
        a, b = _user('a@example.com', 'A'), _user('b@example.com', 'B')
        auth.save_push_subscription(a, ENDPOINT, P256DH, AUTH_KEY)
        assert auth.delete_push_subscription(ENDPOINT, b) == 0
        assert auth.delete_push_subscription(ENDPOINT, a) == 1
        assert auth.count_push_subscriptions(a) == 0

    def test_subscriptions_follow_the_user_deletion(self):
        uid = _user()
        auth.save_push_subscription(uid, ENDPOINT, P256DH, AUTH_KEY)
        with auth._db() as conn:
            conn.execute('DELETE FROM users WHERE id = ?', (uid,))
        assert auth.get_push_subscriptions(uid) == []


class TestVapidKeys:
    def test_generated_once_and_persisted(self):
        first = push_service.public_key()
        assert len(first) > 80 and '=' not in first
        assert auth.get_setting('vapid_private_pem').startswith('-----BEGIN')
        push_service._keys = None                       # újraindítás
        assert push_service.public_key() == first

    def test_environment_key_wins(self, monkeypatch):
        vapid_pem = push_service.get_keys()[0].private_pem().decode()
        expected = push_service.public_key()
        with auth._db() as conn:
            conn.execute('DELETE FROM app_settings')
        push_service._keys = None
        monkeypatch.setenv('VAPID_PRIVATE_KEY', vapid_pem.replace('\n', '\\n'))
        assert push_service.public_key() == expected
        assert auth.get_setting('vapid_private_pem') is None     # nem generált újat

    def test_environment_key_in_base64url_form(self, monkeypatch):
        # a `web-push generate-vapid-keys` formája: a nyers 32 bájtos kulcs base64url-ben
        vapid = push_service.get_keys()[0]
        expected = push_service.public_key()
        raw = vapid.private_key.private_numbers().private_value.to_bytes(32, 'big')
        with auth._db() as conn:
            conn.execute('DELETE FROM app_settings')
        push_service._keys = None
        monkeypatch.setenv('VAPID_PRIVATE_KEY', base64.urlsafe_b64encode(raw).rstrip(b'=').decode())
        assert push_service.public_key() == expected
        assert auth.get_setting('vapid_private_pem') is None     # nem cserélte le egy generáltra

    def test_unreadable_environment_key_falls_back_to_the_stored_one(self, monkeypatch):
        stored = push_service.public_key()
        push_service._keys = None
        monkeypatch.setenv('VAPID_PRIVATE_KEY', 'nem-kulcs')
        assert push_service.public_key() == stored

    def test_subject(self, monkeypatch):
        assert push_service.subject().startswith('mailto:')
        monkeypatch.setenv('VAPID_SUBJECT', 'https://scrabble.example.com')
        assert push_service.subject() == 'https://scrabble.example.com'


class TestSending:
    def test_notifies_every_device_in_its_language(self, sent):
        uid = _user()
        auth.save_push_subscription(uid, ENDPOINT, P256DH, AUTH_KEY, 'hu')
        auth.save_push_subscription(uid, ENDPOINT + '2', P256DH, AUTH_KEY, 'en')
        assert push_service.notify_user(uid, 'turn', room='Szoba', room_id='r1') == 2
        bodies = {c['data']['body'] for c in sent}
        assert bodies == {'Te jössz! · Szoba', "It's your turn! · Szoba"}
        assert all(c['claims']['sub'].startswith('mailto:') for c in sent)
        assert all(c['ttl'] == push_service.TTL_SECONDS for c in sent)
        assert sent[0]['info']['keys'] == {'p256dh': P256DH, 'auth': AUTH_KEY}

    def test_expired_subscriptions_are_removed(self, sent):
        uid = _user()
        auth.save_push_subscription(uid, ENDPOINT, P256DH, AUTH_KEY)
        sent.behaviour['status'] = 410
        assert push_service.notify_user(uid, 'turn', room='X', room_id='r') == 0
        assert auth.count_push_subscriptions(uid) == 0

    def test_temporary_errors_keep_the_subscription(self, sent):
        uid = _user()
        auth.save_push_subscription(uid, ENDPOINT, P256DH, AUTH_KEY)
        sent.behaviour['status'] = 500
        assert push_service.notify_user(uid, 'turn', room='X', room_id='r') == 0
        assert auth.count_push_subscriptions(uid) == 1

    def test_network_errors_never_raise(self, monkeypatch):
        uid = _user()
        auth.save_push_subscription(uid, ENDPOINT, P256DH, AUTH_KEY)

        def boom(*args, **kwargs):
            raise ConnectionError('nincs hálózat')
        monkeypatch.setattr(push_service, 'webpush', boom)
        assert push_service.notify_user(uid, 'turn', room='X', room_id='r') == 0
        assert auth.count_push_subscriptions(uid) == 1

    def test_notify_turn_without_subscription_does_nothing(self, sent):
        uid = _user()
        assert push_service.notify_turn(uid, 'Szoba', 'r1') is False
        assert sent == []

    def test_notify_turn_runs_in_the_background_when_a_spawner_is_set(self, sent, monkeypatch):
        uid = _user()
        auth.save_push_subscription(uid, ENDPOINT, P256DH, AUTH_KEY)
        queued = []
        monkeypatch.setattr(push_service, '_spawn', queued.append)
        assert push_service.notify_turn(uid, 'Szoba', 'r1') is True
        assert sent == [] and len(queued) == 1
        queued[0]()
        assert len(sent) == 1

    def test_unavailable_library_disables_everything(self, monkeypatch, sent):
        uid = _user()
        auth.save_push_subscription(uid, ENDPOINT, P256DH, AUTH_KEY)
        monkeypatch.setattr(push_service, '_LIBS', False)
        assert push_service.notify_user(uid, 'turn', room='X', room_id='r') == 0
        assert push_service.notify_turn(uid, 'X', 'r') is False
        assert sent == []


# ---------------------------------------------------------------- API

@pytest.fixture
def client():
    import server
    server.app.config['TESTING'] = True
    return server.app.test_client()


def _login(client, uid):
    client.set_cookie('session_token', auth.create_session(uid), domain='localhost')


class TestApi:
    def test_public_key_requires_login(self, client):
        assert client.get('/api/push/public-key').status_code == 401

    def test_public_key(self, client):
        uid = _user()
        _login(client, uid)
        data = client.get('/api/push/public-key').get_json()
        assert data['available'] and data['public_key'] == push_service.public_key()
        assert data['subscribed'] is False
        auth.save_push_subscription(uid, ENDPOINT, P256DH, AUTH_KEY)
        assert client.get('/api/push/public-key').get_json()['subscribed'] is True

    def test_public_key_when_unavailable(self, client, monkeypatch):
        _login(client, _user())
        monkeypatch.setattr(push_service, '_LIBS', False)
        assert client.get('/api/push/public-key').get_json() == {'success': True, 'available': False}

    def test_subscribe_and_unsubscribe(self, client):
        uid = _user()
        _login(client, uid)
        body = {'endpoint': ENDPOINT, 'keys': {'p256dh': P256DH, 'auth': AUTH_KEY}, 'lang': 'en'}
        assert client.post('/api/push/subscribe', json=body).get_json()['success']
        assert auth.get_push_subscriptions(uid)[0]['lang'] == 'en'
        assert client.post('/api/push/unsubscribe', json={'endpoint': ENDPOINT}).get_json()['success']
        assert auth.count_push_subscriptions(uid) == 0

    def test_unknown_language_is_stored_as_hungarian(self, client):
        uid = _user()
        _login(client, uid)
        body = {'endpoint': ENDPOINT, 'keys': {'p256dh': P256DH, 'auth': AUTH_KEY}, 'lang': 'xx'}
        client.post('/api/push/subscribe', json=body)
        assert auth.get_push_subscriptions(uid)[0]['lang'] == 'hu'

    @pytest.mark.parametrize('body', [
        {}, {'endpoint': 'http://insecure.example.com/x', 'keys': {'p256dh': P256DH, 'auth': AUTH_KEY}},
        {'endpoint': ENDPOINT}, {'endpoint': ENDPOINT, 'keys': {'p256dh': 'rovid', 'auth': AUTH_KEY}},
        {'endpoint': ENDPOINT, 'keys': {'p256dh': P256DH, 'auth': 5}}, {'endpoint': 5, 'keys': {}},
        {'endpoint': ENDPOINT, 'keys': 'nem dict'},
    ])
    def test_subscribe_validation(self, client, body):
        uid = _user()
        _login(client, uid)
        assert client.post('/api/push/subscribe', json=body).status_code == 400
        assert auth.count_push_subscriptions(uid) == 0

    def test_requires_login_and_is_rate_limited(self, client):
        body = {'endpoint': ENDPOINT, 'keys': {'p256dh': P256DH, 'auth': AUTH_KEY}}
        assert client.post('/api/push/subscribe', json=body).status_code == 401
        assert client.post('/api/push/unsubscribe', json={'endpoint': ENDPOINT}).status_code == 401
        _login(client, _user())
        codes = [client.post('/api/push/subscribe', json=body).status_code for _ in range(21)]
        assert codes[-1] == 429

    def test_unsubscribe_cannot_remove_someone_elses_subscription(self, client):
        a, b = _user('a@example.com', 'A'), _user('b@example.com', 'B')
        auth.save_push_subscription(a, ENDPOINT, P256DH, AUTH_KEY)
        _login(client, b)
        client.post('/api/push/unsubscribe', json={'endpoint': ENDPOINT})
        assert auth.count_push_subscriptions(a) == 1

    def test_subscribe_without_library(self, client, monkeypatch):
        _login(client, _user())
        monkeypatch.setattr(push_service, '_LIBS', False)
        body = {'endpoint': ENDPOINT, 'keys': {'p256dh': P256DH, 'auth': AUTH_KEY}}
        assert client.post('/api/push/subscribe', json=body).status_code == 503


# ---------------------------------------------------------------- szerver: Te jössz!

def _registered(email, name):
    import server
    from helpers import registered_set_name_payload
    _, uid = auth.create_user(email, name, 'password123')
    c = server.socketio.test_client(server.app)
    c.emit('set_name', registered_set_name_payload(uid, name=name))
    c.get_received()
    return c, uid


def _two_players():
    """Két regisztrált játékos elindított játékban. Visszatér: (a, a_id, b, b_id, room, game)."""
    import server
    a, a_id = _registered('anna@example.com', 'Anna')
    b, b_id = _registered('bela@example.com', 'Béla')
    a.emit('create_room', {'name': 'Esti játék', 'max_players': 2})
    code = next(m for m in a.get_received() if m['name'] == 'room_code')['args'][0]['code']
    b.emit('join_room', {'code': code})
    b.get_received()
    a.emit('start_game')
    a.get_received(); b.get_received()
    room = next(iter(server.state.rooms.values()))
    return a, a_id, b, b_id, room, room.game


def _current(game, a, a_id, b, b_id):
    return (a, a_id, b, b_id) if game.current_player().name == 'Anna' else (b, b_id, a, a_id)


class TestTurnPush:
    def test_disconnected_player_is_notified_once(self, sent):
        import server
        a, a_id, b, b_id, room, game = _two_players()
        mover, mover_id, waiting, waiting_id = _current(game, a, a_id, b, b_id)
        auth.save_push_subscription(waiting_id, ENDPOINT, P256DH, AUTH_KEY)
        mover.emit('pass_turn')                          # a kör átmegy, a másik játékos online és látható
        assert sent == []
        game.current_player().disconnected = True        # közben lecsatlakozik (pl. levelezős játékban)
        server._emit_all_states(game, room.id)
        assert len(sent) == 1
        assert sent[0]['data']['body'] == 'Te jössz! · Esti játék'
        server._emit_all_states(game, room.id)           # ugyanaz a kör: nincs újabb értesítés
        assert len(sent) == 1

    def test_online_visible_player_gets_no_push(self, sent):
        a, a_id, b, b_id, room, game = _two_players()
        mover, mover_id, waiting, waiting_id = _current(game, a, a_id, b, b_id)
        auth.save_push_subscription(waiting_id, ENDPOINT, P256DH, AUTH_KEY)
        mover.emit('pass_turn')
        assert sent == []

    def test_hidden_tab_triggers_the_push(self, sent):
        a, a_id, b, b_id, room, game = _two_players()
        mover, mover_id, waiting, waiting_id = _current(game, a, a_id, b, b_id)
        auth.save_push_subscription(waiting_id, ENDPOINT, P256DH, AUTH_KEY)
        waiting.emit('set_visibility', {'hidden': True})   # még nem az ő köre
        assert sent == []
        mover.emit('pass_turn')                            # most ő jön, és a lapja rejtett
        assert len(sent) == 1

    def test_hiding_during_your_own_turn_pushes_immediately(self, sent):
        a, a_id, b, b_id, room, game = _two_players()
        mover, mover_id, *_ = _current(game, a, a_id, b, b_id)
        auth.save_push_subscription(mover_id, ENDPOINT, P256DH, AUTH_KEY)
        mover.emit('set_visibility', {'hidden': True})
        assert len(sent) == 1
        mover.emit('set_visibility', {'hidden': True})     # ugyanarra a körre nem ismétel
        assert len(sent) == 1

    def test_returning_to_the_tab_cancels_the_hidden_state(self, sent):
        import server
        a, a_id, b, b_id, room, game = _two_players()
        mover, mover_id, waiting, waiting_id = _current(game, a, a_id, b, b_id)
        auth.save_push_subscription(waiting_id, ENDPOINT, P256DH, AUTH_KEY)
        waiting.emit('set_visibility', {'hidden': True})
        waiting.emit('set_visibility', {'hidden': False})
        assert not server.state.hidden_sids
        mover.emit('pass_turn')
        assert sent == []

    def test_no_subscription_means_no_push(self, sent):
        a, a_id, b, b_id, room, game = _two_players()
        mover, mover_id, waiting, waiting_id = _current(game, a, a_id, b, b_id)
        waiting.emit('set_visibility', {'hidden': True})
        mover.emit('pass_turn')
        assert sent == []
        assert room.pushed_turn == -1       # nem jelöljük meg a kört, ha nem ment ki értesítés

    def test_guests_and_bots_are_never_notified(self, sent):
        import server
        guest = server.socketio.test_client(server.app)
        guest.emit('set_name', {'name': 'Vendég', 'is_guest': True, 'user_id': None})
        guest.get_received()
        guest.emit('create_room', {'name': 'R', 'max_players': 2, 'ai_players': [3]})
        guest.get_received()
        guest.emit('start_game')
        guest.get_received()
        room = next(iter(server.state.rooms.values()))
        guest.emit('set_visibility', {'hidden': True})
        server._emit_all_states(room.game, room.id)
        assert sent == []

    def test_puzzle_rooms_are_never_notified(self, sent):
        import server
        client, uid = _registered('p@example.com', 'Puzzle')
        auth.save_push_subscription(uid, ENDPOINT, P256DH, AUTH_KEY)
        import daily
        puzzle = {'date': daily.today_str(), 'board': [[None] * 15 for _ in range(15)],
                  'rack': ['A'] * 7, 'best_score': 10, 'best': {}}
        auth.save_daily_puzzle(puzzle['date'], puzzle['board'], puzzle['rack'], 10, {'tiles': [], 'words': []})
        client.emit('start_daily')
        client.get_received()
        client.emit('set_visibility', {'hidden': True})
        assert sent == []

    def test_disconnect_forgets_the_hidden_state(self):
        import server
        a, a_id, b, b_id, room, game = _two_players()
        a.emit('set_visibility', {'hidden': True})
        assert server.state.hidden_sids
        a.disconnect()
        assert not server.state.hidden_sids

    def test_malformed_visibility_events_are_ignored(self):
        import server
        a, *_ = _two_players()
        a.emit('set_visibility', 'nem dict')
        a.emit('set_visibility', {'hidden': 'igen'})
        assert not server.state.hidden_sids

