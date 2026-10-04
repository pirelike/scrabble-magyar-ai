"""Admin panel: a levelező szerver (SMTP) beállítása, kapcsolat-próbája és visszaállítása (`mail_config`, `admin_mail`)."""
import json
from unittest.mock import MagicMock, patch

import pytest

import admin
import auth
import config
import email_service
import mail_config
import server

REASON = 'Új levelező szolgáltató'
CTX = admin.AdminContext(1, '127.0.0.1', 'teszt')

GOOD = {'host': 'smtp.example.org', 'port': 587, 'security': 'starttls', 'username': 'mailer',
        'password': 'titkos-jelszo-123', 'from_address': 'noreply@example.org', 'from_name': 'Magyar Scrabble',
        'verify_tls': True}


@pytest.fixture(autouse=True)
def no_background(monkeypatch):
    monkeypatch.setattr(server.socketio, 'start_background_task', lambda fn, *a, **k: None)


@pytest.fixture(autouse=True)
def blank_environment(monkeypatch):
    """A környezeti változók ne befolyásolják a próbát: nincs beállított levelezés."""
    for key, value in (('SMTP_HOST', 'smtp.gmail.com'), ('SMTP_PORT', 587), ('SMTP_USER', ''), ('SMTP_PASSWORD', ''),
                       ('SMTP_FROM', ''), ('SMTP_CONFIGURED', False)):
        monkeypatch.setattr(config, key, value)


def _audit(action):
    return admin.query_audit(action=action, limit=50)['items']


def save(api, **overrides):
    body = {**GOOD, **overrides, 'reason': REASON}
    return api.patch('/api/admin/system/mail', body)


# ---------------------------------------------------------------- tárolás és érvényes beállítás

class TestMailConfig:
    def test_without_an_override_the_environment_applies(self, monkeypatch):
        monkeypatch.setattr(config, 'SMTP_HOST', 'smtp.env.example')
        monkeypatch.setattr(config, 'SMTP_USER', 'envuser')
        monkeypatch.setattr(config, 'SMTP_PASSWORD', 'envpass')
        monkeypatch.setattr(config, 'SMTP_FROM', 'env@example.org')
        monkeypatch.setattr(config, 'SMTP_CONFIGURED', True)
        cfg = mail_config.current()
        assert cfg['source'] == 'environment' and cfg['configured'] is True and cfg['security'] == 'starttls'
        assert cfg['host'] == 'smtp.env.example' and mail_config.from_address() == 'env@example.org'

    def test_the_saved_override_wins_and_can_be_cleared(self, monkeypatch):
        monkeypatch.setattr(config, 'SMTP_CONFIGURED', True)
        with auth.transaction() as conn:
            mail_config.save(conn, GOOD, 7)
        cfg = mail_config.current()
        assert cfg['source'] == 'database' and cfg['host'] == 'smtp.example.org' and cfg['configured'] is True
        with auth.transaction() as conn:
            assert mail_config.clear(conn) is True
            assert mail_config.clear(conn) is False
        assert mail_config.current()['source'] == 'environment'

    def test_a_username_without_a_password_is_not_configured(self):
        with auth.transaction() as conn:
            mail_config.save(conn, {**GOOD, 'password': ''}, 7)
        assert mail_config.is_configured() is False

    def test_a_relay_without_login_is_configured(self):
        with auth.transaction() as conn:
            mail_config.save(conn, {**GOOD, 'username': '', 'password': '', 'security': 'none', 'port': 25}, 7)
        assert mail_config.is_configured() is True

    @pytest.mark.parametrize('raw', ['nem json', '[]', '{"host": "x"}', json.dumps({**GOOD, 'security': 'ssh'}),
                                     json.dumps({**GOOD, 'port': '587'})])
    def test_a_damaged_row_falls_back_to_the_environment(self, raw):
        auth.set_setting(mail_config.KEY, raw)
        assert mail_config.stored() is None and mail_config.current()['source'] == 'environment'

    def test_the_public_view_has_no_password(self):
        with auth.transaction() as conn:
            mail_config.save(conn, GOOD, 7)
        view = mail_config.public_view()
        assert view['password_set'] is True and 'password' not in view and 'password' not in view['environment']
        assert GOOD['password'] not in json.dumps(view)


# ---------------------------------------------------------------- lekérdezés és mentés

