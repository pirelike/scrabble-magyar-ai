"""Admin panel: hozzáférés-szabályozás (az 1. lépés legfontosabb tesztjei).

Alapelv: aki nem admin, az semmilyen módon nem tudhatja meg, hogy admin panel létezik — minden admin útvonal
(oldal, asset, API, rossz metódus, nem létező alútvonal) ugyanazt a 404-et adja, mint egy nem létező cím.
"""
import os
import re
import shutil
import subprocess
import sys

import pytest
from flask import Flask, jsonify

import admin
import admin_routes
import auth
import config
import server
from helpers import client_with_session, create_user_with_session

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ADMIN_EMAIL = 'boss@example.com'
PASSWORD = 'secret12'
# Az őr csak akkor engedi át a nem biztonságos metódusokat, ha ezek is megvannak
HEADERS = {'X-Admin-Request': '1', 'Origin': 'http://localhost'}

# Minden admin végpont, metódussal (a rossz metódusúak is: a 405 sem árulkodhat)
ADMIN_ENDPOINTS = [
    ('GET', '/admin'),
    ('GET', '/admin/assets/admin.js'),
    ('GET', '/admin/assets/admin.css'),
    ('GET', '/admin/assets/admin-i18n.js'),
    ('GET', '/admin/assets/admin-boot.js'),
    ('GET', '/admin/assets/admin-ui.js'),
    ('GET', '/admin/assets/admin-views-a.js'),
    ('GET', '/admin/assets/admin-views-b.js'),
    ('GET', '/admin/assets/admin-views-c.js'),
    ('GET', '/admin/assets/admin-main.js'),
    ('GET', '/admin/assets/nincs-ilyen.js'),
    ('GET', '/admin/masik'),
    ('GET', '/api/admin'),
    ('GET', '/api/admin/session'),
    ('POST', '/api/admin/session'),
    ('GET', '/api/admin/audit'),
    ('GET', '/api/admin/audit?format=csv'),
    ('GET', '/api/admin/audit?download=1'),
    ('DELETE', '/api/admin/audit'),
    ('POST', '/api/admin/reauth'),
    ('GET', '/api/admin/reauth'),
    ('POST', '/api/admin/sudo'),
    ('DELETE', '/api/admin/sudo'),
    ('GET', '/api/admin/sudo'),
    ('GET', '/api/admin/nincs-ilyen'),
    ('POST', '/api/admin/users/1/ban'),
]


@pytest.fixture(autouse=True)
def admin_config(monkeypatch):
    monkeypatch.setattr(config, 'ADMIN_EMAILS', frozenset({ADMIN_EMAIL}))
    monkeypatch.setattr(config, 'ADMIN_IP_ALLOWLIST', frozenset())
    server._ip_rate_limits.clear()
    yield
    server._ip_rate_limits.clear()


@pytest.fixture
def admin_token():
    user_id, token = create_user_with_session(ADMIN_EMAIL, 'Főnök', PASSWORD)
    auth.touch_admin_session(token)
    return token


@pytest.fixture
def admin_client(admin_token):
    return client_with_session(admin_token)


def _request(client, method, path, **kwargs):
    return client.open(path, method=method, **kwargs)


def _reference_404():
    response = client_with_session(None).get('/ez-az-oldal-nem-letezik')
    assert response.status_code == 404
    return response


def _assert_plain_404(response, reference):
    assert response.status_code == 404
    assert response.get_data() == reference.get_data()
    for header in ('Content-Type', 'Content-Length'):
        assert response.headers.get(header) == reference.headers.get(header)
    # Az admin válaszok fejlécei sem szivároghatnak a 404-re
    for header in ('Cache-Control', 'X-Frame-Options', 'Content-Security-Policy', 'X-Robots-Tag'):
        assert header not in response.headers


def _all_admin_routes():
    """Minden admin útvonal és minden megengedett metódusa az alkalmazás útvonaltérképéből (mintaazonosítókkal), plusz
    egy nem engedélyezett metódus (PUT): új végpont így automatikusan bekerül a hozzáférési tesztekbe."""
    items = set()
    for rule in server.app.url_map.iter_rules():
        if not (rule.rule == '/admin' or rule.rule.startswith(('/admin/', '/api/admin'))):
            continue
        path = re.sub(r'<(?:\w+:)?\w+>', '1', rule.rule)
        for method in sorted(rule.methods - {'HEAD', 'OPTIONS'}):
            items.add((method, path))
        items.add(('PUT', path))
    return sorted(items)


ALL_ADMIN_ROUTES = _all_admin_routes()
UNSAFE = {'POST', 'PUT', 'PATCH', 'DELETE'}


