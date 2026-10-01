"""Regressziós tesztek a kódátvizsgálás során talált hibákra."""
import time

import pytest

from board import Board
from game import Game, HAND_SIZE
from tiles import TileBag, TILE_DISTRIBUTION

TOTAL_TILES = sum(count for _, _, count in TILE_DISTRIBUTION)


# ---------------------------------------------------------------- játéklogika

class TestTileBagDraw:
    def test_draw_zero_keeps_the_bag(self):
        bag = TileBag()
        assert bag.draw(0) == []
        assert bag.remaining() == TOTAL_TILES

    def test_draw_negative_keeps_the_bag(self):
        bag = TileBag()
        assert bag.draw(-3) == []
        assert bag.remaining() == TOTAL_TILES

    def test_draw_more_than_available_returns_the_rest(self):
        bag = TileBag()
        bag.tiles = ['A', 'B']
        assert sorted(bag.draw(5)) == ['A', 'B']
        assert bag.remaining() == 0
        assert bag.draw(3) == []


class TestDuplicatePositions:
    def test_board_rejects_two_tiles_on_one_cell(self):
        board = Board()
        valid, _, err = board.validate_placement(
            [(7, 7, 'A', False), (7, 7, 'L', False), (7, 8, 'M', False)],
            skip_dictionary=True,
        )
        assert not valid
        assert 'több zseton' in err

    def test_game_does_not_consume_tiles_on_duplicate_cell(self):
        game = Game('g')
        game.add_player('a', 'A')
        game.add_player('b', 'B')
        game.start()
        player = game.players[0]
        player.hand = ['A', 'L', 'M', 'A', 'K', 'E', 'T']
        before = list(player.hand)
        ok, msg, _ = game.place_tiles('a', [
            (7, 7, 'A', False), (7, 7, 'L', False), (7, 8, 'M', False),
        ])
        assert not ok
        assert player.hand == before
        assert game.board.is_empty


def _three_player_game(**kwargs):
    game = Game('g', **kwargs)
    for pid in ('a', 'b', 'c'):
        game.add_player(pid, pid.upper())
    game.start()
    return game


class TestPassesWithDisconnectedPlayer:
    def test_game_ends_when_all_connected_players_passed_twice(self):
        game = _three_player_game()
        game.mark_disconnected('c')
        for _ in range(10):
            if game.finished:
                break
            game.pass_turn(game.current_player().id)
        assert game.finished

    def test_game_does_not_end_after_single_pass_round(self):
        game = _three_player_game()
        game.mark_disconnected('c')
        game.pass_turn('a')
        game.pass_turn('b')
        assert not game.finished


class TestSkipDisconnectedCurrent:
    def test_advances_when_current_player_is_disconnected(self):
        game = _three_player_game()
        game.current_player_idx = 1
        game.mark_disconnected('b')
        assert game.skip_disconnected_current() is True
        assert game.current_player().id == 'c'

    def test_noop_when_current_player_is_connected(self):
        game = _three_player_game()
        assert game.skip_disconnected_current() is False
        assert game.current_player().id == 'a'

    def test_noop_when_everyone_is_disconnected(self):
        game = _three_player_game()
        for pid in 'abc':
            game.mark_disconnected(pid)
        assert game.skip_disconnected_current() is False

    def test_noop_during_pending_challenge(self):
        """Függő szavazás alatt a lezárás adja tovább a kört (különben dupla ugrás lenne)."""
        game = _three_player_game(challenge_mode=True)
        game.players[0].hand = ['A', 'L', 'M', 'A', 'K', 'E', 'T']
        ok, _, _ = game.place_tiles('a', [(7, 7, 'A', False), (7, 8, 'L', False)])
        assert ok and game.pending_challenge
        game.mark_disconnected('a')
        assert game.skip_disconnected_current() is False
        assert game.current_player().id == 'a'

    def test_accept_after_placer_disconnect_does_not_skip_a_player(self):
        game = _three_player_game(challenge_mode=True)
        game.players[0].hand = ['A', 'L', 'M', 'A', 'K', 'E', 'T']
        game.place_tiles('a', [(7, 7, 'A', False), (7, 8, 'L', False)])
        game.mark_disconnected('a')
        game.skip_disconnected_current()  # nem léptet
        game.accept_pending_by_player('b')
        game.accept_pending_by_player('c')
        assert game.current_player().id == 'b'


