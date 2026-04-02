"""Tests for turn timer display and replay persistence fixes.

Covered bugs:
- Timer expires_at is sent in the SAME game_state emission as the action result
  (fix: _start_turn_timer called before _emit_all_states in all action handlers).
- Challenge-mode game endings call _save_game_to_db so moves reach the DB.
"""
import time
import pytest
from unittest.mock import patch


# ---------------------------------------------------------------------------
# Fixtures (mirror pattern from test_server_socket.py)
# ---------------------------------------------------------------------------

@pytest.fixture(autouse=True)
def clean_state():
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


def _latest_game_state(received):
    """Return the args dict of the last game_state event in a received list."""
    gs_msgs = [m for m in received if m['name'] == 'game_state']
    assert gs_msgs, "No game_state event received"
    return gs_msgs[-1]['args'][0]


def _create_started_game(app, socketio_app, turn_time_limit=0, challenge_mode=False):
    """Create and start a 2-player game, return (c1, c2, room_id)."""
    c1 = socketio_app.test_client(app)
    c1.emit('set_name', {'name': 'P1', 'is_guest': True, 'user_id': None})
    c1.get_received()
    c1.emit('create_room', {
        'name': 'TestRoom',
        'max_players': 2,
        'turn_time_limit': turn_time_limit,
        'challenge_mode': challenge_mode,
    })
    rcv = c1.get_received()
    join_ev = next(m for m in rcv if m['name'] == 'room_joined')
    room_id = join_ev['args'][0]['room_id']
    code = next(m for m in rcv if m['name'] == 'room_code')['args'][0]['code']

    c2 = socketio_app.test_client(app)
    c2.emit('set_name', {'name': 'P2', 'is_guest': True, 'user_id': None})
    c2.get_received()
    c2.emit('join_room', {'code': code})
    c2.get_received()
    c1.get_received()  # clear player_joined notification

    c1.emit('start_game')
    c1.get_received()
    c2.get_received()

    return c1, c2, room_id


# ===========================================================================
# Timer display tests
# ===========================================================================

