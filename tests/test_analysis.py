"""Játékelemzés: kezek rögzítése, a lépések elemzése, gyorsítótár és API."""
import json
from unittest.mock import patch

import pytest

import ai_player
import analysis
import auth
from ai_player import Vocabulary
from game import Game


@pytest.fixture
def small_vocab():
    """Kis szókincs: gyors keresés, ismert legjobb lépések."""
    previous = ai_player._vocabulary
    ai_player.set_vocabulary(Vocabulary(['ALMA', 'KÖR', 'ÖRÖK', 'KARÓ', 'AL', 'LÁ', 'RAK', 'KAR']))
    yield
    ai_player.set_vocabulary(previous)


def _game(rack=('A', 'L', 'M', 'A', 'K', 'Ö', 'R')):
    g = Game('t')
    g.add_player('p1', 'Anna')
    g.add_player('p2', 'Béla')
    g.start()
    g.players[0].hand = list(rack)
    return g


def _place(g, player_id, tiles):
    with patch('board.check_words', return_value=(True, [])):
        ok, msg, _score = g.place_tiles(player_id, tiles)
    assert ok, msg


def _details(g, index):
    return json.loads(g.move_log[index]['details_json'])


class TestRackRecording:
    def test_place_records_the_rack_before_the_move(self):
        g = _game()
        _place(g, 'p1', [(7, 7, 'A', False), (7, 8, 'L', False)])
        assert _details(g, 0)['rack'] == ['A', 'L', 'M', 'A', 'K', 'Ö', 'R']

    def test_pass_and_exchange_record_the_rack(self):
        g = _game()
        g.pass_turn('p1')
        assert _details(g, 0)['rack'] == ['A', 'L', 'M', 'A', 'K', 'Ö', 'R']
        hand = list(g.players[1].hand)
        g.exchange_tiles('p2', [0, 1])
        assert _details(g, 1)['rack'] == hand

    def test_blank_is_recorded_as_empty_string(self):
        g = _game(rack=('', 'L', 'M', 'A', 'K', 'Ö', 'R'))
        _place(g, 'p1', [(7, 7, 'A', True), (7, 8, 'L', False)])
        assert '' in _details(g, 0)['rack']

    def test_challenge_accept_records_the_rack_before_the_move(self):
        g = Game('t', challenge_mode=True)
        g.add_player('p1', 'Anna')
        g.add_player('p2', 'Béla')
        g.start()
        g.players[0].hand = ['A', 'L', 'M', 'A', 'K', 'Ö', 'R']
        _place(g, 'p1', [(7, 7, 'A', False), (7, 8, 'L', False)])
        g.accept_pending_by_player('p2')
        assert sorted(_details(g, 0)['rack']) == sorted(['A', 'L', 'M', 'A', 'K', 'Ö', 'R'])

    def test_rack_is_not_part_of_the_public_history(self):
        g = _game()
        _place(g, 'p1', [(7, 7, 'A', False), (7, 8, 'L', False)])
        assert all('rack' not in h for h in g.get_history())


