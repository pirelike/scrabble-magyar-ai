"""Napi feladvány: előállítás, játék, ranglista, socket és HTTP."""
import pytest

import auth
import daily
from board import Board
from game import Game
from tiles import TILE_DISTRIBUTION

TOTAL_TILES = sum(count for _, _, count in TILE_DISTRIBUTION)
FIXED_DATE = '2026-01-05'


@pytest.fixture(scope='module')
def generated():
    """Egyszer előállított feladvány (a tesztek ezt írják be az aznapi adatbázisba)."""
    import ai_player
    import dictionary
    dictionary.warm_up()
    ai_player.get_vocabulary()
    return daily.generate_puzzle(FIXED_DATE)


@pytest.fixture
def puzzle(generated):
    """A mai feladvány az adatbázisban."""
    today = daily.today_str()
    auth.save_daily_puzzle(today, generated['board'], generated['rack'],
                           generated['best_score'], generated['best'])
    return auth.get_daily_puzzle(today)


class TestDates:
    def test_today_format(self):
        assert daily.is_valid_date(daily.today_str())

    def test_previous_date(self):
        assert daily.previous_date('2026-03-01') == '2026-02-28'
        assert daily.previous_date('2026-01-01') == '2025-12-31'

    @pytest.mark.parametrize('text,valid', [
        ('2026-01-05', True), ('2026-13-01', False), ('2026-1-5', False), ('nem dátum', False),
        ('', False), (None, False), ('2026-02-30', False),
    ])
    def test_is_valid_date(self, text, valid):
        assert daily.is_valid_date(text) is valid


class TestGeneration:
    def test_puzzle_shape(self, generated):
        assert len(generated['rack']) == 7
        assert daily.MIN_BEST_SCORE <= generated['best_score'] <= daily.MAX_BEST_SCORE
        board = Board.from_dict(generated['board'])
        assert not board.is_empty
        best = generated['best']
        assert best['score'] == generated['best_score'] and best['words'] and best['tiles']

    def test_is_deterministic_for_a_date(self, generated):
        assert daily.generate_puzzle(FIXED_DATE) == generated

    def test_different_dates_give_different_puzzles(self, generated):
        other = daily.generate_puzzle('2026-01-06')
        assert (other['rack'], other['board']) != (generated['rack'], generated['board'])

    def test_best_move_is_playable_and_scores_as_stated(self, generated):
        game = daily.build_game('r', 'sid', 'Anna', {**generated, 'date': FIXED_DATE})
        tiles = [(t['row'], t['col'], t['letter'], t['is_blank']) for t in generated['best']['tiles']]
        ok, msg, score = game.place_tiles('sid', tiles)
        assert ok, msg
        assert score == generated['best_score']

    def test_ensure_puzzle_saves_once_and_reuses(self, generated, monkeypatch):
        calls = []
        monkeypatch.setattr(daily, 'generate_puzzle', lambda d, yield_fn=None: calls.append(d) or generated)
        first = daily.ensure_puzzle('2026-02-02')
        second = daily.ensure_puzzle('2026-02-02')
        assert calls == ['2026-02-02']
        assert first == second and first['date'] == '2026-02-02'
        assert first['best_score'] == generated['best_score']

    def test_save_does_not_overwrite_an_existing_puzzle(self, generated):
        auth.save_daily_puzzle('2026-02-03', generated['board'], generated['rack'], 40, {'words': ['A']})
        auth.save_daily_puzzle('2026-02-03', generated['board'], ['A'] * 7, 99, {'words': ['B']})
        stored = auth.get_daily_puzzle('2026-02-03')
        assert stored['best_score'] == 40 and stored['rack'] == generated['rack']


