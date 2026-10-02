"""Publikus API-k: ranglista, szótár-ellenőrző, PWA végpontok."""
import json
import os

import pytest

import auth


@pytest.fixture
def client():
    from server import app
    app.config['TESTING'] = True
    import server
    server._ip_rate_limits.clear()
    with app.test_client() as c:
        yield c
    server._ip_rate_limits.clear()


def _user(name, email=None):
    ok, uid = auth.create_user(email or f'{name.lower()}@example.com', name, 'password123')
    assert ok is not False
    return uid


def _finished_game(room_id, results):
    """results: [(user_id|None, név, pont, nyert)]"""
    players = [{'player_name': n, 'user_id': uid, 'final_score': score, 'is_winner': won}
               for uid, n, score, won in results]
    return auth.finish_game(room_id, '{}', players, room_name=room_id)


class TestLeaderboardQuery:
    def test_empty(self):
        assert auth.get_leaderboard() == ([], None)

    def test_users_without_games_are_not_listed(self):
        _user('Nulla')
        assert auth.get_leaderboard()[0] == []

    def test_wins_ordering(self):
        a, b, c = _user('Aladár'), _user('Béla'), _user('Cili')
        _finished_game('r1', [(a, 'Aladár', 300, 1), (b, 'Béla', 200, 0)])
        _finished_game('r2', [(a, 'Aladár', 310, 1), (c, 'Cili', 100, 0)])
        _finished_game('r3', [(b, 'Béla', 250, 1), (c, 'Cili', 120, 0)])
        entries, _ = auth.get_leaderboard('wins')
        assert [e['display_name'] for e in entries] == ['Aladár', 'Béla', 'Cili']
        assert [e['rank'] for e in entries] == [1, 2, 3]
        assert entries[0]['games_won'] == 2 and entries[0]['games_played'] == 2
        assert entries[0]['win_rate'] == 100.0

    def test_avg_score_needs_minimum_games(self):
        a, b = _user('Aladár'), _user('Béla')
        for i in range(3):
            _finished_game(f'a{i}', [(a, 'Aladár', 100, 0)])
        _finished_game('b0', [(b, 'Béla', 999, 1)])  # csak egy játék
        entries, _ = auth.get_leaderboard('avg_score')
        assert [e['display_name'] for e in entries] == ['Aladár']

    def test_win_rate_needs_minimum_games(self):
        a, b = _user('Aladár'), _user('Béla')
        _finished_game('b0', [(b, 'Béla', 100, 1)])
        for i in range(3):
            _finished_game(f'a{i}', [(a, 'Aladár', 100, 1 if i else 0)])
        entries, _ = auth.get_leaderboard('win_rate')
        assert [e['display_name'] for e in entries] == ['Aladár']
        assert entries[0]['win_rate'] == 66.7

    def test_best_game(self):
        a, b = _user('Aladár'), _user('Béla')
        _finished_game('r1', [(a, 'Aladár', 150, 0), (b, 'Béla', 400, 1)])
        _finished_game('r2', [(a, 'Aladár', 420, 1), (b, 'Béla', 90, 0)])
        entries, _ = auth.get_leaderboard('best_game')
        assert [(e['display_name'], e['best_score']) for e in entries] == [
            ('Aladár', 420), ('Béla', 400)]

    def test_limit_and_my_rank_outside_top(self):
        users = [_user(f'U{i}') for i in range(5)]
        for i, uid in enumerate(users):
            _finished_game(f'g{i}', [(uid, f'U{i}', 100 + i, 1 if i < 4 - i else 0)])
        entries, me = auth.get_leaderboard('wins', limit=1, user_id=users[4])
        assert len(entries) == 1
        assert me is not None and me['rank'] > 1 and me['user_id'] == users[4]

    def test_guests_and_bots_are_not_listed(self):
        a = _user('Aladár')
        _finished_game('r1', [(a, 'Aladár', 100, 0), (None, 'Vendég', 300, 1), (None, 'Rezső', 200, 0)])
        names = [e['display_name'] for e in auth.get_leaderboard()[0]]
        assert names == ['Aladár']

    def test_unknown_metric_falls_back_to_wins(self):
        a = _user('Aladár')
        _finished_game('r1', [(a, 'Aladár', 100, 1)])
        assert auth.get_leaderboard('hack; DROP TABLE users')[0][0]['display_name'] == 'Aladár'

    def test_limit_is_clamped(self):
        a = _user('Aladár')
        _finished_game('r1', [(a, 'Aladár', 100, 1)])
        assert len(auth.get_leaderboard('wins', limit=-5)[0]) == 1
        assert len(auth.get_leaderboard('wins', limit=10 ** 9)[0]) == 1

    def test_emails_are_never_exposed(self):
        a = _user('Aladár', 'titkos@example.com')
        _finished_game('r1', [(a, 'Aladár', 100, 1)])
        assert 'titkos' not in json.dumps(auth.get_leaderboard()[0])


