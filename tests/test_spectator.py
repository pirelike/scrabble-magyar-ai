"""Megfigyelő (spectator) mód: folyamatban lévő játék megfigyelése játékos nélkül."""
import pytest

from room import Room


@pytest.fixture(autouse=True)
def clean_state():
    import server
    for d in (server.rooms, server.join_codes, server.player_rooms, server.player_names,
              server.player_auth, server._reconnect_tokens, server._sid_to_token,
              server._disconnected_players, server.state._online_users,
              server.state._sid_to_user_id, server.state._pending_invites,
              server.state.spectator_rooms):
        d.clear()
    server.rate_limiter._socket_history.clear()
    yield
    for d in (server.rooms, server.join_codes, server.player_rooms, server.player_names,
              server.player_auth, server.state.spectator_rooms):
        d.clear()


@pytest.fixture
def make_client():
    import server

    def _make(name='Alice'):
        c = server.socketio.test_client(server.app)
        c.emit('set_name', {'name': name, 'is_guest': True, 'user_id': None})
        c.get_received()
        return c
    return _make


def _events(client, name):
    return [e['args'][0] if e['args'] else None for e in client.get_received() if e['name'] == name]


def _live_room(make_client, private=False, finished=False):
    """Folyamatban lévő két játékosos szoba. Visszatér: (a, b, room)."""
    import server
    a = make_client('Anna')
    a.emit('create_room', {'name': 'Élő', 'max_players': 2, 'is_private': private})
    a.get_received()
    room = list(server.state.rooms.values())[-1]
    b = make_client('Béla')
    b.emit('join_room', {'code': room.join_code})
    b.get_received()
    a.emit('start_game')
    a.get_received()
    b.get_received()
    if finished:
        room.game.finished = True
    return a, b, room


class TestLiveGamesList:
    def test_started_public_game_is_listed(self, make_client):
        import server
        a, b, room = _live_room(make_client)
        live = server.state.get_live_games()
        assert [g['id'] for g in live] == [room.id]
        assert live[0]['name'] == 'Élő'
        assert {p['name'] for p in live[0]['players']} == {'Anna', 'Béla'}
        assert live[0]['spectators'] == 0

    def test_private_game_not_listed(self, make_client):
        import server
        _live_room(make_client, private=True)
        assert server.state.get_live_games() == []

    def test_waiting_room_not_listed(self, make_client):
        import server
        a = make_client()
        a.emit('create_room', {'name': 'Vár'})
        assert server.state.get_live_games() == []

    def test_finished_game_not_listed(self, make_client):
        import server
        _live_room(make_client, finished=True)
        assert server.state.get_live_games() == []

    def test_get_rooms_sends_live_games(self, make_client):
        a, b, room = _live_room(make_client)
        c = make_client('Cili')
        c.emit('get_rooms')
        live = _events(c, 'live_games')
        assert live and live[0][0]['id'] == room.id

    def test_game_start_broadcasts_live_games(self, make_client):
        import server
        a = make_client('Anna')
        a.emit('create_room', {'name': 'Élő'})
        a.get_received()
        watcher = make_client('Néző')
        watcher.get_received()
        a.emit('start_game')
        assert _events(watcher, 'live_games')


class TestSpectate:
    def test_spectate_public_room_by_id(self, make_client):
        a, b, room = _live_room(make_client)
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        events = s.get_received()
        names = [e['name'] for e in events]
        assert 'spectate_joined' in names and 'game_state' in names
        joined = next(e['args'][0] for e in events if e['name'] == 'spectate_joined')
        assert joined['room_name'] == 'Élő'
        assert len(room.spectators) == 1

    def test_spectator_state_has_no_hands(self, make_client):
        a, b, room = _live_room(make_client)
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        state = [e['args'][0] for e in s.get_received() if e['name'] == 'game_state'][-1]
        assert state['spectator'] is True
        assert all('hand' not in p for p in state['players'])
        assert all(p['hand_count'] == 7 for p in state['players'])

    def test_players_do_not_get_spectator_flag(self, make_client):
        a, b, room = _live_room(make_client)
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        state = [e['args'][0] for e in a.get_received() if e['name'] == 'game_state'][-1]
        assert 'spectator' not in state
        assert state['spectator_count'] == 1

    def test_spectator_receives_updates(self, make_client):
        a, b, room = _live_room(make_client)
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        s.get_received()
        current = a if room.game.current_player().name == 'Anna' else b
        current.emit('pass_turn')
        states = _events(s, 'game_state')
        assert states and states[-1]['history'][-1]['type'] == 'pass'

    def test_private_room_needs_code(self, make_client):
        a, b, room = _live_room(make_client, private=True)
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        assert any('privát' in e['message'] for e in _events(s, 'error'))
        s.emit('spectate_room', {'code': room.join_code})
        assert 'spectate_joined' in [e['name'] for e in s.get_received()]

    def test_cannot_spectate_waiting_room(self, make_client):
        import server
        a = make_client('Anna')
        a.emit('create_room', {'name': 'Vár'})
        a.get_received()
        room = list(server.state.rooms.values())[-1]
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        assert any('folyamatban' in e['message'] for e in _events(s, 'error'))

    def test_cannot_spectate_finished_game(self, make_client):
        a, b, room = _live_room(make_client, finished=True)
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        assert _events(s, 'error')

    def test_unknown_room_and_bad_code(self, make_client):
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': 'nincs'})
        assert any('nem létezik' in e['message'] for e in _events(s, 'error'))
        s.emit('spectate_room', {'code': '12'})
        assert any('Érvénytelen kód' in e['message'] for e in _events(s, 'error'))
        s.emit('spectate_room', {'code': '999999'})
        assert any('Nincs ilyen kódú' in e['message'] for e in _events(s, 'error'))
        s.emit('spectate_room', {})
        assert _events(s, 'error')

    def test_player_cannot_spectate_other_room(self, make_client):
        a, b, room = _live_room(make_client)
        c, d, other = _live_room(make_client)
        a.emit('spectate_room', {'room_id': other.id})
        assert any('Már egy szobában' in e['message'] for e in _events(a, 'error'))

    def test_spectator_cannot_spectate_twice(self, make_client):
        a, b, room = _live_room(make_client)
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        s.get_received()
        s.emit('spectate_room', {'room_id': room.id})
        assert any('Már egy szobában' in e['message'] for e in _events(s, 'error'))

    def test_spectator_limit(self, make_client, monkeypatch):
        monkeypatch.setattr(Room, 'MAX_SPECTATORS', 1)
        a, b, room = _live_room(make_client)
        s1 = make_client('N1')
        s1.emit('spectate_room', {'room_id': room.id})
        s1.get_received()
        s2 = make_client('N2')
        s2.emit('spectate_room', {'room_id': room.id})
        assert any('megfigyelő' in e['message'] for e in _events(s2, 'error'))

    def test_spectator_cannot_act_in_game(self, make_client):
        a, b, room = _live_room(make_client)
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        s.get_received()
        before = room.game.turn_number
        s.emit('pass_turn')
        s.emit('place_tiles', {'tiles': [{'row': 7, 'col': 7, 'letter': 'A', 'is_blank': False},
                                         {'row': 7, 'col': 8, 'letter': 'L', 'is_blank': False}]})
        s.emit('send_chat', {'message': 'szia'})
        s.emit('request_hint')
        assert room.game.turn_number == before
        assert room.chat_messages == []

    def test_spectator_receives_chat(self, make_client):
        a, b, room = _live_room(make_client)
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        s.get_received()
        a.emit('send_chat', {'message': 'Sziasztok'})
        chats = _events(s, 'chat_message')
        assert chats and chats[0]['message'] == 'Sziasztok'

    def test_spectator_sees_chat_history(self, make_client):
        a, b, room = _live_room(make_client)
        a.emit('send_chat', {'message': 'régi'})
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        joined = _events(s, 'spectate_joined')[0]
        assert [m['message'] for m in joined['chat_messages']] == ['régi']


