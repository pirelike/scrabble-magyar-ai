"""Admin panel kliens: JS szintaxis, fordítások teljessége, a hívott API útvonalak és elem-azonosítók léte.

A `test_frontend_consistency.py` és a `test_i18n.py` mintájára, de az admin fájlokra: azok nem a nyilvános
`static/` mappában vannak, és a fordításaik sem a nyilvános `i18n-data.js`-ben.
"""
import ast
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


def _read(*parts):
    with open(os.path.join(ROOT, *parts), encoding='utf-8') as fh:
        return fh.read()


@pytest.fixture(scope='module')
def admin_js():
    return _read('admin_assets', 'admin.js')


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
    @pytest.mark.parametrize('name', ['admin.js', 'admin-boot.js', 'admin-i18n.js'])
    def test_files_parse(self, name):
        result = subprocess.run(['node', '--check', os.path.join(ROOT, 'admin_assets', name)],
                                capture_output=True, text=True, timeout=30)
        assert result.returncode == 0, result.stderr

    def test_app_js_still_parses_with_the_entry_point(self):
        result = subprocess.run(['node', '--check', os.path.join(ROOT, 'static', 'app.js')],
                                capture_output=True, text=True, timeout=30)
        assert result.returncode == 0, result.stderr


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

    def test_no_unused_admin_keys(self, admin_data, admin_js, admin_html):
        used = set(re.findall(r"""['"](admin\.[a-z_]+)['"]""", admin_js))
        used |= set(re.findall(r'data-i18n(?:-[a-z]+)?="(admin\.[a-z_]+)"', admin_html))
        unused = _ui_keys(admin_data['hu']) - used - _IMPLICIT_KEYS
        assert not unused, unused

    def test_the_document_title_is_overridden_for_the_admin_page(self, admin_data):
        assert admin_data['hu']['app.title'].startswith('Admin')
        assert admin_data['en']['app.title'].startswith('Admin')

    def test_file_is_loadable_by_the_browser(self):
        source = _read('admin_assets', 'admin-i18n.js')
        assert source.rstrip().endswith('};')
        assert source.count('window.ADMIN_I18N') == 1


def _server_messages():
    """Az admin végpontok magyar üzenetei: `_json_error(status, 'üzenet')` és `AdminError('üzenet')` hívások."""
    messages = set()
    for name in ('admin_routes.py', 'admin.py'):
        tree = ast.parse(_read(name))
        for node in ast.walk(tree):
            if not isinstance(node, ast.Call):
                continue
            func = node.func.id if isinstance(node.func, ast.Name) else getattr(node.func, 'attr', '')
            index = {'_json_error': 1, 'AdminError': 0}.get(func)
            if index is None or len(node.args) <= index:
                continue
            arg = node.args[index]
            if isinstance(arg, ast.Constant) and isinstance(arg.value, str):
                messages.add(arg.value)
    return messages


class TestServerMessages:
    def test_messages_were_found(self):
        assert len(_server_messages()) >= 10

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


class TestApiAndElements:
    def test_every_api_path_called_by_admin_js_exists(self, admin_js):
        from server import app
        rules = [rule.rule for rule in app.url_map.iter_rules()]
        paths = set(re.findall(r"""['"`](/api/admin[a-z0-9/_-]*)""", admin_js))
        assert paths
        for path in paths:
            assert path in rules, path

    def test_every_element_id_used_by_admin_js_exists_in_the_html(self, admin_js, admin_html):
        used = set(re.findall(r"""getElementById\(['"]([\w-]+)['"]\)""", admin_js))
        used |= set(re.findall(r"""(?:Dialogs\.(?:open|close|isOpen)|showFormError|hideFormError)\(['"]([\w-]+)['"]""",
                               admin_js))
        assert used
        defined = set(re.findall(r'\bid="([\w-]+)"', admin_html))
        assert used <= defined, used - defined

    def test_every_static_reference_in_the_page_resolves(self, admin_html):
        from server import app
        paths = re.findall(r'(?:href|src)="(/(?:static|admin/assets)/[^"?{]+)', admin_html)
        assert paths
        for path in paths:
            if path.startswith('/static/'):
                assert os.path.isfile(os.path.join(ROOT, path.lstrip('/'))), path
            else:
                assert os.path.isfile(os.path.join(ROOT, 'admin_assets', path.split('/admin/assets/')[1])), path

    def test_scripts_load_in_dependency_order(self, admin_html):
        order = re.findall(r'<script src="/(?:static|admin/assets)/([^"?]+)', admin_html)
        assert order == ['admin-boot.js', 'i18n-data.js', 'admin-i18n.js', 'i18n.js', 'admin.js']


class TestCodingRules:
    def test_no_innerhtml(self, admin_js):
        assert 'innerHTML' not in admin_js
        assert 'outerHTML' not in admin_js
        assert 'insertAdjacentHTML' not in admin_js
        assert 'document.write' not in admin_js

    def test_no_dynamic_code(self, admin_js):
        assert not re.search(r'\beval\(|new Function\(', admin_js)

    def test_every_request_carries_the_csrf_header(self, admin_js):
        assert "'X-Admin-Request': '1'" in admin_js
        # Minden hálózati hívás az `Api.request`-en megy át
        assert len(re.findall(r'\bfetch\(', admin_js)) == 1

    def test_admin_page_has_no_inline_code(self, admin_html):
        assert not re.findall(r'<script(?![^>]*\bsrc=)[^>]*>', admin_html)
        assert not re.search(r'\sstyle=', admin_html)
        assert not re.search(r'\son[a-z]+=', admin_html)

    def test_page_is_not_indexable(self, admin_html):
        assert '<meta name="robots" content="noindex,nofollow">' in admin_html

    def test_visible_text_is_translatable(self, admin_html):
        from html.parser import HTMLParser

        class Collector(HTMLParser):
            def __init__(self):
                super().__init__()
                self.stack, self.untranslated = [], []

            def handle_starttag(self, tag, attrs):
                if tag in ('br', 'input', 'meta', 'link', 'use', 'path', 'line', 'circle', 'rect', 'polyline'):
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