class TestAnalyzeGame:
    def test_weak_move_shows_the_missed_points(self, small_vocab):
        g = _game()
        _place(g, 'p1', [(7, 7, 'A', False), (7, 8, 'L', False)])   # AL — kevés pont
        result = analysis.analyze_game(g.move_log, seconds=1.0)
        entry = result['moves'][0]
        assert entry['player'] == 'Anna' and entry['type'] == 'place'
        assert entry['words'] == ['AL']
        assert entry['best_score'] > entry['score'] > 0
        assert entry['lost'] == entry['best_score'] - entry['score']
        assert entry['best_tiles'] and entry['best_words']
        summary = result['players'][0]
        assert summary['player'] == 'Anna'
        assert summary['missed'] == entry['lost']
        assert summary['efficiency'] < 100
        assert summary['biggest_miss']['n'] == 1

    def test_best_move_is_marked_optimal(self, small_vocab):
        g = _game()
        best = ai_player.best_moves(g.board, list(g.players[0].hand), count=1)[0]
        _place(g, 'p1', [(r, c, l, b) for r, c, l, b in best.tiles])
        entry = analysis.analyze_game(g.move_log, seconds=1.0)['moves'][0]
        assert entry['lost'] == 0
        assert entry['score'] == best.score
        assert analysis.analyze_game(g.move_log, seconds=1.0)['players'][0]['efficiency'] == 100.0

    def test_played_move_better_than_the_engine_found_is_the_best(self, small_vocab):
        """A motor szókincse szűkebb: ha a játszott lépés jobb, az számít legjobbnak."""
        g = _game(rack=('Z', 'Z', 'Z', 'Z', 'Z', 'Z', 'Z'))
        move = g.move_log
        g.move_log = [{
            'move_number': 1, 'player_name': 'Anna', 'action_type': 'place',
            'details_json': json.dumps({'rack': ['A', 'L', 'M', 'A', 'K', 'Ö', 'R'], 'score': 500,
                                        'words': ['KARÓ'], 'tiles': [{'row': 7, 'col': 7, 'letter': 'K',
                                                                      'is_blank': False}]}),
            'board_snapshot_json': json.dumps(g.board.to_dict()),
        }]
        entry = analysis.analyze_game(g.move_log, seconds=1.0)['moves'][0]
        assert entry['lost'] == 0 and entry['best_score'] == 500
        assert entry['best_words'] == ['KARÓ']

    def test_pass_loses_the_best_score(self, small_vocab):
        g = _game()
        g.pass_turn('p1')
        entry = analysis.analyze_game(g.move_log, seconds=1.0)['moves'][0]
        assert entry['type'] == 'pass' and entry['score'] == 0
        assert entry['best_score'] > 0 and entry['lost'] == entry['best_score']

    def test_exchange_counts_as_a_turn(self, small_vocab):
        g = _game()
        g.exchange_tiles('p1', [0])
        entry = analysis.analyze_game(g.move_log, seconds=1.0)['moves'][0]
        assert entry['type'] == 'exchange' and entry['score'] == 0

    def test_board_before_each_move_comes_from_the_previous_snapshot(self, small_vocab):
        g = _game()
        _place(g, 'p1', [(7, 7, 'K', False), (7, 8, 'Ö', False), (7, 9, 'R', False)])  # KÖR
        g.players[1].hand = ['A', 'L', 'M', 'A', 'K', 'Ö', 'R']
        g.pass_turn('p2')
        result = analysis.analyze_game(g.move_log, seconds=1.0)
        second = result['moves'][1]
        assert second['player'] == 'Béla'
        # a második elemzésnél már áll a táblán a KÖR, ezért a legjobb lépés hozzá kapcsolódik
        assert second['best_tiles']
        assert all(not (t['row'] == 7 and t['col'] in (7, 8, 9)) for t in second['best_tiles'])

    def test_rejected_placements_are_not_turns(self, small_vocab):
        g = Game('t', challenge_mode=True)
        g.add_player('p1', 'Anna')
        g.add_player('p2', 'Béla')
        g.start()
        g.players[0].hand = ['A', 'L', 'M', 'A', 'K', 'Ö', 'R']
        _place(g, 'p1', [(7, 7, 'A', False), (7, 8, 'L', False)])
        g.reject_pending_by_player('p2')
        assert g.move_log[-1]['action_type'] == 'challenge_reject'
        assert analysis.analyze_game(g.move_log, seconds=1.0)['moves'] == []

    def test_progress_callback(self, small_vocab):
        g = _game()
        g.pass_turn('p1')
        g.pass_turn('p2')
        seen = []
        analysis.analyze_game(g.move_log, seconds=0.5, progress=lambda done, total: seen.append((done, total)))
        assert seen == [(1, 2), (2, 2)]

    def test_old_games_without_racks_are_not_analyzable(self):
        moves = [{'move_number': 1, 'player_name': 'A', 'action_type': 'pass',
                  'details_json': json.dumps({'score': 0}), 'board_snapshot_json': '[]'}]
        assert analysis.analyzable_turns(moves) == []


# ---------------------------------------------------------------- API

@pytest.fixture
def client():
    import server
    server.app.config['TESTING'] = True
    import routes
    routes._analysis_jobs.clear()
    server.rate_limiter._ip_history.clear()
    yield server.app.test_client()
    routes._analysis_jobs.clear()
    server.rate_limiter._ip_history.clear()


@pytest.fixture
def run_tasks_inline(monkeypatch):
    import server
    monkeypatch.setattr(server.socketio, 'start_background_task', lambda fn, *a, **k: fn(*a, **k))


def _saved_game(with_racks=True, status='finished'):
    _, alice = auth.create_user('alice@example.com', 'Alice', 'pass123')
    _, bob = auth.create_user('bob@example.com', 'Bob', 'pass123')
    _, stranger = auth.create_user('x@example.com', 'X', 'pass123')
    g = _game()
    _place(g, 'p1', [(7, 7, 'A', False), (7, 8, 'L', False)])
    g.pass_turn('p2')
    players = [{'player_name': 'Anna', 'user_id': alice, 'final_score': 5, 'is_winner': True},
               {'player_name': 'Béla', 'user_id': bob, 'final_score': 0, 'is_winner': False}]
    if status == 'finished':
        game_id = auth.finish_game('room-a', '{}', players)
    else:
        game_id = auth.save_game('room-a', 'R', '{}', False,
                                 [{'player_name': p['player_name'], 'user_id': p['user_id'], 'score': 0}
                                  for p in players], owner_name='Anna')
    for m in g.move_log:
        details = json.loads(m['details_json'])
        if not with_racks:
            details.pop('rack', None)
        auth.add_game_move(game_id, m['move_number'], m['player_name'], m['action_type'],
                           json.dumps(details), m['board_snapshot_json'])
    return game_id, alice, bob, stranger


