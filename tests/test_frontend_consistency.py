"""A kliens (app.js, index.html) és a szerver összhangja — elírások, elcsúszott konstansok kiszűrése."""
import json
import os
import re
import shutil
import subprocess

import pytest

import board
from tiles import TILE_COUNTS, TILE_DISTRIBUTION, TILE_VALUES, LETTERS

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def _read(*parts):
    with open(os.path.join(ROOT, *parts), encoding='utf-8') as fh:
        return fh.read()


@pytest.fixture(scope='module')
def app_js():
    return _read('static', 'app.js')


@pytest.fixture(scope='module')
def index_html():
    return _read('templates', 'index.html')


def _js_object(source, name):
    """A `const NAME = { 'A': 1, ... };` objektum literál kiolvasása."""
    body = re.search(rf'const {name} = \{{(.*?)\}};', source, re.S).group(1)
    return {key: int(value) for key, value in re.findall(r"'([^']*)':\s*(\d+)", body)}


class TestConstants:
    def test_tile_values_match_the_server(self, app_js):
        assert _js_object(app_js, 'TILE_VALUES') == TILE_VALUES

    def test_tile_counts_match_the_server(self, app_js):
        assert _js_object(app_js, 'TILE_COUNTS') == TILE_COUNTS

    def test_all_letters_match_the_server(self, app_js):
        body = re.search(r'const ALL_LETTERS = \[(.*?)\];', app_js, re.S).group(1)
        assert sorted(re.findall(r"'([^']+)'", body)) == sorted(LETTERS)

    def test_premium_squares_match_the_server(self, app_js):
        body = re.search(r'const PREMIUM_QUARTER = \[(.*?)\n\];', app_js, re.S).group(1)
        js = {(int(r), int(c)): t for r, c, t in re.findall(r"\[(\d+),\s*(\d+),\s*'(\w+)'\]", body)}
        py = {(r, c): t for r, c, t in board._PREMIUM_QUARTER}
        assert js == py

    def test_distribution_is_the_standard_one(self):
        assert len(TILE_DISTRIBUTION) == 39


class TestHtmlIds:
    def test_every_element_id_used_in_js_exists_in_html(self, app_js, index_html):
        html_ids = set(re.findall(r'\bid="([^"]+)"', index_html))
        used = set(re.findall(r"getElementById\('([^']+)'\)", app_js))
        missing = sorted(i for i in used if i not in html_ids)
        assert not missing, missing

    def test_no_duplicate_ids_in_html(self, index_html):
        ids = re.findall(r'\bid="([^"]+)"', index_html)
        duplicates = sorted({i for i in ids if ids.count(i) > 1})
        assert not duplicates, duplicates

    def test_scripts_are_loaded_in_dependency_order(self, index_html):
        order = [m for m in re.findall(r'<script src="([^"]+)"', index_html)]
        names = [re.sub(r'\?.*', '', o).rsplit('/', 1)[-1] for o in order]
        assert names.index('i18n-data.js') < names.index('i18n.js') < names.index('app.js')
        assert names.index('socket.io.min.js') < names.index('app.js')

    def test_id_selectors_in_js_exist(self, app_js, index_html):
        html_ids = set(re.findall(r'\bid="([^"]+)"', index_html))
        # felépítés közben létrehozott elemek azonosítói nem szerepelhetnek a HTML-ben
        dynamic = {'game-screen'}
        used = set(re.findall(r"querySelector\('#([\w-]+)", app_js)) - dynamic
        assert not sorted(i for i in used if i not in html_ids)


def _server_source():
    return _read('server.py') + _read('routes.py')


class TestBotLevelSelector:
    def test_difficulty_select_offers_exactly_the_server_levels(self, index_html):
        import ai_player
        select = re.search(r'<select id="room-ai-difficulty"[^>]*>(.*?)</select>', index_html, re.S).group(1)
        values = re.findall(r'<option value="(\w+)"', select)
        assert values == [str(d) for d in ai_player.DIFFICULTIES] + [ai_player.ADAPTIVE]

    def test_default_option_is_the_default_level(self, index_html):
        import ai_player
        select = re.search(r'<select id="room-ai-difficulty"[^>]*>(.*?)</select>', index_html, re.S).group(1)
        selected = re.findall(r'<option value="(\d+)" selected', select)
        assert selected == [str(ai_player.DEFAULT_LEVEL)]

    def test_every_option_has_a_matching_translation_key(self, index_html):
        select = re.search(r'<select id="room-ai-difficulty"[^>]*>(.*?)</select>', index_html, re.S).group(1)
        for value, key in re.findall(r'<option value="(\w+)"[^>]*data-i18n="([^"]+)"', select):
            assert key == f'ai.level_{value}'


