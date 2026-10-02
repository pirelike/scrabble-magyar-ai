"""Kitüntetések: játékonkénti kiértékelés, összesítők, mentés, profil, szerver."""
import json
import re
from pathlib import Path

import pytest

import achievements
import auth
from game import Game


def _move(player, tiles=0, score=10, words=('AL',), blank=False, action='place'):
    return {
        'move_number': 1, 'player_name': player, 'action_type': action,
        'details_json': json.dumps({
            'tiles': [{'row': 7, 'col': 7 + i, 'letter': 'A', 'is_blank': blank and i == 0}
                      for i in range(tiles)],
            'words': list(words), 'score': score,
        }),
        'board_snapshot_json': '[]',
    }


def _game(moves=(), scores=(0, 0), bot_level=None):
    g = Game('t')
    g.add_player('p1', 'Anna')
    g.add_player('p2', 'Béla')
    if bot_level is not None:
        g.players.pop()
        g.add_bot('Robi', bot_level)
    g.start()
    for p, s in zip(g.players, scores):
        p.score = s
        p.hand = []
    g.move_log = list(moves)
    g._end_game(None)
    return g


NAMES = {'Anna': 1, 'Béla': 2}


class TestEvaluateGame:
    def test_bingo(self):
        g = _game([_move('Anna', tiles=7, score=70)])
        assert 'bingo' in achievements.evaluate_game(g, NAMES)[1]

    def test_six_tiles_is_not_a_bingo(self):
        g = _game([_move('Anna', tiles=6)])
        assert 'bingo' not in achievements.evaluate_game(g, NAMES)[1]

    def test_big_move(self):
        g = _game([_move('Anna', score=100), _move('Béla', score=99)])
        earned = achievements.evaluate_game(g, NAMES)
        assert 'score_100' in earned[1] and 'score_100' not in earned[2]

    def test_challenge_accepted_move_counts(self):
        g = _game([_move('Anna', score=120, action='challenge_accept')])
        assert 'score_100' in achievements.evaluate_game(g, NAMES)[1]

    def test_other_actions_do_not_count(self):
        g = _game([_move('Anna', tiles=7, score=200, action='challenge_reject')])
        assert achievements.evaluate_game(g, NAMES)[1] == set()

    def test_blank_tile(self):
        g = _game([_move('Anna', tiles=2, blank=True)])
        assert 'joker_play' in achievements.evaluate_game(g, NAMES)[1]

    def test_long_word_counts_tiles_not_letters(self):
        # 8 zseton: CS-Á-R-D-A-Ő-R-E (CS egy zseton), 9 karakter
        g = _game([_move('Anna', words=('CSÁRDAŐRE',))])
        assert 'long_word' in achievements.evaluate_game(g, NAMES)[1]
        # 7 zseton: SZ-Á-M-Í-T-Ó-K (SZ egy zseton) — csak 7, bár 8 karakter
        g = _game([_move('Anna', words=('SZÁMÍTÓK',))])
        assert 'long_word' not in achievements.evaluate_game(g, NAMES)[1]

    def test_high_game(self):
        g = _game(scores=(300, 299))
        earned = achievements.evaluate_game(g, NAMES)
        assert 'game_300' in earned[1] and 'game_300' not in earned[2]

    def test_beating_a_strong_bot(self):
        g = _game(scores=(100, 50), bot_level=9)
        assert 'bot_slayer' in achievements.evaluate_game(g, {'Anna': 1})[1]

    def test_losing_to_or_beating_a_weak_bot_gives_nothing(self):
        assert 'bot_slayer' not in achievements.evaluate_game(
            _game(scores=(50, 100), bot_level=9), {'Anna': 1})[1]
        assert 'bot_slayer' not in achievements.evaluate_game(
            _game(scores=(100, 50), bot_level=7), {'Anna': 1})[1]

    def test_guests_and_bots_get_no_entry(self):
        g = _game([_move('Anna', tiles=7)], bot_level=9)
        assert achievements.evaluate_game(g, {}) == {}

    def test_tied_game_is_scored_for_every_tied_human(self):
        g = _game(scores=(300, 300))
        earned = achievements.evaluate_game(g, NAMES)
        assert earned[1] == earned[2] == {'game_300'}


