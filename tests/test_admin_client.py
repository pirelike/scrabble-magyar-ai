"""Admin panel kliens: JS szintaxis, fordítások teljessége, a hívott API útvonalak és elem-azonosítók léte.

A `test_frontend_consistency.py` és a `test_i18n.py` mintájára, de az admin fájlokra: azok nem a nyilvános
`static/` mappában vannak, és a fordításaik sem a nyilvános `i18n-data.js`-ben. A kliens több fájlból áll
(mag, komponensek, három nézetfájl, indítás): a tesztek az összes fájlt együtt vizsgálják.
"""
import ast
import glob
import json
import os
import re
import shutil
import subprocess

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
_PLACEHOLDER = re.compile(r'\{(\w+)\}')
# Implicit használat: az I18N.apply() a dokumentum címét az `app.title` kulcsból veszi
_IMPLICIT_KEYS = {'app.title'}
# A betöltés sorrendje a HTML-ben (a későbbi fájlok az előzők globális neveit használják)
JS_FILES = ['admin.js', 'admin-ui.js', 'admin-views-a.js', 'admin-views-b.js', 'admin-views-c.js', 'admin-main.js']
ALL_JS = JS_FILES + ['admin-boot.js', 'admin-i18n.js']
# Az összes menüpont (a nézetfájlok `registerSection` hívásai): azonosító → sorrend
SECTIONS = {'overview': 10, 'users': 20, 'rooms': 30, 'games': 40, 'async': 50, 'ratings': 60, 'dictionary': 70,
            'daily': 80, 'moderation': 90, 'comm': 100, 'stats': 110, 'security': 120, 'system': 130,
            'settings': 140, 'audit': 150}


def _read(*parts):
    with open(os.path.join(ROOT, *parts), encoding='utf-8') as fh:
        return fh.read()


@pytest.fixture(scope='module')
def admin_js():
    return '\n'.join(_read('admin_assets', name) for name in JS_FILES)


@pytest.fixture(scope='module')
def admin_html():
    return _read('templates', 'admin.html')


@pytest.fixture(scope='module')
def admin_data():
    source = _read('admin_assets', 'admin-i18n.js')
    body = re.search(r'^window\.ADMIN_I18N = (.*);\s*$', source, re.S | re.M).group(1)
    return json.loads(body)


@pytest.fixture(scope='module')
def public_data():
    source = _read('static', 'i18n-data.js')
    return json.loads(re.search(r'^window\.I18N_DATA = (.*);\s*$', source, re.S | re.M).group(1))


def _ui_keys(translations):
    return {key for key in translations if key != 'server'}


@pytest.mark.skipif(shutil.which('node') is None, reason='node nem elérhető')
class TestJavaScriptSyntax:
    @pytest.mark.parametrize('name', ALL_JS)
    def test_files_parse(self, name):
        result = subprocess.run(['node', '--check', os.path.join(ROOT, 'admin_assets', name)],
                                capture_output=True, text=True, timeout=30)
        assert result.returncode == 0, result.stderr

    def test_app_js_still_parses_with_the_entry_point(self):
        result = subprocess.run(['node', '--check', os.path.join(ROOT, 'static', 'app.js')],
                                capture_output=True, text=True, timeout=30)
        assert result.returncode == 0, result.stderr

    def test_no_stray_script_in_the_assets_folder(self):
        names = {os.path.basename(p) for p in glob.glob(os.path.join(ROOT, 'admin_assets', '*.js'))}
        assert names == set(ALL_JS)


