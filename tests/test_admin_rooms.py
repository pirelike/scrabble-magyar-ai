"""Admin panel: élő szobák — lista, részletek, beavatkozások (a játék közben sem romolhat el semmi)."""
import json
import time

import pytest

import admin
import admin_live
import auth
import server
from helpers import guest_client, live_room, make_user, registered_client
from tiles import TILE_DISTRIBUTION

REASON = 'Teszt beavatkozás'


@pytest.fixture(autouse=True)
def no_background(monkeypatch):
    """Háttérfeladatok (időzítők, robotok) nem futnak: a tesztek közvetlenül hívják a függvényeiket."""
    started = []
    monkeypatch.setattr(server.socketio, 'start_background_task', lambda fn, *a, **k: started.append((fn, a, k)))
    return started


def _audit(action):
    return admin.query_audit(action=action, limit=50)['items']


def _action(api, room, **body):
    return api.post(f'/api/admin/rooms/{room.id}/action', {'reason': REASON, **body})


def _tile_total(room):
    """A zsákban, a kezekben és a táblán lévő zsetonok száma: 100 kell legyen."""
    game = room.game
    on_board = sum(1 for row in game.board.cells for cell in row if cell is not None)
    pending = len(game.pending_challenge.removed_from_hand) if game.pending_challenge else 0
    return game.bag.remaining() + sum(len(p.hand) for p in game.players) + on_board + pending


def _first_move(room, client, player_name):
    """Az első lerakás két zsetonnal (megtámadásos módban a szótár nem számít)."""
    player = next(p for p in room.game.players if p.name == player_name)
    letters = [t for t in player.hand if t][:2]
    tiles = [{'row': 7, 'col': 7 + i, 'letter': letter, 'is_blank': False} for i, letter in enumerate(letters)]
    client.emit('place_tiles', {'tiles': tiles})
    client.get_received()
    return tiles


@pytest.fixture
def duo():
    anna, bela = guest_client('Anna'), guest_client('Béla')
    return anna, bela, live_room(anna, [bela])


class TestList:
    def test_lists_rooms_with_their_state(self, api, duo):
        anna, bela, room = duo
        data = api.get('/api/admin/rooms').get_json()
        assert data['total'] == 1
        row = data['items'][0]
        assert row['code'] == room.join_code and row['status'] == 'playing' and row['kind'] == 'game'
        assert [p['name'] for p in row['players']] == ['Anna', 'Béla'] and row['owner'] == 'Anna'
        assert row['players'][0]['online'] is True and row['stuck'] is None

    def test_filters(self, api, duo):
        anna, bela, room = duo
        waiting = guest_client('Cili')
        waiting.emit('create_room', {'name': 'Várakozó'})
        waiting.get_received()
        names = lambda q: [r['name'] for r in api.get('/api/admin/rooms?' + q).get_json()['items']]  # noqa: E731
        assert names('status=waiting') == ['Várakozó'] and names('status=playing') == ['Teszt szoba']
        assert names('kind=async') == [] and names('bots=1') == []
        assert names('q=cili') == ['Várakozó'] and names(f'q={room.join_code}') == ['Teszt szoba']

    def test_robots_show_their_level(self, api):
        solo = guest_client('Anna')
        live_room(solo, ai_players=[7])
        row = api.get('/api/admin/rooms?bots=1').get_json()['items'][0]
        assert row['has_bots'] and [p['difficulty'] for p in row['players'] if p['is_bot']] == [7]

    def test_csv(self, api, duo):
        text = api.get('/api/admin/rooms?format=csv').get_data(as_text=True)
        assert text.splitlines()[0].startswith('code,id,name') and 'Anna; Béla' in text


