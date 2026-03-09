"""Integration tests for server.py Socket.IO events - lobby, rooms, join codes."""
import os
import sys
import pytest
from unittest.mock import patch


@pytest.fixture(autouse=True)
def clean_state():
    """Clean server state between tests."""
    import server
    server.rooms.clear()
    server.join_codes.clear()
    server.player_rooms.clear()
    server.player_names.clear()
    server.player_auth.clear()
    server._reconnect_tokens.clear()
    server._sid_to_token.clear()
    server._disconnected_players.clear()
    yield
    server.rooms.clear()
    server.join_codes.clear()
    server.player_rooms.clear()
    server.player_names.clear()
    server.player_auth.clear()
    server._reconnect_tokens.clear()
    server._sid_to_token.clear()
    server._disconnected_players.clear()


@pytest.fixture
def app():
    from server import app
    app.config['TESTING'] = True
    return app


@pytest.fixture
def socketio_app():
    from server import socketio
    return socketio


@pytest.fixture
def client(app, socketio_app):
    return socketio_app.test_client(app)


@pytest.fixture
def registered_client(app, socketio_app):
    """A client that is set up as a registered user."""
    c = socketio_app.test_client(app)
    c.emit('set_name', {'name': 'RegUser', 'is_guest': False, 'user_id': 1})
    c.get_received()  # Clear
    return c


@pytest.fixture
def guest_client(app, socketio_app):
    """A client that is set up as a guest user."""
    c = socketio_app.test_client(app)
    c.emit('set_name', {'name': 'GuestUser', 'is_guest': True, 'user_id': None})
    c.get_received()  # Clear
    return c


def _get_sid(server_module, name):
    """Find Socket.IO SID by player name in server state."""
    for sid, n in server_module.player_names.items():
        if n == name:
            return sid
    return None


class TestSetName:
    def test_set_name(self, client):
        client.emit('set_name', {'name': 'TestPlayer', 'is_guest': True, 'user_id': None})
        import server
        assert 'TestPlayer' in server.player_names.values()

    def test_set_name_registered(self, client):
        client.emit('set_name', {'name': 'RegPlayer', 'is_guest': False, 'user_id': 42})
        import server
        sid = _get_sid(server, 'RegPlayer')
        assert sid is not None
        assert server.player_auth[sid]['is_guest'] is False
        assert server.player_auth[sid]['user_id'] == 42

    def test_set_name_sanitization(self, client):
        client.emit('set_name', {'name': '', 'is_guest': True})
        import server
        assert 'Névtelen' in server.player_names.values()


class TestGetRooms:
    def test_get_rooms_empty(self, client):
        client.emit('get_rooms')
        received = client.get_received()
        rooms_events = [r for r in received if r['name'] == 'rooms_list']
        assert len(rooms_events) >= 1
        assert rooms_events[0]['args'][0] == []


class TestCreateRoom:
    def test_guest_can_create_room(self, guest_client):
        guest_client.emit('create_room', {'name': 'TestRoom', 'max_players': 4})
        received = guest_client.get_received()
        join_events = [r for r in received if r['name'] == 'room_joined']
        assert len(join_events) >= 1
        assert join_events[0]['args'][0]['room_name'] == 'TestRoom'
        assert join_events[0]['args'][0]['is_owner'] is True

    def test_registered_can_create_room(self, registered_client):
        registered_client.emit('create_room', {'name': 'TestRoom', 'max_players': 4})
        received = registered_client.get_received()
        join_events = [r for r in received if r['name'] == 'room_joined']
        assert len(join_events) >= 1
        assert join_events[0]['args'][0]['room_name'] == 'TestRoom'
        assert join_events[0]['args'][0]['is_owner'] is True

    def test_create_room_generates_code(self, registered_client):
        registered_client.emit('create_room', {'name': 'TestRoom', 'max_players': 4})
        received = registered_client.get_received()
        code_events = [r for r in received if r['name'] == 'room_code']
        assert len(code_events) >= 1
        code = code_events[0]['args'][0]['code']
        assert len(code) == 6
        assert code.isdigit()

    def test_create_room_default_name(self, registered_client):
        registered_client.emit('create_room', {'name': '', 'max_players': 4})
        received = registered_client.get_received()
        join_events = [r for r in received if r['name'] == 'room_joined']
        assert len(join_events) >= 1
        assert join_events[0]['args'][0]['room_name'] == 'Szoba'

    def test_create_room_clamps_max_players(self, registered_client):
        registered_client.emit('create_room', {'name': 'Room', 'max_players': 10})
        import server
        # Find the room
        for room in server.rooms.values():
            if room.name == 'Room':
                assert room.max_players == 4
                break


