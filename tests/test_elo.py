"""ELO számítás (elo.py) és az értékszám tárolása / ranglistája (auth.py)."""
import pytest

import auth
import elo


class TestMath:
    def test_equal_ratings_expect_half(self):
        assert elo.expected_score(1200, 1200) == pytest.approx(0.5)

    def test_expected_scores_are_complementary(self):
        assert elo.expected_score(1400, 1200) + elo.expected_score(1200, 1400) == pytest.approx(1)
        assert elo.expected_score(1400, 1200) > 0.7

    def test_k_factor_drops_after_provisional_period(self):
        assert elo.k_factor(0) == elo.K_PROVISIONAL
        assert elo.k_factor(elo.PROVISIONAL_GAMES - 1) == elo.K_PROVISIONAL
        assert elo.k_factor(elo.PROVISIONAL_GAMES) == elo.K_ESTABLISHED


class TestRatingChanges:
    def test_even_match_winner_gains_half_k(self):
        ch = elo.rating_changes([('a', 1200, 20, 100), ('b', 1200, 20, 80)])
        assert ch == {'a': 10, 'b': -10}

    def test_draw_between_equals_changes_nothing(self):
        ch = elo.rating_changes([('a', 1200, 20, 90), ('b', 1200, 20, 90)])
        assert ch == {'a': 0, 'b': 0}

    def test_upset_pays_more_than_expected_win(self):
        upset = elo.rating_changes([('weak', 1000, 20, 100), ('strong', 1400, 20, 80)])
        expected_win = elo.rating_changes([('strong', 1400, 20, 100), ('weak', 1000, 20, 80)])
        assert upset['weak'] > expected_win['strong'] > 0

    def test_draw_moves_rating_towards_the_opponent(self):
        ch = elo.rating_changes([('strong', 1400, 20, 90), ('weak', 1000, 20, 90)])
        assert ch['strong'] < 0 < ch['weak']

    def test_new_players_move_faster(self):
        new = elo.rating_changes([('a', 1200, 0, 100), ('b', 1200, 0, 80)])
        old = elo.rating_changes([('a', 1200, 50, 100), ('b', 1200, 50, 80)])
        assert new['a'] > old['a']

    def test_four_players_are_zero_sum_for_equal_k(self):
        entries = [('a', 1200, 20, 300), ('b', 1200, 20, 250), ('c', 1200, 20, 200), ('d', 1200, 20, 100)]
        ch = elo.rating_changes(entries)
        assert abs(sum(ch.values())) <= 1  # kerekítés
        assert ch['a'] > ch['b'] > ch['c'] > ch['d']
        assert ch['a'] <= elo.K_ESTABLISHED  # akárhány ellenfél, legfeljebb K

    def test_single_player_has_no_change(self):
        assert elo.rating_changes([('a', 1200, 0, 50)]) == {'a': 0}
        assert elo.rating_changes([]) == {}


# ---------------------------------------------------------------- adatbázis

def _users(n=2):
    return [auth.create_user(f'u{i}@example.com', f'U{i}', 'pass123')[1] for i in range(n)]


def _finish(room, scores, users, guests=(), has_bots=False):
    players = []
    best = max(scores)
    for name, uid, score in zip([f'U{i}' for i in range(len(users))], users, scores):
        players.append({'player_name': name, 'user_id': uid, 'final_score': score,
                        'is_winner': score == best})
    for i, score in enumerate(guests):
        players.append({'player_name': f'G{i}', 'user_id': None, 'final_score': score,
                        'is_winner': False})
    return auth.finish_game(room, '{}', players, has_bots=has_bots)


class TestStoredRatings:
    def test_new_users_start_at_the_initial_rating(self):
        (a,) = _users(1)
        user = auth.get_user_by_id(a)
        assert user['rating'] == elo.INITIAL_RATING and user['rated_games'] == 0

    def test_rated_game_updates_both_players(self):
        a, b = _users(2)
        game_id = _finish('r1', [120, 80], [a, b])
        assert auth.get_user_by_id(a)['rating'] == 1216   # K=32 → +16
        assert auth.get_user_by_id(b)['rating'] == 1184
        assert auth.get_user_by_id(a)['rated_games'] == 1
        changes = auth.get_game_rating_changes(game_id)
        assert changes[a] == (1200, 1216) and changes[b] == (1200, 1184)

    def test_draw_keeps_equal_ratings(self):
        a, b = _users(2)
        _finish('r1', [100, 100], [a, b])
        assert auth.get_user_by_id(a)['rating'] == auth.get_user_by_id(b)['rating'] == 1200

    def test_game_with_bots_is_not_rated(self):
        a, b = _users(2)
        _finish('r1', [120, 80], [a, b], has_bots=True)
        assert auth.get_user_by_id(a)['rating'] == 1200
        assert auth.get_user_by_id(a)['rated_games'] == 0

    def test_solo_registered_player_is_not_rated(self):
        (a,) = _users(1)
        _finish('r1', [120], [a], guests=(50,))
        assert auth.get_user_by_id(a)['rating'] == 1200
        assert auth.get_user_by_id(a)['rated_games'] == 0

    def test_guests_do_not_take_part_in_the_calculation(self):
        a, b = _users(2)
        _finish('r1', [100, 90], [a, b], guests=(500,))
        assert auth.get_user_by_id(a)['rating'] == 1216

    def test_ratings_accumulate_over_games(self):
        a, b = _users(2)
        _finish('r1', [120, 80], [a, b])
        _finish('r2', [120, 80], [a, b])
        assert auth.get_user_by_id(a)['rating'] > 1216
        assert auth.get_user_by_id(a)['rated_games'] == 2

    def test_cleanup_without_players_changes_nothing(self):
        a, b = _users(2)
        auth.finish_game('r-clean', '{}', [])
        assert auth.get_user_by_id(a)['rating'] == 1200


