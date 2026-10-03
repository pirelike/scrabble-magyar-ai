"""Szótár-építő: az elutasított szavak a szótárban, a szavazás és a küszöb, a mintavétel és az API."""
import random

import pytest

import auth
import config
import dictionary
import practice
import word_review
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


def _user(name='Anna'):
    ok, uid = auth.create_user(f'{name.lower()}@example.com', name, 'jelszo12345')
    assert ok
    return uid


def _valid(word):
    return word in dictionary.filter_valid({word})


class TestRejectedList:
    def test_file_is_parsed(self, tmp_path):
        path = tmp_path / 'rejected.txt'
        path.write_text('# megjegyzés\n\nAlma  # inline\nKÖRTE\n  barack \n', encoding='utf-8')
        assert dictionary.load_rejected(str(path)) == {'alma', 'körte', 'barack'}

    def test_missing_file_is_empty(self, tmp_path):
        assert dictionary.load_rejected(str(tmp_path / 'nincs.txt')) == frozenset()

    def test_listed_words_are_not_valid(self, monkeypatch):
        dictionary._valid_cache.clear()
        monkeypatch.setattr(dictionary, '_rejected_listed', frozenset({'alma'}))
        try:
            assert not _valid('ALMA')
            assert dictionary.check_words(['ALMA']) == (False, ['ALMA'])
            assert dictionary.is_rejected('Alma')
            assert _valid('ALMÁT')                       # a ragozott alak nem esik ki
            assert 'ALMA' not in dictionary.suggest_words('ALMAA', limit=20)
        finally:
            dictionary._valid_cache.clear()

    def test_shipped_list_is_clean(self):
        listed = dictionary.listed_rejected()
        assert len(listed) >= 100                         # az AI első átnézésének ítéletei
        checker = dictionary.get_checker()
        for word in listed:
            assert word == word.lower() and tokenize_word(word.upper()) is not None
            # csak olyan szó kerülhet a listára, amelyet a szótár egyébként elfogadna (nem elírás)
            assert checker.check(word), word

    def test_shipped_list_takes_effect(self):
        for word in sorted(dictionary.listed_rejected())[:25]:
            assert dictionary.is_rejected(word) and not _valid(word.upper()), word

    def test_shipped_list_is_sorted_and_unique(self):
        with open(dictionary._REJECTED_PATH, encoding='utf-8') as fh:
            words = [line.strip() for line in fh if line.strip() and not line.startswith('#')]
        assert words == sorted(set(words))

    def test_robot_vocabulary_leaves_them_out(self):
        import ai_player
        vocabulary = ai_player.get_vocabulary()
        assert all(word.upper() not in vocabulary for word in dictionary.listed_rejected())
        assert 'ALMA' in vocabulary


class TestVotedRejection:
    def test_mark_and_unmark(self):
        assert _valid('ALMA')
        version = dictionary.rejected_version()
        dictionary.mark_voted_rejected('alma', True)
        assert not _valid('ALMA') and dictionary.is_rejected('ALMA')
        assert dictionary.rejected_version() == version + 1
        dictionary.mark_voted_rejected('ALMA', False)
        assert _valid('ALMA') and not dictionary.is_rejected('ALMA')

    def test_unchanged_state_keeps_the_version(self):
        version = dictionary.rejected_version()
        dictionary.mark_voted_rejected('alma', False)
        assert dictionary.rejected_version() == version

    def test_set_replaces_everything(self):
        dictionary.mark_voted_rejected('alma', True)
        dictionary.set_voted_rejected(['körte', 'BARACK'])
        assert _valid('ALMA') and not _valid('KÖRTE') and not _valid('BARACK')

    def test_short_word_lists_follow_the_rejections(self):
        assert any(e['word'] == 'AZ' for e in practice.short_words(2))
        dictionary.mark_voted_rejected('az', True)
        assert not any(e['word'] == 'AZ' for e in practice.short_words(2))
        dictionary.mark_voted_rejected('az', False)
        assert any(e['word'] == 'AZ' for e in practice.short_words(2))

    def test_quiz_answer_treats_rejected_words_as_invalid(self):
        assert practice.check_answer('ALMA', True)['valid']
        dictionary.mark_voted_rejected('alma', True)
        assert practice.check_answer('ALMA', True)['valid'] is False