class TestEveryAdminRoute:
    """A teljes útvonaltérkép: semmilyen admin útvonal nem árulkodhat a nem adminnak, és az őr minden útvonalra egyformán
    érvényes (CSRF, tétlenség)."""

    @pytest.fixture(autouse=True)
    def _no_rate_limit(self, monkeypatch):
        # Több száz kérés megy ki egy IP-ről: a forgalomkorlátot külön tesztek fedik le
        import routes
        monkeypatch.setattr(routes._rate_limiter, 'check_ip', lambda ip, action: True)

    def test_the_route_map_is_complete(self):
        assert len(ALL_ADMIN_ROUTES) > 200
        assert ('GET', '/api/admin/overview') in ALL_ADMIN_ROUTES
        assert ('POST', '/api/admin/rooms/1/action') in ALL_ADMIN_ROUTES

    def test_anonymous_gets_the_plain_404_everywhere(self):
        reference = _reference_404()
        client = client_with_session(None)
        for method, path in ALL_ADMIN_ROUTES:
            _assert_plain_404(_request(client, method, path, headers=HEADERS), reference)

    def test_a_normal_user_gets_the_plain_404_everywhere(self):
        reference = _reference_404()
        _, token = create_user_with_session('player@example.com', 'Játékos', PASSWORD)
        client = client_with_session(token)
        for method, path in ALL_ADMIN_ROUTES:
            _assert_plain_404(_request(client, method, path, headers=HEADERS), reference)

    def test_every_unsafe_request_needs_the_csrf_header(self, admin_client):
        for method, path in ALL_ADMIN_ROUTES:
            if method in UNSAFE:
                response = _request(admin_client, method, path, headers={'Origin': 'http://localhost'})
                assert response.status_code == 403, (method, path)

    def test_every_unsafe_request_needs_a_same_origin_header(self, admin_client):
        for method, path in ALL_ADMIN_ROUTES:
            if method in UNSAFE:
                response = _request(admin_client, method, path,
                                    headers={'X-Admin-Request': '1', 'Origin': 'http://evil.example'})
                assert response.status_code == 403, (method, path)

    def test_an_idle_session_needs_the_password_on_every_api_route(self, admin_token, admin_client):
        exempt = {'/api/admin/session', '/api/admin/reauth'}
        with auth.transaction() as conn:
            conn.execute("UPDATE sessions SET admin_seen_at = '2000-01-01 00:00:00' WHERE token = ?", (admin_token,))
        for method, path in ALL_ADMIN_ROUTES:
            if not path.startswith('/api/admin') or path in exempt or method == 'PUT':
                continue
            response = _request(admin_client, method, path, headers=HEADERS)
            assert response.status_code == 401 and response.get_json().get('reauth') is True, (method, path)

    def test_admin_responses_carry_the_security_headers(self, admin_client):
        for path in ('/admin', '/api/admin/overview', '/api/admin/system', '/admin/assets/admin-main.js'):
            response = admin_client.get(path)
            assert response.status_code == 200, path
            assert response.headers['Cache-Control'] == 'no-store'
            assert response.headers['X-Frame-Options'] == 'DENY'
            assert 'frame-ancestors \'none\'' in response.headers['Content-Security-Policy']

    def test_the_csp_allows_only_the_socket_io_cdn_besides_self(self, admin_client):
        csp = admin_client.get('/admin').headers['Content-Security-Policy']
        assert "script-src 'self' https://cdnjs.cloudflare.com;" in csp
        assert "style-src 'self';" in csp and 'unsafe-inline' not in csp and 'unsafe-eval' not in csp
        assert "default-src 'none'" in csp


class TestNotAdminSeesNothing:
    @pytest.mark.parametrize('method,path', ADMIN_ENDPOINTS)
    def test_anonymous(self, method, path):
        response = _request(client_with_session(None), method, path, headers=HEADERS)
        _assert_plain_404(response, _reference_404())

    @pytest.mark.parametrize('method,path', ADMIN_ENDPOINTS)
    def test_invalid_session_token(self, method, path):
        response = _request(client_with_session('ervenytelen-token'), method, path, headers=HEADERS)
        _assert_plain_404(response, _reference_404())

    @pytest.mark.parametrize('method,path', ADMIN_ENDPOINTS)
    def test_registered_non_admin(self, method, path):
        _, token = create_user_with_session('player@example.com', 'Játékos', PASSWORD)
        response = _request(client_with_session(token), method, path, headers=HEADERS)
        _assert_plain_404(response, _reference_404())

    @pytest.mark.parametrize('method,path', ADMIN_ENDPOINTS)
    def test_admin_account_while_admin_list_is_empty(self, admin_token, monkeypatch, method, path):
        monkeypatch.setattr(config, 'ADMIN_EMAILS', frozenset())
        response = _request(client_with_session(admin_token), method, path, headers=HEADERS)
        _assert_plain_404(response, _reference_404())

    @pytest.mark.parametrize('path', ['//admin', '/admin//assets/admin.js', '//api/admin/session',
                                      '/api//admin/session', '/api/admin//audit'])
    def test_double_slashes_do_not_bypass_the_guard(self, path):
        """Nyers WSGI kérés (a tesztkliens a `//host` alakot külön értelmezné)."""
        from werkzeug.test import EnvironBuilder

        def raw(request_path):
            environ = EnvironBuilder(path='/').get_environ()
            environ['PATH_INFO'] = request_path
            captured = {}

            def start_response(status, headers, exc_info=None):
                captured['status'] = status
                captured['headers'] = dict(headers)
            body = b''.join(server.app.wsgi_app(environ, start_response))
            return captured['status'], captured['headers'].get('Location'), body

        assert raw(path) == raw('//ez-az-oldal-nem-letezik')

    def test_display_name_does_not_make_an_admin(self):
        # Az adminság az e-mail címhez kötött: a megjelenítési név átírható, ezért nem számít
        _, token = create_user_with_session('masik@example.com', ADMIN_EMAIL, PASSWORD)
        response = client_with_session(token).get('/api/admin/session')
        _assert_plain_404(response, _reference_404())

    def test_expired_session_is_not_admin(self, admin_token):
        with auth.transaction() as conn:
            conn.execute("UPDATE sessions SET expires_at = '2000-01-01 00:00:00' WHERE token = ?", (admin_token,))
        response = client_with_session(admin_token).get('/api/admin/session')
        _assert_plain_404(response, _reference_404())


