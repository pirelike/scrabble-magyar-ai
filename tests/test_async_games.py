"""Levelezős (aszinkron) játékmód: játéklogika, szerver, mentés, határidők, visszaállítás."""
import json
import random
import time
from types import SimpleNamespace

import pytest

import async_games
import auth
import push_service
from game import Game, MAX_TIMEOUTS

ALMA = [{'row': 7, 'col': 6 + i, 'letter': l, 'is_blank': False} for i, l in enumerate('ALMA')]
ALMA_HAND = ['A', 'L', 'M', 'A', 'K', 'Ö', 'R']


# ---------------------------------------------------------------- játéklogika

def _game(names=('Anna', 'Béla'), hours=24):
    g = Game('room1')
    g.async_mode = True
    g.turn_hours = hours
    for i, name in enumerate(names):
        g.add_player(f'p{i}', name)
    g.start()
    return g


class TestBuildGame:
    def test_players_and_state(self):
        game, known = async_games.build_game(
            'r1', 'sid-anna', 'Anna', 10,
            [{'id': 20, 'name': 'Béla'}, {'id': 30, 'name': 'Cili'}], 48, rng=SimpleNamespace(randrange=lambda n: 0))
        assert [p.name for p in game.players] == ['Anna', 'Béla', 'Cili']
        assert known == {'Anna': 10, 'Béla': 20, 'Cili': 30}
        assert [p.disconnected for p in game.players] == [False, True, True]
        assert game.players[1].id == async_games.placeholder_id('r1', 20)
        assert game.started and game.async_mode and game.turn_hours == 48
        assert game.turn_deadline and abs(game.turn_deadline - (time.time() + 48 * 3600)) < 5
        assert all(len(p.hand) == 7 for p in game.players)
        assert game.challenge_mode is False and game.hint_limit == 0

    def test_starting_player_is_random(self):
        starters = {async_games.build_game('r', 's', 'A', 1, [{'id': 2, 'name': 'B'}], 24,
                                           rng=random.Random(seed))[0].current_player().name
                    for seed in range(30)}
        assert starters == {'A', 'B'}

    def test_duplicate_names_are_made_unique(self):
        game, known = async_games.build_game(
            'r', 's', 'Anna', 1, [{'id': 2, 'name': 'anna'}, {'id': 3, 'name': 'Anna'}], 24)
        assert len({p.name.casefold() for p in game.players}) == 3
        assert set(known.values()) == {1, 2, 3}

    def test_unique_name_helper(self):
        assert async_games.unique_name('Béla', {'Anna'}) == 'Béla'
        assert async_games.unique_name('Anna', {'anna'}) == 'Anna (2)'
        assert async_games.unique_name('Anna', {'Anna', 'Anna (2)'}) == 'Anna (3)'


class TestAsyncTurns:
    def test_offline_players_are_not_skipped(self):
        g = _game(('Anna', 'Béla', 'Cili'))
        g.players[1].disconnected = True
        g.pass_turn('p0')
        assert g.current_player().name == 'Béla'          # élő játékban átugornánk

    def test_live_games_still_skip_disconnected_players(self):
        g = Game('live')
        for i, name in enumerate(('Anna', 'Béla', 'Cili')):
            g.add_player(f'p{i}', name)
        g.start()
        g.players[1].disconnected = True
        g.pass_turn('p0')
        assert g.current_player().name == 'Cili'

    def test_skip_disconnected_current_is_a_noop(self):
        g = _game()
        g.players[0].disconnected = True
        assert g.skip_disconnected_current() is False and g.current_player().name == 'Anna'

    def test_deadline_restarts_every_turn(self):
        g = _game(hours=24)
        first = g.turn_deadline
        g.turn_deadline -= 3600
        g.pass_turn('p0')
        assert g.turn_deadline > first - 3600
        assert abs(g.turn_deadline - (time.time() + 24 * 3600)) < 5

    def test_live_games_have_no_deadline(self):
        g = Game('live')
        g.add_player('p', 'A')
        g.start()
        assert g.turn_deadline is None
        assert g.get_state('p')['async_mode'] is False

    def test_state_exposes_the_deadline(self):
        g = _game(hours=72)
        state = g.get_state('p0')
        assert state['async_mode'] is True and state['turn_hours'] == 72
        assert state['turn_deadline'] == g.turn_deadline