class TestLeaderboardRoute:
    def test_returns_entries(self, client):
        a = _user('Aladár')
        _finished_game('r1', [(a, 'Aladár', 100, 1)])
        res = client.get('/api/leaderboard')
        data = res.get_json()
        assert res.status_code == 200 and data['success']
        assert data['metric'] == 'wins' and data['entries'][0]['display_name'] == 'Aladár'
        assert data['me'] is None
        assert 'email' not in json.dumps(data)

    def test_metric_parameter(self, client):
        res = client.get('/api/leaderboard?metric=avg_score')
        assert res.get_json()['metric'] == 'avg_score'
        assert res.get_json()['min_games'] == 3

    def test_unknown_metric_is_rejected(self, client):
        assert client.get('/api/leaderboard?metric=x').status_code == 400

    def test_bad_limit_is_tolerated(self, client):
        assert client.get('/api/leaderboard?limit=abc').status_code == 200

    def test_marks_own_row(self, client):
        a = _user('Aladár')
        _finished_game('r1', [(a, 'Aladár', 100, 1)])
        client.set_cookie('session_token', auth.create_session(a))
        data = client.get('/api/leaderboard').get_json()
        assert data['entries'][0]['is_me'] is True
        assert data['me']['user_id'] == a

    def test_is_rate_limited(self, client):
        codes = [client.get('/api/leaderboard').status_code for _ in range(35)]
        assert 429 in codes and codes[0] == 200


class TestDictionaryRoute:
    def test_valid_word(self, client):
        data = client.get('/api/dictionary/check?q=alma').get_json()
        entry = data['results'][0]
        assert entry['word'] == 'ALMA' and entry['valid'] is True
        assert entry['tiles'] == ['A', 'L', 'M', 'A'] and entry['score'] == 4

    def test_invalid_word_has_reason_and_suggestions(self, client):
        entry = client.get('/api/dictionary/check?q=almak').get_json()['results'][0]
        assert entry['valid'] is False and entry['reason'] == 'not_in_dictionary'
        assert isinstance(entry['suggestions'], list)

    def test_digraph_tiles_and_score(self, client):
        entry = client.get('/api/dictionary/check?q=szék').get_json()['results'][0]
        assert entry['tiles'] == ['SZ', 'É', 'K'] and entry['score'] == 3 + 3 + 1
        assert entry['valid'] is True

    def test_multiple_words(self, client):
        results = client.get('/api/dictionary/check?q=alma,körte xyzzy').get_json()['results']
        assert [r['word'] for r in results] == ['ALMA', 'KÖRTE', 'XYZZY']
        assert [r['valid'] for r in results] == [True, True, False]

    def test_duplicates_are_merged(self, client):
        results = client.get('/api/dictionary/check?q=alma ALMA Alma').get_json()['results']
        assert len(results) == 1

    def test_at_most_eight_words(self, client):
        q = ' '.join(f'szó{i}' for i in range(20))
        assert len(client.get(f'/api/dictionary/check?q={q}').get_json()['results']) == 8

    def test_invalid_characters(self, client):
        entry = client.get('/api/dictionary/check?q=al1ma').get_json()['results'][0]
        assert entry['valid'] is False and entry['reason'] == 'invalid_chars'

    def test_too_long(self, client):
        entry = client.get('/api/dictionary/check?q=' + 'a' * 20).get_json()['results'][0]
        assert entry['reason'] == 'too_long' and entry['valid'] is False

    def test_single_letter_is_too_short(self, client):
        entry = client.get('/api/dictionary/check?q=a').get_json()['results'][0]
        assert entry['reason'] == 'too_short' and entry['valid'] is False

    def test_proper_nouns_are_not_valid(self, client):
        assert client.get('/api/dictionary/check?q=budapest').get_json()['results'][0]['valid'] is False

    def test_empty_query(self, client):
        res = client.get('/api/dictionary/check?q=')
        assert res.status_code == 400 and not res.get_json()['success']

    def test_post_json(self, client):
        res = client.post('/api/dictionary/check', json={'words': ['alma', 'körte']})
        assert [r['valid'] for r in res.get_json()['results']] == [True, True]

    def test_post_garbage(self, client):
        assert client.post('/api/dictionary/check', data='nem json').status_code == 400
        assert client.post('/api/dictionary/check', json={'words': 5}).status_code in (200, 400)

    def test_is_rate_limited(self, client):
        codes = [client.get('/api/dictionary/check?q=alma').status_code for _ in range(65)]
        assert 429 in codes