class TestLeaveSpectate:
    def test_leave_spectate(self, make_client):
        import server
        a, b, room = _live_room(make_client)
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        s.get_received()
        s.emit('leave_spectate')
        assert 'spectate_left' in [e['name'] for e in s.get_received()]
        assert room.spectators == {}
        assert server.state.spectator_rooms == {}

    def test_leave_updates_spectator_count(self, make_client):
        a, b, room = _live_room(make_client)
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        a.get_received()
        s.emit('leave_spectate')
        state = [e['args'][0] for e in a.get_received() if e['name'] == 'game_state'][-1]
        assert state['spectator_count'] == 0

    def test_leave_without_spectating_is_noop(self, make_client):
        s = make_client('Néző')
        s.emit('leave_spectate')
        assert s.get_received() == []

    def test_disconnect_removes_spectator(self, make_client):
        import server
        a, b, room = _live_room(make_client)
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        s.disconnect()
        assert room.spectators == {}
        assert server.state.spectator_rooms == {}

    def test_logout_removes_spectator(self, make_client):
        a, b, room = _live_room(make_client)
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        s.get_received()
        s.emit('logout')
        assert room.spectators == {}

    def test_spectator_can_join_a_room_after_leaving(self, make_client):
        a, b, room = _live_room(make_client)
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        s.emit('leave_spectate')
        s.get_received()
        s.emit('create_room', {'name': 'Saját'})
        assert 'room_joined' in [e['name'] for e in s.get_received()]


class TestRoomLifecycle:
    def test_spectators_are_notified_when_room_disbands(self, make_client):
        import server
        a, b, room = _live_room(make_client)
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        s.get_received()
        a.emit('leave_room')  # a tulajdonos kilépése feloszlatja az aktív játékot
        assert 'room_disbanded' in [e['name'] for e in s.get_received()]
        assert room.id not in server.state.rooms
        assert server.state.spectator_rooms == {}

    def test_spectators_notified_when_everyone_leaves(self, make_client):
        import server
        a, b, room = _live_room(make_client)
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        s.get_received()
        server._cleanup_room(room.id)
        assert 'room_disbanded' in [e['name'] for e in s.get_received()]
        assert server.state.spectator_rooms == {}

    def test_finished_game_state_reaches_spectator(self, make_client):
        a, b, room = _live_room(make_client)
        s = make_client('Néző')
        s.emit('spectate_room', {'room_id': room.id})
        s.get_received()
        for _ in range(4):
            current = a if room.game.current_player().name == 'Anna' else b
            import server
            server.rate_limiter._socket_history.clear()
            current.emit('pass_turn')
            if room.game.finished:
                break
        states = _events(s, 'game_state')
        assert states[-1]['finished'] is True

    def test_spectator_keeps_room_alive_for_bots(self, make_client, monkeypatch):
        """Ember nélkül nem lépnek a robotok, de néző jelenlétében igen."""
        import server
        a = make_client('Anna')
        a.emit('create_room', {'name': 'Bot', 'ai_players': ['easy', 'hard'], 'is_private': False})
        a.get_received()
        room = list(server.state.rooms.values())[-1]
        a.emit('start_game')
        a.get_received()
        room.game.players[0].disconnected = True
        room.game.current_player_idx = 1
        assert server._bot_should_move(room) is False
        room.spectators['x'] = 'Néző'
        assert server._bot_should_move(room) is True
