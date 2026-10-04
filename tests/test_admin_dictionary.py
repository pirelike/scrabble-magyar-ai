"""Admin panel: a szótár — szó-vizsgáló, kizárt szavak (tartós lista és szavazatok), felülbírálat, saját szavak,
szótár-építő felügyelet és gyorsítótárak."""
import re

import pytest

import admin
import ai_player
import auth
import dictionary
import word_review
from helpers import make_user

REASON = 'Szótári teszt'


@pytest.fixture(autouse=True)
def isolated(isolated_dictionary):
    """Minden teszt a tartós lista ideiglenes másolatán dolgozik."""
    return isolated_dictionary


def _audit(action):
    return admin.query_audit(action=action, limit=50)['items']


def _accepted_but_listed():
    """Egy tartós listán szereplő szó, amelyet a szótár egyébként elfogadna."""
    checker = dictionary.get_checker()
    return next(w for w in sorted(dictionary.listed_rejected()) if checker.check(w))


def _valid_word(index=0):
    """Egy biztosan érvényes, szokásos szó (nem a listán)."""
    candidates = [w for w in ('almás', 'körte', 'szilva', 'barack', 'szőlő', 'dinnye') if dictionary.is_word_valid(w.upper())]
    return candidates[index]


class TestInspector:
    def test_valid_word(self, api):
        data = api.get('/api/admin/dictionary/word?w=almát').get_json()
        assert data['word'] == 'ALMÁT' and data['valid'] is True and data['in_dictionary'] is True
        assert data['tiles'] == ['A', 'L', 'M', 'Á', 'T'] and data['score'] == 5
        assert any(d['stem'] == 'alma' and d['suffixes'] == ['át'] for d in data['derivations'])
        assert data['rejected_listed'] is False and data['rejected_voted'] is False and data['override'] is None
        assert data['balance'] == {'good': 0, 'bad': 0, 'threshold': 1, 'diff': 0}
        assert data['suggestions'] == []

    def test_digraph_tiles(self, api):
        data = api.get('/api/admin/dictionary/word?w=szék').get_json()
        assert data['tiles'] == ['SZ', 'É', 'K']

    def test_invalid_word_has_suggestions(self, api):
        data = api.get('/api/admin/dictionary/word?w=almaa').get_json()
        assert data['valid'] is False and data['in_dictionary'] is False and 'ALMA' in data['suggestions']

    def test_risky_derivation_needs_the_usage_list(self, api):
        data = api.get('/api/admin/dictionary/word?w=tarom').get_json()
        assert data['needs_attestation'] is True and data['attested'] is False and data['valid'] is False
        assert data['derivations'][0]['risky'] is True

    def test_listed_rejection_is_explained(self, api):
        word = _accepted_but_listed()
        data = api.get(f'/api/admin/dictionary/word?w={word}').get_json()
        assert data['rejected_listed'] is True and data['valid'] is False and data['in_dictionary'] is True

    def test_votes_are_listed(self, api):
        uid, _ = make_user('a@example.com', 'Anna')
        word = _valid_word()
        word_review.record_vote(uid, word.upper(), False)
        data = api.get(f'/api/admin/dictionary/word?w={word}').get_json()
        assert data['rejected_voted'] is True and data['valid'] is False
        assert data['votes'][0]['display_name'] == 'Anna' and data['votes'][0]['valid'] is False
        assert data['balance']['diff'] == 1

    def test_vocabulary_membership(self, api):
        assert api.get('/api/admin/dictionary/word?w=alma').get_json()['in_vocabulary'] is True

    @pytest.mark.parametrize('word', ['', 'x' * 16, 'abc123', 'qwx'])
    def test_invalid_input(self, api, word):
        assert api.get(f'/api/admin/dictionary/word?w={word}').status_code == 400