class TestAdminAccess:
    def test_session_endpoint(self, admin_client):
        response = admin_client.get('/api/admin/session')
        assert response.status_code == 200
        data = response.get_json()
        assert data['admin']['display_name'] == 'Főnök'
        assert data['admin']['email'] == ADMIN_EMAIL
        assert data['session']['idle_expired'] is False
        assert data['session']['sudo_remaining'] == 0

    def test_email_comparison_is_case_insensitive(self):
        _, token = create_user_with_session('Boss@Example.COM', 'Nagybetű', PASSWORD)
        auth.touch_admin_session(token)
        assert client_with_session(token).get('/api/admin/session').status_code == 200

    def test_admin_page_and_headers(self, admin_client):
        response = admin_client.get('/admin')
        assert response.status_code == 200
        body = response.get_data(as_text=True)
        assert 'id="admin-app"' in body
        assert '<meta name="robots" content="noindex,nofollow">' in body
        assert response.headers['Cache-Control'] == 'no-store'
        assert response.headers['X-Frame-Options'] == 'DENY'
        assert 'noindex' in response.headers['X-Robots-Tag']
        csp = response.headers['Content-Security-Policy']
        assert "script-src 'self'" in csp and "frame-ancestors 'none'" in csp
        assert 'unsafe-inline' not in csp

    def test_page_has_no_inline_script_or_style(self, admin_client):
        body = admin_client.get('/admin').get_data(as_text=True)
        assert not re.findall(r'<script(?![^>]*\bsrc=)[^>]*>', body)
        assert not re.search(r'\sstyle=', body)
        assert not re.search(r'\son[a-z]+=', body)

    @pytest.mark.parametrize('name,mime', [('admin.js', 'javascript'), ('admin.css', 'css'),
                                           ('admin-i18n.js', 'javascript'), ('admin-boot.js', 'javascript'),
                                           ('admin-ui.js', 'javascript'), ('admin-views-a.js', 'javascript'),
                                           ('admin-views-b.js', 'javascript'), ('admin-views-c.js', 'javascript'),
                                           ('admin-main.js', 'javascript')])
    def test_assets_are_served_to_the_admin(self, admin_client, name, mime):
        response = admin_client.get('/admin/assets/' + name)
        assert response.status_code == 200
        assert mime in response.headers['Content-Type']
        assert response.headers['Cache-Control'] == 'no-store'

    def test_asset_path_traversal_is_rejected(self, admin_client):
        for path in ('/admin/assets/../auth.py', '/admin/assets/%2e%2e/auth.py', '/admin/assets/..%2fauth.py'):
            assert admin_client.get(path).status_code == 404

    def test_unknown_admin_route_is_a_plain_404_for_the_admin_too(self, admin_client):
        assert admin_client.get('/api/admin/nincs-ilyen').status_code == 404

    def test_wrong_method_is_405_only_for_the_admin(self, admin_client):
        assert admin_client.post('/api/admin/session', headers=HEADERS).status_code == 405


class TestAdminFlagOnlyForAdmins:
    def test_me_has_no_key_for_a_normal_user(self):
        _, token = create_user_with_session('player@example.com', 'Játékos', PASSWORD)
        user = client_with_session(token).get('/api/auth/me').get_json()['user']
        assert 'is_admin' not in user

    def test_me_has_the_flag_for_the_admin(self, admin_client):
        user = admin_client.get('/api/auth/me').get_json()['user']
        assert user['is_admin'] is True

    def test_login_response(self):
        create_user_with_session('player@example.com', 'Játékos', PASSWORD)
        create_user_with_session(ADMIN_EMAIL, 'Főnök', PASSWORD)
        anon = client_with_session(None)
        normal = anon.post('/api/auth/login', json={'email': 'player@example.com', 'password': PASSWORD})
        assert 'is_admin' not in normal.get_json()['user']
        boss = client_with_session(None).post('/api/auth/login', json={'email': ADMIN_EMAIL, 'password': PASSWORD})
        assert boss.get_json()['user']['is_admin'] is True

    def test_registration_of_an_admin_email(self):
        from helpers import verify_email
        verify_email(ADMIN_EMAIL)
        response = client_with_session(None).post('/api/auth/register', json={
            'email': ADMIN_EMAIL, 'password': PASSWORD, 'display_name': 'Új Admin'})
        assert response.status_code == 200
        assert response.get_json()['user']['is_admin'] is True

    def test_fresh_login_opens_the_panel_without_asking_the_password_again(self):
        create_user_with_session(ADMIN_EMAIL, 'Főnök', PASSWORD)
        client = client_with_session(None)
        assert client.post('/api/auth/login', json={'email': ADMIN_EMAIL, 'password': PASSWORD}).status_code == 200
        assert client.get('/api/admin/audit').status_code == 200

    def test_guest_never_gets_admin(self):
        assert not auth.is_admin_user(None)
        assert not auth.is_admin_user({})
        assert not auth.is_admin_user({'email': None})


