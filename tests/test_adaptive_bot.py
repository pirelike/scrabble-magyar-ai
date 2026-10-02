"""Az "igazodik hozzám" robot: fokozat az ember utolsó köreinek átlagából."""
import json
import random
import statistics

import pytest

import ai_player
from game import Game
from player import Player


class TestParsing:
    @pytest.mark.parametrize('value', ['auto', 'AUTO', ' Auto ', 'adaptive'])
    def test_adaptive_spellings(self, value):
        assert ai_player.parse_difficulty(value) == ai_player.ADAPTIVE

    def test_levels_and_legacy_names_still_parse(self):
        assert ai_player.parse_difficulty(7) == 7
        assert ai_player.parse_difficulty('hard') == 10
        assert ai_player.parse_difficulty('4') == 4

    @pytest.mark.parametrize('value', [None, 0, 11, True, 2.5, 'nonsense', {}, []])
    def test_invalid_values(self, value):
        assert ai_player.parse_difficulty(value) is None
        assert ai_player.normalize_difficulty(value) == ai_player.DEFAULT_LEVEL

    def test_parse_level_does_not_accept_auto(self):
        assert ai_player.parse_level('auto') is None

    def test_player_keeps_the_adaptive_marker(self):
        bot = Player('b', 'Rita', is_bot=True, difficulty='auto')
        assert bot.difficulty == ai_player.ADAPTIVE
        assert bot.to_dict()['difficulty'] == 'auto'
        assert Player('h', 'Ember', difficulty='auto').difficulty is None   # embernek nincs


class TestLevelMapping:
    def test_every_level_maps_back_to_itself(self):
        for level, strength in enumerate(ai_player.LEVEL_STRENGTH, start=1):
            assert ai_player.level_for_average(strength) == pytest.approx(level)

    def test_interpolation_between_levels(self):
        low, high = ai_player.LEVEL_STRENGTH[3], ai_player.LEVEL_STRENGTH[4]
        assert ai_player.level_for_average((low + high) / 2) == pytest.approx(4.5)

    def test_clamped_at_the_ends(self):
        assert ai_player.level_for_average(-5) == 1.0
        assert ai_player.level_for_average(0) == 1.0
        assert ai_player.level_for_average(500) == 10.0

    def test_mapping_is_monotonic(self):
        values = [ai_player.level_for_average(x / 2) for x in range(0, 80)]
        assert values == sorted(values)

    def test_scale_matches_the_profiles(self):
        assert len(ai_player.LEVEL_STRENGTH) == len(ai_player.DIFFICULTIES)
        assert list(ai_player.LEVEL_STRENGTH) == sorted(ai_player.LEVEL_STRENGTH)


class TestAdaptiveLevel:
    def test_without_data_starts_near_the_start_level(self):
        rng = random.Random(1)
        levels = {ai_player.adaptive_level(None, 0, rng) for _ in range(50)}
        assert levels == {int(ai_player.ADAPT_START_LEVEL)}
        assert ai_player.adaptive_level(15.0, 0, rng) == int(ai_player.ADAPT_START_LEVEL)

    def test_full_window_follows_the_measured_average(self):
        rng = random.Random(2)
        assert ai_player.adaptive_level(ai_player.LEVEL_STRENGTH[1], ai_player.ADAPT_WINDOW, rng) == 2
        assert ai_player.adaptive_level(ai_player.LEVEL_STRENGTH[7], ai_player.ADAPT_WINDOW, rng) == 8
        assert ai_player.adaptive_level(0, ai_player.ADAPT_WINDOW, rng) == 1
        assert ai_player.adaptive_level(999, ai_player.ADAPT_WINDOW, rng) == 10

    def test_little_data_pulls_towards_the_start_level(self):
        rng = random.Random(3)
        full = statistics.mean(ai_player.adaptive_level(28.0, ai_player.ADAPT_WINDOW, rng) for _ in range(200))
        two = statistics.mean(ai_player.adaptive_level(28.0, 2, rng) for _ in range(200))
        assert two < full
        assert two > ai_player.ADAPT_START_LEVEL

    def test_fractional_target_mixes_neighbouring_levels(self):
        rng = random.Random(4)
        average = (ai_player.LEVEL_STRENGTH[4] * 0.7 + ai_player.LEVEL_STRENGTH[5] * 0.3)  # ~5,3
        levels = [ai_player.adaptive_level(average, ai_player.ADAPT_WINDOW, rng) for _ in range(2000)]
        assert set(levels) <= {5, 6}
        share_six = levels.count(6) / len(levels)
        assert 0.2 < share_six < 0.45

    def test_always_within_the_scale(self):
        rng = random.Random(5)
        for average in (None, -3, 0, 4.3, 15, 28, 100):
            for turns in (0, 1, 3, 6, 20):
                assert 1 <= ai_player.adaptive_level(average, turns, rng) <= 10


def _move(player, kind, score=0):
    return {'move_number': 1, 'player_name': player, 'action_type': kind,
            'details_json': json.dumps({'score': score}), 'board_snapshot_json': '[]'}


def _game(*names):
    g = Game('t')
    for i, name in enumerate(names):
        g.add_player(f'p{i}', name)
    g.add_bot('Rita', 'auto')
    return g