class TestRejectedList:
    def test_listing_and_search(self, api):
        data = api.get('/api/admin/dictionary/rejected').get_json()
        assert data['counts']['listed'] == len(dictionary.listed_rejected()) and data['total'] >= 600
        first = data['items'][0]['word']
        found = api.get(f'/api/admin/dictionary/rejected?q={first[:4]}&source=listed').get_json()
        assert first in [i['word'] for i in found['items']]
        assert api.get('/api/admin/dictionary/rejected?source=rossz').status_code == 400
        page = api.get('/api/admin/dictionary/rejected?limit=5&offset=5').get_json()
        assert len(page['items']) == 5 and page['offset'] == 5

    def test_voted_words_are_listed_with_their_votes(self, api):
        uid, _ = make_user('a@example.com', 'Anna')
        word = _valid_word()
        word_review.record_vote(uid, word.upper(), False)
        item = api.get(f'/api/admin/dictionary/rejected?source=voted').get_json()['items'][0]
        assert item['word'] == word.upper() and item['voted'] is True and item['bad'] == 1 and item['listed'] is False

    def test_csv(self, api):
        text = api.get('/api/admin/dictionary/rejected?format=csv').get_data(as_text=True)
        assert text.startswith('word,listed,voted,allowed,good,bad')


class TestAddToTheList:
    def test_preview_classifies_the_words(self, api):
        listed = _accepted_but_listed()
        fresh = _valid_word()
        data = api.post('/api/admin/dictionary/rejected/preview',
                        {'words': f'{fresh}, {listed}\nnemszó almaa # megjegyzés ide\nqqq 123'}).get_json()
        assert data['new'] == [fresh] and data['already'] == [listed]
        assert 'almaa' in data['invalid'] and 'nemszó' in data['invalid'] and 'qqq' in data['bad'] and 'megjegyzés' not in data['invalid']

    def test_add_writes_the_file_atomically_and_applies_immediately(self, api, isolated):
        word = _valid_word()
        before = dictionary.rejected_version()
        assert dictionary.is_word_valid(word.upper())
        response = api.post('/api/admin/dictionary/rejected', {'words': word + ' almaa', 'reason': REASON})
        assert response.status_code == 200 and response.get_json()['new'] == [word]
        assert word in isolated.read_text(encoding='utf-8').split()
        assert 'almaa' not in isolated.read_text(encoding='utf-8')               # érvénytelen szót nem vesz fel
        assert not dictionary.is_word_valid(word.upper()) and dictionary.rejected_version() > before
        assert [p.name for p in isolated.parent.iterdir() if p.name.startswith('.hu_rejected')] == []
        row = _audit('dict.reject_add')[0]['details']
        assert row['words'] == [word] and row['skipped_invalid'] == 1 and row['reason'] == REASON

    def test_nothing_to_add(self, api):
        assert api.post('/api/admin/dictionary/rejected', {'words': 'almaa', 'reason': REASON}).status_code == 409
        assert api.post('/api/admin/dictionary/rejected', {'words': _accepted_but_listed(), 'reason': REASON}
                        ).status_code == 409
        assert api.post('/api/admin/dictionary/rejected', {'words': _valid_word()}).status_code == 400

    def test_a_failing_audit_write_restores_the_file(self, api, isolated, monkeypatch):
        original = isolated.read_text(encoding='utf-8')
        word = _valid_word()
        monkeypatch.setattr(admin, 'record', lambda *a, **k: (_ for _ in ()).throw(RuntimeError('napló')))
        with pytest.raises(RuntimeError):
            import admin_dict
            admin_dict.add_rejected(api.ctx(), word, REASON)
        assert isolated.read_text(encoding='utf-8') == original
        assert dictionary.is_word_valid(word.upper())

    def test_remove_keeps_the_comments(self, api, isolated):
        word = _valid_word()
        api.post('/api/admin/dictionary/rejected', {'words': word, 'reason': REASON})
        assert api.delete('/api/admin/dictionary/rejected', {'words': word, 'reason': REASON}).status_code == 200
        assert dictionary.is_word_valid(word.upper())
        text = isolated.read_text(encoding='utf-8')
        assert word not in text.split() and '# --- admin panel' in text
        assert api.delete('/api/admin/dictionary/rejected', {'words': word, 'reason': REASON}).status_code == 404

    def test_remove_an_original_entry(self, api, isolated):
        word = _accepted_but_listed()
        original = isolated.read_text(encoding='utf-8')
        api.delete('/api/admin/dictionary/rejected', {'words': word, 'reason': REASON})
        remaining = isolated.read_text(encoding='utf-8')
        assert len(remaining.splitlines()) == len(original.splitlines()) - 1
        assert dictionary.is_word_valid(word.upper())

    def test_download_and_diff(self, api, isolated):
        word = _valid_word()
        api.post('/api/admin/dictionary/rejected', {'words': word, 'reason': REASON})
        text = api.get('/api/admin/dictionary/rejected/download').get_data(as_text=True)
        assert word in text.split()
        diff = api.get('/api/admin/dictionary/rejected/download?diff=1').get_data(as_text=True)
        assert f'+{word}' in diff and '--- a/dict/hu_rejected.txt' in diff

    def test_each_change_is_reversible_by_the_diff(self, api, isolated):
        word = _valid_word()
        api.post('/api/admin/dictionary/rejected', {'words': word, 'reason': REASON})
        api.delete('/api/admin/dictionary/rejected', {'words': word, 'reason': REASON})
        diff = api.get('/api/admin/dictionary/rejected/download?diff=1').get_data(as_text=True)
        assert f'+{word}' not in diff


