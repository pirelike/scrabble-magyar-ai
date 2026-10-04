"""A kliens (static/app.js) néhány állapotkezelő részlete valódi futtatással node-ban, DOM nélkül:
a félkész lerakás törlése új játéknál, a kétjegyű betűk szabálya a betűvadászban."""
import json
import os
import re
import shutil
import subprocess

import pytest

from tiles import LETTERS, forms_digraph

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

pytestmark = pytest.mark.skipif(shutil.which('node') is None, reason='node nem elérhető')


def _app_js():
    with open(os.path.join(ROOT, 'static', 'app.js'), encoding='utf-8') as fh:
        return fh.read()


def _block(source, start, end):
    """A `start` jelölőtől az első `end` sorig (a lezáró sorral együtt) terjedő kódrészlet."""
    i = source.index(start)
    j = source.index(end, i) + len(end)
    return source[i:j]


def _run(harness, code, payload=None):
    proc = subprocess.run(['node', '-e', harness, code], input=json.dumps(payload or {}),
                          capture_output=True, text=True, timeout=60)
    assert proc.returncode == 0, proc.stderr
    return json.loads(proc.stdout)


# --- Kétjegyű betűk: a kliens szabálya a szerverével egyezik ---

DIGRAPH_HARNESS = r"""
const input = JSON.parse(require('fs').readFileSync(0, 'utf8'));
const m = { exports: {} };
new Function('module', process.argv[1] + '\nmodule.exports = { formsDigraph };')(m);
const { formsDigraph } = m.exports;
console.log(JSON.stringify(input.pairs.map(([a, b]) => formsDigraph(a, b))));
"""

HUNT_TYPE_HARNESS = r"""
const input = JSON.parse(require('fs').readFileSync(0, 'utf8'));
const m = { exports: {} };
new Function('module', process.argv[1] + '\nmodule.exports = { formsDigraph, huntType };')(m);
const { huntType } = m.exports;
const out = [];
for (const c of input.cases) {
    const ctx = { hunt: { finished: false, busy: false, rack: c.rack, built: [], pending: '' },
                  huntAdd(i) { this.hunt.built.push(i); } };
    for (const ch of c.typed) huntType.call(ctx, ch);
    out.push(ctx.hunt.built.map(i => ctx.hunt.rack[i]));
}
console.log(JSON.stringify(out));
"""


class TestDigraphRuleInTheClient:
    @staticmethod
    def _digraph_code():
        js = _app_js()
        return _block(js, 'const DIGRAPH_TILES = new Set(', '\n}\n')

    def test_matches_the_server_for_every_pair_of_tiles(self):
        letters = LETTERS + ['Y']    # önálló Y zseton nincs, de a szabály a GY / LY / NY / TY-ra is szól
        pairs = [(a, b) for a in [''] + letters for b in letters]
        got = _run(DIGRAPH_HARNESS, self._digraph_code(), {'pairs': pairs})
        expected = [forms_digraph(a, b) for a, b in pairs]
        assert got == expected
        assert sum(expected) == 7

    def test_keyboard_typing_prefers_the_digraph_tile(self):
        js = _app_js()
        method = _block(js, '    huntType(ch) {', '\n    },\n')
        code = self._digraph_code() + '\nconst huntType = ' + re.sub(r'^\s*huntType\(ch\) \{', 'function (ch) {', method, count=1).rstrip().rstrip(',') + ';'
        cases = [
            # S majd Z: a szabad SZ zseton az S helyére kerül
            {'rack': ['S', 'Z', 'SZ', 'A', 'K', 'T', 'L'], 'typed': 'SZ'},
            # nincs SZ zseton: külön S és Z kerül a sorba (a beküldéskor utasítja el a szabály)
            {'rack': ['S', 'Z', 'A', 'K', 'T', 'L', 'M'], 'typed': 'SZ'},
            # csak SZ zseton van: két billentyű is egy zseton
            {'rack': ['SZ', 'A', 'K', 'T', 'L', 'M', 'O'], 'typed': 'SZ'},
            # nem kétjegyű pár: nem történik csere
            {'rack': ['S', 'A', 'SZ', 'K', 'T', 'L', 'M'], 'typed': 'SA'},
        ]
        got = _run(HUNT_TYPE_HARNESS, code, {'cases': cases})
        assert got == [['SZ'], ['S', 'Z'], ['SZ'], ['S', 'A']]


