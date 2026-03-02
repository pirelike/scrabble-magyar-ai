"""Integration tests for server.py auth HTTP routes."""
import os
import sys
import pytest
import json


@pytest.fixture(autouse=True)
def clear_rate_limits():
    """Clear IP rate limits between tests."""
    import server
    server._ip_rate_limits.clear()
    yield
    server._ip_rate_limits.clear()


@pytest.fixture
def app():
    from server import app
    app.config['TESTING'] = True
    return app


@pytest.fixture
def client(app):
    return app.test_client()


class TestRequestCode:
    def test_request_code_success(self, client):
        res = client.post('/api/auth/request-code',
                         json={'email': 'test@example.com'})
        data = res.get_json()
        assert res.status_code == 200
        assert data['success'] is True

    def test_request_code_invalid_email(self, client):
        res = client.post('/api/auth/request-code',
                         json={'email': 'not-an-email'})
        data = res.get_json()
        assert res.status_code == 400
        assert data['success'] is False

    def test_request_code_empty_email(self, client):
        res = client.post('/api/auth/request-code',
                         json={'email': ''})
        data = res.get_json()
        assert res.status_code == 400

    def test_request_code_missing_email(self, client):
        res = client.post('/api/auth/request-code',
                         json={})
        data = res.get_json()
        assert res.status_code == 400

    def test_request_code_no_body(self, client):
        res = client.post('/api/auth/request-code',
                         content_type='application/json')
        assert res.status_code == 400

    def test_request_code_already_registered(self, client):
        import auth
        auth.create_user('test@example.com', 'TestUser', 'password123')
        res = client.post('/api/auth/request-code',
                         json={'email': 'test@example.com'})
        data = res.get_json()
        assert res.status_code == 409
        assert data['success'] is False
        assert 'már regisztrálva' in data['message']


class TestVerifyCode:
    def test_verify_code_success(self, client):
        import auth
        code = auth.create_verification_code('test@example.com')
        res = client.post('/api/auth/verify-code',
                         json={'email': 'test@example.com', 'code': code})
        data = res.get_json()
        assert res.status_code == 200
        assert data['success'] is True

    def test_verify_code_wrong(self, client):
        import auth
        auth.create_verification_code('test@example.com')
        res = client.post('/api/auth/verify-code',
                         json={'email': 'test@example.com', 'code': '000000'})
        data = res.get_json()
        assert res.status_code == 400
        assert data['success'] is False

    def test_verify_code_invalid_format(self, client):
        res = client.post('/api/auth/verify-code',
                         json={'email': 'test@example.com', 'code': 'abc'})
        data = res.get_json()
        assert res.status_code == 400

    def test_verify_code_empty(self, client):
        res = client.post('/api/auth/verify-code',
                         json={'email': '', 'code': ''})
        assert res.status_code == 400


class TestRegister:
    def test_register_success(self, client):
        res = client.post('/api/auth/register',
                         json={
                             'email': 'test@example.com',
                             'password': 'password123',
                             'display_name': 'TestUser',
                         })
        data = res.get_json()
        assert res.status_code == 200
        assert data['success'] is True
        assert data['user']['display_name'] == 'TestUser'
        # Should set session cookie
        assert 'session_token' in _get_cookies(res)

    def test_register_missing_fields(self, client):
        res = client.post('/api/auth/register',
                         json={'email': 'test@example.com'})
        assert res.status_code == 400

    def test_register_short_password(self, client):
        res = client.post('/api/auth/register',
                         json={
                             'email': 'test@example.com',
                             'password': '12345',
                             'display_name': 'TestUser',
                         })
        data = res.get_json()
        assert res.status_code == 400
        assert 'legalább 6' in data['message']

    def test_register_long_password(self, client):
        res = client.post('/api/auth/register',
                         json={
                             'email': 'test2@example.com',
                             'password': 'x' * 129,
                             'display_name': 'TestUser',
                         })
        data = res.get_json()
        assert res.status_code == 400

    def test_register_invalid_email(self, client):
        res = client.post('/api/auth/register',
                         json={
                             'email': 'bad-email',
                             'password': 'password123',
                             'display_name': 'TestUser',
                         })
        assert res.status_code == 400

    def test_register_invalid_display_name(self, client):
        res = client.post('/api/auth/register',
                         json={
                             'email': 'test3@example.com',
                             'password': 'password123',
                             'display_name': '<script>alert(1)</script>',
                         })
        assert res.status_code == 400

    def test_register_duplicate_email(self, client):
        import auth
        auth.create_user('test@example.com', 'User1', 'password123')
        res = client.post('/api/auth/register',
                         json={
                             'email': 'test@example.com',
                             'password': 'password456',
                             'display_name': 'User2',
                         })
        assert res.status_code == 409


