"""Tests for auth.py - DB, user CRUD, sessions, verification codes."""
import os
import sys
import tempfile
import pytest

# Use a temporary DB for each test
@pytest.fixture(autouse=True)
def temp_db(monkeypatch, tmp_path):
    db_path = str(tmp_path / 'test.db')
    monkeypatch.setattr('config.DB_PATH', db_path)
    # Re-import auth module to pick up new DB_PATH
    import auth
    monkeypatch.setattr(auth, 'DB_PATH', db_path)
    auth.init_db()
    yield db_path


class TestInitDB:
    def test_creates_tables(self):
        import auth
        conn = auth.get_db()
        tables = conn.execute(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
        ).fetchall()
        table_names = [t['name'] for t in tables]
        assert 'users' in table_names
        assert 'verification_codes' in table_names
        assert 'sessions' in table_names
        conn.close()

    def test_idempotent(self):
        import auth
        # Calling init_db twice should not raise
        auth.init_db()
        auth.init_db()


class TestUserCRUD:
    def test_create_user_success(self):
        import auth
        success, user_id = auth.create_user('test@example.com', 'TestUser', 'password123')
        assert success is True
        assert isinstance(user_id, int)

    def test_create_user_duplicate_email(self):
        import auth
        auth.create_user('test@example.com', 'TestUser', 'password123')
        success, msg = auth.create_user('test@example.com', 'TestUser2', 'password456')
        assert success is False
        assert 'már regisztrálva' in msg

    def test_create_user_case_insensitive_email(self):
        import auth
        auth.create_user('Test@Example.COM', 'User1', 'pass1')
        success, msg = auth.create_user('test@example.com', 'User2', 'pass2')
        assert success is False

    def test_get_user_by_email(self):
        import auth
        auth.create_user('test@example.com', 'TestUser', 'password123')
        user = auth.get_user_by_email('test@example.com')
        assert user is not None
        assert user['display_name'] == 'TestUser'
        assert user['email'] == 'test@example.com'

    def test_get_user_by_email_case_insensitive(self):
        import auth
        auth.create_user('test@example.com', 'TestUser', 'password123')
        user = auth.get_user_by_email('TEST@EXAMPLE.COM')
        assert user is not None

    def test_get_user_by_email_not_found(self):
        import auth
        user = auth.get_user_by_email('nonexistent@example.com')
        assert user is None

    def test_get_user_by_id(self):
        import auth
        success, user_id = auth.create_user('test@example.com', 'TestUser', 'password123')
        user = auth.get_user_by_id(user_id)
        assert user is not None
        assert user['display_name'] == 'TestUser'

    def test_get_user_by_id_not_found(self):
        import auth
        user = auth.get_user_by_id(99999)
        assert user is None

    def test_user_default_stats(self):
        import auth
        success, user_id = auth.create_user('test@example.com', 'TestUser', 'password123')
        user = auth.get_user_by_id(user_id)
        assert user['games_played'] == 0
        assert user['games_won'] == 0
        assert user['total_score'] == 0


class TestPasswordVerification:
    def test_verify_password_success(self):
        import auth
        auth.create_user('test@example.com', 'TestUser', 'MyPassword123')
        success, user = auth.verify_password('test@example.com', 'MyPassword123')
        assert success is True
        assert user['display_name'] == 'TestUser'

    def test_verify_password_wrong_password(self):
        import auth
        auth.create_user('test@example.com', 'TestUser', 'MyPassword123')
        success, msg = auth.verify_password('test@example.com', 'WrongPassword')
        assert success is False
        assert 'Hibás' in msg

    def test_verify_password_nonexistent_user(self):
        import auth
        success, msg = auth.verify_password('nonexistent@example.com', 'pass')
        assert success is False
        assert 'Hibás' in msg

    def test_password_hash_is_not_plaintext(self):
        import auth
        auth.create_user('test@example.com', 'TestUser', 'MyPassword123')
        user = auth.get_user_by_email('test@example.com')
        assert user['password_hash'] != 'MyPassword123'
        assert 'pbkdf2' in user['password_hash']