class TestDetail:
    def test_detail_has_no_hands(self, api, duo):
        anna, bela, room = duo
        data = api.get(f'/api/admin/rooms/{room.join_code}').get_json()['room']
        assert data['id'] == room.id and len(data['board']) == 15
        assert sum(data['bag'].values()) == data['bag_count'] == 100 - 14
        assert all('hand' not in p for p in data['players'])        # csak a darabszám (hand_count) látszik
        assert not _audit('view.racks')

    def test_racks_are_logged(self, api, duo):
        anna, bela, room = duo
        racks = api.get(f'/api/admin/rooms/{room.id}/racks').get_json()['racks']
        assert [r['name'] for r in racks] == ['Anna', 'Béla'] and all(len(r['hand']) == 7 for r in racks)
        row = _audit('view.racks')[0]
        assert row['target_id'] == room.id and row['details']['code'] == room.join_code

    def test_unknown_room(self, api):
        assert api.get('/api/admin/rooms/nincs').status_code == 404

    def test_chat_has_timestamps(self, api, duo):
        anna, bela, room = duo
        anna.emit('send_chat', {'message': 'Szia'})
        chat = api.get(f'/api/admin/rooms/{room.id}').get_json()['room']['chat']
        assert chat[0]['name'] == 'Anna' and chat[0]['message'] == 'Szia'
        assert abs(chat[0]['ts'] - time.time()) < 60

    def test_pending_vote_state(self, api):
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela], challenge_mode=True)
        _first_move(room, anna, 'Anna')
        data = api.get(f'/api/admin/rooms/{room.id}').get_json()['room']
        assert data['status'] == 'voting' and data['pending']['player'] == 'Anna'
        assert data['pending']['voters'] == ['Béla'] and data['pending']['votes'] == {}


class TestSystemMessage:
    def test_message_reaches_everyone_and_the_history(self, api, duo):
        anna, bela, room = duo
        response = api.post(f'/api/admin/rooms/{room.id}/action', {'action': 'message', 'message': 'Figyelem!'})
        assert response.status_code == 200
        for client in (anna, bela):
            messages = [e['args'][0] for e in client.get_received() if e['name'] == 'chat_message']
            assert messages == [{'name': 'Rendszer', 'message': 'Figyelem!', 'system': True}]
        assert room.chat_messages[-1] == {'name': 'Rendszer', 'message': 'Figyelem!', 'system': True}
        assert _audit('room.message')[0]['details']['message'] == 'Figyelem!'

    def test_empty_or_long_message(self, api, duo):
        anna, bela, room = duo
        for text in ('', '   ', 'x' * 201):
            assert api.post(f'/api/admin/rooms/{room.id}/action',
                            {'action': 'message', 'message': text}).status_code == 400

    def test_nobody_can_pose_as_the_system(self):
        client = guest_client('Rendszer')
        assert server.state.player_names[next(iter(server.state.player_names))] == 'Névtelen'
        assert client


class TestEveryInterventionIsAudited:
    @pytest.mark.parametrize('body', [
        {'action': 'kick', 'player': 'Béla'}, {'action': 'transfer', 'player': 'Béla'}, {'action': 'skip'},
        {'action': 'save'}, {'action': 'extend', 'seconds': 60}, {'action': 'pause'}, {'action': 'resume'},
        {'action': 'reschedule_bot'}, {'action': 'resolve_vote', 'accept': True},
    ])
    def test_reason_is_required(self, api, body):
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela], turn_time_limit=60)
        before = len(admin.query_audit()['items'])
        response = api.post(f'/api/admin/rooms/{room.id}/action', body)
        assert response.status_code == 400 and response.get_json()['field'] == 'reason'
        assert len(admin.query_audit()['items']) == before
        assert [p.name for p in room.game.players] == ['Anna', 'Béla']

    def test_unknown_action(self, api, duo):
        anna, bela, room = duo
        assert _action(api, room, action='robbants').status_code == 400

    def test_the_heaviest_ones_need_sudo(self, api, duo):
        anna, bela, room = duo
        for action in ('end', 'void', 'disband'):
            response = _action(api, room, action=action)
            assert response.status_code == 401 and response.get_json()['sudo_required'] is True
        assert room.id in server.state.rooms and not room.game.finished