class TestJoinRoom:
    def test_join_by_code(self, app, socketio_app, registered_client):
        # Create room first
        registered_client.emit('create_room', {'name': 'JoinTest', 'max_players': 4})
        received = registered_client.get_received()
        code_events = [r for r in received if r['name'] == 'room_code']
        code = code_events[0]['args'][0]['code']

        # Second client joins by code
        client2 = socketio_app.test_client(app)
        client2.emit('set_name', {'name': 'Joiner', 'is_guest': True, 'user_id': None})
        client2.get_received()

        client2.emit('join_room', {'code': code})
        received2 = client2.get_received()
        join_events = [r for r in received2 if r['name'] == 'room_joined']
        assert len(join_events) >= 1
        assert join_events[0]['args'][0]['room_name'] == 'JoinTest'
        assert join_events[0]['args'][0]['is_owner'] is False
        client2.disconnect()

    def test_join_invalid_code(self, guest_client):
        guest_client.emit('join_room', {'code': '999999'})
        received = guest_client.get_received()
        error_events = [r for r in received if r['name'] == 'error']
        assert len(error_events) >= 1

    def test_join_bad_format_code(self, guest_client):
        guest_client.emit('join_room', {'code': 'abc'})
        received = guest_client.get_received()
        error_events = [r for r in received if r['name'] == 'error']
        assert len(error_events) >= 1
        assert 'Érvénytelen' in error_events[0]['args'][0]['message']

    def test_join_room_by_id(self, app, socketio_app, registered_client):
        # Create room
        registered_client.emit('create_room', {'name': 'IdJoinTest', 'max_players': 4})
        received = registered_client.get_received()
        join_events = [r for r in received if r['name'] == 'room_joined']
        room_id = join_events[0]['args'][0]['room_id']

        # Second client joins by ID
        client2 = socketio_app.test_client(app)
        client2.emit('set_name', {'name': 'Joiner2', 'is_guest': True, 'user_id': None})
        client2.get_received()

        client2.emit('join_room', {'room_id': room_id})
        received2 = client2.get_received()
        join_events2 = [r for r in received2 if r['name'] == 'room_joined']
        assert len(join_events2) >= 1
        client2.disconnect()

    def test_join_nonexistent_room(self, guest_client):
        guest_client.emit('join_room', {'room_id': 'nonexist'})
        received = guest_client.get_received()
        error_events = [r for r in received if r['name'] == 'error']
        assert len(error_events) >= 1

    def test_join_full_room(self, app, socketio_app, registered_client):
        # Create 2-player room
        registered_client.emit('create_room', {'name': 'FullRoom', 'max_players': 2})
        received = registered_client.get_received()
        code_events = [r for r in received if r['name'] == 'room_code']
        code = code_events[0]['args'][0]['code']

        # Player 2 joins
        c2 = socketio_app.test_client(app)
        c2.emit('set_name', {'name': 'P2', 'is_guest': True, 'user_id': None})
        c2.get_received()
        c2.emit('join_room', {'code': code})
        c2.get_received()

        # Player 3 tries to join
        c3 = socketio_app.test_client(app)
        c3.emit('set_name', {'name': 'P3', 'is_guest': True, 'user_id': None})
        c3.get_received()
        c3.emit('join_room', {'code': code})
        received3 = c3.get_received()
        error_events = [r for r in received3 if r['name'] == 'error']
        assert len(error_events) >= 1
        assert 'megtelt' in error_events[0]['args'][0]['message']
        c2.disconnect()
        c3.disconnect()


class TestLeaveRoom:
    def test_leave_room(self, app, socketio_app, registered_client):
        registered_client.emit('create_room', {'name': 'LeaveTest', 'max_players': 4})
        registered_client.get_received()

        registered_client.emit('leave_room')
        received = registered_client.get_received()
        left_events = [r for r in received if r['name'] == 'room_left']
        assert len(left_events) >= 1

        # Room should be deleted (no players left)
        import server
        assert len(server.rooms) == 0

    def test_leave_room_transfers_ownership(self, app, socketio_app, registered_client):
        registered_client.emit('create_room', {'name': 'OwnerTest', 'max_players': 4})
        received = registered_client.get_received()
        code_events = [r for r in received if r['name'] == 'room_code']
        code = code_events[0]['args'][0]['code']

        # Player 2 joins
        c2 = socketio_app.test_client(app)
        c2.emit('set_name', {'name': 'P2Owner', 'is_guest': True, 'user_id': None})
        c2.get_received()
        c2.emit('join_room', {'code': code})
        c2.get_received()

        # Owner leaves
        registered_client.emit('leave_room')
        registered_client.get_received()

        # Room should still exist with new owner
        import server
        assert len(server.rooms) == 1
        room = list(server.rooms.values())[0]
        assert room.owner_name == 'P2Owner'
        c2.disconnect()

    def test_leave_cleans_join_code(self, registered_client):
        registered_client.emit('create_room', {'name': 'CodeClean', 'max_players': 4})
        registered_client.get_received()

        import server
        assert len(server.join_codes) == 1

        registered_client.emit('leave_room')
        registered_client.get_received()

        assert len(server.join_codes) == 0