class TestExpireTurn:
    def test_timeout_passes_and_is_logged(self):
        g = _game()
        assert g.expire_turn() is True
        assert g.current_player().name == 'Béla'
        assert g.players[0].timeouts == 1
        assert g.last_action_info == {'type': 'timeout', 'player': 'Anna'}
        assert g.move_log[-1]['action_type'] == 'pass'

    def test_own_moves_reset_the_counter(self):
        g = _game()
        g.expire_turn()                    # Anna 1
        g.pass_turn('p1')
        g.pass_turn('p0')                  # saját passz nullázza
        assert g.players[0].timeouts == 0

    def test_three_timeouts_in_a_row_forfeit_the_game(self):
        g = _game(('Anna', 'Béla', 'Cili'))
        for _ in range(MAX_TIMEOUTS):
            assert not g.finished
            g.current_player_idx = 0
            g.expire_turn()
        assert g.finished and g.players[0].resigned
        assert 'Anna' not in {p.name for p in g.winners}

    def test_noop_outside_async_games_and_after_the_end(self):
        live = Game('live')
        live.add_player('p', 'A')
        live.start()
        assert live.expire_turn() is False
        g = _game()
        g.resign('p0')
        assert g.expire_turn() is False

    def test_timeouts_end_a_dead_game_through_the_scoreless_rule(self):
        g = _game()
        for _ in range(6):
            if g.finished:
                break
            g.expire_turn()
        assert g.finished


class TestResign:
    def test_resigner_never_wins(self):
        g = _game()
        g.players[0].score, g.players[1].score = 200, 10
        g.players[0].hand = g.players[1].hand = []
        ok, msg = g.resign('p0')
        assert ok and g.finished
        assert [p.name for p in g.winners] == ['Béla']
        assert g.last_action_info == {'type': 'resigned', 'player': 'Anna'}

    def test_best_remaining_player_wins_a_multiplayer_game(self):
        g = _game(('Anna', 'Béla', 'Cili'))
        for p, s in zip(g.players, (5, 50, 30)):
            p.score, p.hand = s, []
        g.resign('p1')
        assert [p.name for p in g.winners] == ['Cili']

    def test_resign_errors(self):
        g = _game()
        assert g.resign('nincs')[0] is False
        g.resign('p0')
        assert g.resign('p1') == (False, 'A játék véget ért.')
        fresh = Game('x')
        fresh.add_player('p', 'A')
        assert fresh.resign('p')[0] is False

    def test_state_marks_the_resigned_player(self):
        g = _game()
        g.resign('p0')
        assert g.get_state('p1')['players'][0]['resigned'] is True


class TestPersistence:
    def test_async_fields_survive_a_save(self):
        g = _game(hours=72)
        g.players[0].timeouts = 2
        restored = Game.from_save_dict(json.loads(json.dumps(g.to_save_dict())))
        assert restored.async_mode and restored.turn_hours == 72
        assert restored.turn_deadline == g.turn_deadline
        assert restored.players[0].timeouts == 2

    def test_resigned_flag_survives_a_save(self):
        g = _game()
        g.resign('p0')
        restored = Game.from_save_dict(g.to_save_dict())
        assert restored.players[0].resigned and [p.name for p in restored.winners] == ['Béla']

    def test_old_saves_are_not_async(self):
        data = _game().to_save_dict()
        for key in ('async_mode', 'turn_hours', 'turn_deadline'):
            del data[key]
        restored = Game.from_save_dict(data)
        assert restored.async_mode is False and restored.turn_deadline is None


# ---------------------------------------------------------------- szerver

