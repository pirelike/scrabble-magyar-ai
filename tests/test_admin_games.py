"""Admin panel: játékarchívum, levelezős játékok, értékszám (újraszámolás, gyanús minták) és napi feladvány."""
import json
import time
from types import SimpleNamespace

import pytest

import admin
import admin_games
import async_games
import auth
import daily
import push_service
import server
from board import Board
from helpers import (
    ADMIN_EMAIL, finished_game, guest_client, live_room, make_user, registered_client,
)

REASON = 'Teszt indoklás'


@pytest.fixture(autouse=True)
def no_background(monkeypatch):
    started = []
    monkeypatch.setattr(server.socketio, 'start_background_task', lambda fn, *a, **k: started.append((fn, a, k)))
    return started


def _audit(action):
    return admin.query_audit(action=action, limit=50)['items']


@pytest.fixture
def duo():
    a, _ = make_user('anna@example.com', 'Anna')
    b, _ = make_user('bela@example.com', 'Béla')
    return a, b


def _rating(uid):
    row = auth.get_user_by_id(uid)
    return row['rating'], row['rated_games']


class TestArchive:
    def test_lists_games_with_players_and_winners(self, api, duo):
        a, b = duo
        game_id = finished_game('r1', [(a, 'Anna', 120), (b, 'Béla', 80)], moves=4)
        data = api.get('/api/admin/games').get_json()
        row = data['items'][0]
        assert row['id'] == game_id and row['status'] == 'finished' and row['moves'] == 4
        assert row['winners'] == ['Anna'] and [p['name'] for p in row['players']] == ['Anna', 'Béla']
        assert row['bots'] is False and row['async'] is False and row['shared'] is False

    def test_filters(self, api, duo):
        a, b = duo
        g1 = finished_game('r1', [(a, 'Anna', 120), (b, 'Béla', 80)])
        g2 = finished_game('r2', [(a, 'Anna', 50), (None, 'Robi', 80)], has_bots=True)
        auth.abandon_game('nincs')
        ids = lambda q: [g['id'] for g in api.get('/api/admin/games?' + q).get_json()['items']]  # noqa: E731
        assert ids('bots=1') == [g2] and ids('bots=0') == [g1]
        assert sorted(ids(f'user={b}')) == [g1] and sorted(ids('user=Anna')) == [g1, g2]
        assert ids('status=abandoned') == [] and ids('status=finished&q=r2') == [g2]
        assert ids(f'q={g1}') == [g1] and ids('since=2999-01-01') == []
        assert api.get('/api/admin/games?status=rossz').status_code == 400

    def test_detail_shows_hands_and_is_logged(self, api, duo):
        a, b = duo
        game_id = finished_game('r1', [(a, 'Anna', 120), (b, 'Béla', 80)], moves=2)
        data = api.get(f'/api/admin/games/{game_id}').get_json()['game']
        assert [p['rating_change'] for p in data['players']] == [16, -16]
        assert data['moves'][0]['rack'] == ['A'] and data['moves'][0]['type'] == 'pass'
        assert _audit('view.game')[0]['target_id'] == str(game_id)
        assert api.get('/api/admin/games/9999').status_code == 404

    def test_moves_with_snapshots(self, api, duo):
        a, b = duo
        game_id = finished_game('r1', [(a, 'Anna', 120), (b, 'Béla', 80)], moves=2)
        moves = api.get(f'/api/admin/games/{game_id}/moves').get_json()['moves']
        assert [m['n'] for m in moves] == [1, 2] and moves[0]['board'] == {}

    def test_csv(self, api, duo):
        a, b = duo
        finished_game('r1', [(a, 'Anna', 120), (b, 'Béla', 80)])
        text = api.get('/api/admin/games?format=csv').get_data(as_text=True)
        assert 'Anna (120); Béla (80)' in text