class TestAdminEmailCannotBeClaimed:
    """SMTP nélkül a `request-code` a kódot a válaszban adja vissza (fejlesztői mód): az admin címnél ez nem
    lehet így, különben bárki megerősíthetné az (még nem regisztrált) admin címet és admin lenne."""

    @pytest.fixture(autouse=True)
    def no_smtp(self, monkeypatch):
        import routes
        monkeypatch.setattr(config, 'SMTP_CONFIGURED', False)
        monkeypatch.setattr(routes, 'send_verification_email', lambda email, code: None)

    def test_dev_code_is_withheld_for_the_admin_address(self):
        response = client_with_session(None).post('/api/auth/request-code', json={'email': ADMIN_EMAIL})
        assert response.status_code == 200
        assert response.get_json()['success'] is True
        assert 'dev_code' not in response.get_json()

    def test_case_variants_of_the_admin_address_too(self):
        response = client_with_session(None).post('/api/auth/request-code', json={'email': 'BOSS@Example.com'})
        assert 'dev_code' not in response.get_json()

    def test_other_addresses_keep_the_development_convenience(self):
        response = client_with_session(None).post('/api/auth/request-code', json={'email': 'player@example.com'})
        assert re.fullmatch(r'\d{6}', response.get_json()['dev_code'])

    def test_the_code_is_still_valid_for_whoever_reads_the_server_console(self):
        client_with_session(None).post('/api/auth/request-code', json={'email': ADMIN_EMAIL})
        with auth.transaction() as conn:
            code = conn.execute('SELECT code FROM verification_codes WHERE email = ?', (ADMIN_EMAIL,)).fetchone()[0]
        response = client_with_session(None).post('/api/auth/verify-code', json={'email': ADMIN_EMAIL, 'code': code})
        assert response.status_code == 200


class TestPublicSurfaceHasNoAdmin:
    """Az admin felület és fordításai nincsenek a nyilvános fájlok között."""

    def test_index_html_has_no_admin_element(self):
        with open(os.path.join(ROOT, 'templates', 'index.html'), encoding='utf-8') as fh:
            assert 'admin' not in fh.read().lower()

    def test_served_index_has_no_admin_element(self):
        body = client_with_session(None).get('/').get_data(as_text=True)
        assert 'admin' not in body.lower()

    def test_public_i18n_has_no_admin_text(self):
        with open(os.path.join(ROOT, 'static', 'i18n-data.js'), encoding='utf-8') as fh:
            source = fh.read()
        assert '"admin.' not in source
        assert 'admin' not in source.lower()

    def test_app_js_only_has_the_entry_point(self):
        with open(os.path.join(ROOT, 'static', 'app.js'), encoding='utf-8') as fh:
            source = fh.read()
        assert 'admin_assets' not in source and 'admin-i18n' not in source
        assert "t('admin" not in source
        assert '/api/admin' not in source  # az API-t csak a panel hívja

    def test_no_admin_file_in_the_public_static_folder(self):
        for folder, _, files in os.walk(os.path.join(ROOT, 'static')):
            for name in files:
                assert 'admin' not in name.lower(), os.path.join(folder, name)

    def test_public_static_route_does_not_serve_admin_assets(self, admin_client):
        for name in ('admin.js', 'admin.css', 'admin-i18n.js', 'admin-ui.js', 'admin-views-a.js', 'admin-views-b.js',
                     'admin-views-c.js', 'admin-main.js'):
            assert admin_client.get('/static/' + name).status_code == 404