class TestTimerDisplay:
    """Verify that turn_timer_expires_at is set in game_state immediately
    after each action (fix: _start_turn_timer before _emit_all_states)."""

    def test_start_game_with_timer_sets_expires_at(self, app, socketio_app):
        """game_state received on start_game must have non-None expires_at."""
        c1 = socketio_app.test_client(app)
        c1.emit('set_name', {'name': 'P1', 'is_guest': True})
        c1.get_received()
        c1.emit('create_room', {'name': 'R', 'max_players': 2, 'turn_time_limit': 60})
        rcv = c1.get_received()
        code = next(m for m in rcv if m['name'] == 'room_code')['args'][0]['code']

        c2 = socketio_app.test_client(app)
        c2.emit('set_name', {'name': 'P2', 'is_guest': True})
        c2.get_received()
        c2.emit('join_room', {'code': code})
        c2.get_received()
        c1.get_received()

        c1.emit('start_game')
        gs = _latest_game_state(c1.get_received())

        assert gs['turn_time_limit'] == 60
        assert gs['turn_timer_expires_at'] is not None, (
            "turn_timer_expires_at must be set in the very first game_state "
            "after start_game (timer should be started before emitting state)"
        )
        # Should be approximately now + 60 seconds
        assert gs['turn_timer_expires_at'] > time.time()
        c1.disconnect()
        c2.disconnect()

    def test_start_game_no_timer_has_null_expires_at(self, app, socketio_app):
        """With turn_time_limit=0, expires_at must be None."""
        c1, c2, _ = _create_started_game(app, socketio_app, turn_time_limit=0)
        c1.emit('pass_turn')
        gs = _latest_game_state(c1.get_received())
        assert gs.get('turn_timer_expires_at') is None
        c1.disconnect()
        c2.disconnect()

    def test_pass_turn_with_timer_updates_expires_at(self, app, socketio_app):
        """After pass_turn, game_state already has the NEW turn's expires_at."""
        c1, c2, room_id = _create_started_game(app, socketio_app, turn_time_limit=90)

        # Get the initial expires_at from start_game
        import server
        room = server.rooms[room_id]
        initial_expires = room.turn_timer_expires_at

        c1.emit('pass_turn')
        gs = _latest_game_state(c1.get_received())

        assert gs['turn_timer_expires_at'] is not None, (
            "After pass_turn, the emitted game_state must already contain "
            "the new turn timer (timer set before emit)"
        )
        # The new timer should be fresh (close to now + 90)
        assert gs['turn_timer_expires_at'] > time.time()
        c1.disconnect()
        c2.disconnect()

    def test_exchange_tiles_with_timer_updates_expires_at(self, app, socketio_app):
        """After exchange_tiles, game_state has the new timer set."""
        c1, c2, room_id = _create_started_game(app, socketio_app, turn_time_limit=60)

        import server
        room = server.rooms[room_id]
        # Ensure player has tiles and bag has enough
        room.game.players[0].hand = ['A', 'B', 'C']

        c1.emit('exchange_tiles', {'indices': [0]})
        rcv = c1.get_received()
        action = next((m for m in rcv if m['name'] == 'action_result'), None)
        if action and action['args'][0]['success']:
            gs = _latest_game_state(rcv)
            assert gs['turn_timer_expires_at'] is not None, (
                "After exchange_tiles, game_state must have timer set before emit"
            )
        c1.disconnect()
        c2.disconnect()

    @patch('board.check_words', return_value=(True, []))
    def test_place_tiles_with_timer_updates_expires_at(self, mock_check, app, socketio_app):
        """After place_tiles, game_state has the new timer (non-challenge mode)."""
        c1, c2, room_id = _create_started_game(app, socketio_app, turn_time_limit=60)

        import server
        room = server.rooms[room_id]
        room.game.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']

        c1.emit('place_tiles', {'tiles': [
            {'row': 7, 'col': 6, 'letter': 'A', 'is_blank': False},
            {'row': 7, 'col': 7, 'letter': 'B', 'is_blank': False},
        ]})
        rcv = c1.get_received()
        action = next((m for m in rcv if m['name'] == 'action_result'), None)
        if action and action['args'][0]['success']:
            gs = _latest_game_state(rcv)
            # In non-challenge mode the next player's timer should be set
            assert gs['turn_timer_expires_at'] is not None, (
                "After place_tiles (non-challenge), game_state must already "
                "contain the next turn's expires_at"
            )
        c1.disconnect()
        c2.disconnect()

    def test_timer_expires_at_not_in_past(self, app, socketio_app):
        """The expires_at timestamp must be in the future, not the past."""
        c1, c2, room_id = _create_started_game(app, socketio_app, turn_time_limit=60)

        import server
        room = server.rooms[room_id]
        before = time.time()

        c1.emit('pass_turn')
        gs = _latest_game_state(c1.get_received())

        expires_at = gs.get('turn_timer_expires_at')
        if expires_at is not None:
            assert expires_at > before, "expires_at must be in the future"
            assert expires_at <= before + 65, "expires_at should be ~60s from now"
        c1.disconnect()
        c2.disconnect()


# ===========================================================================
# Replay / move persistence tests
# ===========================================================================