class TestSocketEvents:
    def test_events_emitted_by_the_client_have_server_handlers(self, app_js):
        handlers = set(re.findall(r"@socketio\.on\('(\w+)'\)", _server_source()))
        emitted = set(re.findall(r"socket\.emit\('(\w+)'", app_js))
        assert emitted, 'nem találtam socket.emit hívást'
        missing = sorted(e for e in emitted if e not in handlers)
        assert not missing, missing

    def test_events_listened_by_the_client_are_sent_by_the_server(self, app_js):
        source = _server_source()
        sent = set(re.findall(r"emit\(\s*'(\w+)'", source))
        listened = set(re.findall(r"socket\.on\('(\w+)'", app_js))
        # a Socket.IO saját eseményei
        builtin = {'connect', 'disconnect', 'connect_error'}
        missing = sorted(e for e in listened - builtin if e not in sent)
        assert not missing, missing

    def test_new_features_are_wired_on_both_sides(self, app_js):
        source = _server_source()
        for event in ('spectate_room', 'leave_spectate', 'preview_move', 'request_hint'):
            assert f"@socketio.on('{event}')" in source
            assert f"'{event}'" in app_js  # a kilépés feltételes: socket.emit(cond ? 'a' : 'b')
        for event in ('spectate_joined', 'spectate_left', 'move_preview', 'hint_result', 'live_games'):
            assert f"'{event}'" in source
            assert f"socket.on('{event}'" in app_js


class TestApiRoutes:
    def test_fetched_api_paths_exist_on_the_server(self, app_js):
        from server import app
        rules = {rule.rule for rule in app.url_map.iter_rules()}
        paths = set(re.findall(r"fetch\(`?'?(/api/[\w/-]+)", app_js))
        assert paths
        for path in paths:
            normalized = re.sub(r'/\d+', '/<int:game_id>', path)
            assert any(
                rule == path or re.sub(r'<[^>]+>', '*', rule) == re.sub(r'\$\{[^}]+\}', '*', normalized)
                or path.rstrip('/') == rule
                for rule in rules
            ) or any(rule.startswith(path) for rule in rules), path


@pytest.mark.skipif(shutil.which('node') is None, reason='node nem elérhető')
class TestJavaScriptSyntax:
    @pytest.mark.parametrize('name', ['app.js', 'i18n.js', 'i18n-data.js'])
    def test_files_parse(self, name):
        result = subprocess.run(['node', '--check', os.path.join(ROOT, 'static', name)],
                                capture_output=True, text=True, timeout=30)
        assert result.returncode == 0, result.stderr

    def test_service_worker_template_parses(self):
        from server import app
        with app.test_client() as client:
            body = client.get('/sw.js').get_data(as_text=True)
        result = subprocess.run(['node', '--check', '-'], input=body, capture_output=True,
                                text=True, timeout=30)
        assert result.returncode == 0, result.stderr


class TestStaticAssets:
    def test_every_static_reference_in_html_resolves(self, index_html):
        from server import app
        paths = set(re.findall(r'(?:href|src)="(/static/[^"?]+)', index_html))
        assert paths
        with app.test_client() as client:
            for path in paths:
                assert client.get(path).status_code == 200, path

    def test_html_lang_toggle_has_aria_label(self, index_html):
        for match in re.finditer(r'<button[^>]*btn-lang-toggle[^>]*>', index_html):
            assert 'aria-label' in match.group(0)


class TestInviteLinkOrdering:
    def test_url_actions_run_only_after_identity_is_sent(self, app_js):
        """A meghívó linkes csatlakozás előtt a szervernek ismernie kell a nevet (set_name)."""
        assert 'const identityReady = this.sendIdentity();' in app_js
        assert 'identityReady.then(() => this.handleUrlActions());' in app_js
        assert app_js.count('this.handleUrlActions()') == 1
