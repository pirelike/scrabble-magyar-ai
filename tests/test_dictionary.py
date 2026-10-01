"""Tests for dictionary.py - word validation."""
import pytest
from unittest.mock import patch, MagicMock


@pytest.fixture(autouse=True)
def reset_checker():
    """Reset dictionary checker state before each test."""
    import dictionary
    dictionary._checker = None
    dictionary._checker_type = None
    dictionary._init_attempted = False
    yield
    dictionary._checker = None
    dictionary._checker_type = None
    dictionary._init_attempted = False


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
        mock_checker.check.side_effect = lambda w: w != 'xyzzy'
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


class TestProperNounsAndLookup:
    """A tábla nagybetűs szavait kisbetűvel keressük: tulajdonnév nem érvényes Scrabble-szó."""

    def test_enchant_is_queried_with_lowercase_words(self):
        from dictionary import check_words
        import dictionary
        mock_checker = MagicMock()
        mock_checker.check.return_value = True
        dictionary._checker = mock_checker
        dictionary._checker_type = 'enchant'

        check_words(['ÁLMOK', 'SZÉK'])
        asked = [c.args[0] for c in mock_checker.check.call_args_list]
        assert asked == ['álmok', 'szék']

    def test_proper_noun_is_rejected_by_enchant(self):
        from dictionary import check_words
        import dictionary
        known = {'alma', 'duna'.capitalize()}  # a szótárban a tulajdonnév nagy kezdőbetűs
        mock_checker = MagicMock()
        mock_checker.check.side_effect = lambda w: w in known
        dictionary._checker = mock_checker
        dictionary._checker_type = 'enchant'

        valid, invalid = check_words(['ALMA', 'DUNA'])
        assert valid is False
        assert invalid == ['DUNA']

    def test_cli_receives_lowercase_and_reports_original_words(self):
        from dictionary import check_words
        import dictionary
        dictionary._checker_type = 'cli'
        mock_result = MagicMock()
        mock_result.stdout = 'duna\n'  # a hunspell kisbetűs bemenetre kisbetűvel válaszol
        with patch('dictionary.subprocess.run', return_value=mock_result) as run:
            valid, invalid = check_words(['ALMA', 'DUNA'])
        assert run.call_args.kwargs['input'] == 'alma\nduna'
        assert valid is False
        assert invalid == ['DUNA']

    def test_missing_dictionary_is_initialised_only_once(self):
        import dictionary
        with patch('dictionary.subprocess.run', side_effect=FileNotFoundError), \
                patch.dict('sys.modules', {'enchant': None}), \
                patch('dictionary._init_checker', wraps=dictionary._init_checker) as init:
            dictionary.check_words(['ALMA'])
            dictionary.check_words(['KUTYA'])
            dictionary.check_words(['MACSKA'])
        assert init.call_count == 1