class TestSaveAndRead:
    def test_get_before_any_change(self, api):
        mail = api.get('/api/admin/system/mail').get_json()['mail']
        assert mail['source'] == 'environment' and mail['configured'] is False
        assert mail['defaults'] == {'starttls': 587, 'ssl': 465, 'none': 25}

    def test_saving_needs_sudo_and_a_reason(self, api):
        assert save(api).status_code == 401
        api.sudo()
        no_reason = api.patch('/api/admin/system/mail', GOOD)
        assert no_reason.status_code == 400 and no_reason.get_json()['field'] == 'reason'
        assert mail_config.stored() is None

    def test_save_applies_immediately_and_never_returns_the_password(self, api):
        api.sudo()
        response = save(api)
        assert response.status_code == 200
        assert GOOD['password'] not in response.get_data(as_text=True)
        mail = response.get_json()['mail']
        assert mail['source'] == 'database' and mail['configured'] is True and mail['password_set'] is True
        assert mail['host'] == 'smtp.example.org' and mail['updated_by_name']
        assert mail_config.current()['password'] == GOOD['password']
        assert GOOD['password'] not in api.get('/api/admin/system/mail').get_data(as_text=True)

    def test_the_audit_log_has_no_secret(self, api):
        api.sudo()
        save(api)
        entry = _audit('system.mail_update')[0]
        text = json.dumps(entry)
        assert GOOD['password'] not in text
        assert entry['details']['after']['password_set'] is True and entry['details']['password_changed'] is True
        assert entry['details']['before']['source'] == 'environment' and entry['reason'] == REASON

    def test_a_blank_password_keeps_the_saved_one_for_the_same_server(self, api):
        api.sudo()
        save(api)
        response = save(api, password=None, from_name='Új név')
        assert response.status_code == 200
        assert mail_config.current()['password'] == GOOD['password'] and mail_config.current()['from_name'] == 'Új név'
        assert _audit('system.mail_update')[0]['details']['password_changed'] is False

    def test_a_changed_server_needs_the_password_again(self, api):
        api.sudo()
        save(api)
        response = save(api, password=None, host='evil.example.net')
        assert response.status_code == 400 and response.get_json()['field'] == 'password'
        assert mail_config.current()['host'] == 'smtp.example.org'
        assert save(api, password=None, username='masik').status_code == 400
        assert save(api, password='uj-jelszo', host='other.example.org').status_code == 200

    def test_without_a_username_no_password_is_kept(self, api):
        api.sudo()
        assert save(api, username='', password='ignorált', security='none', port=25).status_code == 200
        assert mail_config.current()['password'] == '' and mail_config.is_configured() is True

    @pytest.mark.parametrize('field, value', [
        ('host', ''), ('host', 'nem érvényes host'), ('host', 'a' * 300), ('host', 'host;rm -rf'), ('host', None),
        ('port', 0), ('port', 70000), ('port', '587'), ('port', True), ('port', None),
        ('security', 'tls'), ('security', None),
        ('username', 'x\ny'), ('password', 'a\nb'), ('password', 5),
        ('from_address', ''), ('from_address', 'nem-cim'), ('from_address', 'a@b.hu\nBcc: x@y.hu'),
        ('from_name', 'Név\r\nBcc: x@y.hu'), ('from_name', 'n' * 81),
        ('verify_tls', 'igen'),
    ])
    def test_invalid_values_are_refused(self, api, field, value):
        api.sudo()
        response = save(api, **{field: value})
        assert response.status_code == 400 and response.get_json()['field'] == field
        assert mail_config.stored() is None

    def test_a_username_without_a_password_is_refused(self, api):
        api.sudo()
        response = save(api, password='')
        assert response.status_code == 400 and response.get_json()['field'] == 'password'

    def test_ipv4_and_ipv6_hosts_are_accepted(self, api):
        api.sudo()
        assert save(api, host='192.168.1.10').status_code == 200
        assert save(api, host='::1', password='x').status_code == 200

    def test_the_overview_alert_goes_away(self, api):
        codes = lambda: {a['code'] for a in api.get('/api/admin/overview').get_json()['alerts']}   # noqa: E731
        assert 'smtp_missing' in codes()
        api.sudo()
        save(api)
        assert 'smtp_missing' not in codes()
        assert api.get('/api/admin/overview').get_json()['services']['smtp'] is True

    def test_the_config_table_shows_the_effective_values(self, api):
        api.sudo()
        save(api)
        items = {i['key']: i for i in api.get('/api/admin/system').get_json()['config']}
        assert items['SMTP_HOST']['value'] == 'smtp.example.org' and items['SMTP_SOURCE']['value'] == 'database'
        assert items['SMTP_PASSWORD']['secret'] is True and GOOD['password'] not in json.dumps(items)

    def test_a_saved_configuration_is_used_for_sending(self, api):
        api.sudo()
        save(api, security='ssl', port=465)
        with patch('email_service.smtplib.SMTP_SSL') as secure:
            server_mock = MagicMock()
            secure.return_value.__enter__ = MagicMock(return_value=server_mock)
            secure.return_value.__exit__ = MagicMock(return_value=False)
            assert email_service.send_plain_email('x@example.com', 'T', 'S', wait=True) == (True, None)
        assert secure.call_args[0] == ('smtp.example.org', 465)
        server_mock.login.assert_called_once_with('mailer', GOOD['password'])
        assert 'From: Magyar Scrabble <noreply@example.org>' in server_mock.sendmail.call_args[0][2]