class TestVerificationCodes:
    def test_create_code(self):
        import auth
        code = auth.create_verification_code('test@example.com')
        assert len(code) == 6
        assert code.isdigit()

    def test_verify_code_success(self):
        import auth
        code = auth.create_verification_code('test@example.com')
        success, msg = auth.verify_code('test@example.com', code)
        assert success is True

    def test_verify_code_wrong_code(self):
        import auth
        auth.create_verification_code('test@example.com')
        success, msg = auth.verify_code('test@example.com', '000000')
        assert success is False
        assert 'Hibás' in msg or 'próbálkozás' in msg

    def test_verify_code_already_used(self):
        import auth
        code = auth.create_verification_code('test@example.com')
        auth.verify_code('test@example.com', code)
        success, msg = auth.verify_code('test@example.com', code)
        assert success is False

    def test_verify_code_max_attempts(self):
        import auth
        code = auth.create_verification_code('test@example.com')
        # Use up all attempts with wrong code
        for _ in range(5):
            auth.verify_code('test@example.com', '000000')
        # Even the correct code should fail now
        success, msg = auth.verify_code('test@example.com', code)
        assert success is False

    def test_verify_code_no_code_exists(self):
        import auth
        success, msg = auth.verify_code('nobody@example.com', '123456')
        assert success is False

    def test_new_code_invalidates_old(self):
        import auth
        code1 = auth.create_verification_code('test@example.com')
        code2 = auth.create_verification_code('test@example.com')
        # Old code should be invalidated
        success1, _ = auth.verify_code('test@example.com', code1)
        assert success1 is False
        # But we already used the verify attempt; let's create fresh
        code3 = auth.create_verification_code('test@example.com')
        success3, _ = auth.verify_code('test@example.com', code3)
        assert success3 is True

    def test_verify_code_case_insensitive_email(self):
        import auth
        code = auth.create_verification_code('Test@Example.com')
        success, msg = auth.verify_code('test@example.com', code)
        assert success is True


class TestSessions:
    def test_create_session(self):
        import auth
        success, user_id = auth.create_user('test@example.com', 'TestUser', 'pass123')
        token = auth.create_session(user_id)
        assert isinstance(token, str)
        assert len(token) > 40

    def test_validate_session_success(self):
        import auth
        success, user_id = auth.create_user('test@example.com', 'TestUser', 'pass123')
        token = auth.create_session(user_id)
        user = auth.validate_session(token)
        assert user is not None
        assert user['id'] == user_id
        assert user['display_name'] == 'TestUser'
        assert user['email'] == 'test@example.com'

    def test_validate_session_invalid_token(self):
        import auth
        user = auth.validate_session('invalid_token_xyz')
        assert user is None

    def test_validate_session_none_token(self):
        import auth
        user = auth.validate_session(None)
        assert user is None

    def test_validate_session_empty_token(self):
        import auth
        user = auth.validate_session('')
        assert user is None

    def test_delete_session(self):
        import auth
        success, user_id = auth.create_user('test@example.com', 'TestUser', 'pass123')
        token = auth.create_session(user_id)
        assert auth.validate_session(token) is not None
        auth.delete_session(token)
        assert auth.validate_session(token) is None

    def test_delete_session_none(self):
        import auth
        # Should not raise
        auth.delete_session(None)

    def test_multiple_sessions(self):
        import auth
        success, user_id = auth.create_user('test@example.com', 'TestUser', 'pass123')
        token1 = auth.create_session(user_id)
        token2 = auth.create_session(user_id)
        assert token1 != token2
        assert auth.validate_session(token1) is not None
        assert auth.validate_session(token2) is not None

    def test_session_returns_user_stats(self):
        import auth
        success, user_id = auth.create_user('test@example.com', 'TestUser', 'pass123')
        token = auth.create_session(user_id)
        user = auth.validate_session(token)
        assert 'games_played' in user
        assert 'games_won' in user
        assert 'total_score' in user