class TestTranslations:
    def test_both_languages_have_the_same_keys(self, admin_data):
        assert _ui_keys(admin_data['hu']) == _ui_keys(admin_data['en'])

    def test_no_empty_translations(self, admin_data):
        for lang in ('hu', 'en'):
            empty = [k for k, v in admin_data[lang].items() if k != 'server' and not v.strip()]
            assert not empty, (lang, empty)

    def test_placeholders_match_between_languages(self, admin_data):
        for key in _ui_keys(admin_data['hu']):
            assert set(_PLACEHOLDER.findall(admin_data['hu'][key])) == \
                set(_PLACEHOLDER.findall(admin_data['en'][key])), key

    def test_keys_are_plain_lowercase_names(self, admin_data):
        """A kulcsok csak kisbetűt és aláhúzást tartalmaznak: a kódban szó szerint kereshetők (nincs összefűzés)."""
        for key in _ui_keys(admin_data['hu']) - _IMPLICIT_KEYS:
            assert re.fullmatch(r'admin\.[a-z_]+', key), key

    def test_admin_text_is_not_in_the_public_translations(self, admin_data, public_data):
        for lang in ('hu', 'en'):
            assert not [k for k in public_data[lang] if k.startswith('admin.')]
            overlap = (_ui_keys(admin_data[lang]) - _IMPLICIT_KEYS) & _ui_keys(public_data[lang])
            assert not overlap, overlap

    def test_every_key_used_in_js_and_html_exists(self, admin_data, admin_js, admin_html, public_data):
        used_js = set(re.findall(r"""['"]((?:admin|common)\.[a-z_]+)['"]""", admin_js))
        used_html = set(re.findall(r'data-i18n(?:-[a-z]+)?="([^"]+)"', admin_html))
        known = _ui_keys(admin_data['hu']) | _ui_keys(public_data['hu'])
        assert not (used_js | used_html) - known
        for key in used_html:  # a közös (nyilvános) kulcsok mindkét nyelven megvannak
            assert key in admin_data['hu'] or key in public_data['en']

    def test_every_translation_key_literal_in_js_is_well_formed(self, admin_js):
        """Minden `'admin.…'` szövegkonstans érvényes kulcs (elgépelt vagy számjegyes kulcs nem maradhat észrevétlen)."""
        for literal in re.findall(r"""['"](admin\.[^'"\s]*)['"]""", admin_js):
            assert re.fullmatch(r'admin\.[a-z_]+', literal), literal

    def test_no_unused_admin_keys(self, admin_data, admin_js, admin_html):
        used = set(re.findall(r"""['"](admin\.[a-z_]+)['"]""", admin_js))
        used |= set(re.findall(r'data-i18n(?:-[a-z]+)?="(admin\.[a-z_]+)"', admin_html))
        unused = _ui_keys(admin_data['hu']) - used - _IMPLICIT_KEYS
        assert not unused, unused

    def test_parameters_passed_in_js_match_the_placeholders(self, admin_data, admin_js):
        """`t('kulcs', {a, b})`: a hívás pontosan a fordításban szereplő paramétereket adja át."""
        checked = 0
        for match in re.finditer(r"""\bt\(\s*'(admin\.[a-z_]+)'\s*,\s*\{([^{}]*)\}""", admin_js):
            key, body = match.group(1), match.group(2)
            if key not in admin_data['hu']:
                continue
            expected = set(_PLACEHOLDER.findall(admin_data['hu'][key]))
            names = set()
            for part in re.split(r',(?![^()]*\))', body):
                part = part.strip()
                if part:
                    names.add(part.split(':')[0].strip())
            if not expected <= names:
                pytest.fail(f'{key}: hiányzó paraméter {expected - names}')
            checked += 1
        assert checked > 60

    def test_the_document_title_is_overridden_for_the_admin_page(self, admin_data):
        assert admin_data['hu']['app.title'].startswith('Admin')
        assert admin_data['en']['app.title'].startswith('Admin')

    def test_file_is_loadable_by_the_browser(self):
        source = _read('admin_assets', 'admin-i18n.js')
        assert source.rstrip().endswith('};')
        assert source.count('window.ADMIN_I18N') == 1


