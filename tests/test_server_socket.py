"""Integration tests for server.py Socket.IO events - lobby, rooms, join codes."""
import os
import sys
import pytest
from unittest.mock import patch


@pytest.fixture(autouse=True)
def temp_db(monkeypatch, tmp_path):
    db_path = str(tmp_path / 'test.db')
    monkeypatch.setattr('config.DB_PATH', db_path)
    import auth
    monkeypatch.setattr(auth, 'DB_PATH', db_path)
    auth.init_db()
    yield db_path


@pytest.fixture(autouse=True)
def clean_state():
    """Clean server state between tests."""
    import server
    server.rooms.clear()
    server.join_codes.clear()
    server.player_rooms.clear()
    server.player_names.clear()
    server.player_auth.clear()
    yield
    server.rooms.clear()
    server.join_codes.clear()
    server.player_rooms.clear()
    server.player_names.clear()
    server.player_auth.clear()


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
            if room['name'] == 'Room':
                assert room['max_players'] == 4
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
        assert room['owner_name'] == 'P2Owner'
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