class TestRejectWithDisconnectedPlacer:
    def test_turn_moves_on_when_rejected_placer_is_gone(self):
        game = _three_player_game(challenge_mode=True)
        placer = game.players[0]
        placer.hand = ['A', 'L', 'M', 'A', 'K', 'E', 'T']
        game.place_tiles('a', [(7, 7, 'A', False), (7, 8, 'L', False)])
        game.mark_disconnected('a')
        game.reject_pending_by_player('b')
        game.reject_pending_by_player('c')
        assert game.pending_challenge is None
        assert sorted(placer.hand) == sorted(['A', 'L', 'M', 'A', 'K', 'E', 'T'])
        assert game.current_player().id == 'b'

    def test_placer_keeps_turn_when_still_connected(self):
        game = _three_player_game(challenge_mode=True)
        game.players[0].hand = ['A', 'L', 'M', 'A', 'K', 'E', 'T']
        game.place_tiles('a', [(7, 7, 'A', False), (7, 8, 'L', False)])
        game.reject_pending_by_player('b')
        game.reject_pending_by_player('c')
        assert game.current_player().id == 'a'


def _all_tiles(game):
    board = sum(1 for row in game.board.cells for cell in row if cell is not None)
    hands = sum(len(p.hand) for p in game.players)
    return board + hands + game.bag.remaining()


class TestSaveDuringPendingChallenge:
    def test_pending_tiles_return_to_the_placers_saved_hand(self):
        game = _three_player_game(challenge_mode=True)
        game.players[0].hand = ['A', 'L', 'M', 'A', 'K', 'E', 'T']
        game.place_tiles('a', [(7, 7, 'A', False), (7, 8, 'L', False)])
        assert len(game.players[0].hand) == 5

        saved = game.to_save_dict()
        assert len(saved['players'][0]['hand']) == 7
        restored = Game.from_save_dict(saved)
        assert sorted(restored.players[0].hand) == sorted(['A', 'L', 'M', 'A', 'K', 'E', 'T'])
        assert restored.board.is_empty
        assert restored.current_player().id == 'a'
        assert _all_tiles(restored) == TOTAL_TILES

    def test_live_game_is_not_changed_by_saving(self):
        game = _three_player_game(challenge_mode=True)
        game.players[0].hand = ['A', 'L', 'M', 'A', 'K', 'E', 'T']
        game.place_tiles('a', [(7, 7, 'A', False), (7, 8, 'L', False)])
        game.to_save_dict()
        assert len(game.players[0].hand) == 5
        assert game.pending_challenge is not None

    def test_save_without_pending_is_unchanged(self):
        game = _three_player_game()
        saved = game.to_save_dict()
        assert all(len(p['hand']) == HAND_SIZE for p in saved['players'])


# ----------------------------------------------------------------- rate limiter