def _server_messages():
    """Az admin végpontok magyar üzenetei: `_json_error(status, 'üzenet')`, `AdminError('üzenet')` és
    `SettingError('üzenet')` hívások az összes admin modulban (a nem konstans üzenetet nem vizsgáljuk)."""
    names = ['admin.py', 'admin_routes.py', 'settings.py'] + \
        sorted(os.path.basename(p) for p in glob.glob(os.path.join(ROOT, 'admin_*.py')))
    messages = set()
    for name in dict.fromkeys(names):
        tree = ast.parse(_read(name))
        for node in ast.walk(tree):
            if not isinstance(node, ast.Call):
                continue
            func = node.func.id if isinstance(node.func, ast.Name) else getattr(node.func, 'attr', '')
            index = {'_json_error': 1, 'AdminError': 0, 'SettingError': 0}.get(func)
            if index is None or len(node.args) <= index:
                continue
            arg = node.args[index]
            if isinstance(arg, ast.Constant) and isinstance(arg.value, str):
                messages.add(arg.value)
    return messages


class TestServerMessages:
    def test_messages_were_found(self):
        assert len(_server_messages()) >= 100

    def test_every_message_has_an_english_translation(self, admin_data):
        exact = admin_data['en']['server']['exact']
        missing = _server_messages() - set(exact)
        assert not missing, missing

    def test_no_stale_translations(self, admin_data):
        exact = admin_data['en']['server']['exact']
        stale = set(exact) - _server_messages()
        assert not stale, stale

    def test_translations_are_not_empty_and_differ_from_the_source(self, admin_data):
        for hungarian, english in admin_data['en']['server']['exact'].items():
            assert english.strip() and english != hungarian


_METHODS = {'get': 'GET', 'post': 'POST', 'patch': 'PATCH', 'del': 'DELETE'}


def _normalise(path):
    """Egy JS útvonal-konstans (sablonbehelyettesítéssel és lekérdezéssel) → illesztésre alkalmas minta útvonal."""
    path = re.sub(r'\$\{[^}]*\}', '1', path)
    return path.split('?')[0]


def _route_matcher(app):
    rules = []
    for rule in app.url_map.iter_rules():
        if rule.rule.startswith('/api/admin') or rule.rule.startswith('/admin'):
            pattern = re.compile('^' + re.sub(r'<(?:\w+:)?\w+>', '[^/]+', re.escape(rule.rule).replace('\\<', '<')
                                              .replace('\\>', '>').replace('\\:', ':')) + '$')
            rules.append((pattern, rule.methods))
    return rules