class TestPwa:
    def test_manifest(self, client):
        res = client.get('/manifest.webmanifest')
        assert res.status_code == 200
        assert res.mimetype == 'application/manifest+json'
        manifest = json.loads(res.data)
        assert manifest['display'] == 'standalone'
        assert manifest['start_url'].startswith('/') and manifest['scope'] == '/'
        assert manifest['name'] and manifest['short_name']
        sizes = {(i['sizes'], i.get('purpose')) for i in manifest['icons']}
        assert ('192x192', 'any') in sizes and ('512x512', 'maskable') in sizes

    def test_manifest_icons_exist(self, client):
        manifest = json.loads(client.get('/manifest.webmanifest').data)
        for icon in manifest['icons']:
            res = client.get(icon['src'])
            assert res.status_code == 200, icon['src']
            assert res.mimetype == icon['type']

    def test_png_icons_have_right_dimensions(self):
        import struct
        base = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'static', 'icons')
        for name, size in (('icon-192.png', 192), ('icon-512.png', 512),
                           ('icon-maskable-512.png', 512), ('apple-touch-icon.png', 180)):
            with open(os.path.join(base, name), 'rb') as fh:
                header = fh.read(24)
            assert header[:8] == b'\x89PNG\r\n\x1a\n'
            width, height = struct.unpack('>II', header[16:24])
            assert (width, height) == (size, size), name

    def test_service_worker(self, client):
        res = client.get('/sw.js')
        assert res.status_code == 200
        assert 'javascript' in res.headers['Content-Type']
        assert res.headers['Service-Worker-Allowed'] == '/'
        assert res.headers['Cache-Control'] == 'no-cache'
        body = res.get_data(as_text=True)
        assert 'const VERSION = ' in body and '{{' not in body
        assert "addEventListener('fetch'" in body

    def test_service_worker_never_caches_api_or_sockets(self, client):
        body = client.get('/sw.js').get_data(as_text=True)
        assert '/socket.io/' in body and '/api/' in body

    def test_service_worker_precache_list_is_servable(self, client):
        import re
        body = client.get('/sw.js').get_data(as_text=True)
        block = body[body.index('const SHELL_URLS'):body.index('];', body.index('const SHELL_URLS'))]
        paths = re.findall(r"'(/[^']*)'", block)
        assert paths
        for path in paths:
            res = client.get(path.split("' +")[0])
            assert res.status_code == 200, path

    def test_offline_page(self, client):
        res = client.get('/static/offline.html')
        assert res.status_code == 200 and b'<!DOCTYPE html>' in res.data

    def test_index_links_manifest_and_icons(self, client):
        html = client.get('/').get_data(as_text=True)
        assert 'rel="manifest" href="/manifest.webmanifest"' in html
        assert 'apple-touch-icon' in html
        assert 'apple-mobile-web-app-capable' in html or 'mobile-web-app-capable' in html

    def test_index_assets_are_versioned(self, client):
        import re
        html = client.get('/').get_data(as_text=True)
        assert re.search(r'/static/app\.js\?v=\d+', html)
        assert re.search(r'/static/style\.css\?v=\d+', html)

    def test_sw_version_matches_index_assets(self, client):
        import re
        html = client.get('/').get_data(as_text=True)
        version = re.search(r'/static/app\.js\?v=(\d+)', html).group(1)
        assert f'const VERSION = {version};' in client.get('/sw.js').get_data(as_text=True).replace('"', '')


