"""Tests for email_service.py - verification email sending."""
import pytest
from unittest.mock import patch, MagicMock


class TestSendVerificationEmail:
    def test_no_smtp_prints_to_console(self, capsys):
        """When SMTP is not configured, code is printed to console."""
        with patch('email_service.SMTP_CONFIGURED', False):
            from email_service import send_verification_email
            send_verification_email('test@example.com', '123456')
            captured = capsys.readouterr()
            assert '123456' in captured.out
            assert 'test@example.com' in captured.out

    def test_with_smtp_starts_thread(self):
        """When SMTP is configured, email is sent in a background thread."""
        with patch('email_service.SMTP_CONFIGURED', True):
            with patch('email_service.threading.Thread') as mock_thread:
                mock_thread_instance = MagicMock()
                mock_thread.return_value = mock_thread_instance

                from email_service import send_verification_email
                send_verification_email('test@example.com', '123456')

                mock_thread.assert_called_once()
                args = mock_thread.call_args
                assert args[1]['target'].__name__ == '_send_email'
                assert args[1]['args'] == ('test@example.com', '123456')
                assert args[1]['daemon'] is True
                mock_thread_instance.start.assert_called_once()


class TestSendEmail:
    def test_sends_email_via_smtp(self):
        """_send_email connects to SMTP and sends message."""
        with patch('email_service.SMTP_HOST', 'smtp.test.com'), \
             patch('email_service.SMTP_PORT', 587), \
             patch('email_service.SMTP_USER', 'user@test.com'), \
             patch('email_service.SMTP_PASSWORD', 'secret'), \
             patch('email_service.SMTP_FROM', 'from@test.com'):
            with patch('email_service.smtplib.SMTP') as mock_smtp:
                mock_server = MagicMock()
                mock_smtp.return_value.__enter__ = MagicMock(return_value=mock_server)
                mock_smtp.return_value.__exit__ = MagicMock(return_value=False)

                from email_service import _send_email
                _send_email('recipient@example.com', '654321')

                mock_smtp.assert_called_once_with('smtp.test.com', 587)
                mock_server.starttls.assert_called_once()
                mock_server.login.assert_called_once_with('user@test.com', 'secret')
                mock_server.sendmail.assert_called_once()
                send_args = mock_server.sendmail.call_args[0]
                assert send_args[0] == 'from@test.com'
                assert send_args[1] == 'recipient@example.com'
                # The email body is MIME-encoded (base64), so check the raw message string
                assert 'recipient@example.com' in send_args[2]

    def test_smtp_error_prints_fallback(self, capsys):
        """SMTP failure should print code to console as fallback."""
        with patch('email_service.SMTP_HOST', 'smtp.test.com'), \
             patch('email_service.SMTP_PORT', 587), \
             patch('email_service.SMTP_USER', 'user@test.com'), \
             patch('email_service.SMTP_PASSWORD', 'secret'), \
             patch('email_service.SMTP_FROM', 'from@test.com'):
            with patch('email_service.smtplib.SMTP', side_effect=Exception('Connection refused')):
                from email_service import _send_email
                _send_email('test@example.com', '111222')
                captured = capsys.readouterr()
                assert '111222' in captured.out
                assert 'HIBA' in captured.out