# --- A félkész lerakás nem marad a következő játék táblájára ---

PLACEMENT_HARNESS = r"""
const m = { exports: {} };
const body = process.argv[1];
const noop = new Proxy(function () {}, { get: () => noop, apply: () => undefined });
const el = { classList: { contains: () => false, add() {}, remove() {}, toggle() {} }, textContent: '' };
const stubs = {
    socket: { id: 'me' },
    Chat: { clear() {} }, Badges: { newInGame: [] }, Daily: { reset() {}, onGameState() {} },
    AsyncGames: { onGameState() {} }, Report: { sync() {} },
    Preview: { cleared: 0, clear() { this.cleared++; } },
    document: { getElementById: () => el },
    localStorage: { removeItem() {} },
    t: (k) => k, showScreen() {}, setGameRoomName() {},
    SoundManager: noop, ChallengeUI: noop, TurnTimerUI: noop, WaitingRoom: noop, GameOver: noop, BoardZoom: noop,
};
const names = Object.keys(stubs);
new Function('module', ...names, body + '\nmodule.exports = { AppState, BoardState, GameBoard, Preview };')(m, ...names.map(n => stubs[n]));
const { AppState, BoardState, GameBoard, Preview } = m.exports;
const placed = () => [{ row: 7, col: 7, letter: 'A', is_blank: false, handIdx: 0 }];
const noRender = ['renderBoard', 'renderHand', 'renderScoreboard', 'renderGameInfo', 'renderHistory', 'updateButtons',
                  'updateSpectatorUi', 'build', '_animateTurnChange'];
for (const name of noRender) GameBoard[name] = () => {};
const state = (id) => ({ game_id: id, started: true, finished: false, current_player: 'me', players: [] });
const out = {};

// kilépés a játékból (AppState.reset): a megmutatott megoldás / tipp nem marad a táblán
BoardState.placedTiles = placed();
BoardState.selectedTileIdx = 3;
BoardState.exchangeMode = true;
AppState.reset();
out.afterReset = { placed: BoardState.placedTiles.length, selected: BoardState.selectedTileIdx,
                   exchange: BoardState.exchangeMode, previewCleared: Preview.cleared };

// ugyanabban a játékban az újrarajzolás nem törli a lerakást
GameBoard.onGameState(state('g1'));
BoardState.placedTiles = placed();
GameBoard.onGameState(state('g1'));
out.sameGame = BoardState.placedTiles.length;

// új játék (új game_id): a saját körön sem marad ott a régi lerakás
GameBoard.onGameState(state('g2'));
out.newGame = BoardState.placedTiles.length;
console.log(JSON.stringify(out));
"""


@pytest.fixture(scope='module')
def placement_result():
    js = _app_js()
    on_game_state = _block(js, '    onGameState(state) {\n        const prevPlayer', '\n    },\n')
    code = '\n'.join([
        _block(js, 'const AppState = {', '\n};\n'),
        _block(js, 'const BoardState = {', '\n};\n'),
        'const GameBoard = { _prevCurrentPlayer: null, _gameId: null, _resetAnimState() {},\n'
        + on_game_state.rstrip().rstrip(',') + '\n};',
    ])
    return _run(PLACEMENT_HARNESS, code)


class TestPlacementDoesNotLeak:
    def test_leaving_a_game_clears_the_pending_placement(self, placement_result):
        assert placement_result['afterReset'] == {'placed': 0, 'selected': None, 'exchange': False, 'previewCleared': 1}

    def test_a_new_game_starts_with_an_empty_board(self, placement_result):
        assert placement_result['newGame'] == 0

    def test_a_redraw_in_the_same_game_keeps_the_placement(self, placement_result):
        assert placement_result['sameGame'] == 1


