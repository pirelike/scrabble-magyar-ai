"""Többnyelvű felület: a fordítások teljessége és a kliens fordító működése."""
import ast
import json
import os
import re
import shutil
import subprocess
from html.parser import HTMLParser

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
_PLACEHOLDER = re.compile(r'\{(\w+)\}')


def _read(*parts):
    with open(os.path.join(ROOT, *parts), encoding='utf-8') as fh:
        return fh.read()


@pytest.fixture(scope='module')
def data():
    source = _read('static', 'i18n-data.js')
    body = re.search(r'^window\.I18N_DATA = (.*);\s*$', source, re.S | re.M).group(1)
    return json.loads(body)


@pytest.fixture(scope='module')
def app_js():
    return _read('static', 'app.js') + _read('static', 'i18n.js')


@pytest.fixture(scope='module')
def index_html():
    return _read('templates', 'index.html')


def _ui_keys(translations):
    return {k for k in translations if k != 'server'}


# Dinamikusan összerakott kulcscsaládok (a kódban `t('előtag.' + ...)` formában szerepelnek)
_DYNAMIC_PREFIXES = ('ai.', 'history.', 'dict.reason_', 'sound.cat_', 'board.', 'last.vote_')


class TestDataFile:
    def test_both_languages_have_the_same_keys(self, data):
        assert _ui_keys(data['hu']) == _ui_keys(data['en'])

    def test_no_empty_translations(self, data):
        for lang in ('hu', 'en'):
            for key in _ui_keys(data[lang]):
                assert data[lang][key].strip() or key == 'lobby.guest_suffix', (lang, key)

    def test_placeholders_match_between_languages(self, data):
        for key in _ui_keys(data['hu']):
            hu = set(_PLACEHOLDER.findall(data['hu'][key]))
            en = set(_PLACEHOLDER.findall(data['en'][key]))
            if key.endswith('_one'):
                # az angol egyes szám elhagyhatja a számot ("1 tile"), de újat nem vezethet be
                assert en <= hu, f'{key}: {hu} != {en}'
            else:
                assert hu == en, f'{key}: {hu} != {en}'

    def test_plural_keys_have_a_base_key(self, data):
        for key in _ui_keys(data['en']):
            if key.endswith('_one'):
                assert key[:-4] in data['en'], key

    def test_english_differs_from_hungarian_for_real_text(self, data):
        # a nyelvfüggetlen szavak kivételével az angol szöveg nem lehet magyar másolat
        same = [k for k in _ui_keys(data['hu']) if data['hu'][k] == data['en'][k]]
        allowed = {'game.chat', 'game.offline', 'history.vs', 'sound.max', 'create.ai_1',
                   'room.badge_bots_one', 'create.limit_none'}
        assert not [k for k in same if k not in allowed and len(data['hu'][k]) > 12], same

    def test_file_is_loadable_by_the_browser(self):
        assert _read('static', 'i18n-data.js').startswith('//')
        assert 'window.I18N_DATA = {' in _read('static', 'i18n-data.js')