class TestLogin:
    def test_login_success(self, client):
        import auth
        auth.create_user('test@example.com', 'TestUser', 'password123')
        res = client.post('/api/auth/login',
                         json={'email': 'test@example.com', 'password': 'password123'})
        data = res.get_json()
        assert res.status_code == 200
        assert data['success'] is True
        assert 'session_token' in _get_cookies(res)

    def test_login_wrong_password(self, client):
        import auth
        auth.create_user('test@example.com', 'TestUser', 'password123')
        res = client.post('/api/auth/login',
                         json={'email': 'test@example.com', 'password': 'wrong'})
        data = res.get_json()
        assert res.status_code == 401
        assert data['success'] is False

    def test_login_nonexistent_user(self, client):
        res = client.post('/api/auth/login',
                         json={'email': 'nobody@example.com', 'password': 'pass'})
        assert res.status_code == 401

    def test_login_empty_fields(self, client):
        res = client.post('/api/auth/login',
                         json={'email': '', 'password': ''})
        assert res.status_code == 400


class TestLogout:
    def test_logout(self, client):
        res = client.post('/api/auth/logout')
        data = res.get_json()
        assert data['success'] is True

    def test_logout_no_session(self, client):
        res = client.post('/api/auth/logout')
        data = res.get_json()
        assert data['success'] is True


class TestMe:
    def test_me_with_session(self, client):
        # Create user and session directly, then set cookie manually
        import auth
        _, user_id = auth.create_user('test@example.com', 'TestUser', 'pass123')
        token = auth.create_session(user_id)
        client.set_cookie('session_token', token, domain='localhost')

        res = client.get('/api/auth/me')
        data = res.get_json()
        assert res.status_code == 200
        assert data['success'] is True
        assert data['user']['display_name'] == 'TestUser'

    def test_me_no_session(self, client):
        res = client.get('/api/auth/me')
        data = res.get_json()
        assert res.status_code == 401
        assert data['success'] is False

    def test_me_invalid_session(self, client):
        client.set_cookie('session_token', 'invalid_token', domain='localhost')
        res = client.get('/api/auth/me')
        assert res.status_code == 401


class TestFullFlow:
    def test_register_login_me_logout(self, client):
        """Full flow: register -> check session -> logout -> session gone."""
        import auth

        # 1. Register directly and create session
        _, user_id = auth.create_user('flow@example.com', 'FlowUser', 'password123')
        token = auth.create_session(user_id)
        client.set_cookie('session_token', token, domain='localhost')

        # 2. Check session
        res = client.get('/api/auth/me')
        data = res.get_json()
        assert data['success'] is True
        assert data['user']['display_name'] == 'FlowUser'

        # 3. Logout
        res = client.post('/api/auth/logout')
        assert res.get_json()['success'] is True

        # 4. Session should be gone (cookie is deleted by logout response)
        # But the test client may still send the old cookie; the server should have
        # deleted the session from DB so it should be invalid
        client.set_cookie('session_token', token, domain='localhost')
        res = client.get('/api/auth/me')
        assert res.status_code == 401

    def test_login_flow_with_cookie(self, client):
        """Login and verify cookie works for /me."""
        import auth
        auth.create_user('test@example.com', 'TestUser', 'password123')

        res = client.post('/api/auth/login',
                         json={'email': 'test@example.com', 'password': 'password123'})
        assert res.get_json()['success'] is True

        # The test client should now have the session cookie
        res = client.get('/api/auth/me')
        data = res.get_json()
        assert data['success'] is True
        assert data['user']['display_name'] == 'TestUser'