class TestStartGame:
    def test_start_game(self, registered_client):
        registered_client.emit('create_room', {'name': 'StartTest', 'max_players': 4})
        registered_client.get_received()

        registered_client.emit('start_game')
        received = registered_client.get_received()
        started_events = [r for r in received if r['name'] == 'game_started']
        assert len(started_events) >= 1

    def test_non_owner_cannot_start(self, app, socketio_app, registered_client):
        registered_client.emit('create_room', {'name': 'NoStart', 'max_players': 4})
        received = registered_client.get_received()
        code_events = [r for r in received if r['name'] == 'room_code']
        code = code_events[0]['args'][0]['code']

        c2 = socketio_app.test_client(app)
        c2.emit('set_name', {'name': 'P2', 'is_guest': True, 'user_id': None})
        c2.get_received()
        c2.emit('join_room', {'code': code})
        c2.get_received()

        c2.emit('start_game')
        received2 = c2.get_received()
        error_events = [r for r in received2 if r['name'] == 'error']
        assert len(error_events) >= 1
        assert 'tulajdonosa' in error_events[0]['args'][0]['message']
        c2.disconnect()


class TestRoomCodeVisibility:
    def test_code_only_sent_to_owner(self, app, socketio_app, registered_client):
        registered_client.emit('create_room', {'name': 'CodeVis', 'max_players': 4})
        owner_received = registered_client.get_received()
        code_events = [r for r in owner_received if r['name'] == 'room_code']
        assert len(code_events) == 1
        code = code_events[0]['args'][0]['code']

        # Joiner should NOT receive room_code
        c2 = socketio_app.test_client(app)
        c2.emit('set_name', {'name': 'Joiner', 'is_guest': True, 'user_id': None})
        c2.get_received()
        c2.emit('join_room', {'code': code})
        joiner_received = c2.get_received()
        joiner_code_events = [r for r in joiner_received if r['name'] == 'room_code']
        assert len(joiner_code_events) == 0
        c2.disconnect()

    def test_rooms_list_does_not_contain_code(self, registered_client):
        registered_client.emit('create_room', {'name': 'NoCodeList', 'max_players': 4})
        registered_client.get_received()

        registered_client.emit('get_rooms')
        received = registered_client.get_received()
        rooms_events = [r for r in received if r['name'] == 'rooms_list']
        assert len(rooms_events) >= 1
        rooms = rooms_events[0]['args'][0]
        for room in rooms:
            assert 'join_code' not in room
            assert 'code' not in room


class TestChallengeMode:
    def test_create_room_with_challenge_mode(self, registered_client):
        registered_client.emit('create_room', {
            'name': 'ChallengeRoom', 'max_players': 4, 'challenge_mode': True
        })
        received = registered_client.get_received()
        join_events = [r for r in received if r['name'] == 'room_joined']
        assert len(join_events) >= 1
        assert join_events[0]['args'][0]['challenge_mode'] is True

    def test_create_room_without_challenge_mode(self, registered_client):
        registered_client.emit('create_room', {'name': 'NormalRoom', 'max_players': 4})
        received = registered_client.get_received()
        join_events = [r for r in received if r['name'] == 'room_joined']
        assert join_events[0]['args'][0]['challenge_mode'] is False

    def test_rooms_list_shows_challenge_mode(self, registered_client):
        registered_client.emit('create_room', {
            'name': 'ChalRoom', 'max_players': 4, 'challenge_mode': True
        })
        registered_client.get_received()

        registered_client.emit('get_rooms')
        received = registered_client.get_received()
        rooms_events = [r for r in received if r['name'] == 'rooms_list']
        rooms = rooms_events[0]['args'][0]
        assert any(r['challenge_mode'] for r in rooms)

    def test_join_room_shows_challenge_mode(self, app, socketio_app, registered_client):
        registered_client.emit('create_room', {
            'name': 'ChalJoin', 'max_players': 4, 'challenge_mode': True
        })
        received = registered_client.get_received()
        code_events = [r for r in received if r['name'] == 'room_code']
        code = code_events[0]['args'][0]['code']

        c2 = socketio_app.test_client(app)
        c2.emit('set_name', {'name': 'P2', 'is_guest': True, 'user_id': None})
        c2.get_received()
        c2.emit('join_room', {'code': code})
        received2 = c2.get_received()
        join_events = [r for r in received2 if r['name'] == 'room_joined']
        assert join_events[0]['args'][0]['challenge_mode'] is True
        c2.disconnect()

    def test_accept_words_no_pending(self, registered_client):
        registered_client.emit('create_room', {
            'name': 'AccTest', 'max_players': 4, 'challenge_mode': True
        })
        registered_client.get_received()
        registered_client.emit('start_game')
        registered_client.get_received()

        registered_client.emit('accept_words')
        received = registered_client.get_received()
        action_events = [r for r in received if r['name'] == 'action_result']
        assert len(action_events) >= 1
        assert action_events[0]['args'][0]['success'] is False

    def test_reject_words_no_pending(self, registered_client):
        registered_client.emit('create_room', {
            'name': 'RejTest', 'max_players': 4, 'challenge_mode': True
        })
        registered_client.get_received()
        registered_client.emit('start_game')
        registered_client.get_received()

        registered_client.emit('reject_words')
        received = registered_client.get_received()
        action_events = [r for r in received if r['name'] == 'action_result']
        assert len(action_events) >= 1
        assert action_events[0]['args'][0]['success'] is False