def _registered(email, name):
    import server
    from helpers import registered_set_name_payload
    _, uid = auth.create_user(email, name, 'password123')
    c = server.socketio.test_client(server.app)
    c.emit('set_name', registered_set_name_payload(uid, name=name))
    c.get_received()
    return c, uid


def _befriend(a_id, b_id):
    auth.send_friend_request(a_id, b_id)
    auth.accept_friend_request(b_id, a_id)


def _events(client, name):
    return [e['args'][0] if e['args'] else None for e in client.get_received() if e['name'] == name]


@pytest.fixture(autouse=True)
def _isolated(monkeypatch):
    import server
    server.rate_limiter._socket_history.clear()
    server.rate_limiter._ip_history.clear()
    server.state.hidden_sids.clear()
    monkeypatch.setattr(push_service, '_spawn', None)
    # Az első lépő mindig a létrehozó, hogy a tesztek determinisztikusak legyenek
    monkeypatch.setattr(async_games, 'random', SimpleNamespace(randrange=lambda n: 0))
    yield


@pytest.fixture
def pushes(monkeypatch):
    sent = []
    monkeypatch.setattr(push_service, 'webpush', lambda info, data=None, **kw: sent.append(json.loads(data)))
    return sent


def _setup(hours=24, names=(('anna@example.com', 'Anna'), ('bela@example.com', 'Béla'))):
    """Két barát; az első létrehoz egy levelezős játékot a másodikkal. Visszatér: (a, a_id, b, b_id, room)."""
    import server
    a, a_id = _registered(*names[0])
    b, b_id = _registered(*names[1])
    _befriend(a_id, b_id)
    a.emit('create_async_game', {'name': 'Esti játék', 'friend_ids': [b_id], 'turn_hours': hours})
    a.get_received(); b.get_received()
    room = next(iter(server.state.rooms.values()))
    return a, a_id, b, b_id, room