class TestKick:
    def test_kick_from_a_running_game(self, api):
        anna, bela, cili = guest_client('Anna'), guest_client('Béla'), guest_client('Cili')
        room = live_room(anna, [bela, cili])
        token = server.state.get_reconnect_token_for_sid(next(p.id for p in room.game.players if p.name == 'Béla'))
        assert token
        assert _action(api, room, action='kick', player='Béla').status_code == 200
        player = next(p for p in room.game.players if p.name == 'Béla')
        assert player.disconnected is True
        assert server.state.get_token_info(token) is None            # a token érvénytelen
        events = [e['name'] for e in bela.get_received()]
        assert 'room_left' in events and 'error' in events
        assert bela.get_received() == []
        assert server.state.player_rooms.get(next(iter(server.state.player_names), None)) is not None
        assert _tile_total(room) == 100
        assert _audit('room.kick')[0]['details']['player'] == 'Béla'

    def test_kicking_the_current_player_passes_the_turn(self, api):
        anna, bela, cili = guest_client('Anna'), guest_client('Béla'), guest_client('Cili')
        room = live_room(anna, [bela, cili])
        assert room.game.current_player().name == 'Anna'
        _action(api, room, action='kick', player='Anna')
        assert room.game.current_player().name == 'Béla'
        assert room.owner_name in ('Béla', 'Cili')                    # a tulajdonjog továbbszállt

    def test_kick_from_a_waiting_room_removes_the_player(self, api):
        anna, bela = guest_client('Anna'), guest_client('Béla')
        anna.emit('create_room', {'name': 'Vár'})
        anna.get_received()
        room = list(server.state.rooms.values())[-1]
        bela.emit('join_room', {'code': room.join_code})
        _action(api, room, action='kick', player='Béla')
        assert [p.name for p in room.game.players] == ['Anna']

    def test_kicking_the_last_human_closes_the_room(self, api):
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela])
        _action(api, room, action='kick', player='Béla')
        _action(api, room, action='kick', player='Anna')
        assert room.id not in server.state.rooms

    def test_unknown_player_and_bots(self, api):
        anna = guest_client('Anna')
        room = live_room(anna, ai_players=[3])
        assert _action(api, room, action='kick', player='Senki').status_code == 404
        bot = next(p.name for p in room.game.players if p.is_bot)
        assert _action(api, room, action='kick', player=bot).status_code == 404


class TestTransferOwnership:
    def test_transfer(self, api, duo):
        anna, bela, room = duo
        assert _action(api, room, action='transfer', player='Béla').status_code == 200
        assert room.owner_name == 'Béla' and room.owner == next(p.id for p in room.game.players if p.name == 'Béla')
        assert [e['args'][0] for e in bela.get_received() if e['name'] == 'room_code'] == [{'code': room.join_code}]

    def test_offline_player_cannot_be_the_owner(self, api, duo):
        anna, bela, room = duo
        bela.disconnect()
        assert _action(api, room, action='transfer', player='Béla').status_code == 409


class TestSkipTurn:
    def test_skip_marks_the_move_and_advances(self, api, duo):
        anna, bela, room = duo
        assert _action(api, room, action='skip').status_code == 200
        assert room.game.current_player().name == 'Béla'
        last = room.game.move_log[-1]
        assert last['action_type'] == 'pass' and json.loads(last['details_json'])['admin'] is True
        assert _tile_total(room) == 100
        states = [e['args'][0] for e in bela.get_received() if e['name'] == 'game_state']
        assert states[-1]['current_player'] == next(p.id for p in room.game.players if p.name == 'Béla')

    def test_not_during_a_vote(self, api):
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela], challenge_mode=True)
        _first_move(room, anna, 'Anna')
        assert _action(api, room, action='skip').status_code == 409

    def test_the_next_bot_is_scheduled(self, api, monkeypatch):
        scheduled = []
        monkeypatch.setattr(server, '_schedule_bot_turn', lambda room_id: scheduled.append(room_id))
        anna = guest_client('Anna')
        room = live_room(anna, ai_players=[3])
        _action(api, room, action='skip')
        assert room.game.current_player().is_bot and scheduled[-1] == room.id