class TestIpRateLimiter:
    LIMITS = {'login': (3, 60), 'register': (1, 60)}

    def _limiter(self):
        from rate_limiter import RateLimiter
        return RateLimiter({}, self.LIMITS)

    def test_blocks_after_limit(self):
        rl = self._limiter()
        results = [rl.check_ip('1.1.1.1', 'login') for _ in range(5)]
        assert results == [True, True, True, False, False]

    def test_first_request_is_counted(self):
        rl = self._limiter()
        assert rl.check_ip('1.1.1.1', 'register') is True
        assert rl.check_ip('1.1.1.1', 'register') is False

    def test_ips_and_actions_are_independent(self):
        rl = self._limiter()
        for _ in range(3):
            rl.check_ip('1.1.1.1', 'login')
        assert rl.check_ip('1.1.1.1', 'login') is False
        assert rl.check_ip('2.2.2.2', 'login') is True
        assert rl.check_ip('1.1.1.1', 'register') is True

    def test_window_expires(self, monkeypatch):
        rl = self._limiter()
        now = time.time()
        monkeypatch.setattr('rate_limiter.time.time', lambda: now)
        for _ in range(3):
            rl.check_ip('1.1.1.1', 'login')
        assert rl.check_ip('1.1.1.1', 'login') is False
        monkeypatch.setattr('rate_limiter.time.time', lambda: now + 61)
        assert rl.check_ip('1.1.1.1', 'login') is True

    def test_unknown_action_is_allowed(self):
        assert self._limiter().check_ip('1.1.1.1', 'nope') is True

    def test_old_ip_entries_are_pruned(self, monkeypatch):
        rl = self._limiter()
        monkeypatch.setattr(rl, '_IP_PRUNE_THRESHOLD', 5)
        now = time.time()
        monkeypatch.setattr('rate_limiter.time.time', lambda: now)
        for i in range(10):
            rl.check_ip(f'10.0.0.{i}', 'login')
        monkeypatch.setattr('rate_limiter.time.time', lambda: now + 3600)
        rl.check_ip('9.9.9.9', 'login')
        assert list(rl._ip_history) == ['9.9.9.9']

    def test_login_endpoint_is_rate_limited(self):
        import server
        server._ip_rate_limits.clear()
        client = server.app.test_client()
        statuses = [
            client.post('/api/auth/login',
                        json={'email': 'x@example.com', 'password': 'wrong-pass'}).status_code
            for _ in range(12)
        ]
        assert statuses[:10] == [401] * 10
        assert statuses[10:] == [429, 429]
        server._ip_rate_limits.clear()


class TestClientIp:
    def _ip(self, remote_addr, headers=None):
        import server
        from routes import _get_client_ip
        with server.app.test_request_context(
                '/', headers=headers or {}, environ_base={'REMOTE_ADDR': remote_addr}):
            return _get_client_ip()

    def test_proxy_headers_trusted_from_loopback(self):
        assert self._ip('127.0.0.1', {'CF-Connecting-IP': '8.8.8.8'}) == '8.8.8.8'
        assert self._ip('127.0.0.1', {'X-Forwarded-For': '8.8.4.4, 10.0.0.1'}) == '8.8.4.4'

    def test_proxy_headers_ignored_from_remote_clients(self):
        assert self._ip('203.0.113.9', {'X-Forwarded-For': '1.2.3.4'}) == '203.0.113.9'
        assert self._ip('203.0.113.9', {'CF-Connecting-IP': '1.2.3.4'}) == '203.0.113.9'

    def test_falls_back_to_remote_addr(self):
        assert self._ip('127.0.0.1') == '127.0.0.1'


# ------------------------------------------------------------------------- auth

class TestEmailVerification:
    def test_verified_after_correct_code(self):
        import auth
        from helpers import verify_email
        assert not auth.is_email_verified('a@example.com')
        verify_email('a@example.com')
        assert auth.is_email_verified('A@Example.com')

    def test_wrong_code_does_not_verify(self):
        import auth
        auth.create_verification_code('a@example.com')
        auth.verify_code('a@example.com', '000000')
        assert not auth.is_email_verified('a@example.com')

    def test_verification_expires(self):
        import auth
        from helpers import verify_email
        verify_email('a@example.com')
        with auth._db() as conn:
            conn.execute("UPDATE verified_emails SET expires_at = '2000-01-01 00:00:00'")
        assert not auth.is_email_verified('a@example.com')

    def test_clear_removes_verification(self):
        import auth
        from helpers import verify_email
        verify_email('a@example.com')
        auth.clear_email_verification('a@example.com')
        assert not auth.is_email_verified('a@example.com')