class TestVoid:
    def test_void_recomputes_the_ratings_and_unvoid_restores_them(self, api, duo):
        a, b = duo
        finished_game('r1', [(a, 'Anna', 120), (b, 'Béla', 80)])
        after_one = (_rating(a), _rating(b))
        g2 = finished_game('r2', [(a, 'Anna', 120), (b, 'Béla', 80)])
        after_two = (_rating(a), _rating(b))
        assert after_two[0][0] > after_one[0][0]
        response = api.post(f'/api/admin/games/{g2}/void', {'reason': REASON})
        assert response.status_code == 200 and response.get_json()['rating_changes'] == 2
        assert (_rating(a), _rating(b)) == after_one
        assert auth.get_game_by_id(g2)['status'] == 'voided'
        assert auth.get_user_by_id(a)['games_played'] == 1
        assert [e['games_played'] for e in auth.get_leaderboard('wins')[0] if e['user_id'] == a] == [1]
        assert api.post(f'/api/admin/games/{g2}/unvoid', {'reason': REASON}).status_code == 200
        assert (_rating(a), _rating(b)) == after_two and auth.get_user_by_id(a)['games_played'] == 2
        assert [r['details']['before'] for r in _audit('game.void')] == [{'status': 'finished'}]

    def test_badges_are_only_revoked_on_request(self, api, duo):
        a, b = duo
        game_id = finished_game('r1', [(a, 'Anna', 120), (b, 'Béla', 80)])
        auth.grant_achievements(a, {'bingo'}, game_id)
        api.post(f'/api/admin/games/{game_id}/void', {'reason': REASON})
        assert [x['badge'] for x in auth.get_user_achievements(a)] == ['bingo']
        api.post(f'/api/admin/games/{game_id}/unvoid', {'reason': REASON})
        api.post(f'/api/admin/games/{game_id}/void', {'reason': REASON, 'revoke_badges': True})
        assert auth.get_user_achievements(a) == []

    def test_only_finished_games_and_a_reason_is_needed(self, api, duo):
        a, b = duo
        game_id = finished_game('r1', [(a, 'Anna', 120), (b, 'Béla', 80)])
        assert api.post(f'/api/admin/games/{game_id}/void', {}).status_code == 400
        assert api.post(f'/api/admin/games/{game_id}/unvoid', {'reason': REASON}).status_code == 409
        with auth.transaction() as conn:
            conn.execute("UPDATE saved_games SET status = 'abandoned' WHERE id = ?", (game_id,))
        assert api.post(f'/api/admin/games/{game_id}/void', {'reason': REASON}).status_code == 409


class TestGameMaintenance:
    def test_unshare(self, api, duo):
        a, b = duo
        game_id = finished_game('r1', [(a, 'Anna', 120), (b, 'Béla', 80)])
        assert api.post(f'/api/admin/games/{game_id}/unshare', {'reason': REASON}).status_code == 409
        token = auth.get_or_create_share_token(game_id)
        assert api.get(f'/api/admin/games/{game_id}').get_json()['game']['share_token'] == token
        assert api.post(f'/api/admin/games/{game_id}/unshare', {'reason': REASON}).status_code == 200
        assert auth.get_game_by_share_token(token) is None

    def test_status_toggle(self, api, duo):
        a, b = duo
        game_id = auth.save_game('x1', 'Mentés', '{}', False, [{'player_name': 'Anna', 'user_id': a, 'score': 0}])
        assert api.post(f'/api/admin/games/{game_id}/status', {'status': 'abandoned', 'reason': REASON}
                        ).status_code == 200
        assert auth.get_game_by_id(game_id)['status'] == 'abandoned'
        assert api.post(f'/api/admin/games/{game_id}/status', {'status': 'abandoned', 'reason': REASON}
                        ).status_code == 409
        assert api.post(f'/api/admin/games/{game_id}/status', {'status': 'active', 'reason': REASON}).status_code == 200
        assert api.post(f'/api/admin/games/{game_id}/status', {'status': 'voided', 'reason': REASON}).status_code == 400

    def test_a_running_game_cannot_be_changed(self, api):
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela])
        assert room.db_game_id
        assert api.post(f'/api/admin/games/{room.db_game_id}/status', {'status': 'abandoned', 'reason': REASON}
                        ).status_code == 409
        api.sudo()
        assert api.delete(f'/api/admin/games/{room.db_game_id}', {'reason': REASON}).status_code == 409

    def test_export(self, api, duo):
        a, b = duo
        game_id = finished_game('r1', [(a, 'Anna', 120), (b, 'Béla', 80)], moves=2)
        response = api.get(f'/api/admin/games/{game_id}/export')
        data = json.loads(response.get_data(as_text=True))
        assert data['game']['id'] == game_id and len(data['moves']) == 2 and len(data['players']) == 2
        assert _audit('view.game_export')

    def test_reanalyze_clears_the_cache_and_starts_a_job(self, api, duo, no_background):
        import routes
        a, b = duo
        game_id = finished_game('r1', [(a, 'Anna', 120), (b, 'Béla', 80)], moves=3)
        auth.save_game_analysis(game_id, 1, '{}')
        response = api.post(f'/api/admin/games/{game_id}/reanalyze', {'reason': REASON})
        assert response.status_code == 200 and response.get_json()['total'] == 3
        assert auth.get_game_analysis(game_id) is None
        assert routes._analysis_jobs[game_id] == {'done': 0, 'total': 3}
        assert no_background and no_background[-1][0] == routes._run_analysis
        routes._analysis_jobs.clear()

    def test_reanalyze_needs_recorded_hands(self, api, duo):
        a, b = duo
        game_id = finished_game('r1', [(a, 'Anna', 120), (b, 'Béla', 80)])
        assert api.post(f'/api/admin/games/{game_id}/reanalyze', {'reason': REASON}).status_code == 409