# --- A betűtartó helye (HandLayout) ---

HAND_LAYOUT_HARNESS = r"""
const input = JSON.parse(require('fs').readFileSync(0, 'utf8'));
const run = (storage) => {
    const segments = ['right', 'bottom'].map(position => {
        const classes = new Set();
        return { dataset: { position }, classList: { toggle: (c, on) => (on ? classes.add(c) : classes.delete(c)), has: (c) => classes.has(c) },
                 attrs: {}, setAttribute(k, v) { this.attrs[k] = v; } };
    });
    const doc = { documentElement: { dataset: {} }, querySelectorAll: () => segments };
    const localStorage = storage;
    const m = { exports: {} };
    new Function('module', 'document', 'localStorage', process.argv[1] + '\nmodule.exports = { HandLayout };')(m, doc, localStorage);
    return { HandLayout: m.exports.HandLayout, doc, segments };
};
const memory = (initial) => { const data = { ...initial }; return { data, getItem: (k) => (k in data ? data[k] : null), setItem: (k, v) => { data[k] = String(v); } }; };
const broken = { getItem() { throw new Error('blocked'); }, setItem() { throw new Error('blocked'); } };
const out = {};

let r = run(memory({}));
out.defaultValue = r.HandLayout.get();
out.invalid = run(memory({ 'scrabble-hand-position': 'sideways' })).HandLayout.get();
out.storedBottom = run(memory({ 'scrabble-hand-position': 'bottom' })).HandLayout.get();

const store = memory({});
r = run(store);
r.HandLayout.apply();
out.applied = { data: r.doc.documentElement.dataset.hand, active: r.segments.map(s => s.classList.has('active')), checked: r.segments.map(s => s.attrs['aria-checked']) };
r.HandLayout.set('bottom');
out.afterSet = { data: r.doc.documentElement.dataset.hand, stored: store.data['scrabble-hand-position'], active: r.segments.map(s => s.classList.has('active')) };
r.HandLayout.toggle();
out.afterToggle = { data: r.doc.documentElement.dataset.hand, stored: store.data['scrabble-hand-position'] };
r.HandLayout.set('sideways');
out.afterInvalid = r.doc.documentElement.dataset.hand;

r = run(broken);                      // letiltott tároló (privát mód): marad az alapérték, és a munkamenetben váltható
out.broken = { initial: r.HandLayout.get() };
r.HandLayout.toggle();
out.broken.afterToggle = r.HandLayout.get();
out.broken.data = r.doc.documentElement.dataset.hand;
console.log(JSON.stringify(out));
"""


@pytest.fixture(scope='module')
def hand_layout_result():
    return _run(HAND_LAYOUT_HARNESS, _block(_app_js(), 'const HandLayout = {', '\n};\n'))


class TestHandLayout:
    def test_default_is_the_rack_on_the_right(self, hand_layout_result):
        assert hand_layout_result['defaultValue'] == 'right'

    def test_stored_choice_is_used_and_garbage_is_ignored(self, hand_layout_result):
        assert hand_layout_result['storedBottom'] == 'bottom'
        assert hand_layout_result['invalid'] == 'right'

    def test_apply_marks_the_document_and_the_selector(self, hand_layout_result):
        assert hand_layout_result['applied'] == {'data': 'right', 'active': [True, False], 'checked': ['true', 'false']}

    def test_set_saves_and_applies(self, hand_layout_result):
        assert hand_layout_result['afterSet'] == {'data': 'bottom', 'stored': 'bottom', 'active': [False, True]}

    def test_toggle_flips_and_invalid_values_are_ignored(self, hand_layout_result):
        assert hand_layout_result['afterToggle'] == {'data': 'right', 'stored': 'right'}
        assert hand_layout_result['afterInvalid'] == 'right'

    def test_works_without_browser_storage(self, hand_layout_result):
        assert hand_layout_result['broken'] == {'initial': 'right', 'afterToggle': 'bottom', 'data': 'bottom'}