class TestSearchUsers:
    def test_wildcards_are_literal(self):
        import auth
        _, me = auth.create_user('me@example.com', 'Me', 'password123')
        auth.create_user('a@example.com', 'Alice', 'password123')
        auth.create_user('b@example.com', 'Bob', 'password123')
        assert auth.search_users('%%', me) == []
        assert auth.search_users('__', me) == []
        auth.create_user('c@example.com', '100%_Fan', 'password123')
        assert [u['display_name'] for u in auth.search_users('0%_F', me)] == ['100%_Fan']

    def test_accented_names_match_case_insensitively(self):
        import auth
        _, me = auth.create_user('me@example.com', 'Me', 'password123')
        auth.create_user('o@example.com', 'Ödön', 'password123')
        auth.create_user('a@example.com', 'Ágnes', 'password123')
        assert [u['display_name'] for u in auth.search_users('ödön', me)] == ['Ödön']
        assert [u['display_name'] for u in auth.search_users('ÁGN', me)] == ['Ágnes']

    def test_email_matches_only_exactly(self):
        import auth
        _, me = auth.create_user('me@example.com', 'Me', 'password123')
        _, other = auth.create_user('secret.person@example.com', 'Zed', 'password123')
        assert auth.search_users('secret.person', me) == []
        assert auth.search_users('example.com', me) == []
        assert [u['id'] for u in auth.search_users('Secret.Person@Example.com', me)] == [other]


class TestFinishGameRoomName:
    def test_room_name_saved_when_no_active_row_exists(self):
        import auth
        _, uid = auth.create_user('a@example.com', 'A', 'password123')
        game_id = auth.finish_game(
            'room-x', '{}',
            [{'player_name': 'A', 'user_id': uid, 'final_score': 10, 'is_winner': True}],
            room_name='Baráti kör',
        )
        assert auth.get_game_by_id(game_id)['room_name'] == 'Baráti kör'
        assert auth.get_user_game_history(uid)[0]['room_name'] == 'Baráti kör'


class TestSocketToken:
    def test_roundtrip(self):
        from socket_auth import create_socket_token, verify_socket_token
        token = create_socket_token('key', 7)
        assert verify_socket_token('key', token) == 7

    def test_wrong_key_rejected(self):
        from socket_auth import create_socket_token, verify_socket_token
        assert verify_socket_token('other', create_socket_token('key', 7)) is None

    def test_expired_rejected(self):
        from socket_auth import create_socket_token, verify_socket_token
        token = create_socket_token('key', 7)
        time.sleep(1.1)
        assert verify_socket_token('key', token, max_age=0) is None

    @pytest.mark.parametrize('bad', [None, '', 'garbage', 123, {'a': 1}])
    def test_garbage_rejected(self, bad):
        from socket_auth import verify_socket_token
        assert verify_socket_token('key', bad) is None

    def test_endpoint_requires_session(self):
        import server
        res = server.app.test_client().get('/api/auth/socket-token')
        assert res.status_code == 401

    def test_endpoint_returns_token_for_logged_in_user(self):
        import auth
        import server
        from socket_auth import verify_socket_token
        _, uid = auth.create_user('a@example.com', 'A', 'password123')
        client = server.app.test_client()
        client.post('/api/auth/login', json={'email': 'a@example.com', 'password': 'password123'})
        res = client.get('/api/auth/socket-token')
        assert res.status_code == 200
        assert verify_socket_token(server.app.config['SECRET_KEY'], res.get_json()['token']) == uid


# ------------------------------------------------------------ szerver / socket