class TestVoting:
    def test_no_vote_rejects_the_word(self):
        uid = _user()
        assert word_review.record_vote(uid, 'ALMA', False) is True
        assert not _valid('ALMA')
        assert auth.get_voted_rejected_words(1) == {'alma'}

    def test_yes_vote_keeps_the_word(self):
        uid = _user()
        assert word_review.record_vote(uid, 'alma', True) is False
        assert _valid('ALMA')

    def test_changing_the_vote(self):
        uid = _user()
        word_review.record_vote(uid, 'ALMA', False)
        assert not _valid('ALMA')
        # a kizárt szót már nem lehet újra megszavazni, de a döntés visszavonható
        assert word_review.record_vote(uid, 'ALMA', True) is None
        assert word_review.undo_vote(uid, 'ALMA') == (True, False)
        assert _valid('ALMA')
        assert word_review.record_vote(uid, 'ALMA', True) is False

    def test_words_the_dictionary_does_not_accept_cannot_be_voted(self):
        uid = _user()
        assert word_review.record_vote(uid, 'ALMTÁ', True) is None
        assert word_review.record_vote(uid, 'QWX', False) is None
        assert word_review.record_vote(uid, 'A', False) is None
        assert word_review.record_vote(uid, 5, False) is None
        assert auth.get_reviewed_words() == set()

    def test_undo_without_a_vote(self):
        uid = _user()
        assert word_review.undo_vote(uid, 'ALMA') == (False, False)
        assert word_review.undo_vote(uid, 'nem szó!') == (False, False)

    def test_undo_only_touches_your_own_vote(self):
        a, b = _user('Anna'), _user('Bela')
        word_review.record_vote(a, 'ALMA', False)
        assert word_review.undo_vote(b, 'ALMA') == (False, False)
        assert not _valid('ALMA')

    def test_threshold_needs_more_than_one_reviewer(self, monkeypatch):
        monkeypatch.setattr(config, 'WORD_REJECT_THRESHOLD', 2)
        a, b = _user('Anna'), _user('Bela')
        assert word_review.record_vote(a, 'ALMA', False) is False
        assert _valid('ALMA')
        assert word_review.record_vote(b, 'ALMA', False) is True
        assert not _valid('ALMA')

    def test_accepting_votes_offset_rejections(self, monkeypatch):
        monkeypatch.setattr(config, 'WORD_REJECT_THRESHOLD', 2)
        a, b, c = _user('Anna'), _user('Bela'), _user('Csaba')
        word_review.record_vote(a, 'ALMA', False)
        word_review.record_vote(b, 'ALMA', False)
        assert not _valid('ALMA')
        # a harmadik szerint rendes szó: a különbség 1 < 2, a szó visszakerül
        auth.save_word_review(c, 'alma', True)
        assert word_review._apply('ALMA') is False
        assert _valid('ALMA')

    def test_refresh_restores_the_rejections_after_a_restart(self):
        uid = _user()
        word_review.record_vote(uid, 'ALMA', False)
        word_review.record_vote(uid, 'KÖRTE', True)
        dictionary.set_voted_rejected([])               # újraindulás: a memória üres
        assert _valid('ALMA')
        word_review.refresh()
        assert not _valid('ALMA') and _valid('KÖRTE')

    def test_stats(self):
        uid = _user()
        assert word_review.stats(uid) == {'total': 0, 'valid': 0, 'invalid': 0, 'today': 0}
        word_review.record_vote(uid, 'ALMA', True)
        word_review.record_vote(uid, 'KÖRTE', True)
        word_review.record_vote(uid, 'BARACK', False)
        assert word_review.stats(uid) == {'total': 3, 'valid': 2, 'invalid': 1, 'today': 3}

    def test_a_deleted_user_takes_the_votes_along(self):
        uid = _user()
        word_review.record_vote(uid, 'ALMA', False)
        with auth._db() as conn:
            conn.execute('DELETE FROM users WHERE id = ?', (uid,))
        assert auth.get_reviewed_words() == set()


