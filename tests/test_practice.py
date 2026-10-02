"""Gyakorló módok: szókvíz és rövid szavak."""
import random

import pytest

import dictionary
import practice
from tiles import tokenize_word


@pytest.fixture(autouse=True)
def _ready():
    import ai_player
    dictionary.warm_up()
    ai_player.get_vocabulary()


@pytest.fixture(autouse=True)
def _fresh_rate_limits():
    import server
    server.rate_limiter._ip_history.clear()
    yield
    server.rate_limiter._ip_history.clear()


class TestShortWords:
    def test_two_tile_words(self):
        words = practice.short_words(2)
        names = {e['word'] for e in words}
        assert 100 < len(words) < 400
        assert {'AZ', 'EZ', 'BE', 'EL', 'AGY', 'CSŐ'} <= names     # AGY = A+GY, CSŐ = CS+Ő: két zseton
        assert all(len(tokenize_word(w)) == 2 for w in names)
        assert 'XY' not in names and 'BK' not in names

    def test_every_listed_word_is_valid_and_scored(self):
        for entry in practice.short_words(2)[:60]:
            assert dictionary.filter_valid({entry['word']}) == {entry['word']}
            assert entry['score'] >= 2
        assert next(e for e in practice.short_words(2) if e['word'] == 'AZ')['score'] == 5

    def test_three_tile_words_are_a_superset_in_size(self):
        three = practice.short_words(3)
        assert len(three) > len(practice.short_words(2))
        assert all(len(tokenize_word(e['word'])) == 3 for e in three)

    def test_sorted_and_cached(self):
        words = practice.short_words(2)
        assert [e['word'] for e in words] == sorted(e['word'] for e in words)
        assert practice.short_words(2) is words

    def test_other_lengths_are_refused(self):
        with pytest.raises(ValueError):
            practice.short_words(4)


class TestQuiz:
    def _classify(self, questions):
        return [practice.check_answer(w, True)['valid'] for w in questions]

    def test_mixed_quiz_is_half_valid_half_invalid(self):
        for seed in range(5):
            questions = practice.make_quiz(10, rng=random.Random(seed))
            assert len(questions) == 10 == len(set(questions))
            valid = self._classify(questions)
            assert valid.count(True) == 5 and valid.count(False) == 5

    def test_odd_counts_split_unevenly_but_cover_both(self):
        questions = practice.make_quiz(7, rng=random.Random(1))
        valid = self._classify(questions)
        assert len(questions) == 7 and 3 <= valid.count(True) <= 4

    def test_questions_are_words_the_game_can_place(self):
        for word in practice.make_quiz(20, rng=random.Random(2)):
            tokens = tokenize_word(word)
            assert tokens is not None and 2 <= len(tokens) <= 9
            assert any(t[0] in 'AÁEÉIÍOÓÖŐUÚÜŰ' for t in tokens)

    def test_questions_do_not_reveal_the_answer(self):
        questions = practice.make_quiz(10, rng=random.Random(3))
        assert all(isinstance(q, str) for q in questions)

    def test_short_quiz_uses_only_that_length(self):
        for length in (2, 3):
            questions = practice.make_quiz(10, length, rng=random.Random(4))
            assert all(len(tokenize_word(w)) == length for w in questions)
            valid = self._classify(questions)
            assert valid.count(True) == 5 and valid.count(False) == 5

    def test_quiz_is_random(self):
        a = practice.make_quiz(10, rng=random.Random(5))
        b = practice.make_quiz(10, rng=random.Random(6))
        assert a != b

    def test_count_is_clamped(self):
        assert len(practice.make_quiz(1, rng=random.Random(1))) == 2
        assert len(practice.make_quiz(500, rng=random.Random(1))) <= practice.MAX_QUESTIONS

    def test_fake_words_are_plausible_edits(self):
        fakes = practice._fake_words(['ALMA', 'KÖRTE', 'SZÉK'], 8, random.Random(7))
        assert len(fakes) == 8
        assert not (dictionary.filter_valid(set(fakes)))
        assert all(any(c in 'AÁEÉIÍOÓÖŐUÚÜŰ' for c in w) for w in fakes)


class TestCheckAnswer:
    def test_valid_word(self):
        result = practice.check_answer('ALMÁT', True)
        assert result['valid'] and result['correct'] and result['score'] == 5
        assert result['tiles'] == ['A', 'L', 'M', 'Á', 'T'] and result['suggestions'] == []

    def test_wrong_guess_on_a_valid_word(self):
        assert practice.check_answer('alma', False)['correct'] is False

    def test_invalid_word_with_suggestions(self):
        result = practice.check_answer('ALMTÁ', False)
        assert not result['valid'] and result['correct']
        assert 'ALMÁT' in result['suggestions']

    def test_malformed_words(self):
        assert practice.check_answer('', True) is None
        assert practice.check_answer('x', True) is None
        assert practice.check_answer('QWXY', True) is None
        assert practice.check_answer('A' * 20, True) is None
        assert practice.check_answer(None, True) is None


@pytest.fixture
def client():
    import server
    server.app.config['TESTING'] = True
    return server.app.test_client()


class TestApi:
    def test_quiz_endpoint(self, client):
        data = client.get('/api/practice/quiz').get_json()
        assert data['success'] and len(data['questions']) == 10
        assert all(isinstance(q, str) for q in data['questions'])

    def test_quiz_options(self, client):
        data = client.get('/api/practice/quiz?n=6&length=2').get_json()
        assert len(data['questions']) == 6
        assert all(len(tokenize_word(q)) == 2 for q in data['questions'])
        assert client.get('/api/practice/quiz?length=9').status_code == 400
        assert len(client.get('/api/practice/quiz?n=abc').get_json()['questions']) == 10

    def test_answer_endpoint(self, client):
        data = client.post('/api/practice/answer', json={'word': 'ALMÁT', 'answer': True}).get_json()
        assert data['success'] and data['valid'] and data['correct']
        wrong = client.post('/api/practice/answer', json={'word': 'ALMTÁ', 'answer': True}).get_json()
        assert wrong['valid'] is False and wrong['correct'] is False and wrong['suggestions']

    def test_answer_validation(self, client):
        assert client.post('/api/practice/answer', json={'word': 'ALMA'}).status_code == 400
        assert client.post('/api/practice/answer', json={'word': 'ALMA', 'answer': 'igen'}).status_code == 400
        assert client.post('/api/practice/answer', json={'word': 5, 'answer': True}).status_code == 400
        assert client.post('/api/practice/answer', json={'word': 'QWX', 'answer': True}).status_code == 400
        assert client.post('/api/practice/answer', data='nem json').status_code == 400

    def test_short_words_endpoint(self, client):
        data = client.get('/api/practice/short-words?length=2').get_json()
        assert data['success'] and data['length'] == 2 and any(e['word'] == 'AZ' for e in data['words'])
        assert client.get('/api/practice/short-words').get_json()['length'] == 2
        assert client.get('/api/practice/short-words?length=5').status_code == 400
        assert client.get('/api/practice/short-words?length=x').status_code == 400

    def test_unavailable_dictionary_is_reported(self, client, monkeypatch):
        monkeypatch.setattr(dictionary, 'is_available', lambda: False)
        assert client.get('/api/practice/quiz').status_code == 503
        assert client.post('/api/practice/answer', json={'word': 'ALMA', 'answer': True}).status_code == 503
        assert client.get('/api/practice/short-words').status_code == 503

    def test_rate_limited(self, client):
        codes = [client.post('/api/practice/answer', json={'word': 'ALMA', 'answer': True}).status_code
                 for _ in range(121)]
        assert codes[-1] == 429