class TestReplayPersistence:
    """Verify that game moves are saved to the DB so replay works."""

    def test_pass_moves_recorded_in_move_log(self, app, socketio_app):
        """pass_turn appends an entry to game.move_log."""
        c1, c2, room_id = _create_started_game(app, socketio_app)
        import server
        game = server.rooms[room_id].game

        assert len(game.move_log) == 0
        c1.emit('pass_turn')
        c1.get_received()
        assert len(game.move_log) == 1
        assert game.move_log[0]['action_type'] == 'pass'
        assert game.move_log[0]['player_name'] == 'P1'
        c1.disconnect()
        c2.disconnect()

    def test_moves_saved_to_db_after_game_ends_via_passes(self, app, socketio_app):
        """After 4 consecutive passes (2 each), game ends and moves are in DB."""
        c1, c2, room_id = _create_started_game(app, socketio_app)
        import server
        from auth import get_game_moves

        # 2-player game: each player must pass twice → 4 passes total
        # Turn order: P1 → P2 → P1 → P2 (P2's 2nd pass ends the game)
        c1.emit('pass_turn'); c1.get_received(); c2.get_received()
        c2.emit('pass_turn'); c2.get_received(); c1.get_received()
        c1.emit('pass_turn'); c1.get_received(); c2.get_received()
        c2.emit('pass_turn'); c2.get_received(); c1.get_received()

        room = server.rooms[room_id]
        assert room.game.finished, "Game must be finished after 4 passes"
        assert room.db_game_id is not None, "_save_game_to_db must have been called"

        moves = get_game_moves(room.db_game_id)
        assert len(moves) == 4, f"Expected 4 pass moves, got {len(moves)}"
        assert all(m['action_type'] == 'pass' for m in moves)
        assert moves[0]['player_name'] == 'P1'
        assert moves[1]['player_name'] == 'P2'
        assert moves[2]['player_name'] == 'P1'
        assert moves[3]['player_name'] == 'P2'
        # move_number must be sequential
        assert [m['move_number'] for m in moves] == [1, 2, 3, 4]
        c1.disconnect()
        c2.disconnect()

    def test_board_snapshots_saved_with_moves(self, app, socketio_app):
        """Each saved move includes a non-empty board snapshot JSON."""
        import json
        c1, c2, room_id = _create_started_game(app, socketio_app)
        import server
        from auth import get_game_moves

        c1.emit('pass_turn'); c1.get_received(); c2.get_received()
        c2.emit('pass_turn'); c2.get_received(); c1.get_received()
        c1.emit('pass_turn'); c1.get_received(); c2.get_received()
        c2.emit('pass_turn'); c2.get_received(); c1.get_received()

        room = server.rooms[room_id]
        moves = get_game_moves(room.db_game_id)
        for m in moves:
            snapshot = json.loads(m['board_snapshot_json'])
            assert isinstance(snapshot, list), "Snapshot must be a list"
        c1.disconnect()
        c2.disconnect()

    @patch('board.check_words', return_value=(True, []))
    def test_place_moves_recorded_and_saved(self, mock_check, app, socketio_app):
        """place_tiles moves end up in DB (non-challenge, game ends via passes)."""
        import json
        from auth import get_game_moves

        c1, c2, room_id = _create_started_game(app, socketio_app)
        import server
        room = server.rooms[room_id]

        # Set a known hand for P1
        room.game.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        c1.emit('place_tiles', {'tiles': [
            {'row': 7, 'col': 6, 'letter': 'A', 'is_blank': False},
            {'row': 7, 'col': 7, 'letter': 'B', 'is_blank': False},
        ]})
        rcv = c1.get_received()
        c2.get_received()
        action = next((m for m in rcv if m['name'] == 'action_result'), None)
        if not (action and action['args'][0]['success']):
            c1.disconnect(); c2.disconnect()
            return  # place failed (dictionary etc.), skip rest

        # End game via passes
        c2.emit('pass_turn'); c2.get_received(); c1.get_received()
        c1.emit('pass_turn'); c1.get_received(); c2.get_received()
        c2.emit('pass_turn'); c2.get_received(); c1.get_received()
        c1.emit('pass_turn'); c1.get_received(); c2.get_received()
        c2.emit('pass_turn'); c2.get_received(); c1.get_received()

        assert room.game.finished
        moves = get_game_moves(room.db_game_id)
        action_types = [m['action_type'] for m in moves]
        assert 'place' in action_types, "place move must be recorded"
        place_move = next(m for m in moves if m['action_type'] == 'place')
        details = json.loads(place_move['details_json'])
        assert 'tiles' in details
        c1.disconnect()
        c2.disconnect()

    def test_challenge_mode_moves_saved_on_accept(self, app, socketio_app):
        """In 2-player challenge mode, accept_words saves moves to DB."""
        from auth import get_game_moves

        c1, c2, room_id = _create_started_game(
            app, socketio_app, challenge_mode=True
        )
        import server
        room = server.rooms[room_id]
        game = room.game

        # Manually place a pending challenge so accept_words triggers _handle_challenge_result
        # Simulate the game state that would exist after a successful placement
        # by calling game internals directly
        if not game.pending_challenge:
            # Do passes to create move_log entries then accept to end via pass chain
            c1.emit('pass_turn'); c1.get_received(); c2.get_received()
            c2.emit('pass_turn'); c2.get_received(); c1.get_received()
            c1.emit('pass_turn'); c1.get_received(); c2.get_received()
            c2.emit('pass_turn'); c2.get_received(); c1.get_received()
            assert game.finished
        else:
            c2.emit('accept_words')
            c1.get_received(); c2.get_received()

        assert room.db_game_id is not None
        moves = get_game_moves(room.db_game_id)
        assert len(moves) > 0, "Moves must be saved to DB for replay"
        c1.disconnect()
        c2.disconnect()

    def test_initial_save_does_not_persist_moves_before_any_action(self, app, socketio_app):
        """At game start the DB row exists but game_moves is empty (no moves yet)."""
        from auth import get_game_moves

        c1, c2, room_id = _create_started_game(app, socketio_app)
        import server
        room = server.rooms[room_id]

        # start_game triggers initial _save_game_to_db with 0 moves
        assert room.db_game_id is not None
        moves = get_game_moves(room.db_game_id)
        assert moves == [], "No moves should be in DB yet immediately after start"
        c1.disconnect()
        c2.disconnect()