class TestIdleTimeoutAndReauth:
    def _set_seen(self, token, seconds_ago):
        stamp = auth.format_ts(auth.utcnow() - __import__('datetime').timedelta(seconds=seconds_ago))
        with auth.transaction() as conn:
            conn.execute('UPDATE sessions SET admin_seen_at = ? WHERE token = ?', (stamp, token))
        return stamp

    def _seen(self, token):
        return auth.get_admin_session(token)['admin_seen_at']

    def test_idle_session_needs_the_password(self, admin_token, admin_client):
        self._set_seen(admin_token, config.ADMIN_SESSION_IDLE_MINUTES * 60 + 5)
        response = admin_client.get('/api/admin/audit')
        assert response.status_code == 401
        assert response.get_json()['reauth'] is True

    def test_never_used_admin_session_needs_the_password(self, admin_token, admin_client):
        with auth.transaction() as conn:
            conn.execute('UPDATE sessions SET admin_seen_at = NULL WHERE token = ?', (admin_token,))
        assert admin_client.get('/api/admin/audit').status_code == 401

    def test_page_and_status_still_work_when_idle_and_do_not_refresh_the_timer(self, admin_token, admin_client):
        stamp = self._set_seen(admin_token, config.ADMIN_SESSION_IDLE_MINUTES * 60 + 5)
        assert admin_client.get('/admin').status_code == 200
        assert admin_client.get('/admin/assets/admin.js').status_code == 200
        status = admin_client.get('/api/admin/session')
        assert status.status_code == 200
        assert status.get_json()['session']['idle_expired'] is True
        assert self._seen(admin_token) == stamp

    def test_idle_limit_is_configurable(self, admin_token, admin_client, monkeypatch):
        monkeypatch.setattr(config, 'ADMIN_SESSION_IDLE_MINUTES', 1)
        self._set_seen(admin_token, 30)
        assert admin_client.get('/api/admin/audit').status_code == 200
        self._set_seen(admin_token, 90)
        assert admin_client.get('/api/admin/audit').status_code == 401

    def test_activity_refreshes_the_timer(self, admin_token, admin_client):
        stamp = self._set_seen(admin_token, 120)
        assert admin_client.get('/api/admin/audit').status_code == 200
        assert self._seen(admin_token) > stamp

    def test_timer_writes_are_throttled(self, admin_token, admin_client):
        stamp = self._set_seen(admin_token, 2)
        assert admin_client.get('/api/admin/audit').status_code == 200
        assert self._seen(admin_token) == stamp

    def test_reauth_with_the_right_password(self, admin_token, admin_client):
        self._set_seen(admin_token, config.ADMIN_SESSION_IDLE_MINUTES * 60 + 5)
        response = admin_client.post('/api/admin/reauth', json={'password': PASSWORD}, headers=HEADERS)
        assert response.status_code == 200
        assert response.get_json()['session']['idle_expired'] is False
        assert admin_client.get('/api/admin/audit').status_code == 200
        rows = admin.query_audit(action='admin.reauth')['items']
        assert [r['action'] for r in rows] == ['admin.reauth']

    def test_reauth_with_a_wrong_password_is_logged(self, admin_token, admin_client):
        self._set_seen(admin_token, config.ADMIN_SESSION_IDLE_MINUTES * 60 + 5)
        response = admin_client.post('/api/admin/reauth', json={'password': 'rossz-jelszo'}, headers=HEADERS)
        assert response.status_code == 403
        assert admin_client.get('/api/admin/audit').status_code == 401
        assert [r['action'] for r in admin.query_audit()['items']] == ['admin.reauth_failed']

    def test_reauth_clears_sudo(self, admin_token, admin_client):
        admin_client.post('/api/admin/sudo', json={'password': PASSWORD}, headers=HEADERS)
        assert admin.sudo_active(admin_token)
        admin_client.post('/api/admin/reauth', json={'password': PASSWORD}, headers=HEADERS)
        assert not admin.sudo_active(admin_token)


class TestCsrfProtection:
    def test_missing_custom_header(self, admin_client):
        response = admin_client.post('/api/admin/sudo', json={'password': PASSWORD},
                                     headers={'Origin': 'http://localhost'})
        assert response.status_code == 403
        assert not admin.query_audit()['items']

    def test_missing_origin_and_referer(self, admin_client):
        response = admin_client.post('/api/admin/sudo', json={'password': PASSWORD},
                                     headers={'X-Admin-Request': '1'})
        assert response.status_code == 403

    @pytest.mark.parametrize('origin', ['http://evil.example', 'null', 'http://localhost.evil.example',
                                        'http://localhost:9999'])
    def test_foreign_origin(self, admin_client, origin):
        response = admin_client.post('/api/admin/sudo', json={'password': PASSWORD},
                                     headers={'X-Admin-Request': '1', 'Origin': origin})
        assert response.status_code == 403

    def test_referer_is_accepted_when_origin_is_missing(self, admin_client):
        response = admin_client.post('/api/admin/sudo', json={'password': PASSWORD},
                                     headers={'X-Admin-Request': '1', 'Referer': 'http://localhost/admin'})
        assert response.status_code == 200

    def test_foreign_referer(self, admin_client):
        response = admin_client.post('/api/admin/sudo', json={'password': PASSWORD},
                                     headers={'X-Admin-Request': '1', 'Referer': 'http://evil.example/x'})
        assert response.status_code == 403

    def test_get_needs_no_custom_header(self, admin_client):
        assert admin_client.get('/api/admin/audit').status_code == 200

    def test_delete_is_protected_too(self, admin_client):
        assert admin_client.delete('/api/admin/sudo').status_code == 403
        assert admin_client.delete('/api/admin/sudo', headers=HEADERS).status_code == 200