class TestGame:
    def test_state_of_the_puzzle_game(self, puzzle):
        game = daily.build_game('r', 'sid', 'Anna', puzzle)
        assert game.started and not game.finished
        assert game.players[0].hand == puzzle['rack']
        assert game.board.to_dict() == puzzle['board']
        # a zsákban a táblán és a kézben nem látható zsetonok vannak
        on_board = sum(1 for row in game.board.cells for cell in row if cell)
        assert game.bag.remaining() == TOTAL_TILES - on_board - len(puzzle['rack'])
        state = game.get_state('sid')
        assert state['puzzle'] == {'date': puzzle['date'], 'score': None}
        assert 'best_score' not in str(state['puzzle'])

    def test_one_placement_ends_the_game_with_the_move_score(self, puzzle):
        game = daily.build_game('r', 'sid', 'Anna', puzzle)
        tiles = [(t['row'], t['col'], t['letter'], t['is_blank']) for t in puzzle['best']['tiles']]
        ok, _msg, score = game.place_tiles('sid', tiles)
        assert ok and game.finished
        assert game.players[0].score == score        # nincs végső elszámolás
        assert game.puzzle['score'] == score
        assert [p.name for p in game.winners] == ['Anna']
        assert game.last_action_info['type'] == 'puzzle'

    def test_invalid_placement_keeps_the_puzzle_open(self, puzzle):
        game = daily.build_game('r', 'sid', 'Anna', puzzle)
        ok, _msg, _score = game.place_tiles('sid', [(0, 0, puzzle['rack'][0], False)])
        assert not ok and not game.finished and game.puzzle['score'] is None

    def test_pass_and_exchange_are_refused(self, puzzle):
        game = daily.build_game('r', 'sid', 'Anna', puzzle)
        ok, msg = game.pass_turn('sid')
        assert not ok and 'feladvány' in msg
        ok, msg = game.exchange_tiles('sid', [0])
        assert not ok and 'feladvány' in msg

    def test_reset_restores_the_initial_position(self, puzzle):
        game = daily.build_game('r', 'sid', 'Anna', puzzle)
        tiles = [(t['row'], t['col'], t['letter'], t['is_blank']) for t in puzzle['best']['tiles']]
        game.place_tiles('sid', tiles)
        daily.reset_game(game, puzzle)
        assert not game.finished and game.winners == [] and game.move_log == []
        assert game.players[0].hand == puzzle['rack'] and game.players[0].score == 0
        assert game.board.to_dict() == puzzle['board']
        assert game.puzzle == {'date': puzzle['date'], 'score': None}

    def test_normal_games_have_no_puzzle(self):
        g = Game('x')
        g.add_player('a', 'A')
        g.start()
        assert g.puzzle is None and g.get_state('a')['puzzle'] is None