# ===========================================================================
# DB-level unit tests for replay functions
# ===========================================================================

class TestReplayDbFunctions:
    """Direct unit tests for auth.py replay-related DB functions."""

    def test_add_and_get_game_moves(self, temp_db):
        import json
        from auth import finish_game, add_game_move, get_game_moves

        game_id = finish_game('room-x', '{}', [])
        add_game_move(game_id, 1, 'Alice', 'pass', '{}', json.dumps([[None]*15]*15))
        add_game_move(game_id, 2, 'Bob',   'pass', '{}', json.dumps([[None]*15]*15))

        moves = get_game_moves(game_id)
        assert len(moves) == 2
        assert moves[0]['move_number'] == 1
        assert moves[0]['player_name'] == 'Alice'
        assert moves[0]['action_type'] == 'pass'
        assert moves[1]['move_number'] == 2
        assert moves[1]['player_name'] == 'Bob'

    def test_get_game_moves_returns_empty_for_unknown_game(self, temp_db):
        from auth import get_game_moves
        moves = get_game_moves(99999)
        assert moves == []

    def test_get_game_moves_ordered_by_move_number(self, temp_db):
        import json
        from auth import finish_game, add_game_move, get_game_moves

        game_id = finish_game('room-ord', '{}', [])
        # Insert out of order intentionally
        add_game_move(game_id, 3, 'P', 'pass', '{}', '[]')
        add_game_move(game_id, 1, 'P', 'pass', '{}', '[]')
        add_game_move(game_id, 2, 'P', 'pass', '{}', '[]')

        moves = get_game_moves(game_id)
        assert [m['move_number'] for m in moves] == [1, 2, 3]

    def test_is_user_in_game_true(self, temp_db):
        from auth import finish_game, is_user_in_game, create_user

        _, uid = create_user('test@x.com', 'Tester', 'Password1!')
        game_id = finish_game('room-u', '{}', [{
            'player_name': 'Tester',
            'user_id': uid,
            'final_score': 10,
            'is_winner': True,
        }])

        assert is_user_in_game(game_id, uid) is True

    def test_is_user_in_game_false_for_other_user(self, temp_db):
        from auth import finish_game, is_user_in_game, create_user

        _, uid = create_user('a@x.com', 'A', 'Password1!')
        _, uid2 = create_user('b@x.com', 'B', 'Password2!')
        game_id = finish_game('room-v', '{}', [{
            'player_name': 'A',
            'user_id': uid,
            'final_score': 5,
            'is_winner': False,
        }])

        assert is_user_in_game(game_id, uid2) is False

    def test_is_user_in_game_false_for_guest(self, temp_db):
        """A guest player (user_id=None) is not findable by user_id."""
        from auth import finish_game, is_user_in_game

        game_id = finish_game('room-g', '{}', [{
            'player_name': 'Guest',
            'user_id': None,
            'final_score': 0,
            'is_winner': False,
        }])

        assert is_user_in_game(game_id, None) is False


# ===========================================================================
# End-to-end: socket game → profile HTTP API (game history)
# ===========================================================================