class TestCreate:
    def test_creates_persists_and_invites(self, pushes):
        import server
        a, a_id = _registered('anna@example.com', 'Anna')
        b, b_id = _registered('bela@example.com', 'Béla')
        _befriend(a_id, b_id)
        auth.save_push_subscription(b_id, 'https://push.example.com/send/abcdef123456',
                                    'BNcRdreALRFXTkOOUHK1EtK2wtaz5Ry4YfYCA_0QTpQtUbVlUls0VJXg7A8u', 'tBHItJI5svbpez7KI4CCXg')
        a.emit('create_async_game', {'name': 'Esti játék', 'friend_ids': [b_id], 'turn_hours': 72})
        got = {e['name']: e['args'][0] for e in a.get_received() if e['args']}
        assert got['room_joined']['is_async'] is True and 'game_state' in got
        assert got['game_state']['async_mode'] and got['game_state']['turn_hours'] == 72
        room = next(iter(server.state.rooms.values()))
        assert room.is_async and room.is_private and room.db_game_id
        # tartósan mentve, mindkét játékossal
        row = auth.get_game_by_id(room.db_game_id)
        assert row['is_async'] == 1 and row['status'] == 'active'
        assert {p['user_id'] for p in auth.get_game_players(room.db_game_id)} == {a_id, b_id}
        # a meghívott értesítést kap (a kezdő játékos a létrehozó, ezért meghívót, nem „te jössz”-öt)
        invited = _events(b, 'async_invited')
        assert invited == [{'game_id': room.db_game_id, 'room_name': 'Esti játék', 'from_name': 'Anna'}]
        assert [p['body'] for p in pushes] == ['Meghívtak egy levelezős játékba: Esti játék']
        assert server.state.get_rooms_list() == []

    def test_friend_who_starts_gets_the_turn_notification_only(self, pushes, monkeypatch):
        import server
        monkeypatch.setattr(async_games, 'random', SimpleNamespace(randrange=lambda n: 1))
        a, a_id = _registered('anna@example.com', 'Anna')
        b, b_id = _registered('bela@example.com', 'Béla')
        _befriend(a_id, b_id)
        auth.save_push_subscription(b_id, 'https://push.example.com/send/abcdef123456',
                                    'BNcRdreALRFXTkOOUHK1EtK2wtaz5Ry4YfYCA_0QTpQtUbVlUls0VJXg7A8u', 'tBHItJI5svbpez7KI4CCXg')
        a.emit('create_async_game', {'name': 'Esti', 'friend_ids': [b_id]})
        a.get_received()
        assert [p['body'] for p in pushes] == ['Te jössz! · Esti']
        turn_events = _events(b, 'async_your_turn')
        assert len(turn_events) == 1 and turn_events[0]['room_name'] == 'Esti'

    @pytest.mark.parametrize('payload', [
        {'friend_ids': []}, {'friend_ids': 'x'}, {}, {'friend_ids': [1, 1]}, {'friend_ids': [True]},
        {'friend_ids': [2, 3, 4, 5]}, {'friend_ids': [999]},
    ])
    def test_validation(self, payload):
        import server
        a, a_id = _registered('anna@example.com', 'Anna')
        _registered('bela@example.com', 'Béla')
        a.emit('create_async_game', {'name': 'X', **payload})
        assert _events(a, 'error')
        assert server.state.rooms == {}

    def test_only_friends_can_be_invited(self):
        import server
        a, a_id = _registered('anna@example.com', 'Anna')
        _b, b_id = _registered('bela@example.com', 'Béla')
        a.emit('create_async_game', {'name': 'X', 'friend_ids': [b_id]})
        assert any('barátaidat' in (e or {}).get('message', '') for e in _events(a, 'error'))
        assert server.state.rooms == {}

    def test_guests_cannot_create(self):
        import server
        guest = server.socketio.test_client(server.app)
        guest.emit('set_name', {'name': 'Vendég', 'is_guest': True, 'user_id': None})
        guest.get_received()
        guest.emit('create_async_game', {'name': 'X', 'friend_ids': [1]})
        assert any('regisztrált' in (e or {}).get('message', '') for e in _events(guest, 'error'))

    def test_invalid_turn_hours_fall_back_to_the_default(self):
        a, a_id, b, b_id, room = _setup(hours=5)
        assert room.game.turn_hours == async_games.DEFAULT_TURN_HOURS

    def test_cannot_create_while_in_a_room(self):
        a, a_id, b, b_id, room = _setup()
        a.emit('create_async_game', {'name': 'X', 'friend_ids': [b_id]})
        assert any('szobában' in (e or {}).get('message', '') for e in _events(a, 'error'))