class TestScores:
    def _users(self, n=3):
        return [auth.create_user(f'u{i}@example.com', f'U{i}', 'pass123')[1] for i in range(n)]

    def test_first_attempt_is_recorded(self, puzzle):
        (u,) = self._users(1)
        result = daily.record_result(puzzle, u, 20, [], ['AL'])
        assert result['recorded'] and result['attempts'] == 1 and result['my_best'] == 20
        assert result['rank'] == 1 and not result['is_best']

    def test_best_attempt_counts_and_attempts_accumulate(self, puzzle):
        (u,) = self._users(1)
        daily.record_result(puzzle, u, 20, [], ['AL'])
        worse = daily.record_result(puzzle, u, 10, [], ['AL'])
        better = daily.record_result(puzzle, u, 25, [], ['AL'])
        assert worse['my_best'] == 20 and worse['attempts'] == 2
        assert better['my_best'] == 25 and better['attempts'] == 3
        assert auth.get_daily_entry(puzzle['date'], u)['best_score'] == 25

    def test_ranking_order_with_ties(self, puzzle):
        a, b, c = self._users(3)
        daily.record_result(puzzle, a, 30, [], [])
        daily.record_result(puzzle, b, 40, [], [])
        daily.record_result(puzzle, c, 30, [], [])
        daily.record_result(puzzle, c, 5, [], [])        # c-nek több próbálkozása van
        entries, me = auth.get_daily_leaderboard(puzzle['date'], 10, c)
        assert [e['user_id'] for e in entries] == [b, a, c]
        assert me['rank'] == 3 and entries[0]['best_score'] == 40

    def test_score_above_the_known_best_raises_it(self, puzzle):
        (u,) = self._users(1)
        higher = puzzle['best_score'] + 7
        result = daily.record_result(puzzle, u, higher, [{'row': 7, 'col': 7, 'letter': 'A', 'is_blank': False}],
                                     ['ÚJ'])
        assert result['is_best'] and result['best_score'] == higher
        stored = auth.get_daily_puzzle(puzzle['date'])
        assert stored['best_score'] == higher and stored['best']['words'] == ['ÚJ']

    def test_guest_result_is_not_recorded(self, puzzle):
        result = daily.record_result(puzzle, None, 12, [], ['AL'])
        assert not result['recorded'] and result['rank'] is None and result['attempts'] is None

    def test_revealed_solution_blocks_ranking(self, puzzle):
        (u,) = self._users(1)
        auth.mark_daily_revealed(puzzle['date'], u)
        result = daily.record_result(puzzle, u, 99, [], ['AL'])
        assert not result['recorded']
        assert auth.get_daily_leaderboard(puzzle['date'])[0] == []

    def test_reveal_after_scoring_keeps_the_score_but_blocks_improvement(self, puzzle):
        (u,) = self._users(1)
        daily.record_result(puzzle, u, 20, [], [])
        auth.mark_daily_revealed(puzzle['date'], u)
        assert daily.record_result(puzzle, u, 99, [], [])['recorded'] is False
        entry = auth.get_daily_leaderboard(puzzle['date'])[0][0]
        assert entry['best_score'] == 20

    def test_reveal_without_attempts_stays_off_the_leaderboard(self, puzzle):
        (u,) = self._users(1)
        auth.mark_daily_revealed(puzzle['date'], u)
        assert auth.get_daily_leaderboard(puzzle['date'])[0] == []

    def test_days_are_independent(self, puzzle):
        (u,) = self._users(1)
        daily.record_result(puzzle, u, 20, [], [])
        assert auth.get_daily_leaderboard('2000-01-01')[0] == []


# ---------------------------------------------------------------- socket

def _registered(email, name):
    import server
    from helpers import registered_set_name_payload
    _, uid = auth.create_user(email, name, 'password123')
    c = server.socketio.test_client(server.app)
    c.emit('set_name', registered_set_name_payload(uid, name=name))
    c.get_received()
    return c, uid


def _guest(name='Vendég'):
    import server
    c = server.socketio.test_client(server.app)
    c.emit('set_name', {'name': name, 'is_guest': True, 'user_id': None})
    c.get_received()
    return c


def _events(client, name):
    return [e['args'][0] if e['args'] else None for e in client.get_received() if e['name'] == name]


@pytest.fixture(autouse=True)
def _fresh_rate_limits():
    import server
    server.rate_limiter._socket_history.clear()
    server.rate_limiter._ip_history.clear()
    yield


def _start(client):
    client.emit('start_daily')
    received = client.get_received()
    return {e['name']: e['args'][0] if e['args'] else None for e in received}