class TestDeletion:
    def test_a_finished_game_needs_the_extra_confirmation(self, api, duo):
        a, b = duo
        game_id = finished_game('r1', [(a, 'Anna', 120), (b, 'Béla', 80)])
        assert api.delete(f'/api/admin/games/{game_id}', {'reason': REASON}).status_code == 401
        api.sudo()
        response = api.delete(f'/api/admin/games/{game_id}', {'reason': REASON})
        assert response.status_code == 409 and response.get_json()['needs_confirm'] is True
        assert auth.get_game_by_id(game_id) is not None
        response = api.delete(f'/api/admin/games/{game_id}', {'reason': REASON, 'confirm_finished': True})
        assert response.status_code == 200 and auth.get_game_by_id(game_id) is None
        assert _rating(a) == (1200, 0) and auth.get_user_by_id(a)['games_played'] == 0
        assert _audit('game.delete')[0]['details']['before']['name'] == 'Játék r1'

    def test_other_games_are_deleted_without_the_confirmation(self, api, duo):
        a, b = duo
        game_id = auth.save_game('x1', 'Mentés', '{}', False, [{'player_name': 'Anna', 'user_id': a, 'score': 0}])
        auth.abandon_game_by_id(game_id)
        api.sudo()
        assert api.delete(f'/api/admin/games/{game_id}', {'reason': REASON}).status_code == 200
        assert auth.get_game_by_id(game_id) is None

    def test_bulk_cleanup_of_old_abandoned_saves(self, api, duo):
        a, b = duo
        old = auth.save_game('x1', 'Régi', '{}', False)
        new = auth.save_game('x2', 'Új', '{}', False)
        keep = auth.save_game('x3', 'Aktív', '{}', False)
        auth.abandon_game_by_id(old)
        auth.abandon_game_by_id(new)
        with auth.transaction() as conn:
            conn.execute("UPDATE saved_games SET updated_at = '2020-01-01 00:00:00' WHERE id = ?", (old,))
        preview = api.get('/api/admin/games/cleanup?days=30').get_json()
        assert preview['total'] == 1 and [g['id'] for g in preview['items']] == [old]
        assert api.post('/api/admin/games/cleanup', {'days': 30, 'reason': REASON}).status_code == 401
        api.sudo()
        assert api.post('/api/admin/games/cleanup', {'days': 30, 'reason': REASON}).get_json()['deleted'] == 1
        assert auth.get_game_by_id(old) is None and auth.get_game_by_id(new) and auth.get_game_by_id(keep)