class TestPlaying:
    def test_game_survives_the_disconnect_and_the_other_player_gets_the_turn(self, pushes):
        import server
        a, a_id, b, b_id, room = _setup()
        game = room.game
        auth.save_push_subscription(b_id, 'https://push.example.com/send/abcdef123456',
                                    'BNcRdreALRFXTkOOUHK1EtK2wtaz5Ry4YfYCA_0QTpQtUbVlUls0VJXg7A8u', 'tBHItJI5svbpez7KI4CCXg')
        game.players[0].hand = list(ALMA_HAND)
        a.emit('place_tiles', {'tiles': ALMA})
        assert next(e for e in a.get_received() if e['name'] == 'action_result')['args'][0]['success']
        # Béla offline, de sorra kerül (nem ugorjuk át), és értesítést kap
        assert game.current_player().name == 'Béla'
        assert [p['body'] for p in pushes][-1] == 'Te jössz! · Esti játék'
        assert any(e['room_name'] == 'Esti játék' for e in _events(b, 'async_your_turn'))
        # a lépés azonnal mentve van
        state = json.loads(auth.get_game_by_id(room.db_game_id)['state_json'])
        assert state['current_player_idx'] == 1 and state['board'][7][6]['letter'] == 'A'
        assert len(auth.get_game_moves(room.db_game_id)) == 1

    def test_disconnecting_keeps_the_room_and_the_game(self):
        import server
        a, a_id, b, b_id, room = _setup()
        a.disconnect()
        assert room.id in server.state.rooms
        assert room.game.players[0].disconnected and room.game.started and not room.game.finished
        assert server.state.player_rooms == {}

    def test_leaving_the_view_keeps_the_game(self):
        import server
        a, a_id, b, b_id, room = _setup()
        a.emit('leave_room')
        assert any(e['name'] == 'room_left' for e in a.get_received())
        assert room.id in server.state.rooms and room.game.players[0].disconnected

    def test_open_from_the_list_as_the_invited_friend(self):
        import server
        a, a_id, b, b_id, room = _setup()
        b.emit('open_async_game', {'game_id': room.db_game_id})
        got = {e['name']: e['args'][0] for e in b.get_received() if e['args']}
        assert got['room_joined']['is_async'] and got['room_joined']['is_owner'] is False
        assert got['game_state']['async_mode'] and 'game_started' in got
        me = next(p for p in got['game_state']['players'] if p['name'] == 'Béla')
        assert 'hand' in me and len(me['hand']) == 7
        assert server.state.player_rooms[room.game.players[1].id] == room.id

    def test_both_play_through_a_full_exchange_of_moves(self):
        a, a_id, b, b_id, room = _setup()
        game = room.game
        game.players[0].hand = list(ALMA_HAND)
        a.emit('place_tiles', {'tiles': ALMA})
        a.get_received()
        b.emit('open_async_game', {'game_id': room.db_game_id})
        b.get_received()
        b.emit('pass_turn')
        assert next(e for e in b.get_received() if e['name'] == 'action_result')['args'][0]['success']
        assert game.current_player().name == 'Anna'
        assert len(auth.get_game_moves(room.db_game_id)) == 2

    def test_other_device_takes_over(self):
        import server
        a, a_id, b, b_id, room = _setup()
        b.emit('open_async_game', {'game_id': room.db_game_id})
        b.get_received()
        second = server.socketio.test_client(server.app)
        from helpers import registered_set_name_payload
        second.emit('set_name', registered_set_name_payload(b_id, name='Béla'))
        second.get_received()
        second.emit('open_async_game', {'game_id': room.db_game_id})
        got = [e['name'] for e in second.get_received()]
        assert 'room_joined' in got and 'game_state' in got
        assert any(e['name'] == 'room_left' for e in b.get_received())

    def test_open_errors(self):
        import server
        a, a_id, b, b_id, room = _setup()
        c, c_id = _registered('cili@example.com', 'Cili')
        c.emit('open_async_game', {'game_id': room.db_game_id})
        assert any('részese' in (e or {}).get('message', '') for e in _events(c, 'error'))
        c.emit('open_async_game', {'game_id': 9999})
        assert any('nem található' in (e or {}).get('message', '') for e in _events(c, 'error'))
        c.emit('open_async_game', {'game_id': 'x'})
        assert _events(c, 'error')
        # már szobában
        a.emit('open_async_game', {'game_id': room.db_game_id})
        assert any('szobában' in (e or {}).get('message', '') for e in _events(a, 'error'))

    def test_hints_are_off(self):
        a, a_id, b, b_id, room = _setup()
        a.emit('request_hint')
        assert _events(a, 'hint_result')[0]['success'] is False