class TestLeaderboardIntegrity:
    """A robotos játékok nem számítanak a ranglistába (különben könnyű lenne győzelmeket gyűjteni)."""

    def test_has_bots_column_exists(self):
        with auth._db() as conn:
            conn.execute('SELECT has_bots FROM saved_games LIMIT 1')

    def test_games_with_bots_are_excluded(self):
        a = _user('Aladár')
        players = [{'player_name': 'Aladár', 'user_id': a, 'final_score': 300, 'is_winner': True},
                   {'player_name': 'Rezső', 'user_id': None, 'final_score': 100, 'is_winner': False}]
        auth.finish_game('botgame', '{}', players, room_name='Bot', has_bots=True)
        assert auth.get_leaderboard()[0] == []

    def test_human_games_still_count_next_to_bot_games(self):
        a = _user('Aladár')
        players = [{'player_name': 'Aladár', 'user_id': a, 'final_score': 300, 'is_winner': True}]
        auth.finish_game('botgame', '{}', players, room_name='Bot', has_bots=True)
        auth.finish_game('humangame', '{}', players, room_name='Ember', has_bots=False)
        entries, _ = auth.get_leaderboard()
        assert len(entries) == 1 and entries[0]['games_played'] == 1

    def test_profile_stats_still_include_bot_games(self):
        a = _user('Aladár')
        players = [{'player_name': 'Aladár', 'user_id': a, 'final_score': 300, 'is_winner': True}]
        auth.finish_game('botgame', '{}', players, room_name='Bot', has_bots=True)
        assert auth.get_user_by_id(a)['games_played'] == 1

    def test_active_games_are_not_counted(self):
        a = _user('Aladár')
        auth.save_game('r1', 'R', '{}', False, [{'player_name': 'Aladár', 'user_id': a, 'score': 10}])
        assert auth.get_leaderboard()[0] == []

    def test_save_game_marks_bots_and_finish_updates_the_flag(self):
        auth.save_game('r1', 'R', '{}', False, [], has_bots=True)
        with auth._db() as conn:
            assert conn.execute("SELECT has_bots FROM saved_games WHERE room_id = 'r1'").fetchone()[0] == 1
        auth.finish_game('r1', '{}', [], has_bots=False)
        with auth._db() as conn:
            assert conn.execute("SELECT has_bots FROM saved_games WHERE room_id = 'r1'").fetchone()[0] == 0

    def test_migration_adds_the_column_to_old_databases(self, tmp_path, monkeypatch):
        import sqlite3
        db = str(tmp_path / 'old.db')
        conn = sqlite3.connect(db)
        conn.execute('CREATE TABLE saved_games (id INTEGER PRIMARY KEY AUTOINCREMENT, room_id TEXT NOT NULL, '
                     "room_name TEXT NOT NULL DEFAULT '', state_json TEXT NOT NULL, "
                     "status TEXT NOT NULL DEFAULT 'active', challenge_mode INTEGER NOT NULL DEFAULT 0, "
                     "owner_name TEXT NOT NULL DEFAULT '', owner_token TEXT, "
                     "created_at TEXT NOT NULL DEFAULT (datetime('now')), "
                     "updated_at TEXT NOT NULL DEFAULT (datetime('now')))")
        conn.execute("INSERT INTO saved_games (room_id, state_json) VALUES ('x', '{}')")
        conn.commit()
        conn.close()
        monkeypatch.setattr(auth, 'DB_PATH', db)
        auth.init_db()
        conn = sqlite3.connect(db)
        assert conn.execute('SELECT has_bots FROM saved_games').fetchone()[0] == 0
        conn.close()

    def test_server_marks_bot_games(self):
        """Végigjátszott, robotos játék a szerver mentésén át: nem kerül a ranglistára."""
        import server
        from helpers import registered_set_name_payload
        _, uid = auth.create_user('boter@example.com', 'Boter', 'password123')
        client = server.socketio.test_client(server.app)
        client.emit('set_name', registered_set_name_payload(uid))
        client.emit('create_room', {'name': 'R', 'ai_players': ['easy']})
        room = list(server.state.rooms.values())[-1]
        client.emit('start_game')
        room.game.finished = True
        room.game.winner = room.game.players[0]
        server._save_game_to_db(room.id)
        assert auth.get_user_by_id(uid)['games_played'] == 1
        assert auth.get_leaderboard()[0] == []
        with auth._db() as conn:
            assert conn.execute('SELECT has_bots FROM saved_games').fetchone()[0] == 1


class TestDictionaryInputLimits:
    def test_huge_query_is_truncated(self, client):
        res = client.get('/api/dictionary/check?q=' + 'szó ' * 5000)
        assert res.status_code == 200
        assert len(res.get_json()['results']) <= 8