class TestTimer:
    def test_extend_pause_resume(self, api):
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela], turn_time_limit=60)
        first_id = room.turn_timer_id
        remaining = room.turn_timer_expires_at - time.time()
        assert 55 < remaining <= 60
        assert _action(api, room, action='extend', seconds=120).status_code == 200
        assert room.turn_timer_expires_at - time.time() > remaining + 100 and room.turn_timer_id != first_id
        assert _action(api, room, action='pause').status_code == 200
        assert room.turn_timer_expires_at is None and room.timer_paused_left > 100
        assert _action(api, room, action='pause').status_code == 409
        assert api.get(f'/api/admin/rooms/{room.id}').get_json()['room']['paused'] is True
        assert _action(api, room, action='resume').status_code == 200
        assert room.timer_paused_left is None and room.turn_timer_expires_at - time.time() > 100
        assert _action(api, room, action='resume').status_code == 409

    def test_a_move_while_paused_starts_a_fresh_timer(self, api):
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela], turn_time_limit=60)
        _action(api, room, action='pause')
        anna.emit('pass_turn')
        assert room.timer_paused_left is None and room.turn_timer_expires_at is not None

    def test_no_timer_no_extension(self, api, duo):
        anna, bela, room = duo
        assert _action(api, room, action='extend', seconds=60).status_code == 409

    def test_invalid_extension(self, api):
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela], turn_time_limit=60)
        assert _action(api, room, action='extend', seconds=7).status_code == 400


class TestResolveVote:
    @pytest.fixture
    def voting(self):
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela], challenge_mode=True)
        tiles = _first_move(room, anna, 'Anna')
        assert room.game.pending_challenge
        return anna, bela, room, tiles

    def test_forced_acceptance(self, api, voting):
        anna, bela, room, tiles = voting
        assert _action(api, room, action='resolve_vote', accept=True).status_code == 200
        assert room.game.pending_challenge is None and room.game.board.cells[7][7] is not None
        assert room.game.current_player().name == 'Béla' and _tile_total(room) == 100
        assert [e['args'][0]['challenge_won'] for e in bela.get_received() if e['name'] == 'challenge_result'] == [False]

    def test_forced_rejection_returns_the_tiles(self, api, voting):
        anna, bela, room, tiles = voting
        assert _action(api, room, action='resolve_vote', accept=False).status_code == 200
        assert room.game.pending_challenge is None and room.game.board.cells[7][7] is None
        assert len(next(p for p in room.game.players if p.name == 'Anna').hand) == 7 and _tile_total(room) == 100

    def test_nothing_to_resolve(self, api, duo):
        anna, bela, room = duo
        assert _action(api, room, action='resolve_vote', accept=True).status_code == 409

    def test_needs_a_boolean(self, api, voting):
        anna, bela, room, tiles = voting
        assert _action(api, room, action='resolve_vote', accept='igen').status_code == 400


class TestBots:
    def test_reschedule(self, api, monkeypatch):
        scheduled = []
        monkeypatch.setattr(server, '_schedule_bot_turn', lambda room_id: scheduled.append(room_id))
        anna = guest_client('Anna')
        room = live_room(anna, ai_players=[3])
        assert _action(api, room, action='reschedule_bot').status_code == 409          # ember következik
        room.game.pass_turn(room.game.current_player().id)
        assert _action(api, room, action='reschedule_bot').status_code == 200
        assert scheduled[-1] == room.id and _audit('room.reschedule_bot')

    def test_a_stuck_bot_is_flagged(self, api):
        anna = guest_client('Anna')
        room = live_room(anna, ai_players=[3])
        room.game.pass_turn(room.game.current_player().id)           # a robot jön, de nincs ütemezett lépése
        assert admin_live.stuck_reason(room) == 'bot_idle'
        room.bot_scheduled_at = time.time()
        assert admin_live.stuck_reason(room) is None
        room.bot_scheduled_at = time.time() - 600
        assert admin_live.stuck_reason(room) == 'bot_idle'
        flagged = api.get('/api/admin/rooms?stuck=1').get_json()['items']
        assert [r['id'] for r in flagged] == [room.id]
        assert any(a['code'] == 'stuck_games' for a in api.get('/api/admin/overview').get_json()['alerts'])