class TestApiAndElements:
    def test_every_api_path_called_by_admin_js_exists(self, admin_js):
        from server import app
        rules = _route_matcher(app)
        paths = {_normalise(p) for p in re.findall(r"""[`'"](/api/admin[^`'"\s?]*)""", admin_js)}
        assert len(paths) > 100
        for path in paths:
            assert any(pattern.match(path) for pattern, _methods in rules), path

    def test_every_api_call_uses_a_method_the_route_allows(self, admin_js):
        from server import app
        rules = _route_matcher(app)
        calls = []
        for verb, path in re.findall(r"""Api\.(get|post|patch|del)\(\s*[`'"](/api/admin[^`'"]*)[`'"]""", admin_js):
            calls.append((_METHODS[verb], _normalise(path)))
        for method, path in re.findall(r"""Api\.(?:request|call)\(\s*'([A-Z]+)'\s*,\s*[`'"](/api/admin[^`'"]*)[`'"]""",
                                       admin_js):
            calls.append((method, _normalise(path)))
        assert len(calls) > 120
        for method, path in calls:
            assert any(pattern.match(path) and method in methods for pattern, methods in rules), (method, path)

    def test_every_downloaded_path_is_a_get_route(self, admin_js):
        from server import app
        rules = _route_matcher(app)
        for path in re.findall(r"""(?:UI\.download\([^,]+,\s*|location\.href\s*=\s*|downloadWithSudo\()[`'"](/api/admin[^`'"]*)""",
                               admin_js):
            path = _normalise(path)
            assert any(pattern.match(path) and 'GET' in methods for pattern, methods in rules), path

    def test_every_admin_route_with_a_ui_is_called_somewhere(self, admin_js):
        """Minden admin végpontnak van felülete (kivéve a munkamenet-kezelést és a naplót, amelyet a mag hív):
        új végpont nem maradhat gomb nélkül."""
        from server import app
        called = {_normalise(p) for p in re.findall(r"""[`'"](/api/admin[^`'"\s?]*)""", admin_js)}
        uncovered = []
        for rule in app.url_map.iter_rules():
            if not rule.rule.startswith('/api/admin'):
                continue
            sample = re.sub(r'<(?:\w+:)?\w+>', '1', rule.rule)
            if not any(re.match('^' + re.escape(sample).replace('1', '[^/]+') + '$', path) or path == sample
                       for path in called):
                uncovered.append(rule.rule)
        assert not uncovered, uncovered

    def test_every_element_id_used_by_admin_js_exists_in_the_html(self, admin_js, admin_html):
        used = set(re.findall(r"""getElementById\(['"]([\w-]+)['"]\)""", admin_js))
        used |= set(re.findall(r"""(?:Dialogs\.(?:open|close|isOpen)|showFormError|hideFormError)\(['"]([\w-]+)['"]""",
                               admin_js))
        assert len(used) > 20
        defined = set(re.findall(r'\bid="([\w-]+)"', admin_html))
        assert used <= defined, used - defined

    def test_every_static_reference_in_the_page_resolves(self, admin_html):
        paths = re.findall(r'(?:href|src)="(/(?:static|admin/assets)/[^"?{]+)', admin_html)
        assert paths
        for path in paths:
            if path.startswith('/static/'):
                assert os.path.isfile(os.path.join(ROOT, path.lstrip('/'))), path
            else:
                assert os.path.isfile(os.path.join(ROOT, 'admin_assets', path.split('/admin/assets/')[1])), path

    def test_scripts_load_in_dependency_order(self, admin_html):
        order = re.findall(r'<script src="/(?:static|admin/assets)/([^"?]+)', admin_html)
        assert order == ['admin-boot.js', 'i18n-data.js', 'admin-i18n.js', 'i18n.js'] + JS_FILES

    def test_the_only_external_script_is_the_socket_io_client(self, admin_html):
        external = re.findall(r'<script src="(https?://[^"]+)"', admin_html)
        assert external == ['https://cdnjs.cloudflare.com/ajax/libs/socket.io/4.7.4/socket.io.min.js']

    def test_every_menu_section_is_registered_in_order(self, admin_js):
        registered = {m.group(1): int(m.group(2)) for m in
                      re.finditer(r"registerSection\(\{\s*id:\s*'(\w+)',\s*order:\s*(\d+)", admin_js)}
        assert registered == SECTIONS

    def test_every_section_has_a_translated_menu_label(self, admin_data, admin_js):
        for match in re.finditer(r"registerSection\(\{[^}]*labelKey:\s*'(admin\.[a-z_]+)'", admin_js):
            assert match.group(1) in admin_data['hu'], match.group(1)

    def test_every_icon_used_in_the_menu_is_in_the_sprite(self, admin_js, admin_html):
        icons = set(re.findall(r"icon:\s*'([a-z]+)'", admin_js)) | set(re.findall(r"makeIcon\('([a-z]+)'\)", admin_js))
        sprite = set(re.findall(r'<symbol id="i-([a-z]+)"', admin_html))
        assert icons <= sprite, icons - sprite