class TestRecentStats:
    def test_average_of_the_last_turns_with_passes_as_zero(self):
        g = _game('Anna')
        g.move_log = [_move('Anna', 'place', 30), _move('Rita', 'place', 99),
                      _move('Anna', 'pass'), _move('Anna', 'exchange'), _move('Anna', 'place', 30)]
        average, turns = g.recent_stats(['Anna'])
        assert (average, turns) == (15.0, 4)

    def test_window_keeps_only_the_latest_turns(self):
        g = _game('Anna')
        g.move_log = [_move('Anna', 'place', 100)] * 3 + [_move('Anna', 'place', 10)] * 6
        average, turns = g.recent_stats(['Anna'], window=6)
        assert (average, turns) == (10.0, 6)

    def test_challenge_accept_counts_and_rejected_placements_do_not(self):
        g = _game('Anna')
        g.move_log = [_move('Anna', 'challenge_accept', 40), _move('Anna', 'challenge_reject')]
        assert g.recent_stats(['Anna']) == (40.0, 1)

    def test_several_humans_average_their_averages(self):
        g = _game('Anna', 'Béla')
        g.move_log = [_move('Anna', 'place', 40), _move('Béla', 'place', 10), _move('Béla', 'place', 20)]
        average, turns = g.recent_stats(['Anna', 'Béla'])
        assert average == pytest.approx((40 + 15) / 2) and turns == 2

    def test_no_moves_yet(self):
        g = _game('Anna')
        assert g.recent_stats(['Anna']) == (None, 0)
        assert g.recent_stats([]) == (None, 0)

    def test_broken_details_count_as_zero(self):
        g = _game('Anna')
        g.move_log = [{'move_number': 1, 'player_name': 'Anna', 'action_type': 'place',
                       'details_json': 'nem json', 'board_snapshot_json': '[]'}]
        assert g.recent_stats(['Anna']) == (0.0, 1)


class TestBotLevel:
    def test_fixed_bot_keeps_its_level(self):
        g = Game('t')
        g.add_player('p', 'Anna')
        g.add_bot('Robi', 4)
        g.move_log = [_move('Anna', 'place', 90)] * 6
        assert g.bot_level(g.players[1]) == 4

    def test_adaptive_bot_follows_the_human(self):
        g = _game('Anna')
        bot = g.players[1]
        rng = random.Random(7)
        g.move_log = [_move('Anna', 'pass')] * 6
        assert g.bot_level(bot, rng) == 1
        g.move_log = [_move('Anna', 'place', 60)] * 6
        assert g.bot_level(bot, rng) == 10

    def test_adaptive_bot_ignores_other_bots(self):
        g = _game('Anna')
        g.add_bot('Robi', 10)
        g.move_log = [_move('Anna', 'place', 4)] * 6 + [_move('Robi', 'place', 80)] * 6
        assert g.bot_level(g.players[1], random.Random(8)) == 1

    def test_reference_names_override_for_bot_vs_bot_tests(self):
        g = Game('t')
        g.add_bot('Rita', 'auto')
        g.add_bot('Robi', 6)
        g.move_log = [_move('Robi', 'place', 28)] * 6
        assert g.bot_level(g.players[0], random.Random(9), reference_names=['Robi']) == 10

    def test_start_level_before_the_human_moved(self):
        g = _game('Anna')
        assert g.bot_level(g.players[1], random.Random(10)) == int(ai_player.ADAPT_START_LEVEL)

    def test_save_and_restore_keep_the_adaptive_marker(self):
        g = _game('Anna')
        g.start()
        restored = Game.from_save_dict(g.to_save_dict())
        assert restored.players[1].difficulty == ai_player.ADAPTIVE
        assert restored.players[1].is_bot


# ---------------------------------------------------------------- szerver

@pytest.fixture
def make_client():
    import server
    server.rate_limiter._socket_history.clear()

    def _make(name='Alice'):
        c = server.socketio.test_client(server.app)
        c.emit('set_name', {'name': name, 'is_guest': True, 'user_id': None})
        c.get_received()
        return c
    return _make


def _create(client, **data):
    import server
    client.emit('create_room', {'name': 'R', 'max_players': 4, **data})
    client.get_received()
    return list(server.state.rooms.values())[-1]


class TestServer:
    def test_create_room_accepts_the_adaptive_robot(self, make_client):
        room = _create(make_client(), ai_players=['auto', 3, 'AUTO'])
        assert [p.difficulty for p in room.game.players[1:]] == ['auto', 3, 'auto']

    def test_bot_turn_uses_the_effective_level(self, make_client, monkeypatch):
        import server
        monkeypatch.setattr(server.socketio, 'start_background_task', lambda fn, *a, **k: None)
        client = make_client()
        room = _create(client, ai_players=['auto'])
        client.emit('start_game')
        client.get_received()
        game = room.game
        game.move_log = [_move('Alice', 'place', 60)] * 6        # az ember nagyon jól játszik
        game.current_player_idx = 1                                # a robot jön
        seen = []

        def fake_choose(board, rack, difficulty, bag_remaining, **kwargs):
            seen.append(difficulty)
            return {'action': 'pass'}
        monkeypatch.setattr(server.ai_player, 'choose_action', fake_choose)
        server._play_bot_turn(room.id)
        assert seen == [10]

    def test_waiting_room_state_shows_the_marker(self, make_client):
        client = make_client()
        room = _create(client, ai_players=['auto'])
        state = room.game.get_state(for_player_id=room.game.players[0].id)
        assert state['players'][1]['difficulty'] == 'auto'
        assert state['players'][1]['is_bot']
