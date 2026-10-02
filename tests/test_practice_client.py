"""A gyakorló felület kliensoldali logikája (static/app.js) valódi futtatással node-ban:
a zsetonokra bontás a szerverével egyezik, a magyar ábécé-rendezés, a sorozat és a „Hibáim” pakli szabályai."""
import json
import os
import random
import re
import shutil
import subprocess

import pytest

import practice
from tiles import tokenize_word

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

pytestmark = pytest.mark.skipif(shutil.which('node') is None, reason='node nem elérhető')


def _app_js():
    with open(os.path.join(ROOT, 'static', 'app.js'), encoding='utf-8') as fh:
        return fh.read()


def _block(source, start, end):
    """A `start` jelölőtől az első `end` sorig (a lezáró sorral együtt) terjedő felső szintű kódrészlet."""
    i = source.index(start)
    j = source.index(end, i) + len(end)
    return source[i:j]


def _extract():
    js = _app_js()
    return '\n'.join([
        _block(js, 'const TILE_VALUES = {', '\n};\n'),
        _block(js, 'const HU_ALPHABET = [', '];\n'),
        _block(js, 'const HU_ORDER =', ';\n'),
        _block(js, 'function tokenizeWord(word) {', '\n}\n'),
        _block(js, 'function compareWords(a, b) {', '\n}\n'),
        _block(js, 'function localDateKey(', '\n}\n'),
        _block(js, 'const PracticeStore = {', '\n};\n'),
    ])


HARNESS = r"""
const input = JSON.parse(require('fs').readFileSync(0, 'utf8'));
const store = {};
const localStorage = { getItem: (k) => (k in store ? store[k] : null), setItem: (k, v) => { store[k] = String(v); } };
const m = { exports: {} };
new Function('module', 'localStorage', process.argv[1] + '\nmodule.exports = { tokenizeWord, compareWords, localDateKey, PracticeStore };')(m, localStorage);
const { tokenizeWord, compareWords, localDateKey, PracticeStore } = m.exports;
const out = {};

out.tokens = input.words.map(w => tokenizeWord(w));
out.sorted = input.unsorted.slice().sort(compareWords);

const fresh = () => { PracticeStore._data = null; delete store[PracticeStore.KEY]; return PracticeStore; };
const day = (offset) => { const d = new Date(); d.setDate(d.getDate() + offset); return localDateKey(d); };

// sorozat
const streakOf = (offsets) => { const s = fresh(); s.get().days = offsets.map(day); return s.streak(); };
out.streak = {
    three: streakOf([0, -1, -2]), todayEmpty: streakOf([-1, -2]), gap: streakOf([0, -2]),
    old: streakOf([-2]), none: streakOf([]),
};

// mai számláló
let s = fresh();
s.touch(); s.touch();
out.today = { count: s.todayCount(), days: s.get().days.length };
s.get().today.date = '2000-01-01';
out.today.stale = s.todayCount();

// Hibáim pakli
s = fresh();
const deck = () => JSON.parse(JSON.stringify(s.get().missed));
s.recordAnswer('ALMA', ['A', 'L', 'M', 'A'], true, false);
out.deck = { afterWrong: deck().ALMA };
s.recordAnswer('ALMA', ['A', 'L', 'M', 'A'], true, true);
out.deck.afterOneRight = deck().ALMA;
s.recordAnswer('ALMA', ['A', 'L', 'M', 'A'], true, false);              // újra téves: a számláló nullázódik
out.deck.afterRelapse = deck().ALMA;
s.recordAnswer('ALMA', ['A', 'L', 'M', 'A'], true, true);
s.recordAnswer('ALMA', ['A', 'L', 'M', 'A'], true, true);
out.deck.afterTwoRight = 'ALMA' in deck();
s.recordAnswer('KÖRTE', ['K', 'Ö', 'R', 'T', 'E'], true, true);        // helyes válasz nem kerül a pakliba
out.deck.rightNotAdded = 'KÖRTE' in deck();
out.quiz = { ...s.get().quiz, accuracy: s.accuracy() };
out.words = fresh().missedWords();

s = fresh();
for (let i = 0; i < 205; i++) s.recordAnswer('W' + i, ['A'], false, false);
out.cap = { size: Object.keys(s.get().missed).length, first: Object.keys(s.get().missed)[0], hasOldest: 'W0' in s.get().missed, hasNewest: 'W204' in s.get().missed };

// mentés + újratöltés, sérült tároló
s = fresh();
s.recordAnswer('ALMA', ['A', 'L', 'M', 'A'], true, false);
s.touch();
PracticeStore._data = null;
out.reload = { missed: Object.keys(PracticeStore.get().missed), today: PracticeStore.todayCount() };
PracticeStore._data = null; store[PracticeStore.KEY] = '{nem json';
out.corrupt = { missed: Object.keys(PracticeStore.get().missed).length, questions: PracticeStore.get().quiz.questions };
PracticeStore._data = null; store[PracticeStore.KEY] = JSON.stringify({ quiz: { questions: 4, correct: 3, best_streak: 2 } });
out.partial = { accuracy: PracticeStore.accuracy(), hasMissed: typeof PracticeStore.get().missed, hasHunt: typeof PracticeStore.get().hunt };
console.log(JSON.stringify(out));
"""