class TestKeyUsage:
    def test_every_key_used_in_js_exists(self, data, app_js):
        used = set(re.findall(r"\bt\('([a-z_]+\.[a-z0-9_]+)'", app_js))
        used = {k for k in used if not k.endswith('_')}  # a `t('előtag_' + ...)` családok előtagjai
        missing = sorted(k for k in used if k not in data['hu'])
        assert not missing, missing

    def test_every_html_key_exists(self, data, index_html):
        used = set(re.findall(r'data-i18n(?:-placeholder|-title|-aria)?="([^"]+)"', index_html))
        assert used
        assert not sorted(k for k in used if k not in data['hu'])
        assert not sorted(k for k in used if k not in data['en'])

    def test_dynamic_key_families_are_complete(self, data):
        for level in range(1, 11):
            assert f'ai.level_{level}' in data['en'] and f'ai.level_{level}' in data['hu']
        for kind in ('exchange', 'pass', 'challenge_reject', 'other'):
            assert f'history.{kind}' in data['en']
        for reason in ('not_in_dictionary', 'invalid_chars', 'too_long', 'too_short'):
            assert f'dict.reason_{reason}' in data['en']
        for cat in ('tile_place', 'vote', 'challenge_result', 'your_turn', 'chat', 'game_events'):
            assert f'sound.cat_{cat}' in data['en'] and f'sound.cat_{cat}_desc' in data['en']
        for prefix in ('dl', 'tl', 'dw', 'tw'):
            assert f'board.{prefix}_long' in data['en'] and f'board.{prefix}_short' in data['en']
        for kind in ('exchange', 'pass', 'rejected', 'skip', 'vote_accept', 'vote_reject',
                     'game_over', 'game_over_draw', 'save_revert', 'place', 'pending'):
            assert f'last.{kind}' in data['en']

    def test_no_unused_keys(self, data, app_js, index_html):
        corpus = app_js + index_html
        unused = []
        for key in _ui_keys(data['hu']):
            if key.endswith('_one'):
                continue
            if key.startswith(_DYNAMIC_PREFIXES):
                continue
            if f"'{key}'" not in corpus and f'"{key}"' not in corpus:
                unused.append(key)
        assert not unused, unused

    def test_js_params_cover_the_placeholders(self, data, app_js):
        """Minden `t('kulcs', {..})` hívás megadja a szöveg összes {helyőrzőjét}."""
        for match in re.finditer(r"\bt\('([a-z_]+\.[a-z0-9_]+)',\s*\{", app_js):
            key = match.group(1)
            # a nyitó kapcsos zárójeltől a párjáig
            i = match.end()
            depth = 1
            start = i
            while depth and i < len(app_js):
                if app_js[i] == '{':
                    depth += 1
                elif app_js[i] == '}':
                    depth -= 1
                i += 1
            body = app_js[start:i - 1]
            # felső szintű vesszők mentén szétvágva
            parts, level, current = [], 0, ''
            for ch in body:
                if ch in '([{':
                    level += 1
                elif ch in ')]}':
                    level -= 1
                if ch == ',' and level == 0:
                    parts.append(current)
                    current = ''
                else:
                    current += ch
            parts.append(current)
            names = {p.split(':')[0].strip() for p in parts if p.strip()}
            needed = set(_PLACEHOLDER.findall(data['hu'][key]))
            assert needed <= names, f'{key}: hiányzó paraméter {needed - names}'


class _TextCollector(HTMLParser):
    """A HTML látható szövegei és attribútumai, a `data-i18n*` jelölések ellenőrzéséhez."""

    VOID = {'meta', 'link', 'input', 'br', 'hr', 'img', 'source', 'use', 'path', 'circle', 'line',
            'polyline', 'polygon', 'rect', 'ellipse'}

    def __init__(self):
        super().__init__()
        self.stack = []
        self.untranslated_text = []
        self.untranslated_attrs = []

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag not in self.VOID:
            self.stack.append((tag, attrs))
        if 'placeholder' in attrs and 'data-i18n-placeholder' not in attrs:
            self.untranslated_attrs.append(('placeholder', attrs['placeholder']))
        if 'title' in attrs and tag != 'title' and 'data-i18n-title' not in attrs:
            self.untranslated_attrs.append(('title', attrs['title']))
        if 'aria-label' in attrs and 'data-i18n-title' not in attrs and 'data-i18n-aria' not in attrs:
            self.untranslated_attrs.append(('aria-label', attrs['aria-label']))

    def handle_endtag(self, tag):
        for i in range(len(self.stack) - 1, -1, -1):
            if self.stack[i][0] == tag:
                del self.stack[i:]
                break

    def handle_data(self, data):
        text = data.strip()
        if not text or not re.search(r'[A-Za-zÁ-űá-ű]{2,}', text):
            return
        tags = [t for t, _ in self.stack]
        if any(t in ('script', 'style', 'title', 'svg', 'symbol') for t in tags):
            return
        if any('data-i18n' in attrs for _t, attrs in self.stack):
            return
        classes = ' '.join(attrs.get('class', '') for _t, attrs in self.stack)
        if 'brand-mark' in classes or 'lang-code' in classes:
            return
        self.untranslated_text.append(text)


class TestHtmlCoverage:
    def test_all_visible_text_is_translatable(self, index_html):
        parser = _TextCollector()
        parser.feed(index_html)
        # a kód által feltöltött, kezdetben üres elemek nem szerepelnek; a statikus szövegek igen
        assert not parser.untranslated_text, parser.untranslated_text

    def test_all_attributes_are_translatable(self, index_html):
        parser = _TextCollector()
        parser.feed(index_html)
        assert not parser.untranslated_attrs, parser.untranslated_attrs

    def test_language_toggle_buttons_exist_on_main_screens(self, index_html):
        assert index_html.count('btn-lang-toggle') >= 5

    def test_html_lang_attribute_is_set_before_paint(self, index_html):
        assert "document.documentElement.setAttribute('lang', lang)" in index_html
        assert 'scrabble-lang' in index_html