class TestSampling:
    def test_words_are_valid_distinct_and_playable(self):
        words = word_review.sample_words(30, random.Random(3))
        assert len(words) == 30 and len(set(words)) == 30
        assert all(_valid(w) and tokenize_word(w) for w in words)
        assert all(2 <= len(w) <= 15 for w in words)

    def test_mixes_stems_and_inflected_forms(self):
        stems = set(practice.stem_words())
        words = word_review.sample_words(50, random.Random(5))
        inflected = [w for w in words if w not in stems]
        assert 5 <= len(inflected) <= 25

    def test_excluded_words_never_appear(self):
        first = word_review.sample_words(50, random.Random(7))
        again = word_review.sample_words(50, random.Random(7), exclude={w.lower() for w in first})
        assert not set(first) & set(again)

    def test_reviewed_words_are_skipped(self):
        uid = _user()
        shown = word_review.next_words(20, random.Random(11))
        for word in shown:
            word_review.record_vote(uid, word, True)
        assert not set(shown) & set(word_review.next_words(50, random.Random(11)))

    def test_rejected_words_are_not_offered(self):
        dictionary.set_voted_rejected(['alma'])
        for seed in range(5):
            assert 'ALMA' not in word_review.sample_words(50, random.Random(seed))

    def test_count_is_bounded(self):
        assert len(word_review.next_words(1000)) == word_review.MAX_BATCH
        assert len(word_review.next_words(0)) == 1
        assert len(word_review.sample_words(120, random.Random(2))) == 120      # az eszköz nagyobb mintát is kér

    def test_random_each_time(self):
        assert word_review.sample_words(20) != word_review.sample_words(20)

    def test_normalize(self):
        assert word_review.normalize(' alma ') == 'ALMA'
        assert word_review.normalize('szék') == 'SZÉK'
        assert word_review.normalize('a') is None
        assert word_review.normalize('qwx') is None
        assert word_review.normalize('a' * 16) is None
        assert word_review.normalize(None) is None


@pytest.fixture
def client():
    import server
    server.app.config['TESTING'] = True
    return server.app.test_client()


def _login(client, uid=None):
    uid = uid or _user()
    client.set_cookie('session_token', auth.create_session(uid), domain='localhost')
    return uid


class TestApi:
    def test_login_is_required(self, client):
        assert client.get('/api/practice/word-review').status_code == 401
        assert client.post('/api/practice/word-review', json={'word': 'ALMA', 'valid': False}).status_code == 401
        assert client.post('/api/practice/word-review/undo', json={'word': 'ALMA'}).status_code == 401
        assert client.get('/api/practice/word-review/stats').status_code == 401
        assert _valid('ALMA')

    def test_next_words(self, client):
        _login(client)
        data = client.get('/api/practice/word-review?n=12').get_json()
        assert data['success'] and len(data['words']) == 12
        assert data['tiles'] == [tokenize_word(w) for w in data['words']]
        assert data['stats'] == {'total': 0, 'valid': 0, 'invalid': 0, 'today': 0}
        assert len(client.get('/api/practice/word-review').get_json()['words']) == word_review.BATCH_SIZE
        assert len(client.get('/api/practice/word-review?n=abc').get_json()['words']) == word_review.BATCH_SIZE

    def test_vote_no_removes_the_word_from_the_game(self, client):
        _login(client)
        data = client.post('/api/practice/word-review', json={'word': 'alma', 'valid': False}).get_json()
        assert data['success'] and data['rejected'] is True and data['word'] == 'ALMA'
        assert data['stats']['invalid'] == 1
        assert client.post('/api/practice/answer', json={'word': 'ALMA', 'answer': True}).get_json()['valid'] is False
        assert client.get('/api/dictionary/check?q=alma').get_json()['results'][0]['valid'] is False

    def test_vote_yes(self, client):
        _login(client)
        data = client.post('/api/practice/word-review', json={'word': 'ALMA', 'valid': True}).get_json()
        assert data['success'] and data['rejected'] is False and data['stats']['valid'] == 1
        assert _valid('ALMA')

    def test_undo_brings_the_word_back(self, client):
        _login(client)
        client.post('/api/practice/word-review', json={'word': 'ALMA', 'valid': False})
        data = client.post('/api/practice/word-review/undo', json={'word': 'ALMA'}).get_json()
        assert data['success'] and data['rejected'] is False and data['stats']['total'] == 0
        assert _valid('ALMA')
        again = client.post('/api/practice/word-review/undo', json={'word': 'ALMA'})
        assert again.status_code == 404 and not again.get_json()['success']

    def test_vote_validation(self, client):
        _login(client)
        post = lambda **body: client.post('/api/practice/word-review', json=body).status_code
        assert post(word='ALMA') == 400                          # nincs döntés
        assert post(word='ALMA', valid='nem') == 400
        assert post(valid=True) == 400
        assert post(word=5, valid=True) == 400
        assert post(word='QWX', valid=False) == 400
        assert post(word='ALMTÁ', valid=False) == 409            # a szótár már nem fogadja el
        assert client.post('/api/practice/word-review', data='nem json').status_code == 400
        assert client.post('/api/practice/word-review/undo', json={'word': 5}).status_code == 400

    def test_stats_endpoint(self, client):
        _login(client)
        client.post('/api/practice/word-review', json={'word': 'ALMA', 'valid': True})
        assert client.get('/api/practice/word-review/stats').get_json()['stats']['total'] == 1

    def test_other_users_do_not_see_each_others_stats(self, client):
        _login(client)
        client.post('/api/practice/word-review', json={'word': 'ALMA', 'valid': True})
        other = client.application.test_client()
        _login(other, _user('Bela'))
        assert other.get('/api/practice/word-review/stats').get_json()['stats']['total'] == 0

    def test_unavailable_dictionary_is_reported(self, client, monkeypatch):
        _login(client)
        monkeypatch.setattr(dictionary, 'is_available', lambda: False)
        assert client.get('/api/practice/word-review').status_code == 503
        assert client.post('/api/practice/word-review', json={'word': 'ALMA', 'valid': True}).status_code == 503

    def test_rate_limited(self, client):
        _login(client)
        codes = [client.get('/api/practice/word-review/stats').status_code for _ in range(241)]
        assert codes[-1] == 429 and codes[0] == 200


