"""Zseton-felbontás (tiles.py) és tömeges szótár-ellenőrzés (dictionary.py)."""
import pytest

import dictionary
from tiles import (
    TILE_COUNTS, TILE_DISTRIBUTION, TILE_VALUES, LETTERS, DIGRAPHS, VOWELS,
    tokenize_word, word_base_score,
)


class TestTileTables:
    def test_counts_sum_to_100(self):
        assert sum(TILE_COUNTS.values()) == 100

    def test_counts_match_distribution(self):
        for letter, _value, count in TILE_DISTRIBUTION:
            assert TILE_COUNTS[letter] == count

    def test_letters_exclude_blank(self):
        assert '' not in LETTERS
        assert len(LETTERS) == 38

    def test_digraphs(self):
        assert DIGRAPHS == {'CS', 'GY', 'LY', 'NY', 'SZ', 'TY', 'ZS'}

    def test_vowels(self):
        assert set('AÁEÉIÍOÓÖŐUÚÜŰ') == VOWELS


class TestTokenize:
    def test_plain_word(self):
        assert tokenize_word('alma') == ['A', 'L', 'M', 'A']

    def test_digraphs_count_as_one_tile(self):
        assert tokenize_word('szék') == ['SZ', 'É', 'K']
        assert tokenize_word('tyúk') == ['TY', 'Ú', 'K']
        assert tokenize_word('gyors') == ['GY', 'O', 'R', 'S']

    def test_prefers_fewer_tiles(self):
        # "asszony": A S SZ O NY (5 zseton) kevesebb, mint A S S Z O NY (6)
        assert tokenize_word('asszony') == ['A', 'S', 'SZ', 'O', 'NY']

    def test_uppercase_input(self):
        assert tokenize_word('SZÉK') == ['SZ', 'É', 'K']

    def test_unknown_character(self):
        assert tokenize_word('qwe') is None
        assert tokenize_word('al1ma') is None

    def test_empty(self):
        assert tokenize_word('') == []

    def test_tiles_rebuild_the_word(self):
        for word in ('KÖNYVEK', 'ZSÍR', 'LYUK', 'CSÁSZÁR'):
            assert ''.join(tokenize_word(word)) == word

    def test_base_score(self):
        assert word_base_score('alma') == 4
        assert word_base_score('szék') == TILE_VALUES['SZ'] + TILE_VALUES['É'] + TILE_VALUES['K']
        assert word_base_score('xyz') is None


class TestFilterValid:
    def test_valid_and_invalid(self):
        assert dictionary.filter_valid(['ALMA', 'XQZ']) == {'ALMA'}

    def test_inflected_forms_are_valid(self):
        assert 'ALMÁK' in dictionary.filter_valid(['ALMÁK'])
        assert 'KÖNYVEKET' in dictionary.filter_valid(['KÖNYVEKET'])

    def test_proper_nouns_are_invalid(self):
        assert dictionary.filter_valid(['BUDAPEST', 'DUNA']) == set()

    def test_empty_input(self):
        assert dictionary.filter_valid([]) == set()

    def test_malformed_words_are_never_valid(self):
        assert dictionary.filter_valid(['AL1MA', '', 'A' * 40, 'al ma']) == set()

    def test_non_strings_are_ignored(self):
        assert dictionary.filter_valid([None, 5, 'ALMA']) == {'ALMA'}

    def test_returns_original_spelling(self):
        result = dictionary.filter_valid(['ALMA'])
        assert result == {'ALMA'}

    def test_duplicates_are_fine(self):
        assert dictionary.filter_valid(['ALMA', 'ALMA']) == {'ALMA'}

    def test_cache_is_used(self, monkeypatch):
        dictionary.filter_valid(['KÖRTE'])
        calls = []
        real = dictionary._raw_check_many
        monkeypatch.setattr(dictionary, '_raw_check_many', lambda words: calls.append(words) or real(words))
        assert dictionary.filter_valid(['KÖRTE']) == {'KÖRTE'}
        assert calls == []  # a második hívás a gyorsítótárból válaszol

    def test_agrees_with_check_words(self):
        words = ['ALMA', 'KÖRTE', 'XQZ', 'SZÉKEK', 'FOO', 'ÉJSZAKA', 'BUDAPEST']
        valid = dictionary.filter_valid(words)
        for word in words:
            assert (word in valid) == dictionary.check_words([word])[0], word

    def test_cache_is_bounded(self, monkeypatch):
        monkeypatch.setattr(dictionary, '_VALID_CACHE_LIMIT', 5)
        dictionary._valid_cache.clear()
        for i, word in enumerate(['ALMA', 'KÖRTE', 'ASZTAL', 'SZÉK', 'LÁMPA', 'AJTÓ', 'ABLAK']):
            dictionary.filter_valid([word])
        assert len(dictionary._valid_cache) <= 5

    def test_is_word_valid(self):
        assert dictionary.is_word_valid('ALMA') is True
        assert dictionary.is_word_valid('XQZ') is False


class TestSuggestWords:
    def test_suggests_for_typo(self):
        suggestions = dictionary.suggest_words('ALMAK')
        assert suggestions and 'ALMA' in suggestions or 'ALMÁK' in suggestions

    def test_suggestions_are_board_playable(self):
        for word in dictionary.suggest_words('KÖNYVKET'):
            assert word == word.upper() and len(word) <= 15

    def test_limit(self):
        assert len(dictionary.suggest_words('ALMAK', limit=2)) <= 2

    def test_invalid_input(self):
        assert dictionary.suggest_words('al1ma') == []
        assert dictionary.suggest_words('') == []
        assert dictionary.suggest_words(None) == []
        assert dictionary.suggest_words('A' * 40) == []

    def test_does_not_suggest_the_word_itself(self):
        assert 'ALMAK' not in dictionary.suggest_words('ALMAK')