@pytest.fixture(autouse=True)
def _clean_server_state():
    import server
    st = server.state
    for attr in ('rooms', 'join_codes', 'player_rooms', 'player_names', 'player_auth',
                 '_reconnect_tokens', '_sid_to_token', '_disconnected_players',
                 '_online_users', '_sid_to_user_id', '_pending_invites'):
        getattr(st, attr).clear()
    server._ip_rate_limits.clear()
    yield
    for attr in ('rooms', 'join_codes', 'player_rooms', 'player_names', 'player_auth',
                 '_reconnect_tokens', '_sid_to_token', '_disconnected_players',
                 '_online_users', '_sid_to_user_id', '_pending_invites'):
        getattr(st, attr).clear()


def _names(received):
    return [m['name'] for m in received]


def _last(received, name):
    found = [m for m in received if m['name'] == name]
    return found[-1]['args'][0] if found else None


def _guest(name):
    import server
    c = server.socketio.test_client(server.app)
    c.emit('set_name', {'name': name, 'is_guest': True, 'user_id': None})
    c.get_received()
    return c


def _registered(email, name):
    import auth
    import server
    from helpers import registered_set_name_payload
    _, uid = auth.create_user(email, name, 'password123')
    c = server.socketio.test_client(server.app)
    c.emit('set_name', registered_set_name_payload(uid, name=name))
    c.get_received()
    return c, uid


def _start_room(owner, others=(), **room_opts):
    """Szoba létrehozása, a többiek csatlakoztatása, játék indítása. Visszaadja a join kódot."""
    owner.emit('create_room', {'name': 'R', 'max_players': 4, **room_opts})
    received = owner.get_received()
    code = _last(received, 'room_code')['code']
    for other in others:
        other.emit('join_room', {'code': code})
        other.get_received()
    owner.emit('start_game')
    owner.get_received()
    for other in others:
        other.get_received()
    return code


class TestOwnerTimeoutDisband:
    def test_disband_works_outside_request_context(self):
        """A grace period háttérszálban nincs kérés-/alkalmazáskontextus — korábban
        RuntimeError-ral elszállt, és a szoba beragadt a szerver állapotában."""
        import server
        owner, _ = _registered('owner@example.com', 'Owner')
        guest = _guest('Guest')
        _start_room(owner, [guest])
        room_id = next(iter(server.state.rooms))

        owner.disconnect()
        tokens = list(server.state._disconnected_players)
        assert len(tokens) == 1
        guest.get_received()

        server._finalize_player_disconnect(tokens[0])  # kontextus nélkül, mint a háttérszál

        assert room_id not in server.state.rooms
        received = guest.get_received()
        assert 'room_disbanded' in _names(received)
        assert 'room_left' in _names(received)
        assert server.state.player_rooms == {}

    def test_disband_autosaves_the_game(self):
        import auth
        import server
        owner, uid = _registered('owner@example.com', 'Owner')
        guest = _guest('Guest')
        _start_room(owner, [guest])
        owner.disconnect()
        server._finalize_player_disconnect(list(server.state._disconnected_players)[0])
        assert len(auth.get_user_active_games(uid)) == 1 or auth.load_active_games()

    def test_transfer_ownership_works_outside_request_context(self):
        import server
        owner = _guest('Owner')
        other = _guest('Other')
        owner.emit('create_room', {'name': 'Lobby', 'max_players': 2})
        code = _last(owner.get_received(), 'room_code')['code']
        other.emit('join_room', {'code': code})
        other.get_received()
        room = next(iter(server.state.rooms.values()))
        owner_sid = next(sid for sid, n in server.player_names.items() if n == 'Owner')
        room.game.remove_player(owner_sid)  # a tulajdonos kilépett, mint a valódi folyamatban
        server._transfer_ownership(room)  # kontextus nélkül
        assert room.owner_name == 'Other'
        assert 'room_code' in _names(other.get_received())