class TestCumulative:
    @pytest.mark.parametrize('played,won,expected', [
        (0, 0, set()),
        (1, 0, {'first_game'}),
        (1, 1, {'first_game', 'first_win'}),
        (10, 9, {'first_game', 'first_win'}),
        (10, 10, {'first_game', 'first_win', 'wins_10'}),
        (25, 0, {'first_game', 'games_25'}),
    ])
    def test_thresholds(self, played, won, expected):
        assert achievements.cumulative_badges(played, won) == expected

    def test_every_badge_is_reachable(self):
        reachable = achievements.cumulative_badges(25, 10) | {
            'bingo', 'score_100', 'long_word', 'joker_play', 'game_300', 'bot_slayer'}
        assert reachable == set(achievements.BADGES)


class TestStorage:
    def test_grant_is_idempotent_and_reports_only_new(self):
        _, uid = auth.create_user('a@example.com', 'A', 'pass123')
        assert auth.grant_achievements(uid, {'bingo', 'first_game'}, 5) == ['bingo', 'first_game']
        assert auth.grant_achievements(uid, {'bingo', 'score_100'}, 6) == ['score_100']
        owned = {a['badge']: a['game_id'] for a in auth.get_user_achievements(uid)}
        assert owned == {'bingo': 5, 'first_game': 5, 'score_100': 6}

    def test_badges_are_per_user(self):
        _, a = auth.create_user('a@example.com', 'A', 'pass123')
        _, b = auth.create_user('b@example.com', 'B', 'pass123')
        auth.grant_achievements(a, {'bingo'})
        assert auth.get_user_achievements(b) == []


class TestFrontendList:
    def test_client_icons_match_the_server_badges(self):
        js = (Path(__file__).resolve().parent.parent / 'static' / 'app.js').read_text(encoding='utf-8')
        block = re.search(r'const BADGE_ICONS = \{(.*?)\};', js, re.S).group(1)
        client = set(re.findall(r'(\w+):', block))
        assert client == set(achievements.BADGES)


# ---------------------------------------------------------------- szerver

def _registered(email, name):
    import server
    from helpers import registered_set_name_payload
    _, uid = auth.create_user(email, name, 'password123')
    c = server.socketio.test_client(server.app)
    c.emit('set_name', registered_set_name_payload(uid, name=name))
    c.get_received()
    return c, uid


def _solo_room():
    import server
    owner, uid = _registered('solo@example.com', 'Solo')
    owner.emit('create_room', {'name': 'Egyedül', 'max_players': 2})
    owner.get_received()
    owner.emit('start_game')
    owner.get_received()
    room_id, room = next(iter(server.state.rooms.items()))
    return owner, uid, room_id, room


class TestServerAwards:
    def test_finished_game_grants_and_notifies(self):
        import server
        owner, uid, room_id, room = _solo_room()
        game = room.game
        game.move_log.append(_move('Solo', tiles=7, score=120, blank=True))
        for _ in range(6):
            game.pass_turn(game.current_player().id)
        assert game.finished
        server._save_game_to_db(room_id)

        owned = {a['badge'] for a in auth.get_user_achievements(uid)}
        assert {'bingo', 'score_100', 'joker_play', 'first_game'} <= owned
        events = [e for e in owner.get_received() if e['name'] == 'achievements_earned']
        assert len(events) == 1
        assert {'bingo', 'first_game'} <= set(events[0]['args'][0]['badges'])

    def test_second_save_does_not_notify_again(self):
        import server
        owner, uid, room_id, room = _solo_room()
        game = room.game
        for _ in range(6):
            game.pass_turn(game.current_player().id)
        server._save_game_to_db(room_id)
        owner.get_received()
        server._save_game_to_db(room_id)
        assert not [e for e in owner.get_received() if e['name'] == 'achievements_earned']

    def test_profile_lists_the_badges(self):
        import server
        owner, uid, room_id, room = _solo_room()
        for _ in range(6):
            room.game.pass_turn(room.game.current_player().id)
        server._save_game_to_db(room_id)
        client = server.app.test_client()
        client.set_cookie('session_token', auth.create_session(uid), domain='localhost')
        data = client.get('/api/auth/profile').get_json()
        assert 'first_game' in {b['badge'] for b in data['badges']}

    def test_award_failure_never_breaks_saving(self, monkeypatch):
        import server
        owner, uid, room_id, room = _solo_room()
        for _ in range(6):
            room.game.pass_turn(room.game.current_player().id)

        def boom(*args, **kwargs):
            raise RuntimeError('db hiba')
        monkeypatch.setattr(server, 'grant_achievements', boom)
        ok, _msg = server._save_game_to_db(room_id)
        assert ok is True