class TestCleanup:
    def test_cleanup_expired(self):
        import auth
        from datetime import datetime, timedelta

        success, user_id = auth.create_user('test@example.com', 'TestUser', 'pass123')
        token = auth.create_session(user_id)

        # Manually expire the session
        conn = auth.get_db()
        past = (datetime.utcnow() - timedelta(days=1)).strftime('%Y-%m-%d %H:%M:%S')
        conn.execute('UPDATE sessions SET expires_at = ?', (past,))
        conn.commit()
        conn.close()

        auth.cleanup_expired()
        assert auth.validate_session(token) is None


class TestSaveGame:
    def test_save_game_new(self):
        import auth
        game_id = auth.save_game('room-1', 'TestRoom', '{"state": 1}', False, owner_name='Alice')
        assert isinstance(game_id, int)
        game = auth.get_game_by_id(game_id)
        assert game is not None
        assert game['room_id'] == 'room-1'
        assert game['room_name'] == 'TestRoom'
        assert game['status'] == 'active'
        assert game['owner_name'] == 'Alice'

    def test_save_game_upsert(self):
        import auth
        game_id1 = auth.save_game('room-1', 'TestRoom', '{"state": 1}', False)
        game_id2 = auth.save_game('room-1', 'TestRoom', '{"state": 2}', False)
        assert game_id1 == game_id2
        game = auth.get_game_by_id(game_id1)
        assert game['state_json'] == '{"state": 2}'

    def test_save_game_with_players(self):
        import auth
        players = [
            {'player_name': 'Alice', 'user_id': None, 'score': 50},
            {'player_name': 'Bob', 'user_id': None, 'score': 30},
        ]
        game_id = auth.save_game('room-1', 'TestRoom', '{}', False, players_data=players)
        game_players = auth.get_game_players(game_id)
        assert len(game_players) == 2
        names = [p['player_name'] for p in game_players]
        assert 'Alice' in names
        assert 'Bob' in names

    def test_save_game_challenge_mode(self):
        import auth
        game_id = auth.save_game('room-1', 'TestRoom', '{}', True)
        game = auth.get_game_by_id(game_id)
        assert game['challenge_mode'] == 1

    def test_save_game_upsert_players(self):
        """Saving twice with same player should update score, not duplicate."""
        import auth
        players = [{'player_name': 'Alice', 'user_id': None, 'score': 50}]
        game_id = auth.save_game('room-1', 'Room', '{}', False, players_data=players)

        players2 = [{'player_name': 'Alice', 'user_id': None, 'score': 80}]
        auth.save_game('room-1', 'Room', '{}', False, players_data=players2)

        game_players = auth.get_game_players(game_id)
        assert len(game_players) == 1
        assert game_players[0]['final_score'] == 80