def _server_messages():
    """A szerver forrásában szereplő, a kliensnek szánt üzenetek (AST alapján)."""
    found = {}
    for name in ('server.py', 'routes.py', 'game.py', 'board.py', 'auth.py'):
        tree = ast.parse(_read(name))
        for node in ast.walk(tree):
            if isinstance(node, ast.Dict):
                for key, value in zip(node.keys, node.values):
                    if (isinstance(key, ast.Constant) and key.value == 'message'
                            and isinstance(value, ast.Constant) and isinstance(value.value, str)):
                        found.setdefault(value.value, name)
            if isinstance(node, ast.Return) and isinstance(node.value, ast.Tuple):
                for el in node.value.elts:
                    if (isinstance(el, ast.Constant) and isinstance(el.value, str)
                            and re.search(r'[a-záéíóöőúüű]{3}', el.value)
                            and el.value.endswith(('.', '!'))):
                        found.setdefault(el.value, name)
            if isinstance(node, ast.JoinedStr):
                text = ''.join(v.value if isinstance(v, ast.Constant) else '7' for v in node.values)
                if (re.search(r'[a-záéíóöőúüű]{3}', text) and ' ' in text
                        and text.endswith(('.', '!'))):
                    found.setdefault(text, name)
    found.pop('', None)
    return found


# A kliens ezeket a szerkezetes adatból maga formázza, vagy sosem jelennek meg a felhasználónak
_NOT_SHOWN = {
    '7 szavai elutasítva: 7. Betűk visszavéve, újra ő következik.',
    '7 elfogadta.',
    '7 elutasította.',
    '7 lerakása a mentés miatt visszavonva.',
}


class TestServerMessages:
    def test_every_server_message_has_an_english_translation(self, data):
        exact = data['en']['server']['exact']
        patterns = [re.compile(p) for p, _t in data['en']['server']['patterns']]
        missing = {
            msg: where for msg, where in _server_messages().items()
            if msg not in exact and msg not in _NOT_SHOWN and not any(p.match(msg) for p in patterns)
        }
        assert not missing, missing

    def test_exact_entries_exist_in_the_server_sources(self, data):
        sources = ''.join(_read(n) for n in ('server.py', 'routes.py', 'game.py', 'board.py', 'auth.py'))
        stale = [m for m in data['en']['server']['exact'] if m not in sources]
        assert not stale, stale

    def test_patterns_compile_and_have_templates(self, data):
        for pattern, template in data['en']['server']['patterns']:
            compiled = re.compile(pattern)
            groups = compiled.groups
            for ref in re.findall(r'\$(\d)', template):
                assert int(ref) <= groups, (pattern, template)