class TestOverrides:
    def test_reject_override(self, api):
        word = _valid_word()
        before = dictionary.rejected_version()
        assert api.post('/api/admin/dictionary/overrides', {'word': word, 'verdict': 'reject', 'reason': REASON}
                        ).status_code == 200
        assert not dictionary.is_word_valid(word.upper()) and dictionary.rejected_version() > before
        assert api.get('/api/admin/dictionary/word?w=' + word).get_json()['override']['verdict'] == 'reject'
        api.delete('/api/admin/dictionary/overrides', {'word': word, 'reason': REASON})
        assert dictionary.is_word_valid(word.upper())

    def test_allow_overrides_the_votes_and_the_list(self, api):
        uid, _ = make_user('a@example.com', 'Anna')
        voted = _valid_word()
        listed = _accepted_but_listed()
        word_review.record_vote(uid, voted.upper(), False)
        assert not dictionary.is_word_valid(voted.upper()) and not dictionary.is_word_valid(listed.upper())
        for word in (voted, listed):
            assert api.post('/api/admin/dictionary/overrides', {'word': word, 'verdict': 'allow', 'reason': REASON}
                            ).status_code == 200
            assert dictionary.is_word_valid(word.upper())
        word_review.refresh()                                  # újraindítás után is érvényben marad
        assert dictionary.is_word_valid(voted.upper())
        assert sorted(o['word'] for o in api.get('/api/admin/dictionary/overrides').get_json()['items']) == \
            sorted([voted.upper(), listed.upper()])

    def test_allow_does_not_make_an_unknown_word_valid(self, api):
        response = api.post('/api/admin/dictionary/overrides', {'word': 'almaa', 'verdict': 'allow', 'reason': REASON})
        assert response.status_code == 409 and not dictionary.is_word_valid('ALMAA')

    def test_validation(self, api):
        word = _valid_word()
        assert api.post('/api/admin/dictionary/overrides', {'word': word, 'verdict': 'talán', 'reason': REASON}
                        ).status_code == 400
        assert api.post('/api/admin/dictionary/overrides', {'word': word, 'verdict': 'reject'}).status_code == 400
        assert api.delete('/api/admin/dictionary/overrides', {'word': word, 'reason': REASON}).status_code == 404

    def test_the_override_is_logged_with_before_and_after(self, api):
        word = _valid_word()
        api.post('/api/admin/dictionary/overrides', {'word': word, 'verdict': 'reject', 'reason': REASON})
        api.post('/api/admin/dictionary/overrides', {'word': word, 'verdict': 'allow', 'reason': REASON})
        rows = _audit('dict.override')
        assert [r['details']['after'] for r in rows] == [{'verdict': 'allow'}, {'verdict': 'reject'}]
        assert rows[0]['details']['before'] == {'verdict': 'reject'}