class TestSudo:
    def test_wrong_password(self, admin_token, admin_client):
        response = admin_client.post('/api/admin/sudo', json={'password': 'rossz-jelszo'}, headers=HEADERS)
        assert response.status_code == 403
        assert not admin.sudo_active(admin_token)
        rows = admin.query_audit()['items']
        assert [r['action'] for r in rows] == ['admin.sudo_failed']
        assert rows[0]['ip'] and rows[0]['admin_user_id']

    @pytest.mark.parametrize('body', [{}, {'password': ''}, {'password': 12345}, {'password': 'x' * 200}, None])
    def test_invalid_password_field(self, admin_client, body):
        response = admin_client.post('/api/admin/sudo', json=body, headers=HEADERS)
        assert response.status_code == 400

    def test_right_password_grants_sudo_for_the_configured_time(self, admin_token, admin_client):
        response = admin_client.post('/api/admin/sudo', json={'password': PASSWORD}, headers=HEADERS)
        assert response.status_code == 200
        remaining = response.get_json()['session']['sudo_remaining']
        assert config.ADMIN_SUDO_MINUTES * 60 - 5 <= remaining <= config.ADMIN_SUDO_MINUTES * 60
        assert admin.sudo_active(admin_token)
        status = admin_client.get('/api/admin/session').get_json()['session']
        assert status['sudo_remaining'] > 0
        rows = admin.query_audit()['items']
        assert [r['action'] for r in rows] == ['admin.sudo']
        assert rows[0]['details']['minutes'] == config.ADMIN_SUDO_MINUTES

    def test_sudo_is_bound_to_the_session(self, admin_token, admin_client):
        admin_client.post('/api/admin/sudo', json={'password': PASSWORD}, headers=HEADERS)
        other_token = auth.create_session(auth.get_user_by_email(ADMIN_EMAIL)['id'])
        auth.touch_admin_session(other_token)
        assert not admin.sudo_active(other_token)

    def test_end_sudo(self, admin_token, admin_client):
        admin_client.post('/api/admin/sudo', json={'password': PASSWORD}, headers=HEADERS)
        response = admin_client.delete('/api/admin/sudo', headers=HEADERS)
        assert response.status_code == 200
        assert not admin.sudo_active(admin_token)
        assert [r['action'] for r in admin.query_audit()['items']] == ['admin.sudo_end', 'admin.sudo']

    def test_wrong_passwords_count_towards_the_login_limit(self, admin_client):
        statuses = [admin_client.post('/api/admin/sudo', json={'password': 'rossz'}, headers=HEADERS).status_code
                    for _ in range(11)]
        assert statuses[:10] == [403] * 10
        assert statuses[10] == 429
        # A korlát alatt a helyes jelszó sem megy át
        assert admin_client.post('/api/admin/sudo', json={'password': PASSWORD}, headers=HEADERS).status_code == 429


@pytest.fixture
def danger_app():
    """Önálló alkalmazás az őrrel és két teszt-útvonallal: a romboló műveletek védelmét ezeken lehet kipróbálni."""
    app = Flask('danger-test')
    app.config['SECRET_KEY'] = 'teszt'
    app.register_blueprint(admin_routes.admin_bp)

    @app.route('/api/admin/_sudo-only', methods=['POST'])
    @admin_routes.sudo_required
    def sudo_only():
        return jsonify(success=True)

    @app.route('/api/admin/_danger-only', methods=['POST'])
    @admin_routes.danger
    def danger_only():
        return jsonify(success=True)

    return app


class TestDestructiveActionGuards:
    def _client(self, app, token):
        client = app.test_client()
        client.set_cookie('session_token', token)
        return client

    def test_no_sudo_means_401_sudo_required(self, danger_app, admin_token):
        response = self._client(danger_app, admin_token).post('/api/admin/_sudo-only', headers=HEADERS)
        assert response.status_code == 401
        assert response.get_json()['sudo_required'] is True

    def test_with_sudo(self, danger_app, admin_token, admin_client):
        admin_client.post('/api/admin/sudo', json={'password': PASSWORD}, headers=HEADERS)
        response = self._client(danger_app, admin_token).post('/api/admin/_sudo-only', headers=HEADERS)
        assert response.status_code == 200

    def test_expired_sudo(self, danger_app, admin_token, admin_client):
        admin_client.post('/api/admin/sudo', json={'password': PASSWORD}, headers=HEADERS)
        with auth.transaction() as conn:
            conn.execute("UPDATE sessions SET sudo_until = '2000-01-01 00:00:00' WHERE token = ?", (admin_token,))
        response = self._client(danger_app, admin_token).post('/api/admin/_sudo-only', headers=HEADERS)
        assert response.status_code == 401
        assert response.get_json()['sudo_required'] is True

    def test_sudo_does_not_survive_idle_expiry(self, danger_app, admin_token, admin_client):
        admin_client.post('/api/admin/sudo', json={'password': PASSWORD}, headers=HEADERS)
        with auth.transaction() as conn:
            conn.execute("UPDATE sessions SET admin_seen_at = '2000-01-01 00:00:00' WHERE token = ?", (admin_token,))
        response = self._client(danger_app, admin_token).post('/api/admin/_sudo-only', headers=HEADERS)
        assert response.status_code == 401
        assert response.get_json().get('reauth') is True

    def test_non_admin_cannot_reach_the_guarded_route(self, danger_app):
        _, token = create_user_with_session('player@example.com', 'Játékos', PASSWORD)
        response = self._client(danger_app, token).post('/api/admin/_sudo-only', headers=HEADERS)
        assert response.status_code == 404

    def test_danger_actions_have_their_own_rate_limit(self, danger_app, admin_token):
        client = self._client(danger_app, admin_token)
        statuses = [client.post('/api/admin/_danger-only', headers=HEADERS).status_code for _ in range(21)]
        assert statuses[:20] == [200] * 20
        assert statuses[20] == 429