class TestRestartAndLoading:
    def test_game_is_rebuilt_from_the_database(self):
        import server
        a, a_id, b, b_id, room = _setup()
        room.game.players[0].hand = list(ALMA_HAND)
        a.emit('place_tiles', {'tiles': ALMA})
        a.get_received()
        deadline = room.game.turn_deadline
        a.disconnect()
        db_id = room.db_game_id
        server.state.rooms.clear(); server.state.join_codes.clear()       # a szerver újraindult
        assert server.restore_async_games() == 1
        restored = next(iter(server.state.rooms.values()))
        assert restored.is_async and restored.db_game_id == db_id
        game = restored.game
        assert all(p.disconnected for p in game.players)
        assert game.current_player().name == 'Béla' and game.turn_deadline == deadline
        assert game.board.cells[7][6][0] == 'A'
        assert len(game.move_log) == 1 and restored.known_user_ids == {'Anna': a_id, 'Béla': b_id}
        assert restored.game.players[1].id == async_games.placeholder_id(restored.id, b_id)

    def test_restoring_twice_does_not_duplicate_rooms(self):
        import server
        a, a_id, b, b_id, room = _setup()
        assert server.restore_async_games() == 1 and len(server.state.rooms) == 1

    def test_opening_loads_a_missing_room_on_demand(self):
        import server
        a, a_id, b, b_id, room = _setup()
        db_id = room.db_game_id
        a.disconnect()
        server.state.rooms.clear(); server.state.join_codes.clear()
        b.emit('open_async_game', {'game_id': db_id})
        got = [e['name'] for e in b.get_received()]
        assert 'room_joined' in got and 'game_state' in got
        assert len(server.state.rooms) == 1

    def test_finished_games_are_not_restored(self):
        import server
        a, a_id, b, b_id, room = _setup()
        a.emit('resign_game')
        a.get_received()
        server.state.rooms.clear()
        assert server.restore_async_games() == 0


class TestDeadlines:
    def test_expired_turn_passes_and_saves(self, pushes):
        import server
        a, a_id, b, b_id, room = _setup()
        game = room.game
        a.disconnect()
        game.turn_deadline = time.time() - 1
        assert server._expire_async_turns() == 1
        assert game.current_player().name == 'Béla'
        assert game.players[0].timeouts == 1
        state = json.loads(auth.get_game_by_id(room.db_game_id)['state_json'])
        assert state['current_player_idx'] == 1 and state['turn_deadline'] > time.time()

    def test_not_yet_expired_turns_are_left_alone(self):
        import server
        a, a_id, b, b_id, room = _setup()
        assert server._expire_async_turns() == 0
        assert server._expire_async_turns(now=room.game.turn_deadline - 10) == 0

    def test_forfeit_after_repeated_timeouts_finishes_and_cleans_up(self):
        import server
        a, a_id, b, b_id, room = _setup()
        game = room.game
        a.disconnect()
        for _ in range(MAX_TIMEOUTS + 1):
            if game.finished:
                break
            game.current_player_idx = 0
            game.turn_deadline = time.time() - 1
            server._expire_async_turns()
        assert game.finished
        assert auth.get_game_by_id(room.db_game_id)['status'] == 'finished'
        assert room.id not in server.state.rooms           # senki sincs bent: a szoba megszűnik
        winners = [p for p in auth.get_game_results(room.db_game_id) if p['is_winner']]
        assert [w['player_name'] for w in winners] == ['Béla']

    def test_connected_players_see_the_timeout(self):
        import server
        a, a_id, b, b_id, room = _setup()
        room.game.turn_deadline = time.time() - 1
        server._expire_async_turns()
        state = [e for e in a.get_received() if e['name'] == 'game_state'][-1]['args'][0]
        assert state['current_player_name'] == 'Béla'
        assert state['last_action_info']['type'] == 'timeout'


class TestResign:
    def test_resigning_ends_and_records_the_result(self):
        import server
        a, a_id, b, b_id, room = _setup()
        a.emit('resign_game')
        result = next(e for e in a.get_received() if e['name'] == 'action_result')['args'][0]
        assert result['success']
        assert room.game.finished and room.game.players[0].resigned
        row = auth.get_game_by_id(room.db_game_id)
        assert row['status'] == 'finished'
        winners = [p['player_name'] for p in auth.get_game_results(room.db_game_id) if p['is_winner']]
        assert winners == ['Béla']
        assert auth.get_user_by_id(b_id)['games_won'] == 1

    def test_resign_is_only_for_correspondence_games(self):
        import server
        a, a_id = _registered('anna@example.com', 'Anna')
        a.emit('create_room', {'name': 'Élő', 'max_players': 2})
        a.get_received()
        a.emit('start_game')
        a.get_received()
        a.emit('resign_game')
        assert not any(e['name'] == 'action_result' for e in a.get_received())
        assert not next(iter(server.state.rooms.values())).game.finished


