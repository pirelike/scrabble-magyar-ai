"""email_service: verifikációs és egyszerű levelek, a környezeti vagy az admin panelen mentett levelező beállítással,
valamint a kapcsolat-próba hibakódjai (a smtplib mindenhol hamisítva van)."""
import smtplib
import socket
import ssl
from types import SimpleNamespace
from unittest.mock import MagicMock, patch

import pytest

import auth
import config
import email_service
import mail_config


def use_environment(monkeypatch, host='smtp.test.com', port=587, user='user@test.com', password='secret',
                    sender='from@test.com'):
    """A környezeti változókból jövő beállítás (a régi viselkedés: STARTTLS)."""
    monkeypatch.setattr(config, 'SMTP_HOST', host)
    monkeypatch.setattr(config, 'SMTP_PORT', port)
    monkeypatch.setattr(config, 'SMTP_USER', user)
    monkeypatch.setattr(config, 'SMTP_PASSWORD', password)
    monkeypatch.setattr(config, 'SMTP_FROM', sender)
    monkeypatch.setattr(config, 'SMTP_CONFIGURED', all([user, password, sender]))


def store(**overrides):
    """Admin panelen mentett felülbírálat az ideiglenes adatbázisban."""
    cfg = {'host': 'mail.example.org', 'port': 465, 'security': 'ssl', 'username': 'mailer', 'password': 'pw-1',
           'from_address': 'noreply@example.org', 'from_name': 'Magyar Scrabble', 'verify_tls': True}
    cfg.update(overrides)
    with auth.transaction() as conn:
        mail_config.save(conn, cfg, None)
    return cfg


@pytest.fixture
def smtp():
    """Hamis `smtplib.SMTP` és `SMTP_SSL`; mindkettő ugyanazt a kapcsolatot adja."""
    with patch('email_service.smtplib.SMTP') as plain, patch('email_service.smtplib.SMTP_SSL') as secure:
        server = MagicMock()
        for factory in (plain, secure):
            factory.return_value.__enter__ = MagicMock(return_value=server)
            factory.return_value.__exit__ = MagicMock(return_value=False)
        yield SimpleNamespace(plain=plain, secure=secure, server=server)


def raw_message(server):
    return server.sendmail.call_args[0][2]


class TestSendVerificationEmail:
    def test_no_smtp_prints_to_console(self, capsys, monkeypatch):
        """When SMTP is not configured, code is printed to console."""
        monkeypatch.setattr(config, 'SMTP_CONFIGURED', False)
        email_service.send_verification_email('test@example.com', '123456')
        captured = capsys.readouterr()
        assert '123456' in captured.out
        assert 'test@example.com' in captured.out

    def test_with_smtp_starts_thread(self, monkeypatch):
        """When SMTP is configured, email is sent in a background thread."""
        use_environment(monkeypatch)
        with patch('email_service.threading.Thread') as mock_thread:
            mock_thread_instance = MagicMock()
            mock_thread.return_value = mock_thread_instance

            email_service.send_verification_email('test@example.com', '123456')

            mock_thread.assert_called_once()
            args = mock_thread.call_args
            assert args[1]['target'].__name__ == '_send_email'
            assert args[1]['args'] == ('test@example.com', '123456')
            assert args[1]['daemon'] is True
            mock_thread_instance.start.assert_called_once()

    def test_a_saved_configuration_turns_mail_on_without_environment(self, monkeypatch):
        monkeypatch.setattr(config, 'SMTP_CONFIGURED', False)
        store()
        with patch('email_service.threading.Thread') as mock_thread:
            email_service.send_verification_email('test@example.com', '123456')
            mock_thread.assert_called_once()