class TestRateLimit:
    def test_admin_requests_are_limited_per_ip(self, admin_client):
        limit, _ = config.AUTH_RATE_LIMITS['admin']
        statuses = [admin_client.get('/api/admin/session').status_code for _ in range(limit + 1)]
        assert statuses[:limit] == [200] * limit
        assert statuses[limit] == 429

    def test_not_admin_requests_do_not_use_the_admin_budget(self):
        client = client_with_session(None)
        for _ in range(config.AUTH_RATE_LIMITS['admin'][0] + 5):
            assert client.get('/api/admin/session').status_code == 404


class TestIpAllowlist:
    def test_parse(self):
        networks = config.parse_ip_networks('10.0.0.0/8, 192.168.1.5 ,::1')
        assert len(networks) == 3
        assert config.parse_ip_networks('') == frozenset()

    def test_invalid_entry_fails_loudly(self):
        with pytest.raises(ValueError):
            config.parse_ip_networks('10.0.0.0/8, nem-ip')

    def test_ip_allowed(self):
        networks = config.parse_ip_networks('10.0.0.0/8,192.168.1.5,fd00::/8')
        assert admin.ip_allowed('10.20.30.40', networks)
        assert admin.ip_allowed('192.168.1.5', networks)
        assert admin.ip_allowed('fd00::1', networks)
        assert admin.ip_allowed('::ffff:10.0.0.1', networks)
        assert not admin.ip_allowed('192.168.1.6', networks)
        assert not admin.ip_allowed('11.0.0.1', networks)
        assert not admin.ip_allowed('nem-ip', networks)
        assert not admin.ip_allowed('', networks)

    def test_empty_list_allows_everyone(self):
        assert admin.ip_allowed('203.0.113.9', frozenset())

    def test_other_ips_get_a_404_even_as_admin(self, admin_token, monkeypatch):
        monkeypatch.setattr(config, 'ADMIN_IP_ALLOWLIST', config.parse_ip_networks('10.0.0.0/8'))
        client = client_with_session(admin_token)
        allowed = client.get('/api/admin/session', environ_overrides={'REMOTE_ADDR': '10.1.2.3'})
        assert allowed.status_code == 200
        for path in ('/api/admin/session', '/admin', '/admin/assets/admin.js'):
            blocked = client.get(path, environ_overrides={'REMOTE_ADDR': '203.0.113.9'})
            _assert_plain_404(blocked, _reference_404())

    def test_forwarded_header_is_ignored_from_a_non_local_peer(self, admin_token, monkeypatch):
        monkeypatch.setattr(config, 'ADMIN_IP_ALLOWLIST', config.parse_ip_networks('10.0.0.0/8'))
        client = client_with_session(admin_token)
        response = client.get('/api/admin/session', headers={'CF-Connecting-IP': '10.1.2.3'},
                              environ_overrides={'REMOTE_ADDR': '203.0.113.9'})
        assert response.status_code == 404

    def test_tunnel_peer_uses_the_forwarded_address(self, admin_token, monkeypatch):
        monkeypatch.setattr(config, 'ADMIN_IP_ALLOWLIST', config.parse_ip_networks('10.0.0.0/8'))
        client = client_with_session(admin_token)
        ok = client.get('/api/admin/session', headers={'CF-Connecting-IP': '10.1.2.3'})
        assert ok.status_code == 200
        blocked = client.get('/api/admin/session', headers={'CF-Connecting-IP': '203.0.113.9'})
        assert blocked.status_code == 404


class TestEnvironmentParsing:
    def _config_value(self, expression, **env):
        full_env = {k: v for k, v in os.environ.items() if not k.startswith('ADMIN_')}
        full_env.update(env)
        result = subprocess.run([sys.executable, '-c', f'import config; print({expression})'], cwd=ROOT,
                                env=full_env, capture_output=True, text=True, timeout=30)
        return result

    def test_admin_emails_are_lowercased_and_trimmed(self):
        result = self._config_value('sorted(config.ADMIN_EMAILS)',
                                    ADMIN_EMAILS=' Boss@Example.com , second@example.org ,, ')
        assert result.returncode == 0, result.stderr
        assert result.stdout.strip() == "['boss@example.com', 'second@example.org']"

    def test_defaults_disable_the_panel(self):
        result = self._config_value('(config.ADMIN_EMAILS, config.ADMIN_IP_ALLOWLIST, '
                                    'config.ADMIN_SESSION_IDLE_MINUTES, config.ADMIN_SUDO_MINUTES)')
        assert result.returncode == 0, result.stderr
        assert result.stdout.strip() == '(frozenset(), frozenset(), 30, 10)'

    def test_invalid_allowlist_stops_the_server_from_starting(self):
        result = self._config_value('1', ADMIN_IP_ALLOWLIST='10.0.0.0/8,elgepelt')
        assert result.returncode != 0
        assert 'ADMIN_IP_ALLOWLIST' in result.stderr