class TestSessionTakeover:
    def test_new_connection_takes_over_a_still_live_session(self):
        """Telefon háttérbe kerül / oldal újratöltés: a szerver a régi kapcsolatot még élőnek
        hiszi, a tokennel érkező új kapcsolatnak ilyenkor is át kell vennie a helyet."""
        import server
        owner, _ = _registered('owner@example.com', 'Owner')
        guest = _guest('Guest')
        _start_room(owner, [guest])
        token = next(iter(server.state._reconnect_tokens))
        owner_token = server.state.get_reconnect_token_for_sid(
            next(sid for sid, n in server.player_names.items() if n == 'Owner'))

        fresh = server.socketio.test_client(server.app)
        fresh.emit('rejoin_room', {'token': owner_token})
        received = fresh.get_received()

        assert 'rejoin_failed' not in _names(received)
        assert 'room_joined' in _names(received)
        state = _last(received, 'game_state')
        assert state['started']
        me = next(p for p in state['players'] if p['name'] == 'Owner')
        assert not me['disconnected'] and me.get('hand')
        assert 'player_reconnected' in _names(guest.get_received())
        assert token  # (a vendég tokenje is létezik)

    def test_takeover_moves_player_rooms_to_the_new_sid(self):
        import server
        owner, _ = _registered('owner@example.com', 'Owner')
        guest = _guest('Guest')
        _start_room(owner, [guest])
        old_sid = next(sid for sid, n in server.player_names.items() if n == 'Owner')
        token = server.state.get_reconnect_token_for_sid(old_sid)

        fresh = server.socketio.test_client(server.app)
        fresh.emit('rejoin_room', {'token': token})
        fresh.get_received()

        new_sid = next(sid for sid, n in server.player_names.items() if n == 'Owner')
        assert new_sid != old_sid
        assert old_sid not in server.state.player_rooms
        assert server.state.player_rooms[new_sid] == next(iter(server.state.rooms))
        room = next(iter(server.state.rooms.values()))
        assert room.owner == new_sid
        assert old_sid not in server.state.player_auth

    def test_rejoin_from_the_same_connection_keeps_it_alive(self):
        """Ha a már bent lévő kapcsolat küldi a rejoint, ne zárja le saját magát."""
        import server
        owner, _ = _registered('owner@example.com', 'Owner')
        guest = _guest('Guest')
        _start_room(owner, [guest])
        token = server.state.get_reconnect_token_for_sid(
            next(sid for sid, n in server.player_names.items() if n == 'Owner'))
        owner.emit('rejoin_room', {'token': token})
        received = owner.get_received()
        assert owner.is_connected()
        assert 'room_joined' in _names(received)
        assert _last(received, 'game_state')['started']
        room = next(iter(server.state.rooms.values()))
        assert room.game.players[0].disconnected is False

    def test_unknown_token_still_fails(self):
        import server
        c = server.socketio.test_client(server.app)
        c.emit('rejoin_room', {'token': 'nope'})
        assert 'rejoin_failed' in _names(c.get_received())

    def test_no_takeover_for_a_game_that_has_not_started(self):
        import server
        owner = _guest('Owner')
        owner.emit('create_room', {'name': 'Lobby', 'max_players': 2})
        owner.get_received()
        token = next(iter(server.state._reconnect_tokens))
        fresh = server.socketio.test_client(server.app)
        fresh.emit('rejoin_room', {'token': token})
        assert 'rejoin_failed' in _names(fresh.get_received())