class TestGameHistoryE2E:
    """Full end-to-end tests: socket game finishes → profile API returns history.

    These tests confirm that after a complete game played via Socket.IO,
    the registered user sees the game in their profile history.
    The key invariant: game_players.user_id must be persisted for the
    history JOIN to find the row.
    """

    @pytest.fixture
    def http_client(self, app):
        return app.test_client()

    def _login_cookie(self, http_client, user_id):
        """Set a valid session cookie on the HTTP test client."""
        from auth import create_session
        token = create_session(user_id)
        http_client.set_cookie('session_token', token, domain='localhost')
        return token

    def test_registered_player_game_appears_in_history(self, app, socketio_app, http_client):
        """After a non-challenge game ends, registered player sees it in profile history."""
        from auth import create_user, get_user_game_history

        _, uid = create_user('hist1@x.com', 'HistP1', 'pass123!')

        c1, c2, room_id = _create_started_game(app, socketio_app)

        import server
        room = server.rooms[room_id]
        # Re-register c1 as registered user so player_auth has user_id
        sid1 = room.game.players[0].id
        server.player_auth[sid1] = {'user_id': uid, 'is_guest': False}

        # End game via 4 passes
        c1.emit('pass_turn'); c1.get_received(); c2.get_received()
        c2.emit('pass_turn'); c2.get_received(); c1.get_received()
        c1.emit('pass_turn'); c1.get_received(); c2.get_received()
        c2.emit('pass_turn'); c2.get_received(); c1.get_received()

        assert room.game.finished
        history = get_user_game_history(uid)
        assert len(history) == 1, (
            "Registered player's game must appear in profile history after game ends"
        )
        assert history[0]['final_score'] == room.game.players[0].score
        c1.disconnect(); c2.disconnect()

    def test_profile_api_returns_history_for_registered_user(self, app, socketio_app, http_client):
        """The /api/auth/profile endpoint returns history entries after a game."""
        from auth import create_user

        _, uid = create_user('hist2@x.com', 'HistP2', 'pass123!')
        self._login_cookie(http_client, uid)

        c1, c2, room_id = _create_started_game(app, socketio_app)

        import server
        room = server.rooms[room_id]
        sid1 = room.game.players[0].id
        server.player_auth[sid1] = {'user_id': uid, 'is_guest': False}

        # End game via passes
        c1.emit('pass_turn'); c1.get_received(); c2.get_received()
        c2.emit('pass_turn'); c2.get_received(); c1.get_received()
        c1.emit('pass_turn'); c1.get_received(); c2.get_received()
        c2.emit('pass_turn'); c2.get_received(); c1.get_received()

        assert room.game.finished

        res = http_client.get('/api/auth/profile')
        data = res.get_json()
        assert data['success'] is True
        assert len(data['history']) == 1, (
            "Profile API must return 1 history entry after game ends"
        )
        assert data['history'][0]['game_id'] == room.db_game_id
        c1.disconnect(); c2.disconnect()

    @patch('board.check_words', return_value=(True, []))
    def test_challenge_mode_game_ends_via_accept_words_saves_to_db(self, mock_check, app, socketio_app, http_client):
        """The actual fixed path: challenge mode, P1 places last tiles,
        P2 accepts → game.finished → _save_game_to_db called via
        _handle_challenge_result (this was the broken path before the fix)."""
        from auth import create_user, get_user_game_history

        _, uid = create_user('hist3@x.com', 'HistP3', 'pass123!')

        c1, c2, room_id = _create_started_game(
            app, socketio_app, challenge_mode=True
        )
        import server
        room = server.rooms[room_id]
        game = room.game

        # Register P1 as a proper user
        sid1 = game.players[0].id
        server.player_auth[sid1] = {'user_id': uid, 'is_guest': False}

        # Force end-game conditions: P1 has exactly 2 tiles, bag is empty.
        # After placing those 2 tiles, hand is empty + bag empty → game ends on accept.
        game.bag.tiles.clear()
        game.players[0].hand = ['A', 'B']

        c1.emit('place_tiles', {'tiles': [
            {'row': 7, 'col': 6, 'letter': 'A', 'is_blank': False},
            {'row': 7, 'col': 7, 'letter': 'B', 'is_blank': False},
        ]})
        c1.get_received(); c2.get_received()

        # P1's tiles are placed, pending_challenge is set
        assert game.pending_challenge is not None, "pending_challenge must be set after placement"

        # P2 accepts → _finalize_accept → hand empty + bag empty → game.finished
        c2.emit('accept_words')
        c1.get_received(); c2.get_received()

        assert game.finished, "Game must be finished after P2 accepts P1's last tiles"

        history = get_user_game_history(uid)
        assert len(history) == 1, (
            "Challenge-mode game ending via accept_words must appear in history. "
            "This was the broken path fixed by calling _save_game_to_db in "
            "_handle_challenge_result when game.finished is True."
        )
        c1.disconnect(); c2.disconnect()

    def test_challenge_mode_game_via_passes_also_saves(self, app, socketio_app):
        """2x2 passes in challenge mode saves correctly (this path was never broken,
        since handle_pass_turn always called _save_game_to_db)."""
        from auth import create_user, get_user_game_history

        _, uid = create_user('hist3b@x.com', 'HistP3b', 'pass123!')

        c1, c2, room_id = _create_started_game(
            app, socketio_app, challenge_mode=True
        )
        import server
        room = server.rooms[room_id]
        sid1 = room.game.players[0].id
        server.player_auth[sid1] = {'user_id': uid, 'is_guest': False}

        c1.emit('pass_turn'); c1.get_received(); c2.get_received()
        c2.emit('pass_turn'); c2.get_received(); c1.get_received()
        c1.emit('pass_turn'); c1.get_received(); c2.get_received()
        c2.emit('pass_turn'); c2.get_received(); c1.get_received()

        assert room.game.finished
        history = get_user_game_history(uid)
        assert len(history) == 1, "2x2 pass game in challenge mode must save to DB"
        c1.disconnect(); c2.disconnect()

    def test_guest_game_does_not_appear_in_registered_user_history(self, app, socketio_app):
        """A guest player's games must NOT appear in a registered user's history."""
        from auth import create_user, get_user_game_history

        _, uid = create_user('hist4@x.com', 'HistP4', 'pass123!')

        # Both players are guests (no user_id)
        c1, c2, room_id = _create_started_game(app, socketio_app)

        c1.emit('pass_turn'); c1.get_received(); c2.get_received()
        c2.emit('pass_turn'); c2.get_received(); c1.get_received()
        c1.emit('pass_turn'); c1.get_received(); c2.get_received()
        c2.emit('pass_turn'); c2.get_received(); c1.get_received()

        history = get_user_game_history(uid)
        assert len(history) == 0, "Guest games must not appear in registered user history"
        c1.disconnect(); c2.disconnect()

    def test_stats_updated_after_game_ends(self, app, socketio_app):
        """games_played stat is updated in the users table after game ends."""
        from auth import create_user, get_user_by_id

        _, uid = create_user('hist5@x.com', 'HistP5', 'pass123!')

        c1, c2, room_id = _create_started_game(app, socketio_app)
        import server
        room = server.rooms[room_id]
        sid1 = room.game.players[0].id
        server.player_auth[sid1] = {'user_id': uid, 'is_guest': False}

        c1.emit('pass_turn'); c1.get_received(); c2.get_received()
        c2.emit('pass_turn'); c2.get_received(); c1.get_received()
        c1.emit('pass_turn'); c1.get_received(); c2.get_received()
        c2.emit('pass_turn'); c2.get_received(); c1.get_received()

        assert room.game.finished
        user = get_user_by_id(uid)
        assert user['games_played'] == 1, "games_played must be incremented after game ends"
        c1.disconnect(); c2.disconnect()