class TestLeaderboardByRating:
    def _play(self, n, a, b):
        for i in range(n):
            _finish(f'g{i}-{a}-{b}', [120, 80], [a, b])

    def test_rating_metric_is_available(self):
        assert 'rating' in auth.LEADERBOARD_METRICS

    def test_ranked_by_rating_after_enough_rated_games(self):
        a, b = _users(2)
        self._play(elo.MIN_RATED_GAMES_FOR_RANKING, a, b)
        entries, me = auth.get_leaderboard('rating', 10, b)
        assert [e['user_id'] for e in entries] == [a, b]
        assert entries[0]['rating'] > entries[1]['rating']
        assert me['user_id'] == b and me['rank'] == 2

    def test_not_ranked_before_enough_rated_games(self):
        a, b = _users(2)
        self._play(elo.MIN_RATED_GAMES_FOR_RANKING - 1, a, b)
        assert auth.get_leaderboard('rating')[0] == []

    def test_games_with_bots_do_not_count_towards_ranking(self):
        a, b = _users(2)
        for i in range(5):
            _finish(f'bot{i}', [120, 80], [a, b], has_bots=True)
        assert auth.get_leaderboard('rating')[0] == []

    def test_history_exposes_the_rating_change(self):
        a, b = _users(2)
        _finish('r1', [120, 80], [a, b])
        row = auth.get_user_game_history(a)[0]
        assert (row['rating_before'], row['rating_after']) == (1200, 1216)


# ---------------------------------------------------------------- szerver és API

def _registered(email, name):
    import server
    from helpers import registered_set_name_payload
    _, uid = auth.create_user(email, name, 'password123')
    c = server.socketio.test_client(server.app)
    c.emit('set_name', registered_set_name_payload(uid, name=name))
    c.get_received()
    return c, uid


def _two_player_room(**room_opts):
    import server
    a, a_id = _registered('anna@example.com', 'Anna')
    b, b_id = _registered('bela@example.com', 'Béla')
    a.emit('create_room', {'name': 'R', 'max_players': 2, **room_opts})
    code = next(m for m in a.get_received() if m['name'] == 'room_code')['args'][0]['code']
    b.emit('join_room', {'code': code})
    b.get_received()
    a.emit('start_game')
    a.get_received(); b.get_received()
    room_id, room = next(iter(server.state.rooms.items()))
    return a, a_id, b, b_id, room_id, room


class TestServerAndApi:
    def test_rating_update_is_sent_to_rated_players(self):
        import server
        a, a_id, b, b_id, room_id, room = _two_player_room()
        game = room.game
        game.players[0].score, game.players[1].score = 90, 40
        for _ in range(6):
            game.pass_turn(game.current_player().id)
        server._save_game_to_db(room_id)

        for client, uid in ((a, a_id), (b, b_id)):
            events = [e['args'][0] for e in client.get_received() if e['name'] == 'rating_update']
            assert len(events) == 1
            user = auth.get_user_by_id(uid)
            assert events[0]['rating'] == user['rating']
            assert events[0]['change'] == user['rating'] - 1200
        assert auth.get_user_by_id(a_id)['rating'] > 1200 > auth.get_user_by_id(b_id)['rating']

    def test_no_rating_update_in_games_with_bots(self):
        import server
        a, a_id = _registered('solo@example.com', 'Solo')
        a.emit('create_room', {'name': 'Bot', 'max_players': 2, 'ai_players': [3]})
        a.get_received()
        a.emit('start_game')
        a.get_received()
        room_id, room = next(iter(server.state.rooms.items()))
        for _ in range(6):
            room.game.pass_turn(room.game.current_player().id)
        server._save_game_to_db(room_id)
        assert not [e for e in a.get_received() if e['name'] == 'rating_update']
        assert auth.get_user_by_id(a_id)['rated_games'] == 0

    def test_profile_and_leaderboard_api(self):
        import server
        a, a_id, b, b_id, room_id, room = _two_player_room()
        room.game.players[0].score, room.game.players[1].score = 90, 40
        for _ in range(6):
            room.game.pass_turn(room.game.current_player().id)
        server._save_game_to_db(room_id)

        client = server.app.test_client()
        client.set_cookie('session_token', auth.create_session(a_id), domain='localhost')
        profile = client.get('/api/auth/profile').get_json()
        assert profile['stats']['rating'] == 1216 and profile['stats']['rated_games'] == 1
        assert profile['history'][0]['rating_change'] == 16

        server.rate_limiter._ip_history.clear()
        board = client.get('/api/leaderboard?metric=rating').get_json()
        assert board['success'] and board['metric'] == 'rating'
        assert board['min_games'] == elo.MIN_RATED_GAMES_FOR_RANKING
        assert board['entries'] == []   # még kevés értékelt játék


class TestResignedPlayersInRatings:
    def test_resigner_loses_even_with_the_higher_score(self):
        a, b = _users(2)
        players = [
            {'player_name': 'U0', 'user_id': a, 'final_score': 300, 'is_winner': False, 'resigned': True},
            {'player_name': 'U1', 'user_id': b, 'final_score': 50, 'is_winner': True},
        ]
        auth.finish_game('resign-room', '{}', players)
        assert auth.get_user_by_id(a)['rating'] < 1200 < auth.get_user_by_id(b)['rating']