class TestFinishGame:
    def test_finish_game_creates_entry(self):
        import auth
        players = [
            {'player_name': 'Alice', 'user_id': None, 'final_score': 100, 'is_winner': True},
            {'player_name': 'Bob', 'user_id': None, 'final_score': 80, 'is_winner': False},
        ]
        game_id = auth.finish_game('room-1', '{}', players)
        game = auth.get_game_by_id(game_id)
        assert game['status'] == 'finished'

    def test_finish_game_updates_existing(self):
        import auth
        # First save as active
        game_id = auth.save_game('room-1', 'Room', '{"active": true}', False)
        # Then finish
        players = [
            {'player_name': 'Alice', 'user_id': None, 'final_score': 100, 'is_winner': True},
        ]
        finished_id = auth.finish_game('room-1', '{"finished": true}', players)
        assert game_id == finished_id
        game = auth.get_game_by_id(game_id)
        assert game['status'] == 'finished'

    def test_finish_game_updates_user_stats(self):
        import auth
        _, user_id = auth.create_user('alice@example.com', 'Alice', 'pass123')
        players = [
            {'player_name': 'Alice', 'user_id': user_id, 'final_score': 150, 'is_winner': True},
        ]
        auth.finish_game('room-1', '{}', players)
        user = auth.get_user_by_id(user_id)
        assert user['games_played'] == 1
        assert user['games_won'] == 1
        assert user['total_score'] == 150

    def test_finish_game_loser_stats(self):
        import auth
        _, user_id = auth.create_user('bob@example.com', 'Bob', 'pass123')
        players = [
            {'player_name': 'Bob', 'user_id': user_id, 'final_score': 80, 'is_winner': False},
        ]
        auth.finish_game('room-1', '{}', players)
        user = auth.get_user_by_id(user_id)
        assert user['games_played'] == 1
        assert user['games_won'] == 0
        assert user['total_score'] == 80

    def test_finish_game_upsert_players(self):
        """Finish should update existing player rows, not duplicate."""
        import auth
        # Save game with player first
        players_save = [{'player_name': 'Alice', 'user_id': None, 'score': 50}]
        game_id = auth.save_game('room-1', 'Room', '{}', False, players_data=players_save)

        # Finish with same player
        players_finish = [
            {'player_name': 'Alice', 'user_id': None, 'final_score': 100, 'is_winner': True},
        ]
        auth.finish_game('room-1', '{}', players_finish)

        game_players = auth.get_game_players(game_id)
        assert len(game_players) == 1
        assert game_players[0]['final_score'] == 100


class TestGameMoves:
    def test_add_and_get_moves(self):
        import auth
        game_id = auth.save_game('room-1', 'Room', '{}', False)
        auth.add_game_move(game_id, 1, 'Alice', 'place', '{"tiles": []}', '{}')
        auth.add_game_move(game_id, 2, 'Bob', 'pass', '{}', '{}')

        moves = auth.get_game_moves(game_id)
        assert len(moves) == 2
        assert moves[0]['move_number'] == 1
        assert moves[0]['player_name'] == 'Alice'
        assert moves[0]['action_type'] == 'place'
        assert moves[1]['move_number'] == 2
        assert moves[1]['player_name'] == 'Bob'

    def test_get_moves_empty(self):
        import auth
        game_id = auth.save_game('room-1', 'Room', '{}', False)
        moves = auth.get_game_moves(game_id)
        assert moves == []

    def test_moves_ordered_by_number(self):
        import auth
        game_id = auth.save_game('room-1', 'Room', '{}', False)
        auth.add_game_move(game_id, 3, 'C', 'pass', '{}', '{}')
        auth.add_game_move(game_id, 1, 'A', 'place', '{}', '{}')
        auth.add_game_move(game_id, 2, 'B', 'exchange', '{}', '{}')
        moves = auth.get_game_moves(game_id)
        assert [m['move_number'] for m in moves] == [1, 2, 3]


class TestLoadActiveGames:
    def test_load_active(self):
        import auth
        auth.save_game('room-1', 'Room1', '{}', False)
        auth.save_game('room-2', 'Room2', '{}', True)
        games = auth.load_active_games()
        assert len(games) == 2

    def test_load_active_excludes_finished(self):
        import auth
        auth.save_game('room-1', 'Room1', '{}', False)
        auth.finish_game('room-1', '{}', [])
        games = auth.load_active_games()
        assert len(games) == 0


class TestGetGameById:
    def test_existing(self):
        import auth
        game_id = auth.save_game('room-1', 'Room', '{}', False)
        game = auth.get_game_by_id(game_id)
        assert game is not None
        assert game['id'] == game_id

    def test_nonexistent(self):
        import auth
        assert auth.get_game_by_id(99999) is None