@pytest.mark.skipif(shutil.which('node') is None, reason='node nem elérhető')
class TestClientTranslator:
    """A böngészős fordító (i18n.js) valódi futtatása node-ban, minimális DOM-helyettesítővel."""

    HARNESS = r"""
    const fs = require('fs');
    const path = require('path');
    const root = process.argv[1];
    const lang = process.argv[2];
    const store = {};
    global.window = { dispatchEvent() {} };
    global.CustomEvent = class { constructor(type, init) { this.type = type; this.detail = init && init.detail; } };
    global.localStorage = { getItem: (k) => store[k] || null, setItem: (k, v) => { store[k] = v; } };
    global.document = {
        title: '',
        documentElement: { _lang: lang, getAttribute() { return this._lang; }, setAttribute(k, v) { if (k === 'lang') this._lang = v; } },
        querySelectorAll() { return []; },
        querySelector() { return null; },
    };
    const code = fs.readFileSync(path.join(root, 'static', 'i18n-data.js'), 'utf8') + '\n' +
                 fs.readFileSync(path.join(root, 'static', 'i18n.js'), 'utf8') +
                 '\nmodule.exports = { I18N, t, tServer };';
    const m = { exports: {} };
    new Function('module', 'exports', 'window', 'document', 'localStorage', 'CustomEvent', code)(
        m, m.exports, global.window, global.document, global.localStorage, global.CustomEvent);
    const { I18N, t, tServer } = m.exports;
    const cases = JSON.parse(process.argv[3]);
    const out = cases.map((c) => {
        if (c.op === 't') return t(c.key, c.params);
        if (c.op === 'server') return tServer(c.msg);
        if (c.op === 'set') { I18N.setLang(c.lang); return I18N.lang; }
        if (c.op === 'title') return document.title;
        return null;
    });
    console.log(JSON.stringify(out));
    """

    def _run(self, lang, cases):
        result = subprocess.run(
            ['node', '-e', self.HARNESS, ROOT, lang, json.dumps(cases)],
            capture_output=True, text=True, timeout=30,
        )
        assert result.returncode == 0, result.stderr
        return json.loads(result.stdout.strip().splitlines()[-1])

    def test_translates_with_parameters(self):
        out = self._run('hu', [
            {'op': 't', 'key': 'room.players_owner', 'params': {'n': 2, 'max': 4, 'owner': 'Anna'}},
        ])
        assert out == ['2/4 játékos · Anna']

    def test_english(self):
        out = self._run('en', [
            {'op': 't', 'key': 'room.players_owner', 'params': {'n': 2, 'max': 4, 'owner': 'Anna'}},
            {'op': 't', 'key': 'lobby.join'},
        ])
        assert out == ['2/4 players · Anna', 'Join']

    def test_singular_form_is_used_for_one(self):
        out = self._run('en', [
            {'op': 't', 'key': 'common.points', 'params': {'n': 1}},
            {'op': 't', 'key': 'common.points', 'params': {'n': 5}},
            {'op': 't', 'key': 'game.bag', 'params': {'n': 1}},
        ])
        assert out == ['1 pt', '5 pts', 'Bag: 1 tile']

    def test_unknown_key_returns_the_key(self):
        assert self._run('en', [{'op': 't', 'key': 'no.such.key'}]) == ['no.such.key']

    def test_missing_param_keeps_placeholder(self):
        assert self._run('hu', [{'op': 't', 'key': 'common.points', 'params': {}}]) == ['{n} pont']

    def test_server_messages_are_translated_in_english_only(self):
        cases = [
            {'op': 'server', 'msg': 'Nem te következel.'},
            {'op': 'server', 'msg': "Érvénytelen szó(k): XQZ"},
            {'op': 'server', 'msg': "Nincs 'K' betűd."},
            {'op': 'server', 'msg': 'A (3,4) mező már foglalt.'},
            {'op': 'server', 'msg': 'Hibás kód. Még 3 próbálkozásod van.'},
            {'op': 'server', 'msg': 'Ismeretlen üzenet, amit nem fordítunk.'},
        ]
        assert self._run('en', cases) == [
            "It's not your turn.", 'Invalid word(s): XQZ', "You don't have the letter 'K'.",
            'Square (3,4) is already occupied.', 'Wrong code. You have 3 attempts left.',
            'Ismeretlen üzenet, amit nem fordítunk.',
        ]
        assert self._run('hu', cases[:1]) == ['Nem te következel.']

    def test_server_exact_lookup_ignores_object_prototype_names(self):
        assert self._run('en', [{'op': 'server', 'msg': 'constructor'}]) == ['constructor']

    def test_switching_language(self):
        out = self._run('hu', [
            {'op': 't', 'key': 'lobby.join'},
            {'op': 'set', 'lang': 'en'},
            {'op': 't', 'key': 'lobby.join'},
            {'op': 'title'},
            {'op': 'set', 'lang': 'xx'},
        ])
        assert out == ['Csatlakozás', 'en', 'Join', 'Hungarian Scrabble', 'en']

    def test_every_key_formats_without_leftover_placeholders(self, data):
        """Ha minden paramétert megadunk, nem marad feldolgozatlan {helyőrző}."""
        cases = []
        keys = [k for k in _ui_keys(data['en'])]
        params = {name: 'X' for name in ('n', 'max', 'owner', 'name', 'names', 'players', 'words', 'score',
                                         'player', 'room', 'word', 'total', 'bag', 'hands',
                                         'vowels', 'consonants', 'blanks', 'played', 'won', 'rate')}
        for key in keys:
            cases.append({'op': 't', 'key': key, 'params': params})
        for lang in ('hu', 'en'):
            out = self._run(lang, cases)
            leftovers = [(k, v) for k, v in zip(keys, out) if '{' in v]
            assert not leftovers, leftovers