class TestRoomJoinGuards:
    def test_cannot_create_second_room_while_in_one(self):
        import server
        c = _guest('Solo')
        c.emit('create_room', {'name': 'One', 'max_players': 2})
        c.get_received()
        c.emit('create_room', {'name': 'Two', 'max_players': 2})
        assert 'error' in _names(c.get_received())
        assert len(server.state.rooms) == 1

    def test_cannot_join_another_room_while_in_one(self):
        import server
        a, b = _guest('A'), _guest('B')
        a.emit('create_room', {'name': 'One', 'max_players': 2})
        code_a = _last(a.get_received(), 'room_code')['code']
        b.emit('create_room', {'name': 'Two', 'max_players': 2})
        b.get_received()
        b.emit('join_room', {'code': code_a})
        assert 'error' in _names(b.get_received())
        room_a = next(r for r in server.state.rooms.values() if r.name == 'One')
        assert len(room_a.game.players) == 1

    def test_can_create_room_after_leaving(self):
        c = _guest('Solo')
        c.emit('create_room', {'name': 'One', 'max_players': 2})
        c.get_received()
        c.emit('leave_room')
        c.get_received()
        c.emit('create_room', {'name': 'Two', 'max_players': 2})
        assert 'room_joined' in _names(c.get_received())

    def test_duplicate_player_name_is_rejected(self):
        import server
        a, b = _guest('Béla'), _guest('béla')
        a.emit('create_room', {'name': 'One', 'max_players': 4})
        code = _last(a.get_received(), 'room_code')['code']
        b.emit('join_room', {'code': code})
        received = b.get_received()
        assert 'error' in _names(received)
        assert 'room_joined' not in _names(received)
        assert len(next(iter(server.state.rooms.values())).game.players) == 1

    @pytest.mark.parametrize('bad_code', [123456, ['1'], {'a': 1}, None, True])
    def test_non_string_code_is_a_clean_error(self, bad_code):
        c = _guest('Solo')
        c.emit('join_room', {'code': bad_code})
        assert 'error' in _names(c.get_received())


class TestLogoutEvent:
    def test_logout_clears_online_state_and_room(self):
        import server
        user, uid = _registered('u@example.com', 'User')
        user.emit('create_room', {'name': 'One', 'max_players': 2})
        user.get_received()
        assert server.state.is_user_online(uid)

        user.emit('logout')
        user.get_received()

        assert not server.state.is_user_online(uid)
        assert server.state.rooms == {}
        sid = next(iter(server.state.player_rooms), None)
        assert sid is None

    def test_relogin_as_other_user_on_same_connection_does_not_leak_presence(self):
        import auth
        import server
        from helpers import registered_set_name_payload
        c, first = _registered('first@example.com', 'First')
        _, second = auth.create_user('second@example.com', 'Second', 'password123')
        c.emit('set_name', registered_set_name_payload(second, name='Second'))
        c.get_received()
        assert server.state.is_user_online(second)
        assert not server.state.is_user_online(first)


class TestFinishedGameSavedOnce:
    def test_stats_are_not_double_counted(self):
        import auth
        import server
        owner, uid = _registered('owner@example.com', 'Owner')
        owner.emit('create_room', {'name': 'Solo', 'max_players': 2})
        owner.get_received()
        owner.emit('start_game')
        owner.get_received()
        room_id, room = next(iter(server.state.rooms.items()))
        # Játék kényszerített befejezése (egyedül: két passz)
        owner.emit('pass_turn')
        owner.emit('pass_turn')
        owner.get_received()
        assert room.game.finished
        server._save_game_to_db(room_id)
        server._save_game_to_db(room_id)
        assert auth.get_user_by_id(uid)['games_played'] == 1
        history = auth.get_user_game_history(uid)
        assert len(history) == 1
        assert history[0]['room_name'] == 'Solo'


# ------------------------------------------------------ mentés / visszaállítás

def _saved_game_with_second_player_to_move():
    """Két regisztrált játékos, a mentéskor Bob következik. Visszaadja: (alice_uid, bob_uid, game_id)."""
    import auth
    import server
    alice, alice_id = _registered('alice@example.com', 'Alice')
    bob, bob_id = _registered('bob@example.com', 'Bob')
    _start_room(alice, [bob])
    room = next(iter(server.state.rooms.values()))
    room.game.pass_turn(room.game.current_player().id)  # most Bob jön
    assert room.game.current_player().name == 'Bob'

    alice.emit('save_game')
    assert _last(alice.get_received(), 'action_result')['success']
    alice.emit('leave_room')
    alice.get_received()
    bob.disconnect()
    game_id = auth.load_active_games()[0]['id']
    return alice, alice_id, bob_id, game_id