class TestSocketAccess:
    def _client(self, user_id=None):
        client = server.socketio.test_client(server.app)
        if user_id is None:
            client.emit('set_name', {'name': 'Vendég', 'is_guest': True})
        else:
            from helpers import registered_set_name_payload
            client.emit('set_name', registered_set_name_payload(user_id))
        client.get_received()
        return client

    def _events(self, client):
        return [event['name'] for event in client.get_received()]

    def test_guest_gets_nothing(self):
        client = self._client()
        client.emit('admin_subscribe')
        assert self._events(client) == []

    def test_non_admin_gets_nothing(self):
        user_id, _ = create_user_with_session('player@example.com', 'Játékos', PASSWORD)
        client = self._client(user_id)
        client.emit('admin_subscribe')
        assert self._events(client) == []
        server.socketio.emit('admin_secret', {}, to='admin')
        assert self._events(client) == []

    def test_admin_is_subscribed(self):
        user_id, _ = create_user_with_session(ADMIN_EMAIL, 'Főnök', PASSWORD)
        client = self._client(user_id)
        client.emit('admin_subscribe')
        assert self._events(client) == ['admin_subscribed', 'admin_overview']
        server.socketio.emit('admin_overview', {'x': 1}, to='admin')
        assert self._events(client) == ['admin_overview']

    def test_admin_list_empty_means_no_subscription(self, monkeypatch):
        user_id, _ = create_user_with_session(ADMIN_EMAIL, 'Főnök', PASSWORD)
        client = self._client(user_id)
        monkeypatch.setattr(config, 'ADMIN_EMAILS', frozenset())
        client.emit('admin_subscribe')
        assert self._events(client) == []

    def test_logout_leaves_the_admin_room(self):
        user_id, _ = create_user_with_session(ADMIN_EMAIL, 'Főnök', PASSWORD)
        client = self._client(user_id)
        client.emit('admin_subscribe')
        client.get_received()
        client.emit('logout')
        client.get_received()
        server.socketio.emit('admin_overview', {}, to='admin')
        assert self._events(client) == []

    def test_switching_to_a_guest_identity_leaves_the_admin_room(self):
        user_id, _ = create_user_with_session(ADMIN_EMAIL, 'Főnök', PASSWORD)
        client = self._client(user_id)
        client.emit('admin_subscribe')
        client.get_received()
        client.emit('set_name', {'name': 'Vendég', 'is_guest': True})
        client.get_received()
        server.socketio.emit('admin_overview', {}, to='admin')
        assert self._events(client) == []

    def test_subscribe_is_rate_limited_silently(self):
        user_id, _ = create_user_with_session(ADMIN_EMAIL, 'Főnök', PASSWORD)
        client = self._client(user_id)
        for _ in range(5):
            client.emit('admin_subscribe')
        client.get_received()
        client.emit('admin_subscribe')
        assert self._events(client) == []


@pytest.mark.skipif(shutil.which('node') is None, reason='node nem elérhető')
class TestServiceWorkerSkipsAdmin:
    HARNESS = r"""
const vm = require('vm');
const code = require('fs').readFileSync(0, 'utf8');
const listeners = {};
const sandbox = {
    self: { addEventListener: (type, fn) => { listeners[type] = fn; }, location: { origin: 'https://example.test' },
            skipWaiting() {}, clients: { claim() {} } },
    caches: { match: async () => undefined, keys: async () => [],
              open: async () => ({ match: async () => undefined, put() {}, addAll: async () => {} }) },
    fetch: async () => ({ ok: true, clone() { return this; } }),
    URL, console,
};
vm.createContext(sandbox);
vm.runInContext(code, sandbox);
function handled(path, mode) {
    let called = false;
    listeners.fetch({ request: { method: 'GET', url: 'https://example.test' + path, mode },
                      respondWith() { called = true; } });
    return called;
}
console.log(JSON.stringify({
    admin_page: handled('/admin', 'navigate'),
    admin_asset: handled('/admin/assets/admin.js', 'no-cors'),
    admin_api: handled('/api/admin/session', 'cors'),
    public_page: handled('/', 'navigate'),
    public_static: handled('/static/app.js', 'no-cors'),
}));
"""

    def test_admin_requests_bypass_the_service_worker(self):
        import json
        source = client_with_session(None).get('/sw.js').get_data(as_text=True)
        result = subprocess.run(['node', '-e', self.HARNESS], input=source, capture_output=True, text=True, timeout=30)
        assert result.returncode == 0, result.stderr
        handled = json.loads(result.stdout)
        assert handled == {'admin_page': False, 'admin_asset': False, 'admin_api': False,
                           'public_page': True, 'public_static': True}

    def test_admin_is_not_part_of_the_precached_shell(self):
        source = client_with_session(None).get('/sw.js').get_data(as_text=True)
        shell = re.search(r'const SHELL_URLS = \[(.*?)\];', source, re.S).group(1)
        assert 'admin' not in shell