class TestPrivateRoom:
    def test_create_private_room(self, registered_client):
        registered_client.emit('create_room', {
            'name': 'PrivateRoom', 'max_players': 4, 'is_private': True
        })
        received = registered_client.get_received()
        join_events = [r for r in received if r['name'] == 'room_joined']
        assert len(join_events) >= 1
        assert join_events[0]['args'][0]['is_private'] is True

    def test_create_public_room_default(self, registered_client):
        registered_client.emit('create_room', {'name': 'PublicRoom', 'max_players': 4})
        received = registered_client.get_received()
        join_events = [r for r in received if r['name'] == 'room_joined']
        assert join_events[0]['args'][0]['is_private'] is False

    def test_private_room_not_in_rooms_list(self, registered_client):
        registered_client.emit('create_room', {
            'name': 'HiddenRoom', 'max_players': 4, 'is_private': True
        })
        registered_client.get_received()

        registered_client.emit('get_rooms')
        received = registered_client.get_received()
        rooms_events = [r for r in received if r['name'] == 'rooms_list']
        assert len(rooms_events) >= 1
        rooms = rooms_events[0]['args'][0]
        assert len(rooms) == 0  # privát szoba nem jelenik meg

    def test_public_room_in_rooms_list(self, registered_client):
        registered_client.emit('create_room', {
            'name': 'VisibleRoom', 'max_players': 4, 'is_private': False
        })
        registered_client.get_received()

        registered_client.emit('get_rooms')
        received = registered_client.get_received()
        rooms_events = [r for r in received if r['name'] == 'rooms_list']
        rooms = rooms_events[0]['args'][0]
        assert len(rooms) == 1
        assert rooms[0]['name'] == 'VisibleRoom'

    def test_join_private_room_by_code(self, app, socketio_app, registered_client):
        registered_client.emit('create_room', {
            'name': 'PrivJoin', 'max_players': 4, 'is_private': True
        })
        received = registered_client.get_received()
        code_events = [r for r in received if r['name'] == 'room_code']
        code = code_events[0]['args'][0]['code']

        c2 = socketio_app.test_client(app)
        c2.emit('set_name', {'name': 'Joiner', 'is_guest': True, 'user_id': None})
        c2.get_received()
        c2.emit('join_room', {'code': code})
        received2 = c2.get_received()
        join_events = [r for r in received2 if r['name'] == 'room_joined']
        assert len(join_events) >= 1
        assert join_events[0]['args'][0]['room_name'] == 'PrivJoin'
        assert join_events[0]['args'][0]['is_private'] is True
        c2.disconnect()

    def test_join_private_room_by_id_blocked(self, app, socketio_app, registered_client):
        registered_client.emit('create_room', {
            'name': 'PrivBlock', 'max_players': 4, 'is_private': True
        })
        received = registered_client.get_received()
        join_events = [r for r in received if r['name'] == 'room_joined']
        room_id = join_events[0]['args'][0]['room_id']

        c2 = socketio_app.test_client(app)
        c2.emit('set_name', {'name': 'Blocked', 'is_guest': True, 'user_id': None})
        c2.get_received()
        c2.emit('join_room', {'room_id': room_id})
        received2 = c2.get_received()
        error_events = [r for r in received2 if r['name'] == 'error']
        assert len(error_events) >= 1
        assert 'privát' in error_events[0]['args'][0]['message'].lower()
        c2.disconnect()

    def test_join_public_room_by_id_allowed(self, app, socketio_app, registered_client):
        registered_client.emit('create_room', {
            'name': 'PubJoin', 'max_players': 4, 'is_private': False
        })
        received = registered_client.get_received()
        join_events = [r for r in received if r['name'] == 'room_joined']
        room_id = join_events[0]['args'][0]['room_id']

        c2 = socketio_app.test_client(app)
        c2.emit('set_name', {'name': 'PubJoiner', 'is_guest': True, 'user_id': None})
        c2.get_received()
        c2.emit('join_room', {'room_id': room_id})
        received2 = c2.get_received()
        join_events2 = [r for r in received2 if r['name'] == 'room_joined']
        assert len(join_events2) >= 1
        assert join_events2[0]['args'][0]['is_private'] is False
        c2.disconnect()