class TestAdditions:
    def test_a_custom_word_is_accepted_but_not_in_the_robot_vocabulary(self, api):
        word = 'zsugorbab'
        assert not dictionary.is_word_valid(word.upper())
        assert api.post('/api/admin/dictionary/additions', {'word': word, 'reason': REASON}).status_code == 200
        assert dictionary.is_word_valid(word.upper()) and dictionary.check_words([word.upper()]) == (True, [])
        assert word.upper() not in ai_player.get_vocabulary()
        assert api.get('/api/admin/dictionary/additions').get_json()['items'][0]['word'] == word.upper()
        assert api.post('/api/admin/dictionary/additions', {'word': word, 'reason': REASON}).status_code == 409
        assert api.delete('/api/admin/dictionary/additions', {'word': word, 'reason': REASON}).status_code == 200
        assert not dictionary.is_word_valid(word.upper())

    def test_rejections_still_win_over_a_custom_word(self, api):
        word = 'zsugorbab'
        api.post('/api/admin/dictionary/additions', {'word': word, 'reason': REASON})
        api.post('/api/admin/dictionary/overrides', {'word': word, 'verdict': 'reject', 'reason': REASON})
        assert not dictionary.is_word_valid(word.upper())

    @pytest.mark.parametrize('word', ['bcdfg', 'a'])
    def test_needs_a_vowel_and_two_tiles(self, api, word):
        assert api.post('/api/admin/dictionary/additions', {'word': word, 'reason': REASON}).status_code == 400