class TestProfile:
    def test_profile_success(self, client):
        import auth
        _, user_id = auth.create_user('test@example.com', 'TestUser', 'pass123')
        token = auth.create_session(user_id)
        client.set_cookie('session_token', token, domain='localhost')

        res = client.get('/api/auth/profile')
        data = res.get_json()
        assert res.status_code == 200
        assert data['success'] is True
        assert 'stats' in data
        assert data['stats']['games_played'] == 0
        assert data['stats']['win_rate'] == 0
        assert data['stats']['avg_score'] == 0
        assert 'history' in data

    def test_profile_with_games(self, client):
        import auth
        _, user_id = auth.create_user('test@example.com', 'TestUser', 'pass123')
        players = [
            {'player_name': 'TestUser', 'user_id': user_id, 'final_score': 100, 'is_winner': True},
            {'player_name': 'Bot', 'user_id': None, 'final_score': 50, 'is_winner': False},
        ]
        auth.finish_game('room-1', '{}', players)

        token = auth.create_session(user_id)
        client.set_cookie('session_token', token, domain='localhost')

        res = client.get('/api/auth/profile')
        data = res.get_json()
        assert data['stats']['games_played'] == 1
        assert data['stats']['games_won'] == 1
        assert data['stats']['win_rate'] == 100.0
        assert len(data['history']) == 1
        assert data['history'][0]['final_score'] == 100
        assert data['history'][0]['is_winner'] is True

    def test_profile_no_session(self, client):
        res = client.get('/api/auth/profile')
        assert res.status_code == 401


class TestGameMovesAPI:
    def test_get_moves_success(self, client):
        import auth
        game_id = auth.save_game('room-1', 'Room', '{}', False)
        auth.add_game_move(game_id, 1, 'Alice', 'place', '{"tiles":[]}', '{}')
        auth.add_game_move(game_id, 2, 'Bob', 'pass', '{}', '{}')

        res = client.get(f'/api/game/{game_id}/moves')
        data = res.get_json()
        assert res.status_code == 200
        assert data['success'] is True
        assert len(data['moves']) == 2
        assert data['moves'][0]['player_name'] == 'Alice'

    def test_get_moves_nonexistent(self, client):
        res = client.get('/api/game/99999/moves')
        data = res.get_json()
        assert res.status_code == 404
        assert data['success'] is False


class TestSavedGamesAPI:
    def test_saved_games_success(self, client):
        import auth
        _, user_id = auth.create_user('test@example.com', 'TestUser', 'pass123')
        players = [{'player_name': 'TestUser', 'user_id': user_id, 'score': 50}]
        auth.save_game('room-1', 'Room', '{}', False, players_data=players, owner_name='TestUser')
        token = auth.create_session(user_id)
        client.set_cookie('session_token', token, domain='localhost')

        res = client.get('/api/auth/saved-games')
        data = res.get_json()
        assert res.status_code == 200
        assert data['success'] is True
        assert len(data['games']) == 1
        assert data['games'][0]['room_name'] == 'Room'

    def test_saved_games_no_session(self, client):
        res = client.get('/api/auth/saved-games')
        assert res.status_code == 401

    def test_saved_games_empty(self, client):
        import auth
        _, user_id = auth.create_user('test@example.com', 'TestUser', 'pass123')
        token = auth.create_session(user_id)
        client.set_cookie('session_token', token, domain='localhost')

        res = client.get('/api/auth/saved-games')
        data = res.get_json()
        assert data['games'] == []