class TestGetUserGameHistory:
    def test_history(self):
        import auth
        _, uid = auth.create_user('test@example.com', 'Test', 'pass')
        players = [
            {'player_name': 'Test', 'user_id': uid, 'final_score': 100, 'is_winner': True},
            {'player_name': 'Bot', 'user_id': None, 'final_score': 50, 'is_winner': False},
        ]
        auth.finish_game('room-1', '{}', players)

        history = auth.get_user_game_history(uid)
        assert len(history) == 1
        assert history[0]['final_score'] == 100
        assert history[0]['is_winner'] == 1
        assert len(history[0]['opponents']) == 1
        assert history[0]['opponents'][0]['player_name'] == 'Bot'

    def test_history_empty(self):
        import auth
        _, uid = auth.create_user('test@example.com', 'Test', 'pass')
        assert auth.get_user_game_history(uid) == []

    def test_history_limit(self):
        import auth
        _, uid = auth.create_user('test@example.com', 'Test', 'pass')
        for i in range(25):
            players = [{'player_name': 'Test', 'user_id': uid, 'final_score': i, 'is_winner': False}]
            auth.finish_game(f'room-{i}', '{}', players)
        history = auth.get_user_game_history(uid, limit=10)
        assert len(history) == 10


class TestGetUserActiveGames:
    def test_active_games(self):
        import auth
        _, uid = auth.create_user('test@example.com', 'Test', 'pass')
        players = [{'player_name': 'Test', 'user_id': uid, 'score': 50}]
        auth.save_game('room-1', 'Room', '{}', False, players_data=players)
        active = auth.get_user_active_games(uid)
        assert len(active) == 1
        assert active[0]['player_name'] == 'Test'

    def test_active_excludes_finished(self):
        import auth
        _, uid = auth.create_user('test@example.com', 'Test', 'pass')
        players_save = [{'player_name': 'Test', 'user_id': uid, 'score': 50}]
        auth.save_game('room-1', 'Room', '{}', False, players_data=players_save)
        players_finish = [{'player_name': 'Test', 'user_id': uid, 'final_score': 50, 'is_winner': False}]
        auth.finish_game('room-1', '{}', players_finish)
        assert auth.get_user_active_games(uid) == []


class TestIsUserInGame:
    def test_user_in_game(self):
        import auth
        _, uid = auth.create_user('test@example.com', 'Test', 'pass')
        players = [{'player_name': 'Test', 'user_id': uid, 'score': 0}]
        game_id = auth.save_game('room-1', 'Room', '{}', False, players_data=players)
        assert auth.is_user_in_game(game_id, uid) is True

    def test_user_not_in_game(self):
        import auth
        _, uid = auth.create_user('test@example.com', 'Test', 'pass')
        game_id = auth.save_game('room-1', 'Room', '{}', False)
        assert auth.is_user_in_game(game_id, uid) is False


class TestGetGamePlayers:
    def test_get_players(self):
        import auth
        players = [
            {'player_name': 'Alice', 'user_id': None, 'score': 100},
            {'player_name': 'Bob', 'user_id': None, 'score': 80},
        ]
        game_id = auth.save_game('room-1', 'Room', '{}', False, players_data=players)
        result = auth.get_game_players(game_id)
        assert len(result) == 2

    def test_get_players_empty(self):
        import auth
        game_id = auth.save_game('room-1', 'Room', '{}', False)
        assert auth.get_game_players(game_id) == []


class TestAbandonGame:
    def test_abandon_by_room_id(self):
        import auth
        game_id = auth.save_game('room-1', 'Room', '{}', False)
        auth.abandon_game('room-1')
        game = auth.get_game_by_id(game_id)
        assert game['status'] == 'abandoned'

    def test_abandon_by_game_id(self):
        import auth
        game_id = auth.save_game('room-1', 'Room', '{}', False)
        auth.abandon_game_by_id(game_id)
        game = auth.get_game_by_id(game_id)
        assert game['status'] == 'abandoned'

    def test_abandon_only_active(self):
        """Abandoning a finished game should have no effect."""
        import auth
        players = [{'player_name': 'A', 'user_id': None, 'final_score': 0, 'is_winner': False}]
        game_id = auth.finish_game('room-1', '{}', players)
        auth.abandon_game('room-1')
        game = auth.get_game_by_id(game_id)
        assert game['status'] == 'finished'

    def test_abandon_nonexistent(self):
        """Abandoning a nonexistent room should not raise."""
        import auth
        auth.abandon_game('nonexistent')  # Should not raise