class TestChat:
    def test_send_chat_in_room(self, app, socketio_app, registered_client):
        registered_client.emit('create_room', {'name': 'ChatRoom', 'max_players': 4})
        registered_client.get_received()

        registered_client.emit('send_chat', {'message': 'Hello!'})
        received = registered_client.get_received()
        chat_events = [r for r in received if r['name'] == 'chat_message']
        assert len(chat_events) >= 1
        assert chat_events[0]['args'][0]['name'] == 'RegUser'
        assert chat_events[0]['args'][0]['message'] == 'Hello!'

    def test_send_chat_to_all_in_room(self, app, socketio_app, registered_client):
        registered_client.emit('create_room', {'name': 'ChatAll', 'max_players': 4})
        received = registered_client.get_received()
        code_events = [r for r in received if r['name'] == 'room_code']
        code = code_events[0]['args'][0]['code']

        c2 = socketio_app.test_client(app)
        c2.emit('set_name', {'name': 'P2Chat', 'is_guest': True, 'user_id': None})
        c2.get_received()
        c2.emit('join_room', {'code': code})
        c2.get_received()

        registered_client.get_received()  # Clear join notifications

        registered_client.emit('send_chat', {'message': 'Hi all!'})
        registered_client.get_received()

        received2 = c2.get_received()
        chat_events = [r for r in received2 if r['name'] == 'chat_message']
        assert len(chat_events) >= 1
        assert chat_events[0]['args'][0]['message'] == 'Hi all!'
        c2.disconnect()

    def test_send_chat_empty_message(self, registered_client):
        registered_client.emit('create_room', {'name': 'ChatEmpty', 'max_players': 4})
        registered_client.get_received()

        registered_client.emit('send_chat', {'message': ''})
        received = registered_client.get_received()
        chat_events = [r for r in received if r['name'] == 'chat_message']
        assert len(chat_events) == 0

    def test_send_chat_whitespace_only(self, registered_client):
        registered_client.emit('create_room', {'name': 'ChatWS', 'max_players': 4})
        registered_client.get_received()

        registered_client.emit('send_chat', {'message': '   '})
        received = registered_client.get_received()
        chat_events = [r for r in received if r['name'] == 'chat_message']
        assert len(chat_events) == 0

    def test_send_chat_too_long(self, registered_client):
        registered_client.emit('create_room', {'name': 'ChatLong', 'max_players': 4})
        registered_client.get_received()

        registered_client.emit('send_chat', {'message': 'x' * 201})
        received = registered_client.get_received()
        chat_events = [r for r in received if r['name'] == 'chat_message']
        assert len(chat_events) == 0

    def test_send_chat_not_in_room(self, registered_client):
        registered_client.emit('send_chat', {'message': 'Lost!'})
        received = registered_client.get_received()
        chat_events = [r for r in received if r['name'] == 'chat_message']
        assert len(chat_events) == 0

    def test_chat_stored_in_room(self, registered_client):
        registered_client.emit('create_room', {'name': 'ChatStore', 'max_players': 4})
        registered_client.get_received()

        registered_client.emit('send_chat', {'message': 'Msg1'})
        registered_client.emit('send_chat', {'message': 'Msg2'})
        registered_client.get_received()

        import server
        room_id = list(server.rooms.keys())[0]
        room = server.rooms[room_id]
        assert len(room.chat_messages) == 2
        assert room.chat_messages[0]['message'] == 'Msg1'

    def test_chat_cleaned_on_room_delete(self, registered_client):
        registered_client.emit('create_room', {'name': 'ChatClean', 'max_players': 4})
        registered_client.get_received()

        registered_client.emit('send_chat', {'message': 'Test'})
        registered_client.get_received()

        import server
        assert len(server.rooms) == 1

        registered_client.emit('leave_room')
        registered_client.get_received()

        assert len(server.rooms) == 0