# ---------------------------------------------------------------- visszaállítás

class TestReset:
    def test_reset_needs_sudo(self, api):
        assert api.delete('/api/admin/system/mail', {'reason': REASON}).status_code == 401

    def test_reset_needs_a_reason(self, api):
        api.sudo()
        save(api)
        response = api.delete('/api/admin/system/mail', {})
        assert response.status_code == 400 and mail_config.stored() is not None

    def test_reset_returns_to_the_environment(self, api):
        api.sudo()
        save(api)
        response = api.delete('/api/admin/system/mail', {'reason': REASON})
        assert response.status_code == 200 and response.get_json()['mail']['source'] == 'environment'
        assert mail_config.stored() is None
        assert _audit('system.mail_reset')[0]['details']['before']['source'] == 'database'

    def test_reset_without_a_saved_setting_is_a_conflict(self, api):
        api.sudo()
        assert api.delete('/api/admin/system/mail', {'reason': REASON}).status_code == 409


# ---------------------------------------------------------------- kapcsolat-próba

class TestCheck:
    def test_needs_sudo(self, api):
        assert api.post('/api/admin/system/mail/check', GOOD).status_code == 401

    def test_success_does_not_save_anything(self, api, monkeypatch):
        calls = []
        monkeypatch.setattr(email_service, 'check_connection', lambda cfg, recipient=None: calls.append((cfg, recipient)) or (True, 'ok'))
        api.sudo()
        data = api.post('/api/admin/system/mail/check', GOOD).get_json()
        assert data['ok'] is True and data['code'] == 'ok' and data['sent_to'] is None
        assert calls[0][0]['host'] == 'smtp.example.org' and calls[0][1] is None
        assert mail_config.stored() is None

    def test_a_test_message_goes_to_the_admin_only(self, api, monkeypatch):
        calls = []
        monkeypatch.setattr(email_service, 'check_connection', lambda cfg, recipient=None: calls.append(recipient) or (True, 'ok'))
        api.sudo()
        data = api.post('/api/admin/system/mail/check', {**GOOD, 'send': True, 'recipient': 'masvalaki@example.com'}).get_json()
        assert calls == [data['sent_to']] and data['sent_to'] and data['sent_to'] != 'masvalaki@example.com'

    def test_failure_returns_a_code_and_is_logged_without_secrets(self, api, monkeypatch):
        monkeypatch.setattr(email_service, 'check_connection', lambda cfg, recipient=None: (False, 'auth'))
        api.sudo()
        data = api.post('/api/admin/system/mail/check', GOOD).get_json()
        assert data['ok'] is False and data['code'] == 'auth' and data['sent_to'] is None
        entry = _audit('system.mail_check')[0]
        assert entry['details']['code'] == 'auth' and GOOD['password'] not in json.dumps(entry)

    def test_the_draft_is_validated(self, api):
        api.sudo()
        assert api.post('/api/admin/system/mail/check', {**GOOD, 'host': ''}).status_code == 400

    def test_a_stored_password_is_not_sent_to_a_different_host(self, api, monkeypatch):
        seen = []
        monkeypatch.setattr(email_service, 'check_connection', lambda cfg, recipient=None: seen.append(cfg) or (True, 'ok'))
        api.sudo()
        save(api)
        assert api.post('/api/admin/system/mail/check', {**GOOD, 'password': None, 'host': 'elsewhere.example.net'}).status_code == 400
        assert seen == []
        assert api.post('/api/admin/system/mail/check', {**GOOD, 'password': None}).status_code == 200
        assert seen[0]['password'] == GOOD['password']

    def test_the_saved_configuration_test_reports_the_failure_code(self, api, monkeypatch):
        monkeypatch.setattr(email_service, 'send_plain_email', lambda *a, **k: (False, 'auth'))
        response = api.post('/api/admin/system/smtp-test', {})
        assert response.status_code == 502 and response.get_json()['code'] == 'auth'