class TestStuckDetection:
    def test_overdue_timer_and_vote(self, duo):
        anna, bela, room = duo
        assert admin_live.stuck_reason(room) is None
        room.game.turn_time_limit = 60
        room.turn_timer_expires_at = time.time() - 600
        assert admin_live.stuck_reason(room) == 'timer_overdue'
        room.turn_timer_expires_at = None
        room.last_activity = time.time() - 600
        assert admin_live.stuck_reason(room) == 'no_timer'
        room.timer_paused_left = 30                                    # a szüneteltetett időzítő nem elakadás
        assert admin_live.stuck_reason(room) is None

    def test_a_long_idle_game_without_a_time_limit(self, duo):
        anna, bela, room = duo
        room.last_activity = time.time() - 3600
        assert admin_live.stuck_reason(room) == 'idle'

    def test_overdue_challenge(self):
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela], challenge_mode=True)
        _first_move(room, anna, 'Anna')
        room.game.pending_challenge.expires_at = time.time() - 600
        assert admin_live.stuck_reason(room) == 'vote_overdue'


class TestSaveEndVoidDisband:
    def test_save_without_the_owner(self, api, duo):
        anna, bela, room = duo
        response = _action(api, room, action='save')
        assert response.status_code == 200 and response.get_json()['game_id'] == room.db_game_id
        assert auth.get_game_by_id(room.db_game_id)['status'] == 'active' and room.manually_saved is True

    def test_end_finishes_the_game_normally(self, api):
        a_id, _ = make_user('anna@example.com', 'Anna')
        b_id, _ = make_user('bela@example.com', 'Béla')
        anna, bela = registered_client(a_id, 'Anna'), registered_client(b_id, 'Béla')
        room = live_room(anna, [bela])
        room.game.players[0].score, room.game.players[1].score = 80, 20
        api.sudo()
        data = _action(api, room, action='end').get_json()
        assert data['winners'] == ['Anna'] and room.game.finished
        saved = auth.get_game_by_id(room.db_game_id)
        assert saved['status'] == 'finished'
        assert auth.get_user_by_id(a_id)['rating'] > 1200 > auth.get_user_by_id(b_id)['rating']
        assert [e['name'] for e in anna.get_received() if e['name'] == 'rating_update']
        assert [r['action'] for r in _audit('room.end')] == ['room.end']

    def test_end_returns_pending_tiles_first(self, api):
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela], challenge_mode=True)
        _first_move(room, anna, 'Anna')
        api.sudo()
        _action(api, room, action='end')
        assert room.game.finished and room.game.pending_challenge is None
        assert all(len(p.hand) == 7 for p in room.game.players)

    def test_void_removes_the_game_from_the_ranking(self, api):
        a_id, _ = make_user('anna@example.com', 'Anna')
        b_id, _ = make_user('bela@example.com', 'Béla')
        anna, bela = registered_client(a_id, 'Anna'), registered_client(b_id, 'Béla')
        room = live_room(anna, [bela])
        api.sudo()
        assert _action(api, room, action='void').status_code == 200
        assert room.id not in server.state.rooms
        game_id = _audit('room.void')[0]
        assert auth.get_game_by_id(1)['status'] == 'voided'
        assert auth.get_leaderboard('wins')[0] == []
        assert [e['name'] for e in bela.get_received() if e['name'] == 'room_disbanded']
        assert game_id

    def test_disband_with_save(self, api, duo):
        anna, bela, room = duo
        api.sudo()
        assert _action(api, room, action='disband', save=True, message='Karbantartás').status_code == 200
        assert room.id not in server.state.rooms
        messages = [e['args'][0]['message'] for e in bela.get_received() if e['name'] == 'room_disbanded']
        assert messages == ['Karbantartás']
        assert auth.get_game_by_id(1)['status'] == 'active'           # a mentés megmaradt

    def test_disband_without_save_abandons_the_game(self, api, duo):
        anna, bela, room = duo
        api.sudo()
        _action(api, room, action='disband')
        assert auth.get_game_by_id(1)['status'] == 'abandoned'

    def test_a_waiting_room_can_be_disbanded(self, api):
        anna = guest_client('Anna')
        anna.emit('create_room', {'name': 'Vár'})
        anna.get_received()
        room = list(server.state.rooms.values())[-1]
        api.sudo()
        assert _action(api, room, action='disband').status_code == 200
        assert not server.state.rooms