class TestSocket:
    def test_start_creates_a_private_puzzle_room(self, puzzle):
        import server
        client, uid = _registered('a@example.com', 'Anna')
        got = _start(client)
        assert {'room_joined', 'game_state', 'game_started', 'daily_started'} <= set(got)
        assert got['daily_started'] == {'date': puzzle['date'], 'registered': True, 'my': None}
        room = next(iter(server.state.rooms.values()))
        assert room.is_puzzle and room.is_private and room.max_players == 1
        assert room.game.players[0].hand == puzzle['rack']
        assert server.state.get_rooms_list() == []
        assert server.state.get_live_games() == []
        assert got['game_state']['puzzle']['date'] == puzzle['date']

    def test_best_move_is_recorded_and_earns_the_badge(self, puzzle):
        client, uid = _registered('a@example.com', 'Anna')
        _start(client)
        client.emit('place_tiles', {'tiles': puzzle['best']['tiles']})
        got = {}
        for e in client.get_received():
            got.setdefault(e['name'], []).append(e['args'][0] if e['args'] else None)
        result = got['daily_result'][0]
        assert result['score'] == puzzle['best_score'] and result['is_best'] and result['recorded']
        assert result['rank'] == 1 and result['attempts'] == 1
        assert got['game_state'][-1]['finished'] is True
        assert got['achievements_earned'][0]['badges'] == ['daily_best']
        assert 'daily_best' in {a['badge'] for a in auth.get_user_achievements(uid)}
        assert auth.get_daily_leaderboard(puzzle['date'])[0][0]['user_id'] == uid

    def test_puzzle_game_is_never_saved_to_the_history(self, puzzle):
        import server
        client, uid = _registered('a@example.com', 'Anna')
        _start(client)
        client.emit('place_tiles', {'tiles': puzzle['best']['tiles']})
        client.get_received()
        room_id = next(iter(server.state.rooms))
        assert server._save_game_to_db(room_id)[0] is True
        assert auth.load_active_games() == []
        assert auth.get_user_game_history(uid) == []
        assert auth.get_user_by_id(uid)['games_played'] == 0

    def test_retry_resets_and_keeps_the_best(self, puzzle):
        import server
        client, uid = _registered('a@example.com', 'Anna')
        _start(client)
        game = next(iter(server.state.rooms.values())).game
        # gyenge, de érvényes lépés: a legjobb lépés első zsetonjából álló szó nem biztos, ezért a legjobbat
        # küldjük be, majd újrapróbálunk
        client.emit('place_tiles', {'tiles': puzzle['best']['tiles']})
        client.get_received()
        assert game.finished
        client.emit('retry_daily')
        got = {e['name']: e['args'][0] for e in client.get_received() if e['args']}
        assert not game.finished and game.players[0].hand == puzzle['rack']
        assert got['game_state']['finished'] is False
        assert got['daily_started']['my']['best_score'] == puzzle['best_score']
        client.emit('place_tiles', {'tiles': puzzle['best']['tiles']})
        second = _events(client, 'daily_result')[0]
        assert second['attempts'] == 2 and second['my_best'] == puzzle['best_score']

    def test_retry_is_ignored_before_a_submission(self, puzzle):
        import server
        client, _ = _registered('a@example.com', 'Anna')
        _start(client)
        game = next(iter(server.state.rooms.values())).game
        game.players[0].hand = ['A'] * 7
        client.emit('retry_daily')
        assert game.players[0].hand == ['A'] * 7

    def test_reveal_marks_the_user_and_blocks_ranking(self, puzzle):
        client, uid = _registered('a@example.com', 'Anna')
        _start(client)
        client.emit('reveal_daily')
        solution = _events(client, 'daily_solution')[0]
        assert solution['tiles'] == puzzle['best']['tiles'] and solution['ranked'] is False
        assert auth.get_daily_entry(puzzle['date'], uid)['revealed'] is True
        client.emit('place_tiles', {'tiles': puzzle['best']['tiles']})
        result = _events(client, 'daily_result')[0]
        assert result['recorded'] is False
        assert auth.get_daily_leaderboard(puzzle['date'])[0] == []

    def test_guest_can_play_but_is_not_ranked(self, puzzle):
        client = _guest()
        got = _start(client)
        assert got['daily_started']['registered'] is False
        client.emit('place_tiles', {'tiles': puzzle['best']['tiles']})
        result = _events(client, 'daily_result')[0]
        assert result['score'] == puzzle['best_score'] and result['recorded'] is False
        assert auth.get_daily_leaderboard(puzzle['date'])[0] == []

    def test_pass_and_exchange_events_are_refused(self, puzzle):
        client, _ = _registered('a@example.com', 'Anna')
        _start(client)
        client.emit('pass_turn')
        assert _events(client, 'action_result')[0]['success'] is False
        client.emit('exchange_tiles', {'indices': [0]})
        assert _events(client, 'action_result')[0]['success'] is False

    def test_cannot_start_while_in_a_room(self, puzzle):
        client, _ = _registered('a@example.com', 'Anna')
        _start(client)
        client.emit('start_daily')
        assert any('szobában' in (e or {}).get('message', '') for e in _events(client, 'error'))

    def test_unavailable_puzzle_is_reported(self, monkeypatch):
        client, _ = _registered('a@example.com', 'Anna')

        def boom(*args, **kwargs):
            raise RuntimeError('nincs feladvány')
        monkeypatch.setattr(daily, 'ensure_puzzle', boom)
        client.emit('start_daily')
        assert any('nem érhető el' in (e or {}).get('message', '') for e in _events(client, 'error'))

    def test_leaving_removes_the_room(self, puzzle):
        import server
        client, _ = _registered('a@example.com', 'Anna')
        _start(client)
        client.emit('leave_room')
        client.get_received()
        assert server.state.rooms == {}

    def test_start_is_rate_limited(self, puzzle):
        client, _ = _registered('a@example.com', 'Anna')
        for _ in range(3):
            _start(client)
            client.emit('leave_room')
            client.get_received()
        client.emit('start_daily')
        assert any('Túl sok' in (e or {}).get('message', '') for e in _events(client, 'error'))