class TestAbandonGameAPI:
    def test_abandon_success(self, client):
        import auth
        _, user_id = auth.create_user('test@example.com', 'TestUser', 'pass123')
        players = [{'player_name': 'TestUser', 'user_id': user_id, 'score': 50}]
        game_id = auth.save_game('room-1', 'Room', '{}', False, players_data=players)
        token = auth.create_session(user_id)
        client.set_cookie('session_token', token, domain='localhost')

        res = client.post(f'/api/game/{game_id}/abandon')
        data = res.get_json()
        assert data['success'] is True

        game = auth.get_game_by_id(game_id)
        assert game['status'] == 'abandoned'

    def test_abandon_no_session(self, client):
        res = client.post('/api/game/1/abandon')
        assert res.status_code == 401

    def test_abandon_nonexistent(self, client):
        import auth
        _, user_id = auth.create_user('test@example.com', 'TestUser', 'pass123')
        token = auth.create_session(user_id)
        client.set_cookie('session_token', token, domain='localhost')

        res = client.post('/api/game/99999/abandon')
        assert res.status_code == 404

    def test_abandon_not_active(self, client):
        import auth
        _, user_id = auth.create_user('test@example.com', 'TestUser', 'pass123')
        players = [{'player_name': 'TestUser', 'user_id': user_id, 'final_score': 50, 'is_winner': False}]
        game_id = auth.finish_game('room-1', '{}', players)
        token = auth.create_session(user_id)
        client.set_cookie('session_token', token, domain='localhost')

        res = client.post(f'/api/game/{game_id}/abandon')
        assert res.status_code == 400

    def test_abandon_not_participant(self, client):
        import auth
        _, user_id = auth.create_user('test@example.com', 'TestUser', 'pass123')
        # Create game without this user
        game_id = auth.save_game('room-1', 'Room', '{}', False)
        token = auth.create_session(user_id)
        client.set_cookie('session_token', token, domain='localhost')

        res = client.post(f'/api/game/{game_id}/abandon')
        assert res.status_code == 403


class TestRateLimit:
    def test_check_ip_rate_limit_allows_within_limit(self):
        """Rate limit function allows requests within limit."""
        import time
        from server import _check_ip_rate_limit, _ip_rate_limits
        _ip_rate_limits.clear()
        ip = '10.0.0.1'
        # Pre-populate with 2 timestamps (limit is 3 for request_code)
        _ip_rate_limits[ip]['request_code'] = [time.time(), time.time()]
        result = _check_ip_rate_limit(ip, 'request_code')
        assert result is True

    def test_check_ip_rate_limit_blocks_over_limit(self):
        """Rate limit function blocks when limit exceeded."""
        import time
        from server import _check_ip_rate_limit, _ip_rate_limits
        _ip_rate_limits.clear()
        ip = '10.0.0.2'
        # Pre-populate at limit (3 for request_code)
        _ip_rate_limits[ip]['request_code'] = [time.time(), time.time(), time.time()]
        result = _check_ip_rate_limit(ip, 'request_code')
        assert result is False

    def test_check_ip_rate_limit_expired_timestamps(self):
        """Expired timestamps should not count towards limit."""
        import time
        from server import _check_ip_rate_limit, _ip_rate_limits
        _ip_rate_limits.clear()
        ip = '10.0.0.3'
        old = time.time() - 600  # 10 minutes ago (beyond 300s window)
        _ip_rate_limits[ip]['request_code'] = [old, old, old]
        result = _check_ip_rate_limit(ip, 'request_code')
        assert result is True

    def test_check_ip_rate_limit_unknown_action(self):
        """Unknown action should always be allowed."""
        from server import _check_ip_rate_limit, _ip_rate_limits
        _ip_rate_limits.clear()
        assert _check_ip_rate_limit('10.0.0.4', 'unknown_action') is True

    def test_check_ip_rate_limit_login(self):
        """Login rate limit: 10 requests per 300s."""
        import time
        from server import _check_ip_rate_limit, _ip_rate_limits
        _ip_rate_limits.clear()
        ip = '10.0.0.5'
        _ip_rate_limits[ip]['login'] = [time.time() for _ in range(10)]
        result = _check_ip_rate_limit(ip, 'login')
        assert result is False


def _get_cookies(response):
    """Extract cookies from a test response."""
    cookies = {}
    for header in response.headers.getlist('Set-Cookie'):
        parts = header.split(';')[0].split('=', 1)
        if len(parts) == 2:
            cookies[parts[0].strip()] = parts[1].strip()
    return cookies