class TestGameActions:
    """Socket.IO game actions: place_tiles, exchange_tiles, pass_turn."""

    def _create_started_2p_game(self, app, socketio_app):
        """Helper: create and start a 2-player game, return (c1, c2, code)."""
        import server
        c1 = socketio_app.test_client(app)
        c1.emit('set_name', {'name': 'Player1', 'is_guest': True, 'user_id': None})
        c1.get_received()

        c1.emit('create_room', {'name': 'GameRoom', 'max_players': 4})
        received = c1.get_received()
        code_events = [r for r in received if r['name'] == 'room_code']
        code = code_events[0]['args'][0]['code']

        c2 = socketio_app.test_client(app)
        c2.emit('set_name', {'name': 'Player2', 'is_guest': True, 'user_id': None})
        c2.get_received()
        c2.emit('join_room', {'code': code})
        c2.get_received()

        c1.get_received()  # Clear join notification

        c1.emit('start_game')
        c1.get_received()
        c2.get_received()

        return c1, c2, code

    @patch('board.check_words', return_value=(True, []))
    def test_place_tiles_success(self, mock_check, app, socketio_app):
        c1, c2, _ = self._create_started_2p_game(app, socketio_app)

        import server
        # Find the room and set up player hand
        room = list(server.rooms.values())[0]
        room.game.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']

        c1.emit('place_tiles', {
            'tiles': [
                {'row': 7, 'col': 6, 'letter': 'A', 'is_blank': False},
                {'row': 7, 'col': 7, 'letter': 'B', 'is_blank': False},
            ]
        })
        received = c1.get_received()
        action_events = [r for r in received if r['name'] == 'action_result']
        assert len(action_events) >= 1
        assert action_events[0]['args'][0]['success'] is True

        c1.disconnect()
        c2.disconnect()

    def test_place_tiles_wrong_turn(self, app, socketio_app):
        c1, c2, _ = self._create_started_2p_game(app, socketio_app)

        c2.emit('place_tiles', {
            'tiles': [
                {'row': 7, 'col': 7, 'letter': 'A', 'is_blank': False},
            ]
        })
        received = c2.get_received()
        action_events = [r for r in received if r['name'] == 'action_result']
        assert len(action_events) >= 1
        assert action_events[0]['args'][0]['success'] is False

        c1.disconnect()
        c2.disconnect()

    def test_place_tiles_empty(self, app, socketio_app):
        c1, c2, _ = self._create_started_2p_game(app, socketio_app)

        c1.emit('place_tiles', {'tiles': []})
        received = c1.get_received()
        action_events = [r for r in received if r['name'] == 'action_result']
        assert len(action_events) >= 1
        assert action_events[0]['args'][0]['success'] is False

        c1.disconnect()
        c2.disconnect()

    def test_exchange_tiles_success(self, app, socketio_app):
        c1, c2, _ = self._create_started_2p_game(app, socketio_app)

        c1.emit('exchange_tiles', {'indices': [0, 1]})
        received = c1.get_received()
        action_events = [r for r in received if r['name'] == 'action_result']
        assert len(action_events) >= 1
        assert action_events[0]['args'][0]['success'] is True

        c1.disconnect()
        c2.disconnect()

    def test_exchange_tiles_wrong_turn(self, app, socketio_app):
        c1, c2, _ = self._create_started_2p_game(app, socketio_app)

        c2.emit('exchange_tiles', {'indices': [0]})
        received = c2.get_received()
        action_events = [r for r in received if r['name'] == 'action_result']
        assert len(action_events) >= 1
        assert action_events[0]['args'][0]['success'] is False

        c1.disconnect()
        c2.disconnect()

    def test_pass_turn_success(self, app, socketio_app):
        c1, c2, _ = self._create_started_2p_game(app, socketio_app)

        c1.emit('pass_turn')
        received = c1.get_received()
        action_events = [r for r in received if r['name'] == 'action_result']
        assert len(action_events) >= 1
        assert action_events[0]['args'][0]['success'] is True

        c1.disconnect()
        c2.disconnect()

    def test_pass_turn_wrong_turn(self, app, socketio_app):
        c1, c2, _ = self._create_started_2p_game(app, socketio_app)

        c2.emit('pass_turn')
        received = c2.get_received()
        action_events = [r for r in received if r['name'] == 'action_result']
        assert len(action_events) >= 1
        assert action_events[0]['args'][0]['success'] is False

        c1.disconnect()
        c2.disconnect()

    def test_place_tiles_not_in_room(self, app, socketio_app):
        """Player not in a room should get no response (silently ignored)."""
        c = socketio_app.test_client(app)
        c.emit('set_name', {'name': 'LostPlayer', 'is_guest': True, 'user_id': None})
        c.get_received()

        c.emit('place_tiles', {'tiles': [{'row': 7, 'col': 7, 'letter': 'A', 'is_blank': False}]})
        received = c.get_received()
        # Server silently returns when player is not in a room
        action_events = [r for r in received if r['name'] == 'action_result']
        assert len(action_events) == 0
        c.disconnect()

    def test_exchange_tiles_invalid_indices(self, app, socketio_app):
        c1, c2, _ = self._create_started_2p_game(app, socketio_app)

        c1.emit('exchange_tiles', {'indices': [99]})
        received = c1.get_received()
        action_events = [r for r in received if r['name'] == 'action_result']
        assert len(action_events) >= 1
        assert action_events[0]['args'][0]['success'] is False

        c1.disconnect()
        c2.disconnect()

    @patch('board.check_words', return_value=(True, []))
    def test_game_state_broadcast_after_place(self, mock_check, app, socketio_app):
        """After placing tiles, game_state should be broadcast to all players."""
        c1, c2, _ = self._create_started_2p_game(app, socketio_app)

        import server
        room = list(server.rooms.values())[0]
        room.game.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']

        c1.emit('place_tiles', {
            'tiles': [
                {'row': 7, 'col': 6, 'letter': 'A', 'is_blank': False},
                {'row': 7, 'col': 7, 'letter': 'B', 'is_blank': False},
            ]
        })
        c1.get_received()

        received2 = c2.get_received()
        state_events = [r for r in received2 if r['name'] == 'game_state']
        assert len(state_events) >= 1

        c1.disconnect()
        c2.disconnect()


