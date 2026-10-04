"""Tests for dictionary.py - word validation."""
import pytest
from unittest.mock import patch


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


@pytest.fixture(scope='module')
def real_checker():
    """A beágyazott szótár egyszer betöltve (a betöltés ~0,3 mp)."""
    import dictionary
    return dictionary.load_checker()   # ugyanúgy, mint a szerveren (a használati listával)


@pytest.fixture
def real_dictionary(real_checker):
    import dictionary
    dictionary._checker = real_checker
    dictionary._checker_type = 'builtin'
    dictionary._init_attempted = True
    return dictionary


class TestBuiltinDictionary:
    """A beágyazott szótár a valódi hu_HU fájlokkal."""

    @pytest.mark.parametrize('word', [
        'ALMA', 'KÖRTE', 'SZÉKEK', 'ÉJSZAKA',            # szótő
        'ALMÁT', 'KUTYÁKNAK', 'HÁZAT', 'KÖNYVEKET',      # ragozott alak
        'LEGSZEBBEK', 'MEGETTE', 'LEGHÍRESEBBEK',     # előtag, többszörös toldalék
    ])
    def test_real_words_are_valid(self, real_dictionary, word):
        assert real_dictionary.check_words([word]) == (True, [])

    @pytest.mark.parametrize('word', [
        'SALYT', 'SALYAK', 'XQZ', 'ÁLMAK', 'KÖNYVKET', 'ALMAK',   # nem létező alakok
        'PAGONYAGY', 'EBÁSZ',                                       # összetétel-szabályokkal átcsúszó értelmetlen szavak
    ])
    def test_nonsense_words_are_invalid(self, real_dictionary, word):
        valid, invalid = real_dictionary.check_words([word])
        assert valid is False
        assert invalid == [word]

    def test_salyt_is_not_a_hungarian_word(self, real_dictionary):
        """Regresszió: a Szótár-eszköz "érvényes"-nek jelölte a SALYT-ot (nem volt szótár a szerveren)."""
        assert real_dictionary.is_word_valid('SALYT') is False
        assert real_dictionary.filter_valid(['SALYT', 'SÁLAT']) == {'SÁLAT'}

    @pytest.mark.parametrize('word', ['DUNA', 'BUDAPEST', 'MAGYARORSZÁG'])
    def test_proper_nouns_are_invalid(self, real_dictionary, word):
        assert real_dictionary.check_words([word])[0] is False

    @pytest.mark.parametrize('word', ['KG', 'DB', 'TV', 'SMS', 'PDF', 'TB', 'CS', 'SZ'])
    def test_abbreviations_and_letter_names_are_invalid(self, real_dictionary, word):
        assert real_dictionary.check_words([word])[0] is False

    @pytest.mark.parametrize('word', [
        'VU', 'UV', 'COS', 'SIN', 'KCAL', 'SZJA', 'MADAME',     # csak kötőjellel ragozható szócikkek
        'FALIM', 'FALIJA', 'TAROM', 'DOMBOSOM', 'BODZÁSUNK',    # melléknév + birtokos személyjel
        'ÉJÉK', 'CIRMOSÉK', 'OKOZATÉK',                          # -ék családi többes tárgyakon
        'BLÖKIÜL', 'CSÜRHÉÜL', 'EREZETÜL',                       # -ul/-ül főnéven
        'ALMÁNÉ',                                                # -né képző
    ])
    def test_grammatical_but_unused_forms_are_invalid(self, real_dictionary, word):
        """A szótár (helyesírás-ellenőrző) ezeket elfogadná, a Scrabble-ban értelmetlenek."""
        assert real_dictionary.check_words([word])[0] is False

    @pytest.mark.parametrize('word', [
        'KEDVESEM', 'DRÁGÁM', 'ÖREGEM', 'GYILKOSA', 'KEDVENCE',  # melléknév + birtokos: használt alakok
        'SZOMSZÉDÉK', 'ANYÁMÉK', 'FELESÉGÜL', 'AJÁNDÉKUL', 'MAGYARUL', 'KIRÁLYNÉ',
        'ROSSZUL', 'VÉLETLENÜL', 'TÉTLENÜL',                     # -ul/-ül melléknéven: szabályos
        'BETEGEM', 'OLVASÓM', 'TANULÓM', 'FIATALOK', 'GITÁROSOKNAK', 'HA', 'PÉLDÁUL',
    ])
    def test_similar_forms_in_real_use_stay_valid(self, real_dictionary, word):
        assert real_dictionary.check_words([word])[0] is True

    @pytest.mark.parametrize('word', ['BRR', 'HM', 'HMM', 'PSZT'])
    def test_vowelless_interjections_are_valid(self, real_dictionary, word):
        assert real_dictionary.check_words([word])[0] is True

    def test_all_words_must_be_valid(self, real_dictionary):
        valid, invalid = real_dictionary.check_words(['ALMA', 'SALYT', 'KÖRTE', 'XQZ'])
        assert valid is False
        assert invalid == ['SALYT', 'XQZ']

    def test_lookup_is_lowercase(self, real_dictionary, real_checker):
        asked = []
        original = real_checker.check
        real_checker.check = lambda w: asked.append(w) or original(w)
        try:
            real_dictionary.check_words(['ÁLMOK', 'SZÉK'])
        finally:
            del real_checker.check
        assert asked == ['álmok', 'szék']

    def test_filter_valid_agrees_with_check_words(self, real_dictionary):
        words = ['ALMA', 'SALYT', 'KUTYÁNAK', 'PAGONYAGY', 'DUNA', 'KG', 'HM']
        valid = real_dictionary.filter_valid(words)
        for word in words:
            assert (word in valid) == real_dictionary.check_words([word])[0], word


