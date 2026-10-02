"""Megosztható visszajátszás: token, nyilvános elérés, jogosultság."""
import json

import pytest

import auth


@pytest.fixture
def client():
    import server
    server.app.config['TESTING'] = True
    return server.app.test_client()


def _finished_game(room_id='r-share', with_moves=True):
    _, alice = auth.create_user('alice@example.com', 'Alice', 'pass123')
    _, bob = auth.create_user('bob@example.com', 'Bob', 'pass123')
    _, carol = auth.create_user('carol@example.com', 'Carol', 'pass123')
    players = [
        {'player_name': 'Alice', 'user_id': alice, 'final_score': 120, 'is_winner': True},
        {'player_name': 'Bob', 'user_id': bob, 'final_score': 80, 'is_winner': False},
    ]
    game_id = auth.finish_game(room_id, '{}', players, room_name='Teszt szoba')
    if with_moves:
        details = json.dumps({'words': ['ALMA'], 'score': 12})
        auth.add_game_move(game_id, 1, 'Alice', 'place', details, '[]')
        auth.add_game_move(game_id, 2, 'Bob', 'pass', '{}', '[]')
    return game_id, alice, bob, carol


def _login(client, user_id):
    client.set_cookie('session_token', auth.create_session(user_id), domain='localhost')


class TestShareToken:
    def test_token_is_created_once_and_reused(self):
        game_id, *_ = _finished_game()
        token = auth.get_or_create_share_token(game_id)
        assert token and len(token) >= 8
        assert auth.get_or_create_share_token(game_id) == token
        assert auth.get_game_by_share_token(token)['id'] == game_id

    def test_unfinished_or_missing_game_has_no_token(self):
        assert auth.get_or_create_share_token(999) is None
        auth.save_game('r-active', 'Aktív', '{}', False, [], owner_name='X')
        active = auth.get_game_by_id(auth.load_active_games()[0]['id'])
        assert auth.get_or_create_share_token(active['id']) is None

    def test_unknown_token(self):
        assert auth.get_game_by_share_token('nincs-ilyen') is None
        assert auth.get_game_by_share_token('') is None

    def test_results_are_sorted_by_score(self):
        game_id, *_ = _finished_game()
        results = auth.get_game_results(game_id)
        assert [r['player_name'] for r in results] == ['Alice', 'Bob']
        assert results[0]['is_winner'] is True and results[1]['is_winner'] is False


class TestShareRoutes:
    def test_participant_can_share_and_anyone_can_watch(self, client):
        game_id, alice, *_ = _finished_game()
        _login(client, alice)
        res = client.post(f'/api/game/{game_id}/share')
        assert res.status_code == 200
        token = res.get_json()['token']

        anonymous = client.application.test_client()
        res = anonymous.get(f'/api/replay/{token}')
        data = res.get_json()
        assert res.status_code == 200 and data['success']
        assert data['room_name'] == 'Teszt szoba'
        assert [m['move_number'] for m in data['moves']] == [1, 2]
        assert [p['player_name'] for p in data['players']] == ['Alice', 'Bob']
        # a nyilvános nézet nem árul el e-mail címet / azonosítót
        assert 'alice@example.com' not in res.get_data(as_text=True)
        assert 'user_id' not in res.get_data(as_text=True)

    def test_non_participant_cannot_share(self, client):
        game_id, _alice, _bob, carol = _finished_game()
        _login(client, carol)
        assert client.post(f'/api/game/{game_id}/share').status_code == 403

    def test_share_requires_login(self, client):
        game_id, *_ = _finished_game()
        assert client.post(f'/api/game/{game_id}/share').status_code == 401

    def test_share_unknown_game(self, client):
        _, alice, *_ = _finished_game()
        _login(client, alice)
        assert client.post('/api/game/12345/share').status_code == 404

    def test_unknown_or_malformed_token_is_404(self, client):
        assert client.get('/api/replay/abcdefgh').status_code == 404
        assert client.get('/api/replay/x').status_code == 404
        assert client.get('/api/replay/' + 'a' * 80).status_code == 404

    def test_replay_endpoint_is_rate_limited(self, client):
        import server
        server.rate_limiter._ip_history.clear()
        statuses = [client.get('/api/replay/abcdefgh').status_code for _ in range(61)]
        assert statuses[-1] == 429
        server.rate_limiter._ip_history.clear()

    def test_moves_endpoint_includes_players_and_finished_flag(self, client):
        game_id, alice, *_ = _finished_game()
        _login(client, alice)
        data = client.get(f'/api/game/{game_id}/moves').get_json()
        assert data['finished'] is True
        assert [p['player_name'] for p in data['players']] == ['Alice', 'Bob']