class TestOwnerLeaveDisband:
    """When the owner leaves an active game, all players should be kicked."""

    def test_owner_leave_active_game_disbands_room(self, app, socketio_app, registered_client):
        """Owner leaving during active game should disband the room."""
        # Create room and have P2 join
        registered_client.emit('create_room', {'name': 'DisbandTest', 'max_players': 4})
        received = registered_client.get_received()
        code_events = [r for r in received if r['name'] == 'room_code']
        code = code_events[0]['args'][0]['code']

        c2 = socketio_app.test_client(app)
        c2.emit('set_name', {'name': 'P2Disband', 'is_guest': True, 'user_id': None})
        c2.get_received()
        c2.emit('join_room', {'code': code})
        c2.get_received()

        # Start game
        registered_client.emit('start_game')
        registered_client.get_received()
        c2.get_received()

        import server
        assert len(server.rooms) == 1

        # Owner leaves
        registered_client.emit('leave_room')
        registered_client.get_received()

        # Room should be disbanded
        assert len(server.rooms) == 0

        # P2 should receive room_disbanded and room_left
        p2_received = c2.get_received()
        disbanded_events = [r for r in p2_received if r['name'] == 'room_disbanded']
        assert len(disbanded_events) >= 1
        assert 'tulajdonosa kilépett' in disbanded_events[0]['args'][0]['message']

        c2.disconnect()

    def test_non_owner_leave_active_game_does_not_disband(self, app, socketio_app, registered_client):
        """Non-owner leaving during active game should NOT disband the room."""
        registered_client.emit('create_room', {'name': 'NoDisband', 'max_players': 4})
        received = registered_client.get_received()
        code_events = [r for r in received if r['name'] == 'room_code']
        code = code_events[0]['args'][0]['code']

        c2 = socketio_app.test_client(app)
        c2.emit('set_name', {'name': 'P2NoDis', 'is_guest': True, 'user_id': None})
        c2.get_received()
        c2.emit('join_room', {'code': code})
        c2.get_received()

        # Start game
        registered_client.emit('start_game')
        registered_client.get_received()
        c2.get_received()

        import server
        assert len(server.rooms) == 1

        # Non-owner (P2) leaves
        c2.emit('leave_room')
        c2.get_received()

        # Room should still exist
        assert len(server.rooms) == 1

        registered_client.disconnect()

    def test_owner_leave_waiting_room_transfers_ownership(self, app, socketio_app, registered_client):
        """Owner leaving a waiting room (not started game) should transfer ownership, not disband."""
        registered_client.emit('create_room', {'name': 'TransferTest', 'max_players': 4})
        received = registered_client.get_received()
        code_events = [r for r in received if r['name'] == 'room_code']
        code = code_events[0]['args'][0]['code']

        c2 = socketio_app.test_client(app)
        c2.emit('set_name', {'name': 'P2Transfer', 'is_guest': True, 'user_id': None})
        c2.get_received()
        c2.emit('join_room', {'code': code})
        c2.get_received()

        import server
        assert len(server.rooms) == 1

        # Owner leaves BEFORE starting game
        registered_client.emit('leave_room')
        registered_client.get_received()

        # Room should still exist with transferred ownership
        assert len(server.rooms) == 1
        room = list(server.rooms.values())[0]
        assert room.owner_name == 'P2Transfer'

        c2.disconnect()

    def test_new_owner_receives_room_code_after_transfer(self, app, socketio_app, registered_client):
        """After ownership transfer the new owner must receive room_code.

        This is the server-side contract that the frontend relies on to set
        AppState.isOwner = true and reveal the start-game / invite buttons.
        Regression test for: new leader buttons not visible after transfer.
        """
        registered_client.emit('create_room', {'name': 'CodeTransfer', 'max_players': 4})
        received = registered_client.get_received()
        code = next(r for r in received if r['name'] == 'room_code')['args'][0]['code']

        c2 = socketio_app.test_client(app)
        c2.emit('set_name', {'name': 'NewLeader', 'is_guest': True, 'user_id': None})
        c2.get_received()
        c2.emit('join_room', {'code': code})
        c2.get_received()  # clear join events

        # Original owner leaves → triggers _transfer_ownership on the server
        registered_client.emit('leave_room')
        registered_client.get_received()

        # New owner MUST receive room_code so the frontend can set isOwner=True
        c2_received = c2.get_received()
        code_events = [r for r in c2_received if r['name'] == 'room_code']
        assert len(code_events) == 1, (
            "New owner did not receive room_code after ownership transfer; "
            "frontend would never set AppState.isOwner=true and buttons stay hidden"
        )

        c2.disconnect()

    def test_new_owner_can_start_game_after_transfer(self, app, socketio_app, registered_client):
        """After ownership transfer the new owner must be able to start the game.

        Verifies the full ownership transfer: server state + permission check.
        Regression test for: new leader buttons not visible after transfer.
        """
        registered_client.emit('create_room', {'name': 'StartTransfer', 'max_players': 4})
        received = registered_client.get_received()
        code = next(r for r in received if r['name'] == 'room_code')['args'][0]['code']

        c2 = socketio_app.test_client(app)
        c2.emit('set_name', {'name': 'NewLeaderStart', 'is_guest': True, 'user_id': None})
        c2.get_received()
        c2.emit('join_room', {'code': code})
        c2.get_received()

        # Original owner leaves
        registered_client.emit('leave_room')
        registered_client.get_received()
        c2.get_received()  # clear transfer events

        # New owner starts the game — must succeed, not return an error
        c2.emit('start_game')
        c2_received = c2.get_received()

        error_events = [r for r in c2_received if r['name'] == 'error']
        assert len(error_events) == 0, (
            f"New owner got an error when trying to start the game: "
            f"{error_events[0]['args'][0]['message'] if error_events else ''}"
        )
        started_events = [r for r in c2_received if r['name'] == 'game_started']
        assert len(started_events) >= 1, "game_started event not received by new owner"

        c2.disconnect()


