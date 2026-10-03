"""Gyakorló módok: szókvíz (csapdákkal), betűvadászat / bingó-edző és a rövid szavak."""
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
            questions = practice.make_quiz(10, str(length), rng=random.Random(4))
            assert all(len(tokenize_word(w)) == length for w in questions)
            valid = self._classify(questions)
            assert valid.count(True) == 5 and valid.count(False) == 5

    def test_unknown_mode_is_refused(self):
        with pytest.raises(ValueError):
            practice.make_quiz(10, 'nehéz')

    def test_tricky_quiz_fakes_are_vowel_length_swaps_of_valid_words(self):
        for seed in range(4):
            questions = practice.make_quiz(10, 'tricky', rng=random.Random(seed))
            valid = self._classify(questions)
            assert 4 <= valid.count(True) <= 6 and len(questions) >= 8
            for word, ok in zip(questions, valid):
                assert any(c in 'AÁEÉIÍOÓÖŐUÚÜŰ' for c in word)
                if not ok:
                    # a hamis szó egyetlen magánhangzó hosszúságának cseréjével lesz érvényes
                    twins = [word[:i] + practice._LENGTH_PAIRS[c.lower()].upper() + word[i + 1:]
                             for i, c in enumerate(word) if c.lower() in practice._LENGTH_PAIRS]
                    assert any(t in dictionary.filter_valid(set(twins)) for t in twins)

    def test_tricky_fakes_come_from_different_words(self):
        fakes = practice._fake_words(['ARANYÉR', 'TERVEZD', 'CELLULÓZ', 'DEZODOR'], 4, random.Random(1),
                                     mutate=practice._mutate_length)
        # négy különböző forrásszóból négy különböző hamis szó (nincs két átírás ugyanabból)
        assert len(set(fakes)) == 4

    def test_length_swap_mutation(self):
        rng = random.Random(0)
        assert practice._mutate_length('KRT', rng) is None
        swapped = practice._mutate_length('ALMA', rng)
        assert swapped != 'ALMA' and len(swapped) == 4 and sum(a != b for a, b in zip(swapped, 'ALMA')) == 1
        assert practice._mutate_length('ŐZ', rng) == 'ÖZ'

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


class TestRackWords:
    RACK = ['K', 'Ú', 'P', 'H', 'O', 'Z', 'N']

    def test_finds_words_that_fit_the_rack(self):
        words = {e['word'] for e in practice.rack_words(['A', 'L', 'M', 'Á', 'T', 'K', 'E'])}
        assert {'ALMÁT', 'ALMÁK', 'MÁK', 'TÁL', 'KELT'} <= words
        assert 'ALMÁKAT' not in words            # nincs elég zseton

    def test_every_word_is_valid_and_buildable(self):
        for entry in practice.rack_words(self.RACK):
            assert dictionary.filter_valid({entry['word']})
            assert practice.check_rack_word(self.RACK, entry['word'])['ok']
            assert 2 <= entry['tiles'] <= practice.RACK_SIZE

    def test_sorted_by_score_and_scored_by_tile_values(self):
        words = practice.rack_words(self.RACK)
        assert [e['score'] for e in words] == sorted((e['score'] for e in words), reverse=True)
        assert next(e for e in words if e['word'] == 'HÚZ')['score'] == 3 + 7 + 4

    def test_digraph_tiles_count_as_one_tile(self):
        words = {e['word']: e for e in practice.rack_words(['SZ', 'Ó', 'T', 'A', 'L', 'M', 'K'])}
        assert words['SZÓ']['tiles'] == 2 and words['SZÓ']['score'] == 3 + 2

    def test_bingo_bonus_when_all_seven_tiles_are_used(self):
        words = {e['word']: e for e in practice.rack_words(['K', 'Ó', 'B', 'O', 'R', 'R', 'A'])}
        assert words['KÓBORRA']['tiles'] == 7
        assert words['KÓBORRA']['score'] == 1 + 2 + 2 + 1 + 1 + 1 + 1 + practice.BINGO_BONUS


class TestMakeRack:
    def test_hunt_rack_is_playable(self):
        for seed in range(6):
            data = practice.make_rack('hunt', random.Random(seed))
            assert data['kind'] == 'hunt' and len(data['rack']) == practice.RACK_SIZE
            assert '' not in data['rack'] and all(t in practice.LETTERS for t in data['rack'])
            assert practice._count_ok(data['rack'])
            assert 2 <= sum(1 for t in data['rack'] if t[0] in 'AÁEÉIÍOÓÖŐUÚÜŰ') <= 4
            assert data['words'] and max(w['tiles'] for w in data['words']) >= 2
            assert data['total_score'] == sum(w['score'] for w in data['words'])

    def test_hunt_rack_has_enough_words(self):
        counts = [len(practice.make_rack('hunt', random.Random(s))['words']) for s in range(8)]
        assert sum(1 for n in counts if practice.HUNT_MIN_WORDS <= n <= practice.HUNT_MAX_WORDS) >= 6

    def test_bingo_rack_always_has_a_seven_tile_word(self):
        for seed in range(8):
            data = practice.make_rack('bingo', random.Random(seed))
            assert data['kind'] == 'bingo' and len(data['rack']) == practice.RACK_SIZE
            assert data['words'] and all(w['tiles'] == practice.RACK_SIZE for w in data['words'])
            for entry in data['words']:
                assert practice.check_rack_word(data['rack'], entry['word'])['bingo']
                assert entry['score'] >= practice.BINGO_BONUS

    def test_racks_are_random(self):
        racks = {tuple(practice.make_rack('hunt', random.Random(s))['rack']) for s in range(6)}
        assert len(racks) > 1

    def test_unknown_kind_is_refused(self):
        with pytest.raises(ValueError):
            practice.make_rack('szokas')