class TestCodingRules:
    def test_no_innerhtml(self, admin_js):
        assert 'innerHTML' not in admin_js
        assert 'outerHTML' not in admin_js
        assert 'insertAdjacentHTML' not in admin_js
        assert 'document.write' not in admin_js

    def test_no_dynamic_code(self, admin_js):
        assert not re.search(r'\beval\(|new Function\(', admin_js)

    def test_no_inline_style_attributes_from_js(self, admin_js):
        """A szigorú CSP (`style-src 'self'`) a `style` attribútumot nem engedi; a CSSOM (`el.style.x`) igen."""
        assert not re.search(r"setAttribute\(\s*['\"]style['\"]", admin_js)
        assert 'cssText' not in admin_js

    def test_every_request_carries_the_csrf_header(self, admin_js):
        assert "'X-Admin-Request': '1'" in admin_js
        # Minden hálózati hívás az `Api.request`-en megy át
        assert len(re.findall(r'\bfetch\(', admin_js)) == 1

    def test_no_other_network_apis(self, admin_js):
        assert not re.search(r'XMLHttpRequest|sendBeacon|new WebSocket|new EventSource', admin_js)

    def test_user_text_is_never_interpreted_as_html(self, admin_js):
        """A szöveg csak szövegcsomópontként kerül be (`h()` / `textContent`): `Node` nélküli gyerek is szöveg."""
        assert 'document.createTextNode(String(child))' in admin_js

    def test_admin_page_has_no_inline_code(self, admin_html):
        assert not re.findall(r'<script(?![^>]*\bsrc=)[^>]*>', admin_html)
        assert not re.search(r'\sstyle=', admin_html)
        assert not re.search(r'\son[a-z]+=', admin_html)

    def test_page_is_not_indexable(self, admin_html):
        assert '<meta name="robots" content="noindex,nofollow">' in admin_html

    def test_public_sources_do_not_know_the_admin_client(self):
        for name in (('static', 'app.js'), ('static', 'i18n-data.js'), ('static', 'i18n.js'), ('templates', 'index.html'),
                     ('templates', 'sw.js')):
            source = _read(*name)
            assert '/api/admin' not in source and 'admin_assets' not in source and 'admin-i18n' not in source, name

    def test_visible_text_is_translatable(self, admin_html):
        from html.parser import HTMLParser

        class Collector(HTMLParser):
            def __init__(self):
                super().__init__()
                self.stack, self.untranslated = [], []

            def handle_starttag(self, tag, attrs):
                if tag in ('br', 'input', 'meta', 'link', 'use', 'path', 'line', 'circle', 'rect', 'polyline',
                           'polygon'):
                    return
                self.stack.append((tag, any(name.startswith('data-i18n') and name != 'data-i18n-placeholder'
                                            and name != 'data-i18n-title' and name != 'data-i18n-aria'
                                            for name, _ in attrs)))

            def handle_endtag(self, tag):
                if self.stack and self.stack[-1][0] == tag:
                    self.stack.pop()

            def handle_data(self, data):
                text = data.strip()
                if not text or not self.stack or self.stack[-1][0] in ('script', 'style', 'title', 'symbol'):
                    return
                if text == 'HU':  # a nyelvváltó gomb kódját az I18N.apply() tölti ki
                    return
                if not any(flag for _, flag in self.stack):
                    self.untranslated.append(text)

        collector = Collector()
        collector.feed(admin_html)
        # Dinamikusan töltött elemek (JS tölti): a felirat üres a HTML-ben
        assert not collector.untranslated, collector.untranslated