class TestRatingRecompute:
    def _games(self, a, b, c):
        finished_game('r1', [(a, 'Anna', 120), (b, 'Béla', 80), (c, 'Cili', 60)])
        finished_game('r2', [(a, 'Anna', 70), (b, 'Béla', 90)])
        finished_game('r3', [(c, 'Cili', 100), (b, 'Béla', 100)])
        finished_game('r4', [(a, 'Anna', 120), (b, 'Béla', 0)], resigned=('Béla',))
        finished_game('r5', [(a, 'Anna', 90), (None, 'Vendég', 10)])           # vendég: nincs értékelés
        finished_game('r6', [(a, 'Anna', 90), (c, 'Cili', 10)], has_bots=True)  # robotos: nincs értékelés

    def test_the_recomputation_matches_the_step_by_step_result(self, api, duo):
        a, b = duo
        c, _ = make_user('cili@example.com', 'Cili')
        self._games(a, b, c)
        expected = {u: _rating(u) for u in (a, b, c)}
        rows = auth.get_game_rating_changes(1)
        with auth.transaction() as conn:
            conn.execute('UPDATE users SET rating = 1500, rated_games = 99')
            conn.execute('UPDATE game_players SET rating_before = 1, rating_after = 2')
        dry = api.post('/api/admin/ratings/recompute', {}).get_json()
        assert dry['dry_run'] is True and dry['changed'] == 4                   # az admin fiók is (1500 → 1200)
        assert _rating(a) == (1500, 99)                                         # az előnézet nem ír
        assert {i['user_id'] for i in dry['items']} == {a, b, c, api.user_id}
        assert api.post('/api/admin/ratings/recompute', {'dry_run': False, 'reason': REASON}).status_code == 401
        api.sudo()
        applied = api.post('/api/admin/ratings/recompute', {'dry_run': False, 'reason': REASON}).get_json()
        assert applied['changed'] == 4
        assert {u: _rating(u) for u in (a, b, c)} == expected
        assert auth.get_game_rating_changes(1) == rows                          # a játékonkénti előzmény is
        assert _audit('ratings.recompute')[0]['details']['changed'] == 4
        again = api.post('/api/admin/ratings/recompute', {}).get_json()
        assert again['changed'] == 0

    def test_manual_adjustments_are_part_of_the_timeline(self, api, duo):
        a, b = duo
        g1 = finished_game('r1', [(a, 'Anna', 120), (b, 'Béla', 80)])
        api.sudo()
        api.post(f'/api/admin/users/{a}/rating', {'rating': 1500, 'reason': REASON})
        g2 = finished_game('r2', [(a, 'Anna', 120), (b, 'Béla', 80)])
        expected = _rating(a)
        with auth.transaction() as conn:      # az időrend a másodperc-pontos időbélyegekből áll össze
            conn.execute("UPDATE saved_games SET updated_at = '2026-01-01 10:00:00' WHERE id = ?", (g1,))
            conn.execute("UPDATE rating_adjustments SET created_at = '2026-01-02 10:00:00'")
            conn.execute("UPDATE saved_games SET updated_at = '2026-01-03 10:00:00' WHERE id = ?", (g2,))
            conn.execute('UPDATE users SET rating = 1')
        api.post('/api/admin/ratings/recompute', {'dry_run': False, 'reason': REASON})
        assert _rating(a) == expected

    def test_recompute_without_games_resets_to_the_start(self, api, duo):
        a, b = duo
        with auth.transaction() as conn:
            conn.execute('UPDATE users SET rating = 1400, rated_games = 5')
        api.sudo()
        api.post('/api/admin/ratings/recompute', {'dry_run': False, 'reason': REASON})
        assert _rating(a) == (1200, 0)

    def test_leaderboard_without_the_minimum(self, api, duo):
        a, b = duo
        finished_game('r1', [(a, 'Anna', 120), (b, 'Béla', 80)])
        assert auth.get_leaderboard('rating')[0] == []                         # a nyilvános: legalább 3 játék
        data = api.get('/api/admin/ratings?metric=rating').get_json()
        assert [e['display_name'] for e in data['entries']] == ['Anna', 'Béla']
        assert api.get('/api/admin/ratings?metric=rossz').status_code == 400
        assert 'rank,user_id' in api.get('/api/admin/ratings?format=csv').get_data(as_text=True)