def _load_tool():
    import importlib.util
    import os
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'tools', 'word_review.py')
    spec = importlib.util.spec_from_file_location('word_review_tool', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class TestTool:
    @pytest.fixture
    def tool(self):
        return _load_tool()

    def _sample(self, tool, tmp_path, count=30, seed=1, extra=()):
        out = tmp_path / f'sample-{seed}.txt'
        rejected = tmp_path / 'hu_rejected.txt'
        tool.main(['--rejected', str(rejected), 'sample', str(count), '--seed', str(seed), '--out', str(out), *extra])
        return [line for line in out.read_text(encoding='utf-8').split('\n') if line]

    def test_sample_is_valid_distinct_and_reproducible(self, tool, tmp_path):
        words = self._sample(tool, tmp_path)
        assert len(words) == 30 and len(set(words)) == 30
        assert all(w == w.lower() and _valid(w.upper()) for w in words)
        assert self._sample(tool, tmp_path) == words
        assert self._sample(tool, tmp_path, seed=2) != words

    def test_sample_can_skip_earlier_batches(self, tool, tmp_path):
        first = self._sample(tool, tmp_path, count=60, seed=3)
        skip = tmp_path / 'done.txt'
        skip.write_text('\n'.join(first) + '\n', encoding='utf-8')
        again = self._sample(tool, tmp_path, count=60, seed=3, extra=['--exclude', str(skip)])
        assert not set(first) & set(again)

    def test_sample_skips_listed_words(self, tool, tmp_path):
        listed = sorted(dictionary.listed_rejected())[:5]
        words = self._sample(tool, tmp_path, count=200, seed=4)
        assert not set(listed) & set(words)

    def test_apply_merges_sorted_and_keeps_the_header(self, tool, tmp_path, capsys):
        rejected = tmp_path / 'hu_rejected.txt'
        rejected.write_text('# saját fejléc\n# második sor\nbarack\n', encoding='utf-8')
        proposal = tmp_path / 'p.txt'
        proposal.write_text('# ítéletek\nkörte\nAlma\nbarack\nalmtá\n', encoding='utf-8')
        tool.main(['--rejected', str(rejected), 'apply', str(proposal)])
        text = rejected.read_text(encoding='utf-8')
        assert text == '# saját fejléc\n# második sor\nalma\nbarack\nkörte\n'
        out = capsys.readouterr().out
        assert 'felvéve: 2' in out and 'már a listán volt: 1' in out and 'almtá' in out

    def test_apply_is_idempotent(self, tool, tmp_path):
        rejected = tmp_path / 'hu_rejected.txt'
        proposal = tmp_path / 'p.txt'
        proposal.write_text('alma\nkörte\n', encoding='utf-8')
        tool.main(['--rejected', str(rejected), 'apply', str(proposal)])
        first = rejected.read_text(encoding='utf-8')
        tool.main(['--rejected', str(rejected), 'apply', str(proposal)])
        assert rejected.read_text(encoding='utf-8') == first
        assert first.startswith('#') and first.endswith('alma\nkörte\n')

    def test_dry_run_changes_nothing(self, tool, tmp_path):
        rejected = tmp_path / 'hu_rejected.txt'
        proposal = tmp_path / 'p.txt'
        proposal.write_text('alma\n', encoding='utf-8')
        tool.main(['--rejected', str(rejected), 'apply', str(proposal), '--dry-run'])
        assert not rejected.exists()

    def test_stats(self, tool, tmp_path, capsys):
        rejected = tmp_path / 'hu_rejected.txt'
        rejected.write_text('# x\nalma\nkörte\n', encoding='utf-8')
        tool.main(['--rejected', str(rejected), 'stats'])
        assert ': 2' in capsys.readouterr().out
