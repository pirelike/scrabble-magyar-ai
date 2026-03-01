"""Integration tests for server.py auth HTTP routes."""
import os
import sys
import pytest
import json


@pytest.fixture(autouse=True)
def temp_db(monkeypatch, tmp_path):
    db_path = str(tmp_path / 'test.db')
    monkeypatch.setattr('config.DB_PATH', db_path)
    import auth
    monkeypatch.setattr(auth, 'DB_PATH', db_path)
    auth.init_db()
    yield db_path


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


def _get_cookies(response):
    """Extract cookies from a test response."""
    cookies = {}
    for header in response.headers.getlist('Set-Cookie'):
        parts = header.split(';')[0].split('=', 1)
        if len(parts) == 2:
            cookies[parts[0].strip()] = parts[1].strip()
    return cookies