class TestSendEmailFromEnvironment:
    def test_sends_email_via_smtp_with_starttls(self, monkeypatch, smtp):
        use_environment(monkeypatch)
        email_service._send_email('recipient@example.com', '654321')

        smtp.plain.assert_called_once_with('smtp.test.com', 587, timeout=15)
        smtp.secure.assert_not_called()
        smtp.server.starttls.assert_called_once()
        smtp.server.login.assert_called_once_with('user@test.com', 'secret')
        smtp.server.sendmail.assert_called_once()
        send_args = smtp.server.sendmail.call_args[0]
        assert send_args[0] == 'from@test.com'
        assert send_args[1] == 'recipient@example.com'
        # The email body is MIME-encoded (base64), so check the raw message string
        message = raw_message(smtp.server)
        assert 'recipient@example.com' in message
        assert 'From: from@test.com' in message
        assert 'Date: ' in message and 'Message-ID: <' in message and '@test.com>' in message

    def test_the_certificate_is_verified_by_default(self, monkeypatch, smtp):
        use_environment(monkeypatch)
        email_service._send_email('recipient@example.com', '654321')
        context = smtp.server.starttls.call_args.kwargs['context']
        assert context.verify_mode == ssl.CERT_REQUIRED and context.check_hostname is True

    def test_smtp_error_prints_fallback(self, monkeypatch, capsys):
        """SMTP failure should print code to console as fallback."""
        use_environment(monkeypatch)
        with patch('email_service.smtplib.SMTP', side_effect=Exception('Connection refused')):
            email_service._send_email('test@example.com', '111222')
        captured = capsys.readouterr()
        assert '111222' in captured.out
        assert 'HIBA' in captured.out


class TestSendEmailFromSavedConfiguration:
    def test_ssl_mode_uses_smtp_ssl_without_starttls(self, monkeypatch, smtp):
        use_environment(monkeypatch)      # a mentett beállítás erősebb a környezetinél
        store()
        email_service._send_email('recipient@example.com', '654321')

        smtp.plain.assert_not_called()
        host, port = smtp.secure.call_args[0]
        assert (host, port) == ('mail.example.org', 465)
        assert smtp.secure.call_args.kwargs['timeout'] == 15
        assert smtp.secure.call_args.kwargs['context'].verify_mode == ssl.CERT_REQUIRED
        smtp.server.starttls.assert_not_called()
        smtp.server.login.assert_called_once_with('mailer', 'pw-1')
        assert smtp.server.sendmail.call_args[0][0] == 'noreply@example.org'

    def test_the_sender_name_goes_into_the_from_header_only(self, smtp):
        store(from_name='Magyar Scrabble')
        email_service._send_email('recipient@example.com', '654321')
        assert 'From: Magyar Scrabble <noreply@example.org>' in raw_message(smtp.server)
        assert smtp.server.sendmail.call_args[0][0] == 'noreply@example.org'

    def test_non_ascii_sender_name_is_encoded(self, smtp):
        store(from_name='Scrabble Klub Győr')
        email_service.send_plain_email('recipient@example.com', 'Tárgy', 'Szöveg', wait=True)
        message = raw_message(smtp.server)
        assert '=?utf-8?' in message.split('\n\n')[0] and '<noreply@example.org>' in message

    def test_no_login_without_a_username(self, smtp):
        store(security='none', port=25, username='', password='')
        ok, error = email_service.send_plain_email('recipient@example.com', 'Tárgy', 'Szöveg', wait=True)
        assert (ok, error) == (True, None)
        smtp.plain.assert_called_once_with('mail.example.org', 25, timeout=15)
        smtp.server.starttls.assert_not_called()
        smtp.server.login.assert_not_called()

    def test_certificate_check_can_be_turned_off(self, smtp):
        store(security='starttls', port=587, verify_tls=False)
        email_service.send_plain_email('recipient@example.com', 'Tárgy', 'Szöveg', wait=True)
        context = smtp.server.starttls.call_args.kwargs['context']
        assert context.verify_mode == ssl.CERT_NONE and context.check_hostname is False

    def test_the_change_applies_without_a_restart(self, smtp):
        store(host='first.example.org')
        email_service.send_plain_email('a@example.com', 'T', 'S', wait=True)
        store(host='second.example.org')
        email_service.send_plain_email('a@example.com', 'T', 'S', wait=True)
        assert [call[0][0] for call in smtp.secure.call_args_list] == ['first.example.org', 'second.example.org']