# ---------------------------------------------------------------- HTTP

@pytest.fixture
def client():
    import server
    server.app.config['TESTING'] = True
    return server.app.test_client()


def _login(client, uid):
    client.set_cookie('session_token', auth.create_session(uid), domain='localhost')


class TestApi:
    def test_info_hides_the_best_score_until_you_played(self, client, puzzle):
        _, uid = _registered('a@example.com', 'Anna')
        _login(client, uid)
        data = client.get('/api/daily').get_json()
        assert data['success'] and data['date'] == puzzle['date'] and data['ready'] is True
        assert data['best_score'] is None and data['my'] is None
        daily.record_result(puzzle, uid, 21, [], [])
        data = client.get('/api/daily').get_json()
        assert data['best_score'] == puzzle['best_score']
        assert data['my']['best_score'] == 21
        assert data['leaderboard'][0]['is_me'] is True and data['me']['rank'] == 1

    def test_anonymous_info(self, client, puzzle):
        data = client.get('/api/daily').get_json()
        assert data['my'] is None and data['best_score'] is None

    def test_not_ready_before_the_first_request_of_the_day(self, client):
        assert client.get('/api/daily').get_json()['ready'] is False

    def test_yesterdays_solution(self, client, generated):
        today = daily.today_str()
        yesterday = daily.previous_date(today)
        auth.save_daily_puzzle(yesterday, generated['board'], generated['rack'],
                               generated['best_score'], generated['best'])
        _, uid = _registered('a@example.com', 'Anna')
        auth.record_daily_score(yesterday, uid, 33)
        data = client.get('/api/daily').get_json()
        assert data['yesterday']['best_words'] == generated['best']['words']
        assert data['yesterday']['top'][0]['best_score'] == 33

    def test_leaderboard_endpoint(self, client, puzzle):
        _, uid = _registered('a@example.com', 'Anna')
        daily.record_result(puzzle, uid, 30, [], [])
        data = client.get('/api/daily/leaderboard').get_json()
        assert [e['display_name'] for e in data['entries']] == ['Anna']
        assert client.get(f"/api/daily/leaderboard?date={puzzle['date']}").get_json()['entries']
        assert client.get('/api/daily/leaderboard?date=2000-01-01').get_json()['entries'] == []

    def test_leaderboard_rejects_bad_and_future_dates(self, client, puzzle):
        assert client.get('/api/daily/leaderboard?date=nem-datum').status_code == 400
        assert client.get('/api/daily/leaderboard?date=2999-01-01').status_code == 400

    def test_api_is_rate_limited(self, client, puzzle):
        codes = [client.get('/api/daily').status_code for _ in range(61)]
        assert codes[-1] == 429