class TestTurnTimeLimit:
    """Turn time limit feature tests."""

    def test_create_room_with_turn_time_limit(self, registered_client):
        import server
        registered_client.emit('create_room', {
            'name': 'TimedRoom', 'max_players': 2,
            'turn_time_limit': 120,
        })
        msgs = registered_client.get_received()
        joined = next(m for m in msgs if m['name'] == 'room_joined')
        assert joined['args'][0]['turn_time_limit'] == 120

    def test_create_room_invalid_turn_time_limit_defaults_zero(self, registered_client):
        import server
        registered_client.emit('create_room', {
            'name': 'BadTimer', 'max_players': 2,
            'turn_time_limit': 999,
        })
        msgs = registered_client.get_received()
        joined = next(m for m in msgs if m['name'] == 'room_joined')
        assert joined['args'][0]['turn_time_limit'] == 0

    def test_create_room_no_turn_time_limit_defaults_zero(self, registered_client):
        import server
        registered_client.emit('create_room', {
            'name': 'NoTimer', 'max_players': 2,
        })
        msgs = registered_client.get_received()
        joined = next(m for m in msgs if m['name'] == 'room_joined')
        assert joined['args'][0]['turn_time_limit'] == 0

    def test_game_state_has_turn_timer_fields(self, app, socketio_app):
        import server
        c1 = socketio_app.test_client(app)
        c1.emit('set_name', {'name': 'P1', 'is_guest': False, 'user_id': 1})
        c1.get_received()
        c1.emit('create_room', {
            'name': 'TimedRoom', 'max_players': 2,
            'turn_time_limit': 60,
        })
        c1.get_received()

        c2 = socketio_app.test_client(app)
        c2.emit('set_name', {'name': 'P2', 'is_guest': True})
        c2.get_received()
        room = list(server.rooms.values())[0]
        c2.emit('join_room', {'code': room.join_code})
        c2.get_received()

        c1.emit('start_game')
        msgs = c1.get_received()
        gs_msgs = [m for m in msgs if m['name'] == 'game_state']
        assert len(gs_msgs) > 0
        gs = gs_msgs[-1]['args'][0]
        assert gs['turn_time_limit'] == 60
        assert 'turn_timer_expires_at' in gs

        c1.disconnect()
        c2.disconnect()

    def test_lobby_list_includes_turn_time_limit(self, registered_client):
        import server
        registered_client.emit('create_room', {
            'name': 'TimedRoom', 'max_players': 2,
            'turn_time_limit': 180,
        })
        msgs = registered_client.get_received()
        rooms_list = [m for m in msgs if m['name'] == 'rooms_list']
        assert len(rooms_list) > 0
        rooms = rooms_list[-1]['args'][0]
        assert any(r['turn_time_limit'] == 180 for r in rooms)

    def test_turn_timer_invalidation(self):
        """Room turn timer invalidation works correctly."""
        from room import Room
        from unittest.mock import MagicMock
        game = MagicMock()
        room = Room('r1', game, 'owner', 'Owner', 'Test', 4, '123456', owner_token='token123')
        assert room.turn_timer_id == 0
        assert room.turn_timer_expires_at is None

        tid1 = room.invalidate_turn_timer()
        assert tid1 == 1
        assert room.turn_timer_expires_at is None

        room.turn_timer_expires_at = 12345.0
        tid2 = room.invalidate_turn_timer()
        assert tid2 == 2
        assert room.turn_timer_expires_at is None