class TestRestoreFlow:
    def test_absent_current_player_is_skipped_on_restore(self):
        import server
        alice, _, _, game_id = _saved_game_with_second_player_to_move()
        alice.emit('restore_game', {'game_id': game_id})
        alice.get_received()
        alice.emit('start_game')
        received = alice.get_received()
        state = _last(received, 'game_state')
        assert state['started']
        current = next(p for p in state['players'] if p['id'] == state['current_player'])
        assert current['name'] == 'Alice'  # nem a hiányzó Bob
        assert any(p['name'] == 'Bob' and p['disconnected'] for p in state['players'])

    def test_missing_player_can_join_the_running_game(self):
        import server
        from helpers import registered_set_name_payload
        alice, _, bob_id, game_id = _saved_game_with_second_player_to_move()
        alice.emit('restore_game', {'game_id': game_id})
        code = _last(alice.get_received(), 'room_code')['code']
        alice.emit('start_game')
        alice.get_received()

        bob = server.socketio.test_client(server.app)
        bob.emit('set_name', registered_set_name_payload(bob_id, name='Bob'))
        bob.get_received()
        bob.emit('join_room', {'code': code})
        received = bob.get_received()

        assert 'error' not in _names(received)
        assert 'room_joined' in _names(received)
        assert 'game_started' in _names(received)
        state = _last(received, 'game_state')
        bob_state = next(p for p in state['players'] if p['name'] == 'Bob')
        assert not bob_state['disconnected']
        assert len(bob_state['hand']) == 7
        assert 'player_reconnected' in _names(alice.get_received())

    def test_impostor_cannot_take_a_registered_players_seat(self):
        import server
        alice, _, _, game_id = _saved_game_with_second_player_to_move()
        alice.emit('restore_game', {'game_id': game_id})
        code = _last(alice.get_received(), 'room_code')['code']
        alice.emit('start_game')
        alice.get_received()

        impostor = _guest('Bob')
        impostor.emit('join_room', {'code': code})
        received = impostor.get_received()
        assert 'error' in _names(received)
        assert 'room_joined' not in _names(received)

        other_user, _ = _registered('mallory@example.com', 'Mallory')
        other_user.emit('set_name', {'name': 'Bob', 'is_guest': True, 'user_id': None})
        other_user.get_received()
        other_user.emit('join_room', {'code': code})
        assert 'room_joined' not in _names(other_user.get_received())

    def test_impostor_cannot_join_the_restore_lobby(self):
        alice, _, _, game_id = _saved_game_with_second_player_to_move()
        alice.emit('restore_game', {'game_id': game_id})
        code = _last(alice.get_received(), 'room_code')['code']
        impostor = _guest('Bob')
        impostor.emit('join_room', {'code': code})
        received = impostor.get_received()
        assert 'error' in _names(received)
        assert 'room_joined' not in _names(received)

    def test_restored_game_keeps_move_history_and_is_saved_again(self):
        import auth
        import server
        alice, alice_id, _, game_id = _saved_game_with_second_player_to_move()
        old_moves = auth.get_game_moves(game_id)
        assert old_moves  # volt egy passz
        alice.emit('restore_game', {'game_id': game_id})
        alice.get_received()
        alice.emit('start_game')
        alice.get_received()

        room = next(iter(server.state.rooms.values()))
        assert len(room.game.move_log) >= len(old_moves)
        assert room.db_game_id is not None and room.db_game_id != game_id
        assert [m['move_number'] for m in auth.get_game_moves(room.db_game_id)][:len(old_moves)] \
            == [m['move_number'] for m in old_moves]
        # a régi mentés lezárult, az új él és a hiányzó játékos fiókjához is tartozik
        assert auth.get_game_by_id(game_id)['status'] == 'abandoned'
        assert len(auth.get_game_players(room.db_game_id)) == 2
        assert all(gp['user_id'] for gp in auth.get_game_players(room.db_game_id))