@pytest.fixture(scope='module')
def result():
    rng = random.Random(7)
    import dictionary
    dictionary.warm_up()
    stems = practice._stems()
    words = (rng.sample(stems, 1500)
             + [e['word'] for e in practice.short_words(2)] + [e['word'] for e in practice.short_words(3)]
             + ['SZÓ', 'ASZTAL', 'GYÓGYSZER', 'NYELV', 'LYUK', 'TYÚK', 'ZSÍR', 'CSÓK', 'szó', 'XYZ', 'QWERTY', ''])
    unsorted = ['ZSÍR', 'SZÓ', 'SÁR', 'CSÓK', 'CIKK', 'ÓRA', 'ŐZ', 'ÖN', 'OLAJ', 'ÁBRA', 'ABA', 'DÉL', 'GYŰRŰ', 'GÉP',
                'NYÁR', 'NAP', 'TYÚK', 'TÁL', 'LYUK', 'LÁNC', 'ÜVEG', 'ÚT', 'ŰR', 'UTCA']
    code = _extract()
    proc = subprocess.run(['node', '-e', HARNESS, code], input=json.dumps({'words': words, 'unsorted': unsorted}),
                          capture_output=True, text=True, timeout=60)
    assert proc.returncode == 0, proc.stderr
    data = json.loads(proc.stdout)
    data['_words'] = words
    return data


class TestTokenizer:
    def test_matches_the_server_for_the_dictionary(self, result):
        mismatches = [(w, t, tokenize_word(w)) for w, t in zip(result['_words'], result['tokens'])
                      if t != tokenize_word(w)]
        assert not mismatches, mismatches[:5]

    def test_digraphs_and_unplaceable_letters(self, result):
        by_word = dict(zip(result['_words'], result['tokens']))
        assert by_word['SZÓ'] == ['SZ', 'Ó'] and by_word['GYÓGYSZER'] == ['GY', 'Ó', 'GY', 'SZ', 'E', 'R']
        assert by_word['szó'] == ['SZ', 'Ó']
        assert by_word['XYZ'] is None and by_word['QWERTY'] is None and by_word[''] == []


class TestHungarianOrder:
    def test_alphabet_with_digraphs_and_long_vowels(self, result):
        assert result['sorted'] == ['ABA', 'ÁBRA', 'CIKK', 'CSÓK', 'DÉL', 'GÉP', 'GYŰRŰ', 'LÁNC', 'LYUK', 'NAP',
                                    'NYÁR', 'OLAJ', 'ÓRA', 'ÖN', 'ŐZ', 'SÁR', 'SZÓ', 'TÁL', 'TYÚK', 'UTCA', 'ÚT',
                                    'ÜVEG', 'ŰR', 'ZSÍR']


class TestStreak:
    def test_consecutive_days(self, result):
        s = result['streak']
        assert s['three'] == 3
        assert s['todayEmpty'] == 2          # a ma még üres nap nem szakítja meg a tegnapig tartó sorozatot
        assert s['gap'] == 1                 # a kihagyott nap megszakítja
        assert s['old'] == 0 and s['none'] == 0

    def test_today_counter(self, result):
        assert result['today'] == {'count': 2, 'days': 1, 'stale': 0}


class TestMistakesDeck:
    def test_wrong_answer_enters_the_deck(self, result):
        assert result['deck']['afterWrong'] == {'tiles': ['A', 'L', 'M', 'A'], 'valid': True, 'ok': 0}

    def test_two_right_answers_in_a_row_remove_a_word(self, result):
        assert result['deck']['afterOneRight']['ok'] == 1
        assert result['deck']['afterTwoRight'] is False

    def test_relapse_resets_the_counter(self, result):
        assert result['deck']['afterRelapse']['ok'] == 0

    def test_right_answers_do_not_add_words(self, result):
        assert result['deck']['rightNotAdded'] is False

    def test_quiz_counters(self, result):
        assert result['quiz']['questions'] == 6 and result['quiz']['correct'] == 4
        assert result['quiz']['accuracy'] == 67

    def test_deck_is_capped_and_drops_the_oldest(self, result):
        assert result['cap']['size'] == 200 and not result['cap']['hasOldest'] and result['cap']['hasNewest']

    def test_words_come_with_tiles(self, result):
        assert result['words'] == []


class TestPersistence:
    def test_survives_a_reload(self, result):
        assert result['reload'] == {'missed': ['ALMA'], 'today': 1}

    def test_corrupt_storage_falls_back_to_defaults(self, result):
        assert result['corrupt'] == {'missed': 0, 'questions': 0}

    def test_partial_data_is_completed_with_defaults(self, result):
        assert result['partial'] == {'accuracy': 75, 'hasMissed': 'object', 'hasHunt': 'object'}