class TestVotes:
    def test_delete_all_votes_of_a_word(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        b, _ = make_user('b@example.com', 'Béla')
        word = _valid_word()
        word_review.record_vote(a, word.upper(), False)
        auth.save_word_review(b, word, False)          # a második szavazat már csak az adatbázisból jöhet
        assert api.delete('/api/admin/dictionary/votes', {'word': word, 'reason': REASON}).status_code == 401
        api.sudo()
        data = api.delete('/api/admin/dictionary/votes', {'word': word, 'reason': REASON}).get_json()
        assert data['deleted'] == 2 and dictionary.is_word_valid(word.upper())
        assert api.delete('/api/admin/dictionary/votes', {'word': word, 'reason': REASON}).status_code == 404
        assert len(_audit('dict.votes_delete')[0]['details']['votes']) == 2

    def test_export_the_voted_words_to_the_persistent_list(self, api, isolated):
        a, _ = make_user('a@example.com', 'Anna')
        w1, w2 = _valid_word(0), _valid_word(1)
        for word in (w1, w2):
            word_review.record_vote(a, word.upper(), False)
        api.sudo()
        data = api.post('/api/admin/dictionary/rejected/export-votes', {'reason': REASON}).get_json()
        assert sorted(data['new']) == sorted([w1, w2])
        text = isolated.read_text(encoding='utf-8').split()
        assert w1 in text and w2 in text
        assert api.post('/api/admin/dictionary/rejected/export-votes', {'reason': REASON}).status_code == 409

    def test_the_threshold_comes_from_the_settings(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        word = _valid_word()
        assert api.get('/api/admin/dictionary/threshold').get_json() == {'success': True, 'value': 1, 'default': 1,
                                                                          'overridden': False}
        response = api.patch('/api/admin/settings', {'changes': {'word_reject_threshold': 2}, 'reason': REASON})
        assert response.status_code == 200
        word_review.record_vote(a, word.upper(), False)
        assert dictionary.is_word_valid(word.upper())                      # egy szavazat már kevés
        b, _ = make_user('b@example.com', 'Béla')
        auth.save_word_review(b, word, False)
        word_review.refresh()
        assert not dictionary.is_word_valid(word.upper())
        api.patch('/api/admin/settings', {'changes': {'word_reject_threshold': None}, 'reason': REASON})
        assert api.get('/api/admin/dictionary/threshold').get_json()['value'] == 1


class TestReviewers:
    def _votes(self, user, verdicts):
        for word, verdict in verdicts:
            auth.save_word_review(user, word, verdict)

    def test_reviewer_statistics(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        b, _ = make_user('b@example.com', 'Béla')
        self._votes(a, [('w1', False), ('w2', True), ('w3', False)])
        self._votes(b, [('w1', False), ('w2', False), ('w4', True)])
        items = {i['display_name']: i for i in api.get('/api/admin/dictionary/reviewers').get_json()['items']}
        assert items['Anna']['total'] == 3 and items['Anna']['invalid'] == 2 and items['Anna']['invalid_share'] == 66.7
        # közös szavak: w1 (egyezik), w2 (ellentétes) → az egyezés 50%
        assert items['Anna']['compared'] == 2 and items['Anna']['agreement'] == 50.0
        assert items['Béla']['agreement'] == 50.0 and items['Anna']['suspicious'] is False

    def test_a_suspicious_reviewer_is_highlighted(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        self._votes(a, [(f'w{i}', i % 10 == 0) for i in range(60)])
        item = api.get('/api/admin/dictionary/reviewers').get_json()['items'][0]
        assert item['total'] == 60 and item['invalid_share'] > 80 and item['suspicious'] is True

    def test_summary(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        self._votes(a, [('w1', False), ('w2', True)])
        data = api.get('/api/admin/dictionary/review-summary').get_json()
        assert data['decisions'] == 2 and data['words_reviewed'] == 2 and data['reviewers'] == 1
        assert data['daily'][0]['decisions'] == 2 and data['daily'][0]['rejections'] == 1
        assert data['active_reviewers'] == 1 and data['threshold'] == 1

    def test_second_opinion_sample(self, api):
        data = api.get('/api/admin/dictionary/second-opinion?n=7').get_json()['items']
        assert len(data) == 7 and all(re.match(r'^[A-ZÁÉÍÓÖŐÚÜŰ]+$', i['word']) for i in data)
        assert all(i['word'].lower() in dictionary.listed_rejected() for i in data)
        assert {'tiles', 'dictionary_accepts'} <= set(data[0])


class TestCaches:
    def test_valid_cache(self, api):
        dictionary.filter_valid({'ALMA', 'KÖRTE'})
        data = api.post('/api/admin/dictionary/caches/clear', {'which': 'valid', 'reason': REASON}).get_json()
        assert data['which'] == 'valid' and data['detail'] >= 2 and data['seconds'] >= 0
        assert dictionary._valid_cache == {}
        assert _audit('dict.cache_clear')[0]['target_id'] == 'valid'

    def test_short_words_are_recomputed(self, api):
        import practice
        practice._short_cache.clear()
        assert api.post('/api/admin/dictionary/caches/clear', {'which': 'short_words', 'reason': REASON}
                        ).status_code == 200
        assert set(practice._short_cache) == set(practice.SHORT_LENGTHS)

    def test_the_robot_vocabulary_is_rebuilt(self, api, monkeypatch):
        real = ai_player.get_vocabulary()
        monkeypatch.setattr(ai_player, 'load_vocabulary', lambda *a, **k: ai_player.Vocabulary(['ALMA', 'KÖRTE']))
        try:
            data = api.post('/api/admin/dictionary/caches/clear', {'which': 'vocabulary', 'reason': REASON}).get_json()
            assert data['detail'] == 2 and len(ai_player.get_vocabulary()) == 2
        finally:
            ai_player.set_vocabulary(real)

    def test_the_analysis_cache(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        from helpers import finished_game
        game_id = finished_game('r1', [(a, 'Anna', 10), (None, 'Vendég', 5)])
        auth.save_game_analysis(game_id, 6, '{}')
        data = api.post('/api/admin/dictionary/caches/clear', {'which': 'analysis', 'reason': REASON}).get_json()
        assert data['detail'] == 1 and auth.get_game_analysis(game_id) is None

    def test_validation(self, api):
        assert api.post('/api/admin/dictionary/caches/clear', {'which': 'minden', 'reason': REASON}).status_code == 400
        assert api.post('/api/admin/dictionary/caches/clear', {'which': 'valid'}).status_code == 400