class TestSendPlainEmail:
    def test_not_configured_goes_to_the_console(self, monkeypatch, capsys):
        monkeypatch.setattr(config, 'SMTP_CONFIGURED', False)
        assert email_service.send_plain_email('a@example.com', 'Tárgy', 'Szöveg') == (False, 'smtp_not_configured')
        assert 'Tárgy' in capsys.readouterr().out

    def test_wait_returns_a_safe_error_code(self, monkeypatch):
        use_environment(monkeypatch)
        error = smtplib.SMTPAuthenticationError(535, b'5.7.8 Username and Password not accepted for host-secret-name')
        with patch('email_service.smtplib.SMTP', side_effect=error):
            ok, code = email_service.send_plain_email('a@example.com', 'T', 'S', wait=True)
        assert (ok, code) == (False, 'auth')           # a kiszolgáló szövege nem megy tovább

    def test_without_wait_it_runs_in_the_background(self, monkeypatch):
        use_environment(monkeypatch)
        with patch('email_service.threading.Thread') as thread:
            assert email_service.send_plain_email('a@example.com', 'T', 'S') == (True, None)
            thread.return_value.start.assert_called_once()


class TestClassifyError:
    @pytest.mark.parametrize('exc, code', [
        (ssl.SSLCertVerificationError('bad certificate'), 'certificate'),
        (ssl.SSLError('handshake'), 'tls'),
        (socket.gaierror(-2, 'Name or service not known'), 'dns'),
        (socket.timeout('timed out'), 'timeout'),
        (ConnectionRefusedError(111, 'refused'), 'refused'),
        (smtplib.SMTPAuthenticationError(535, b'no'), 'auth'),
        (smtplib.SMTPNotSupportedError('STARTTLS extension not supported by server.'), 'unsupported'),
        (smtplib.SMTPSenderRefused(550, b'no', 'a@b.hu'), 'sender'),
        (smtplib.SMTPRecipientsRefused({'a@b.hu': (550, b'no')}), 'recipient'),
        (smtplib.SMTPServerDisconnected('gone'), 'disconnected'),
        (smtplib.SMTPDataError(554, b'spam'), 'protocol'),
        (OSError(101, 'Network is unreachable'), 'connect'),
        (ValueError('x'), 'error'),
    ])
    def test_codes(self, exc, code):
        assert email_service.classify_error(exc) == code


class TestCheckConnection:
    CFG = {'host': 'mail.example.org', 'port': 587, 'security': 'starttls', 'username': 'u', 'password': 'p',
           'from_address': 'noreply@example.org', 'from_name': '', 'verify_tls': True}

    def test_connects_and_logs_in_without_sending(self, smtp):
        assert email_service.check_connection(self.CFG) == (True, 'ok')
        smtp.plain.assert_called_once_with('mail.example.org', 587, timeout=email_service.CHECK_TIMEOUT)
        smtp.server.login.assert_called_once_with('u', 'p')
        smtp.server.sendmail.assert_not_called()

    def test_sends_a_test_message_to_the_recipient(self, smtp):
        assert email_service.check_connection(self.CFG, 'boss@example.com') == (True, 'ok')
        send_args = smtp.server.sendmail.call_args[0]
        assert send_args[0] == 'noreply@example.org' and send_args[1] == 'boss@example.com'

    def test_failure_returns_a_code_only(self):
        with patch('email_service.smtplib.SMTP', side_effect=socket.gaierror(-2, 'secret internal name')):
            result = email_service.check_connection(self.CFG)
        assert result == (False, 'dns')

    def test_login_failure_is_reported_as_auth(self, smtp):
        smtp.server.login.side_effect = smtplib.SMTPAuthenticationError(535, b'nope')
        assert email_service.check_connection(self.CFG) == (False, 'auth')