def _login(client, uid):
    client.set_cookie('session_token', auth.create_session(uid), domain='localhost')


class TestAnalysisApi:
    def test_runs_in_the_background_then_serves_the_cached_result(self, client, small_vocab, run_tasks_inline):
        game_id, alice, *_ = _saved_game()
        _login(client, alice)
        first = client.get(f'/api/game/{game_id}/analysis').get_json()
        assert first['status'] == 'running' and first['total'] == 2
        second = client.get(f'/api/game/{game_id}/analysis').get_json()
        assert second['status'] == 'ready'
        assert [m['player'] for m in second['moves']] == ['Anna', 'Béla']
        assert {p['player'] for p in second['players']} == {'Anna', 'Béla'}
        assert auth.get_game_analysis(game_id)['version'] == analysis.ANALYSIS_VERSION

    def test_running_status_reports_progress_without_starting_a_second_job(self, client, small_vocab, monkeypatch):
        import server
        started = []
        monkeypatch.setattr(server.socketio, 'start_background_task', lambda fn, *a, **k: started.append(fn))
        game_id, alice, *_ = _saved_game()
        _login(client, alice)
        client.get(f'/api/game/{game_id}/analysis')
        again = client.get(f'/api/game/{game_id}/analysis').get_json()
        assert again['status'] == 'running' and again['done'] == 0
        assert len(started) == 1

    def test_other_participant_can_read_too(self, client, small_vocab, run_tasks_inline):
        game_id, _alice, bob, _ = _saved_game()
        _login(client, bob)
        client.get(f'/api/game/{game_id}/analysis')
        assert client.get(f'/api/game/{game_id}/analysis').get_json()['status'] == 'ready'

    def test_old_game_without_racks_is_unavailable(self, client, run_tasks_inline):
        game_id, alice, *_ = _saved_game(with_racks=False)
        _login(client, alice)
        assert client.get(f'/api/game/{game_id}/analysis').get_json()['status'] == 'unavailable'

    def test_outdated_cache_is_recomputed(self, client, small_vocab, run_tasks_inline):
        game_id, alice, *_ = _saved_game()
        auth.save_game_analysis(game_id, analysis.ANALYSIS_VERSION - 1, json.dumps({'moves': [], 'players': []}))
        _login(client, alice)
        assert client.get(f'/api/game/{game_id}/analysis').get_json()['status'] == 'running'
        ready = client.get(f'/api/game/{game_id}/analysis').get_json()
        assert ready['status'] == 'ready' and len(ready['moves']) == 2

    def test_failure_is_reported_and_retried_later(self, client, small_vocab, run_tasks_inline, monkeypatch):
        import routes
        game_id, alice, *_ = _saved_game()
        _login(client, alice)

        real = analysis.analyze_game
        calls = []

        def flaky(*args, **kwargs):
            calls.append(1)
            if len(calls) == 1:
                raise RuntimeError('keresési hiba')
            return real(*args, **kwargs)
        monkeypatch.setattr(analysis, 'analyze_game', flaky)

        client.get(f'/api/game/{game_id}/analysis')
        assert client.get(f'/api/game/{game_id}/analysis').get_json()['status'] == 'error'
        assert len(calls) == 1   # a várakozási idő alatt nem próbálkozik újra
        # a várakozási idő letelte után új próbálkozás indul, és most sikerül
        routes._analysis_jobs[game_id]['at'] -= routes._ANALYSIS_RETRY_AFTER + 1
        client.get(f'/api/game/{game_id}/analysis')
        assert client.get(f'/api/game/{game_id}/analysis').get_json()['status'] == 'ready'

    def test_access_control(self, client, small_vocab, run_tasks_inline):
        game_id, alice, _bob, stranger = _saved_game()
        assert client.get(f'/api/game/{game_id}/analysis').status_code == 401
        _login(client, stranger)
        assert client.get(f'/api/game/{game_id}/analysis').status_code == 403
        assert client.get('/api/game/9999/analysis').status_code == 404

    def test_unfinished_game_cannot_be_analyzed(self, client, run_tasks_inline):
        game_id, alice, *_ = _saved_game(status='active')
        _login(client, alice)
        assert client.get(f'/api/game/{game_id}/analysis').status_code == 400

    def test_rate_limited(self, client, small_vocab, run_tasks_inline):
        game_id, alice, *_ = _saved_game()
        _login(client, alice)
        codes = [client.get(f'/api/game/{game_id}/analysis').status_code for _ in range(31)]
        assert codes[-1] == 429
