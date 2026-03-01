"""Tests for dictionary.py - word validation."""
import pytest
from unittest.mock import patch, MagicMock


@pytest.fixture(autouse=True)
def reset_checker():
    """Reset dictionary checker state before each test."""
    import dictionary
    dictionary._checker = None
    dictionary._checker_type = None
    yield
    dictionary._checker = None
    dictionary._checker_type = None


class TestCheckWordsInputValidation:
    """Test input validation (before checker is used)."""

    def test_empty_list(self):
        from dictionary import check_words
        valid, invalid = check_words([])
        assert valid is True
        assert invalid == []

    def test_non_string_input(self):
        from dictionary import check_words
        valid, invalid = check_words([123])
        assert valid is False
        assert invalid == ['123']

    def test_too_long_word(self):
        from dictionary import check_words
        valid, invalid = check_words(['A' * 16])
        assert valid is False

    def test_word_with_invalid_characters(self):
        from dictionary import check_words
        valid, invalid = check_words(['HELLO!'])
        assert valid is False
        assert 'HELLO!' in invalid

    def test_word_with_spaces(self):
        from dictionary import check_words
        valid, invalid = check_words(['AL MA'])
        assert valid is False

    def test_word_with_lowercase(self):
        from dictionary import check_words
        valid, invalid = check_words(['alma'])
        assert valid is False

    def test_word_with_digits(self):
        from dictionary import check_words
        valid, invalid = check_words(['ABC123'])
        assert valid is False

    def test_max_length_word_ok(self):
        """A 15-character word should pass input validation."""
        from dictionary import check_words
        # Patch checker to accept everything
        import dictionary
        dictionary._checker_type = 'none'
        valid, invalid = check_words(['A' * 15])
        # No checker available => accepted
        assert valid is True

    def test_hungarian_accented_characters(self):
        """Valid Hungarian accented characters should pass validation."""
        from dictionary import check_words
        import dictionary
        dictionary._checker_type = 'none'
        valid, invalid = check_words(['ÁÉÍÓÖŐÚÜŰ'])
        assert valid is True


class TestCheckWordsNoChecker:
    """When no dictionary checker is available, all valid-format words are accepted."""

    def test_no_checker_accepts_all(self):
        from dictionary import check_words
        import dictionary
        # Force no checker
        dictionary._checker_type = 'none'
        valid, invalid = check_words(['BÁRMILYEN', 'SZÓ'])
        assert valid is True
        assert invalid == []


class TestCheckWordsWithEnchant:
    """Test with mocked enchant checker."""

    def test_enchant_valid_words(self):
        from dictionary import check_words
        import dictionary
        mock_checker = MagicMock()
        mock_checker.check.return_value = True
        dictionary._checker = mock_checker
        dictionary._checker_type = 'enchant'

        valid, invalid = check_words(['ALMA', 'KÖRTE'])
        assert valid is True
        assert invalid == []
        assert mock_checker.check.call_count == 2

    def test_enchant_invalid_word(self):
        from dictionary import check_words
        import dictionary
        mock_checker = MagicMock()
        mock_checker.check.side_effect = lambda w: w != 'XYZZY'
        dictionary._checker = mock_checker
        dictionary._checker_type = 'enchant'

        valid, invalid = check_words(['ALMA', 'XYZZY'])
        assert valid is False
        assert 'XYZZY' in invalid

    def test_enchant_all_invalid(self):
        from dictionary import check_words
        import dictionary
        mock_checker = MagicMock()
        mock_checker.check.return_value = False
        dictionary._checker = mock_checker
        dictionary._checker_type = 'enchant'

        valid, invalid = check_words(['XXX', 'YYY'])
        assert valid is False
        assert len(invalid) == 2


class TestCheckWordsWithCLI:
    """Test with mocked hunspell CLI."""

    def test_cli_valid_words(self):
        from dictionary import check_words
        import dictionary
        dictionary._checker_type = 'cli'

        mock_result = MagicMock()
        mock_result.stdout = ''
        with patch('dictionary.subprocess.run', return_value=mock_result):
            valid, invalid = check_words(['ALMA'])
            assert valid is True

    def test_cli_invalid_words(self):
        from dictionary import check_words
        import dictionary
        dictionary._checker_type = 'cli'

        mock_result = MagicMock()
        mock_result.stdout = 'XYZZY\n'
        with patch('dictionary.subprocess.run', return_value=mock_result):
            valid, invalid = check_words(['ALMA', 'XYZZY'])
            assert valid is False
            assert 'XYZZY' in invalid

    def test_cli_timeout_fallback(self):
        from dictionary import check_words
        import dictionary, subprocess
        dictionary._checker_type = 'cli'

        with patch('dictionary.subprocess.run', side_effect=subprocess.TimeoutExpired('cmd', 5)):
            valid, invalid = check_words(['ALMA'])
            # Timeout => fallback to accepting
            assert valid is True


class TestInitChecker:
    def test_init_with_enchant(self):
        import dictionary

        mock_dict = MagicMock()
        mock_enchant = MagicMock()
        mock_enchant.Dict.return_value = mock_dict

        with patch.dict('sys.modules', {'enchant': mock_enchant}):
            dictionary._init_checker()
            assert dictionary._checker_type == 'enchant'
            assert dictionary._checker is mock_dict

    def test_init_fallback_to_cli(self):
        import dictionary

        # enchant import fails, hunspell CLI works
        def fail_enchant_import():
            raise ImportError("no enchant")

        with patch('builtins.__import__', side_effect=lambda name, *a, **kw: fail_enchant_import() if name == 'enchant' else __import__(name, *a, **kw)):
            mock_result = MagicMock()
            mock_result.returncode = 0
            with patch('dictionary.subprocess.run', return_value=mock_result):
                dictionary._init_checker()
                assert dictionary._checker_type == 'cli'

    def test_init_no_checker_available(self):
        import dictionary

        with patch('builtins.__import__', side_effect=lambda name, *a, **kw: (_ for _ in ()).throw(ImportError()) if name == 'enchant' else __import__(name, *a, **kw)):
            with patch('dictionary.subprocess.run', side_effect=FileNotFoundError()):
                dictionary._init_checker()
                assert dictionary._checker_type is None