class TestAvailability:
    def test_available_with_embedded_dictionary(self):
        import dictionary
        assert dictionary.is_available() is True
        assert dictionary._checker_type == 'builtin'

    def test_warm_up_loads_the_dictionary_once(self):
        import dictionary
        assert dictionary.warm_up() is True
        checker = dictionary._checker
        assert dictionary.warm_up() is True
        assert dictionary._checker is checker

    def test_missing_dictionary_files_disable_checking(self, tmp_path):
        import dictionary
        with patch.object(dictionary, '_DICT_DIR', str(tmp_path)):
            dictionary._init_checker()
        assert dictionary._checker_type is None
        assert dictionary.is_available() is False
        # Szótár nélkül minden szót elfogadunk (és ezt a Szótár-eszköz jelzi)
        assert dictionary.check_words(['SALYT']) == (True, [])

    def test_missing_dictionary_is_initialised_only_once(self, tmp_path):
        import dictionary
        with patch.object(dictionary, '_DICT_DIR', str(tmp_path)), \
                patch('dictionary._init_checker', wraps=dictionary._init_checker) as init:
            dictionary.check_words(['ALMA'])
            dictionary.check_words(['KUTYA'])
            dictionary.check_words(['MACSKA'])
        assert init.call_count == 1

    def test_no_checker_suggests_nothing(self):
        import dictionary
        dictionary._checker_type = 'none'
        assert dictionary.suggest_words('SALYT') == []


class TestSuggestions:
    def test_accent_fix_comes_first(self, real_dictionary):
        assert real_dictionary.suggest_words('ALMAK')[0] == 'ALMÁK'

    def test_suggestions_are_valid_words(self, real_dictionary):
        for word in real_dictionary.suggest_words('SALYT'):
            assert real_dictionary.check_words([word])[0] is True

    def test_deletion_and_insertion(self, real_dictionary):
        assert 'KÖNYVEKET' in real_dictionary.suggest_words('KÖNYVEKT') or \
            'KÖNYVEKET' in real_dictionary.suggest_words('KÖNYVEKKET')
        assert 'ALMA' in real_dictionary.suggest_words('ALMMA')  # felesleges betű
        assert 'KUTYA' in real_dictionary.suggest_words('KUTY')  # hiányzó betű

    def test_swapped_letters(self, real_dictionary):
        assert 'KUTYA' in real_dictionary.suggest_words('KUYTA')


class TestBoardUsesTheRealDictionary:
    """Szótár-mock nélkül: a táblán a nem létező szó nem rakható le."""

    @staticmethod
    def _row(word, row=7, start=5):
        return [(row, start + i, ch, False) for i, ch in enumerate(word)]

    def test_nonsense_word_is_rejected(self, real_dictionary):
        from board import Board
        valid, _words, error = Board().validate_placement(self._row('SALKT'))
        assert valid is False
        assert 'SALKT' in error

    def test_real_word_is_accepted(self, real_dictionary):
        from board import Board
        valid, words, _error = Board().validate_placement(self._row('SALAK'))
        assert valid is True
        assert words[0][0] == 'SALAK'