class TestSuspiciousPatterns:
    def test_one_sided_pair(self, api, duo):
        a, b = duo
        for i in range(6):
            finished_game(f'r{i}', [(a, 'Anna', 100), (b, 'Béla', 50)])
        data = api.get('/api/admin/ratings/suspicious').get_json()
        pair = data['one_sided_pairs'][0]
        assert pair['games'] == 6 and {u['name']: u['wins'] for u in pair['users']} == {'Anna': 6, 'Béla': 0}
        flagged = [g['id'] for g in api.get('/api/admin/games?suspicious=1').get_json()['items']]
        assert len(flagged) == 6

    def test_a_balanced_pair_is_not_flagged(self, api, duo):
        a, b = duo
        for i in range(6):
            finished_game(f'r{i}', [(a, 'Anna', 100 if i % 2 else 50), (b, 'Béla', 50 if i % 2 else 100)])
        assert api.get('/api/admin/ratings/suspicious').get_json()['one_sided_pairs'] == []

    def test_shared_ip(self, api, duo):
        a, b = duo
        auth.record_login_event('anna@example.com', True, '198.51.100.9', 'UA', a)
        auth.record_login_event('bela@example.com', True, '198.51.100.9', 'UA', b)
        game_id = finished_game('r1', [(a, 'Anna', 100), (b, 'Béla', 50)])
        entry = api.get('/api/admin/ratings/suspicious').get_json()['same_ip'][0]
        assert entry['ips'] == ['198.51.100.9'] and entry['game_ids'] == [game_id]

    def test_fast_passing_games(self, api, duo):
        a, b = duo
        game_id = finished_game('r1', [(a, 'Anna', 0), (b, 'Béla', 0)], moves=8)
        long_game = finished_game('r2', [(a, 'Anna', 5), (b, 'Béla', 3)], moves=40)
        flagged = api.get('/api/admin/ratings/suspicious').get_json()['fast_pass_games']
        assert [g['game_id'] for g in flagged] == [game_id] and long_game not in [g['game_id'] for g in flagged]

    def test_consistently_best_moves(self, api, duo):
        a, b = duo
        for i in range(3):
            game_id = finished_game(f'r{i}', [(a, 'Anna', 100), (b, 'Béla', 50)])
            auth.save_game_analysis(game_id, 6, json.dumps({'players': [
                {'player': 'Anna', 'turns': 12, 'efficiency': 99.0}, {'player': 'Béla', 'turns': 12, 'efficiency': 70.0}]}))
        entries = api.get('/api/admin/ratings/suspicious').get_json()['high_efficiency']
        assert [(e['name'], e['games'], e['average']) for e in entries] == [('Anna', 3, 99.0)]


# ---------------------------------------------------------------- levelezős

@pytest.fixture
def async_game(monkeypatch):
    """Két barát levelezős játéka (a létrehozó kezd). Visszatér: (a, a_id, b, b_id, room)."""
    monkeypatch.setattr(push_service, '_spawn', None)
    monkeypatch.setattr(async_games, 'random', SimpleNamespace(randrange=lambda n: 0))
    a_id, _ = make_user('anna@example.com', 'Anna')
    b_id, _ = make_user('bela@example.com', 'Béla')
    auth.send_friend_request(a_id, b_id)
    auth.accept_friend_request(b_id, a_id)
    anna, bela = registered_client(a_id, 'Anna'), registered_client(b_id, 'Béla')
    anna.emit('create_async_game', {'name': 'Esti játék', 'friend_ids': [b_id], 'turn_hours': 24})
    anna.get_received()
    bela.get_received()
    room = next(r for r in server.state.rooms.values() if r.is_async)
    return anna, a_id, bela, b_id, room