@pytest.mark.skipif(shutil.which('node') is None, reason='node nem elérhető')
class TestClientRuntime:
    """A kliens magja és a fordítás valódi futtatása node-ban, minimális DOM-helyettesítővel (a tiszta segédek)."""

    HARNESS = r"""
    const fs = require('fs');
    const path = require('path');
    const root = process.argv[1];
    const store = {};
    global.window = { dispatchEvent() {} };
    global.CustomEvent = class { constructor(type, init) { this.type = type; this.detail = init && init.detail; } };
    global.localStorage = { getItem: (k) => store[k] || null, setItem: (k, v) => { store[k] = v; } };
    global.document = {
        title: '',
        documentElement: { _lang: 'hu', getAttribute() { return this._lang; }, setAttribute(k, v) { if (k === 'lang') this._lang = v; } },
        querySelectorAll() { return []; },
        querySelector() { return null; },
    };
    const read = (...p) => fs.readFileSync(path.join(root, ...p), 'utf8');
    const code = read('static', 'i18n-data.js') + '\n' + read('admin_assets', 'admin-i18n.js') + '\n' + read('static', 'i18n.js') +
                 '\n' + read('admin_assets', 'admin.js') +
                 '\nmodule.exports = { I18N, t, tServer, queryString, fmtNum, fmtPercent, fmtBytes, fmtDuration, formatCountdown };';
    const m = { exports: {} };
    new Function('module', 'exports', 'window', 'document', 'localStorage', 'CustomEvent', code)(
        m, m.exports, global.window, global.document, global.localStorage, global.CustomEvent);
    const api = m.exports;
    const cases = JSON.parse(fs.readFileSync(0, 'utf8'));
    const out = cases.map((c) => {
        if (c.op === 'lang') { api.I18N.setLang(c.lang); return api.I18N.lang; }
        if (c.op === 't') return api.t(c.key, c.params);
        if (c.op === 'server') return api.tServer(c.msg);
        return api[c.op].apply(null, c.args || []);
    });
    process.stdout.write(JSON.stringify(out));
    """

    def _run(self, cases):
        result = subprocess.run(['node', '-e', self.HARNESS, ROOT], input=json.dumps(cases), capture_output=True,
                                text=True, timeout=60)
        assert result.returncode == 0, result.stderr
        return json.loads(result.stdout)

    def test_pure_helpers_work_without_a_dom(self):
        out = self._run([
            {'op': 'queryString', 'args': [{'a': 1, 'b': '', 'c': None, 'd': True, 'e': 'x y'}]},
            {'op': 'queryString', 'args': [{}]},
            {'op': 'fmtBytes', 'args': [1536]},
            {'op': 'fmtBytes', 'args': [None]},
            {'op': 'fmtPercent', 'args': [12.34]},
            {'op': 'fmtNum', 'args': [None]},
            {'op': 'formatCountdown', 'args': [75]},
            {'op': 'fmtDuration', 'args': [90061]},
            {'op': 'fmtDuration', 'args': [125]},
            {'op': 'fmtDuration', 'args': [42]},
        ])
        assert out[0] == '?a=1&d=1&e=x+y'
        assert out[1] == ''
        assert out[2].endswith('KB') and out[3] == '–'
        assert out[4].endswith('%') and out[5] == '–'
        assert out[6] == '1:15'
        assert out[7] == '1 n 1 ó' and out[8] == '2 p' and out[9] == '42 mp'

    def test_translations_are_merged_into_the_shared_translator(self, admin_data):
        out = self._run([
            {'op': 't', 'key': 'admin.nav_users'},
            {'op': 'server', 'msg': 'Hibás jelszó.'},
            {'op': 'lang', 'lang': 'en'},
            {'op': 't', 'key': 'admin.nav_users'},
            {'op': 'server', 'msg': 'Hibás jelszó.'},
            {'op': 'server', 'msg': 'A felhasználó nincs kitiltva.'},
            {'op': 'fmtDuration', 'args': [90061]},
            {'op': 't', 'key': 'admin.audit_range', 'params': {'from': 1, 'to': 50, 'total': 120}},
        ])
        assert out[0] == admin_data['hu']['admin.nav_users']
        assert out[1] == 'Hibás jelszó.'                       # magyarul az eredeti marad
        assert out[3] == admin_data['en']['admin.nav_users']
        assert out[4] == 'Wrong password.'
        assert out[5] == 'The user is not banned.'
        assert out[6] == '1 d 1 h'
        assert out[7] == '1–50 / 120'

    def test_every_admin_text_formats_in_both_languages(self, admin_data):
        cases, expected = [{'op': 'lang', 'lang': 'hu'}], []
        for lang in ('hu', 'en'):
            if lang == 'en':
                cases.append({'op': 'lang', 'lang': 'en'})
            for key, text in admin_data[lang].items():
                if key == 'server':
                    continue
                params = {name: 'X' for name in _PLACEHOLDER.findall(text)}
                cases.append({'op': 't', 'key': key, 'params': params})
                expected.append((lang, key))
        out = self._run(cases)
        texts = [o for o, c in zip(out, cases) if c['op'] == 't']
        assert len(texts) == len(expected)
        for (lang, key), text in zip(expected, texts):
            assert '{' not in text and '}' not in text, (lang, key, text)