class TestListsAndRoutes:
    @pytest.fixture
    def client(self):
        import server
        server.app.config['TESTING'] = True
        return server.app.test_client()

    def _login(self, client, uid):
        client.set_cookie('session_token', auth.create_session(uid), domain='localhost')

    def test_list_marks_whose_turn_it_is(self, client):
        a, a_id, b, b_id, room = _setup()
        self._login(client, a_id)
        data = client.get('/api/async/games').get_json()
        assert data['my_turn_count'] == 1 and len(data['games']) == 1
        game = data['games'][0]
        assert game['my_turn'] is True and game['my_name'] == 'Anna' and game['current_player'] == 'Anna'
        assert game['room_name'] == 'Esti játék' and game['turn_hours'] == 24
        assert {p['name'] for p in game['players']} == {'Anna', 'Béla'}
        assert game['turn_deadline'] == room.game.turn_deadline and game['game_id'] == room.db_game_id
        other = client.application.test_client()
        self._login(other, b_id)
        assert other.get('/api/async/games').get_json()['games'][0]['my_turn'] is False

    def test_my_turn_games_come_first(self, client):
        import server
        a, a_id, b, b_id, room1 = _setup()
        a.emit('leave_room'); a.get_received()
        c, c_id = _registered('cili@example.com', 'Cili')
        _befriend(a_id, c_id)
        a.emit('create_async_game', {'name': 'Második', 'friend_ids': [c_id]})
        a.get_received()
        room2 = [r for r in server.state.rooms.values() if r.name == 'Második'][0]
        room2.game.current_player_idx = 1           # a másodikban Cili jön
        room2.game.players[0].hand = list(ALMA_HAND)
        room2.persisted_sig = None
        server._persist_async(room2)
        self._login(client, a_id)
        games = client.get('/api/async/games').get_json()['games']
        assert [g['room_name'] for g in games] == ['Esti játék', 'Második']

    def test_list_requires_login_and_hides_finished_and_foreign_games(self, client):
        a, a_id, b, b_id, room = _setup()
        assert client.get('/api/async/games').status_code == 401
        _c, c_id = _registered('cili@example.com', 'Cili')
        self._login(client, c_id)
        assert client.get('/api/async/games').get_json()['games'] == []
        a.emit('resign_game'); a.get_received()
        self._login(client, a_id)
        assert client.get('/api/async/games').get_json()['games'] == []

    def test_saved_games_and_restore_ignore_correspondence_games(self, client):
        import server
        a, a_id, b, b_id, room = _setup()
        a.emit('leave_room'); a.get_received()
        self._login(client, a_id)
        assert client.get('/api/auth/saved-games').get_json()['games'] == []
        a.emit('restore_game', {'game_id': room.db_game_id})
        assert any('Levelezős' in (e or {}).get('message', '') for e in _events(a, 'error'))

    def test_abandon_route_refuses_correspondence_games(self, client):
        a, a_id, b, b_id, room = _setup()
        self._login(client, a_id)
        res = client.post(f'/api/game/{room.db_game_id}/abandon')
        assert res.status_code == 400

    def test_correspondence_games_have_replay_and_ratings_like_any_game(self, client):
        a, a_id, b, b_id, room = _setup()
        room.game.players[0].hand = list(ALMA_HAND)
        a.emit('place_tiles', {'tiles': ALMA})
        a.get_received()
        a.emit('resign_game'); a.get_received()
        self._login(client, a_id)
        moves = client.get(f'/api/game/{room.db_game_id}/moves').get_json()
        assert moves['success'] and moves['finished'] and len(moves['moves']) == 1
        assert auth.get_user_by_id(b_id)['rating'] > 1200 > auth.get_user_by_id(a_id)['rating']