class TestLiveEvents:
    def test_only_an_admin_can_subscribe(self, api):
        from socket_auth import create_socket_token
        client = server.socketio.test_client(server.app)
        client.emit('admin_subscribe', {'auth_token': 'hamis'})
        assert client.get_received() == []
        client.emit('admin_subscribe')
        assert client.get_received() == []
        token = create_socket_token(server.app.config['SECRET_KEY'], api.user_id)
        client.emit('admin_subscribe', {'auth_token': token})
        names = [e['name'] for e in client.get_received()]
        assert names == ['admin_subscribed', 'admin_overview']
        assert api.user_id not in server.state.get_online_user_ids()        # nem számít online játékosnak

    def test_a_normal_user_token_is_not_enough(self, api):
        from socket_auth import create_socket_token
        uid, _ = make_user('anna@example.com', 'Anna')
        client = server.socketio.test_client(server.app)
        client.emit('admin_subscribe', {'auth_token': create_socket_token(server.app.config['SECRET_KEY'], uid)})
        assert client.get_received() == []

    def test_room_updates_reach_the_admin_room_and_watchers(self, api):
        from socket_auth import create_socket_token
        watcher = server.socketio.test_client(server.app)
        watcher.emit('admin_subscribe', {'auth_token': create_socket_token(server.app.config['SECRET_KEY'], api.user_id)})
        watcher.get_received()
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela])
        updates = [e['args'][0] for e in watcher.get_received() if e['name'] == 'admin_room_update']
        assert updates and updates[-1]['id'] == room.id
        watcher.emit('admin_watch_room', {'room_id': room.join_code})
        assert [e['name'] for e in watcher.get_received()] == ['admin_room_state']
        anna.emit('pass_turn')
        states = [e['args'][0] for e in watcher.get_received() if e['name'] == 'admin_room_state']
        assert states and states[-1]['turn_number'] == 1

    def test_watching_does_not_make_the_admin_a_spectator(self, api):
        from socket_auth import create_socket_token
        watcher = server.socketio.test_client(server.app)
        watcher.emit('admin_subscribe', {'auth_token': create_socket_token(server.app.config['SECRET_KEY'], api.user_id)})
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela])
        watcher.emit('admin_watch_room', {'room_id': room.id})
        assert room.spectators == {} and len(room.admin_watchers) == 1
        watcher.disconnect()
        assert room.admin_watchers == set()

    def test_non_admin_cannot_watch(self, api):
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela])
        bela.emit('admin_watch_room', {'room_id': room.id})
        assert room.admin_watchers == set()


class TestTileConservation:
    """Egyetlen beavatkozás sem veszíthet zsetont."""

    @pytest.mark.parametrize('action', ['skip', 'pause', 'save'])
    def test_total_tiles(self, api, action):
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela], turn_time_limit=60)
        _action(api, room, action=action)
        assert _tile_total(room) == sum(count for _l, _v, count in TILE_DISTRIBUTION)