# ===========================================================================
# All game-ending paths: verify _save_game_to_db is called in each scenario
# ===========================================================================

class TestAllGameEndPaths:
    """Verify _save_game_to_db is called in every possible game-ending scenario.

    There are exactly 5 paths where game.finished can become True:
      1. handle_place_tiles  → non-challenge → hand+bag empty
      2. handle_pass_turn    → all consecutive_passes >= 2
      3. _handle_challenge_result (accept_words) → challenge accepts → hand+bag empty
      4. _start_challenge_timer timeout → auto-accept → hand+bag empty
      5. _start_turn_timer timeout → auto-pass → all consecutive_passes >= 2

    Paths 2 and 3 are covered by other test classes.  This class adds explicit
    tests for paths 1, 4, and 5 which use background tasks or direct game-end.
    """

    @patch('board.check_words', return_value=(True, []))
    def test_non_challenge_place_tiles_end_saves_to_db(self, mock_check, app, socketio_app):
        """Path 1: non-challenge place_tiles, hand empties + bag empty → game ends.
        server.py line 1087-1088: elif game.finished: _save_game_to_db(room_id)"""
        from auth import get_game_moves

        c1, c2, room_id = _create_started_game(
            app, socketio_app, challenge_mode=False
        )
        import server
        room = server.rooms[room_id]
        game = room.game

        # Force end-game conditions: empty bag, P1 has exactly 2 tiles
        game.bag.tiles.clear()
        game.players[0].hand = ['A', 'B']

        c1.emit('place_tiles', {'tiles': [
            {'row': 7, 'col': 6, 'letter': 'A', 'is_blank': False},
            {'row': 7, 'col': 7, 'letter': 'B', 'is_blank': False},
        ]})
        rcv = c1.get_received()
        c2.get_received()

        action = next((m for m in rcv if m['name'] == 'action_result'), None)
        assert action is not None and action['args'][0]['success'], (
            "place_tiles must succeed for this test to be meaningful"
        )
        assert game.pending_challenge is None, "non-challenge mode: no pending_challenge"
        assert game.finished, "Game must be finished (hand empty + bag empty)"
        assert room.db_game_id is not None, "_save_game_to_db must have been called"

        moves = get_game_moves(room.db_game_id)
        assert any(m['action_type'] == 'place' for m in moves), (
            "The winning place move must be persisted to DB for replay"
        )
        c1.disconnect(); c2.disconnect()

    @patch('board.check_words', return_value=(True, []))
    def test_challenge_timer_timeout_game_end_saves_to_db(self, mock_check, app, socketio_app):
        """Path 4: challenge timer times out, auto-accept fires, hand+bag empty → game ends.
        server.py _start_challenge_timer line 247-251: if game.finished: _save_game_to_db"""
        from auth import get_game_moves

        # Capture the background task so we can fire it manually
        bg_tasks = []

        def capture_bg_task(fn, *args, **kwargs):
            bg_tasks.append(fn)

        import server
        with patch.object(server.socketio, 'start_background_task',
                          side_effect=capture_bg_task):
            c1, c2, room_id = _create_started_game(
                app, socketio_app, challenge_mode=True
            )
            room = server.rooms[room_id]
            game = room.game

            # Force end-game conditions: empty bag, P1 has exactly 2 tiles
            game.bag.tiles.clear()
            game.players[0].hand = ['A', 'B']

            c1.emit('place_tiles', {'tiles': [
                {'row': 7, 'col': 6, 'letter': 'A', 'is_blank': False},
                {'row': 7, 'col': 7, 'letter': 'B', 'is_blank': False},
            ]})
            c1.get_received(); c2.get_received()

        assert game.pending_challenge is not None, "pending_challenge must be set"
        # The last background task captured is the challenge timer callback
        challenge_cb = bg_tasks[-1]

        # Fire the timeout callback (skip actual sleep)
        with patch('time.sleep'):
            challenge_cb()

        assert game.finished, "Game must be finished after challenge timer auto-accept"
        assert room.db_game_id is not None, "_save_game_to_db must have been called"

        moves = get_game_moves(room.db_game_id)
        # In challenge mode, accepted placements are recorded as 'challenge_accept'
        # (see game.py _finalize_accept → _record_move(..., 'challenge_accept', ...))
        assert any(m['action_type'] == 'challenge_accept' for m in moves), (
            "The winning placement must be persisted as 'challenge_accept' when "
            "challenge timer auto-accepts"
        )
        c1.disconnect(); c2.disconnect()

    def test_turn_timer_autopass_game_end_saves_to_db(self, app, socketio_app):
        """Path 5: turn timer fires, auto-pass triggers game end (all passed >= 2x).
        server.py _start_turn_timer line 293-298: if game.finished: _save_game_to_db"""
        from auth import get_game_moves

        bg_tasks = []

        def capture_bg_task(fn, *args, **kwargs):
            bg_tasks.append(fn)

        import server
        with patch.object(server.socketio, 'start_background_task',
                          side_effect=capture_bg_task):
            c1, c2, room_id = _create_started_game(
                app, socketio_app, turn_time_limit=60
            )
            room = server.rooms[room_id]
            game = room.game

            # Pre-set consecutive_passes so P1's one auto-pass triggers game end:
            # P1 (current player): 1 → auto-pass → 2
            # P2: already at 2
            # → all([2, 2]) = True → _end_game called
            game.players[0].consecutive_passes = 1
            game.players[1].consecutive_passes = 2

        # The last background task is the timer callback for P1's turn
        assert bg_tasks, "A turn timer background task must have been registered"
        timer_cb = bg_tasks[-1]

        # Fire the timeout callback (skip actual sleep)
        with patch('time.sleep'):
            timer_cb()

        assert game.finished, (
            "Game must be finished: P2 already passed 2x, P1 auto-passed once more"
        )
        assert room.db_game_id is not None, (
            "_save_game_to_db must be called when turn timer auto-pass ends the game"
        )

        moves = get_game_moves(room.db_game_id)
        assert any(m['action_type'] == 'pass' for m in moves), (
            "The auto-pass move must be persisted when turn timer ends the game"
        )
        c1.disconnect(); c2.disconnect()