class TestAsync:
    def test_list(self, api, async_game):
        anna, a_id, bela, b_id, room = async_game
        data = api.get('/api/admin/async').get_json()
        row = data['items'][0]
        assert row['id'] == room.db_game_id and row['loaded'] is True and row['current'] == 'Anna'
        assert row['remaining'] > 23 * 3600 and [p['name'] for p in row['players']] == ['Anna', 'Béla']
        assert data['sweeper']['name'] == 'async_sweeper'

    def test_list_includes_games_that_are_not_loaded(self, api, async_game):
        anna, a_id, bela, b_id, room = async_game
        game_id = room.db_game_id
        anna.disconnect()
        bela.disconnect()
        server._cleanup_room(room.id)
        row = api.get('/api/admin/async').get_json()['items'][0]
        assert row['id'] == game_id and row['loaded'] is False and row['current'] == 'Anna'
        assert api.post(f'/api/admin/async/{game_id}/load', {}).status_code == 200
        assert any(r.is_async for r in server.state.rooms.values())

    def test_extend_the_deadline(self, api, async_game):
        anna, a_id, bela, b_id, room = async_game
        before = room.game.turn_deadline
        assert api.post(f'/api/admin/async/{room.db_game_id}/extend', {'hours': 24, 'reason': REASON}
                        ).status_code == 200
        assert room.game.turn_deadline == pytest.approx(before + 24 * 3600, abs=5)
        assert api.post(f'/api/admin/async/{room.db_game_id}/extend', {'hours': 5, 'reason': REASON}
                        ).status_code == 400
        row = _audit('async.extend')[0]['details']
        assert row['hours'] == 24 and row['after']['deadline'] > row['before']['deadline']

    def test_an_expired_deadline_is_extended_from_now(self, api, async_game):
        anna, a_id, bela, b_id, room = async_game
        room.game.turn_deadline = time.time() - 7200
        api.post(f'/api/admin/async/{room.db_game_id}/extend', {'hours': 6, 'reason': REASON})
        assert room.game.turn_deadline == pytest.approx(time.time() + 6 * 3600, abs=10)

    def test_expire_now(self, api, async_game):
        anna, a_id, bela, b_id, room = async_game
        response = api.post(f'/api/admin/async/{room.db_game_id}/expire', {'reason': REASON})
        assert response.get_json()['player'] == 'Anna'
        assert room.game.current_player().name == 'Béla'
        assert next(p for p in room.game.players if p.name == 'Anna').timeouts == 1

    def test_resign_on_behalf_of_a_player(self, api, async_game):
        anna, a_id, bela, b_id, room = async_game
        assert api.post(f'/api/admin/async/{room.db_game_id}/resign', {'player': 'Senki', 'reason': REASON}
                        ).status_code == 404
        assert api.post(f'/api/admin/async/{room.db_game_id}/resign', {'player': 'Anna', 'reason': REASON}
                        ).status_code == 200
        assert [p.name for p in room.game.winners] == ['Béla'] if room.id in server.state.rooms else True
        assert auth.get_game_by_id(room.db_game_id)['status'] == 'finished'

    def test_remind_pushes_to_the_current_player(self, api, async_game, monkeypatch):
        anna, a_id, bela, b_id, room = async_game
        sent = []
        monkeypatch.setattr(push_service, 'notify_background',
                            lambda user_id, kind, **p: sent.append((user_id, kind, p['room'])) or True)
        assert api.post(f'/api/admin/async/{room.db_game_id}/remind', {}).get_json()['sent'] is True
        assert sent == [(a_id, 'reminder', 'Esti játék')]
        assert _audit('async.remind')

    def test_void_removes_the_room(self, api, async_game):
        anna, a_id, bela, b_id, room = async_game
        game_id = room.db_game_id
        assert api.post(f'/api/admin/async/{game_id}/void', {'reason': REASON}).status_code == 200
        assert auth.get_game_by_id(game_id)['status'] == 'voided' and room.id not in server.state.rooms

    def test_unload_only_when_nobody_is_inside(self, api, async_game):
        anna, a_id, bela, b_id, room = async_game
        game_id = room.db_game_id
        assert api.post(f'/api/admin/async/{game_id}/unload', {'reason': REASON}).status_code == 409
        anna.emit('leave_room')
        bela.emit('leave_room')
        assert api.post(f'/api/admin/async/{game_id}/unload', {'reason': REASON}).status_code == 200
        assert room.id not in server.state.rooms and auth.get_game_by_id(game_id)['status'] == 'active'

    def test_manual_sweep(self, api, async_game):
        anna, a_id, bela, b_id, room = async_game
        room.game.turn_deadline = time.time() - 10
        assert api.post('/api/admin/async/sweep', {}).get_json()['expired'] == 1
        assert room.game.current_player().name == 'Béla' and _audit('async.sweep')

    def test_unknown_game(self, api):
        assert api.post('/api/admin/async/999/expire', {'reason': REASON}).status_code == 404


# ---------------------------------------------------------------- napi feladvány

@pytest.fixture
def puzzle():
    date = daily.today_str()
    auth.save_daily_puzzle(date, Board().to_dict(), ['A'] * 7, 60, {'tiles': [], 'words': ['ALMA'], 'score': 60})
    return date