class TestCheckRackWord:
    RACK = ['A', 'L', 'M', 'Á', 'T', 'K', 'E']

    def test_valid_word(self):
        result = practice.check_rack_word(self.RACK, 'almát')
        assert result['ok'] and result['word'] == 'ALMÁT' and result['score'] == 5
        assert result['tiles'] == ['A', 'L', 'M', 'Á', 'T'] and not result['bingo']

    def test_not_a_word(self):
        assert practice.check_rack_word(self.RACK, 'ALMTÁ')['reason'] == 'not_a_word'

    def test_not_in_rack(self):
        assert practice.check_rack_word(self.RACK, 'ALMAK')['reason'] == 'not_in_rack'   # csak egy A van
        assert practice.check_rack_word(self.RACK, 'KÖRTE')['reason'] == 'not_in_rack'

    def test_too_short_and_bad_characters(self):
        assert practice.check_rack_word(self.RACK, 'A')['reason'] == 'too_short'
        assert practice.check_rack_word(self.RACK, '')['reason'] == 'too_short'
        assert practice.check_rack_word(self.RACK, None)['reason'] == 'too_short'
        assert practice.check_rack_word(self.RACK, 'QWX')['reason'] == 'invalid_chars'
        assert practice.check_rack_word(self.RACK, 'A' * 20)['reason'] == 'invalid_chars'

    def test_digraph_can_be_one_tile_or_two(self):
        one = practice.check_rack_word(['SZ', 'Ó', 'A'], 'SZÓ')
        two = practice.check_rack_word(['S', 'Z', 'Ó'], 'SZÓ')
        assert one['ok'] and one['tiles'] == ['SZ', 'Ó'] and one['score'] == 5
        assert two['ok'] and two['tiles'] == ['S', 'Z', 'Ó'] and two['score'] == 1 + 4 + 2
        # ha mindkét kirakás megvan, a több pontot érő számít
        both = practice.check_rack_word(['SZ', 'S', 'Z', 'Ó'], 'SZÓ')
        assert both['score'] == 7

    def test_bingo_bonus(self):
        result = practice.check_rack_word(['K', 'Ó', 'B', 'O', 'R', 'R', 'A'], 'KÓBORRA')
        assert result['ok'] and result['bingo'] and result['score'] == 9 + practice.BINGO_BONUS


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
        assert data['success'] and data['mode'] == 'mixed' and len(data['questions']) == 10
        assert all(isinstance(q, str) for q in data['questions'])
        # a kérdésekhez a zsetonok is járnak (a kliens ezekből rajzolja a szót)
        assert data['tiles'] == [tokenize_word(q) for q in data['questions']]

    def test_quiz_options(self, client):
        data = client.get('/api/practice/quiz?n=6&mode=2').get_json()
        assert len(data['questions']) == 6 and data['mode'] == '2'
        assert all(len(tokenize_word(q)) == 2 for q in data['questions'])
        assert len(client.get('/api/practice/quiz?n=8&mode=tricky').get_json()['questions']) >= 6
        assert client.get('/api/practice/quiz?mode=9').status_code == 400
        assert client.get('/api/practice/quiz?mode=').status_code == 400
        assert len(client.get('/api/practice/quiz?n=abc').get_json()['questions']) == 10

    def test_rack_endpoint(self, client):
        data = client.get('/api/practice/rack').get_json()
        assert data['success'] and data['kind'] == 'hunt' and len(data['rack']) == 7
        assert data['words'] and {'word', 'score', 'tiles'} <= set(data['words'][0])
        bingo = client.get('/api/practice/rack?kind=bingo').get_json()
        assert bingo['kind'] == 'bingo' and all(w['tiles'] == 7 for w in bingo['words'])
        assert client.get('/api/practice/rack?kind=x').status_code == 400

    def test_rack_word_endpoint(self, client):
        rack = ['A', 'L', 'M', 'Á', 'T', 'K', 'E']
        ok = client.post('/api/practice/rack-word', json={'rack': rack, 'word': 'almát'}).get_json()
        assert ok['success'] and ok['ok'] and ok['score'] == 5
        bad = client.post('/api/practice/rack-word', json={'rack': rack, 'word': 'almtá'}).get_json()
        assert bad['success'] and not bad['ok'] and bad['reason'] == 'not_a_word'
        assert client.post('/api/practice/rack-word', json={'rack': rack, 'word': 'körte'}).get_json()['reason'] == 'not_in_rack'

    def test_rack_word_validation(self, client):
        post = lambda **body: client.post('/api/practice/rack-word', json=body).status_code
        assert post(word='ALMA') == 400                                   # nincs kéz
        assert post(rack='ALMA', word='ALMA') == 400                      # a kéz nem lista
        assert post(rack=['A', 'Q'], word='A') == 400                     # nincs ilyen zseton
        assert post(rack=['A'] * 9, word='AA') == 400                     # túl sok zseton
        assert post(rack=['A', ''], word='AA') == 400                     # joker nem szerepelhet
        assert post(rack=['A', 'L'], word=5) == 400
        assert client.post('/api/practice/rack-word', data='nem json').status_code == 400

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
        assert client.get('/api/practice/rack').status_code == 503
        assert client.post('/api/practice/rack-word', json={'rack': ['A', 'L'], 'word': 'AL'}).status_code == 503

    def test_rate_limited(self, client):
        codes = [client.post('/api/practice/answer', json={'word': 'ALMA', 'answer': True}).status_code
                 for _ in range(121)]
        assert codes[-1] == 429
