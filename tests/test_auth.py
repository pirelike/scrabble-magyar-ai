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