class TestDaily:
    def test_day_overview(self, api, puzzle):
        a, _ = make_user('a@example.com', 'Anna')
        b, _ = make_user('b@example.com', 'Béla')
        c, _ = make_user('c@example.com', 'Cili')
        auth.record_daily_score(puzzle, a, 60)
        auth.record_daily_score(puzzle, b, 30)
        auth.record_daily_score(puzzle, b, 33)
        auth.mark_daily_revealed(puzzle, c)
        data = api.get('/api/admin/daily').get_json()
        assert data['participants'] == 2 and data['revealed'] == 1 and data['puzzle']['best_score'] == 60
        assert data['attempts']['1'] == 1 and data['attempts']['2'] == 1
        assert data['scores']['perfect'] == 1 and data['scores']['half'] == 1
        assert [e['display_name'] for e in data['leaderboard'][:2]] == ['Anna', 'Béla']
        assert api.get('/api/admin/daily?date=hibás').status_code == 400
        assert api.get('/api/admin/daily?date=2999-01-01').status_code == 400

    def test_archive(self, api, puzzle):
        a, _ = make_user('a@example.com', 'Anna')
        auth.record_daily_score(puzzle, a, 45)
        row = api.get('/api/admin/daily/archive').get_json()['items'][0]
        assert row['date'] == puzzle and row['participants'] == 1 and row['avg_score'] == 45

    def test_preview_does_not_save(self, api, monkeypatch):
        monkeypatch.setattr(daily, 'generate_puzzle', lambda date, yield_fn=None: {
            'board': Board().to_dict(), 'rack': list('ALMAFAK'), 'best_score': 77, 'best': {'words': ['ALMA']}})
        data = api.get('/api/admin/daily/preview?date=2999-01-01').get_json()
        assert data['best_score'] == 77 and data['rack'] == list('ALMAFAK')
        assert auth.get_daily_puzzle('2999-01-01') is None

    def test_regeneration_before_anyone_played(self, api, puzzle, monkeypatch):
        monkeypatch.setattr(daily, 'generate_puzzle', lambda date, yield_fn=None: {
            'board': Board().to_dict(), 'rack': list('ALMAFAK'), 'best_score': 77, 'best': {'words': ['ALMA']}})
        response = api.post(f'/api/admin/daily/{puzzle}/regenerate', {'reason': REASON})
        assert response.status_code == 200 and auth.get_daily_puzzle(puzzle)['best_score'] == 77
        row = _audit('daily.regenerate')[0]['details']
        assert row['before'] == {'best_score': 60} and row['after'] == {'best_score': 77}

    def test_regeneration_after_attempts_needs_sudo(self, api, puzzle, monkeypatch):
        a, _ = make_user('a@example.com', 'Anna')
        auth.record_daily_score(puzzle, a, 45)
        monkeypatch.setattr(daily, 'generate_puzzle', lambda date, yield_fn=None: {
            'board': Board().to_dict(), 'rack': list('ALMAFAK'), 'best_score': 77, 'best': {'words': ['ALMA']}})
        response = api.post(f'/api/admin/daily/{puzzle}/regenerate', {'reason': REASON})
        assert response.status_code == 401 and response.get_json()['attempts'] == 1
        assert auth.get_daily_puzzle(puzzle)['best_score'] == 60
        api.sudo()
        assert api.post(f'/api/admin/daily/{puzzle}/regenerate', {'reason': REASON, 'reset_scores': True}
                        ).status_code == 200
        assert auth.get_daily_leaderboard(puzzle)[0] == []

    def test_delete_a_score_and_reset_revealed(self, api, puzzle):
        a, _ = make_user('a@example.com', 'Anna')
        b, _ = make_user('b@example.com', 'Béla')
        auth.record_daily_score(puzzle, a, 45)
        auth.mark_daily_revealed(puzzle, b)
        assert api.post(f'/api/admin/daily/{puzzle}/scores/{a}/reset-revealed', {'reason': REASON}).status_code == 404
        assert api.post(f'/api/admin/daily/{puzzle}/scores/{b}/reset-revealed', {'reason': REASON}).status_code == 200
        assert auth.get_daily_entry(puzzle, b)['revealed'] is False
        assert api.delete(f'/api/admin/daily/{puzzle}/scores/{a}', {'reason': REASON}).status_code == 200
        assert auth.get_daily_entry(puzzle, a) is None
        assert api.delete(f'/api/admin/daily/{puzzle}/scores/{a}', {'reason': REASON}).status_code == 404
