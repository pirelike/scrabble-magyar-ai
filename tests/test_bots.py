"""Robot (számítógépes) ellenfelek: játékmodell + szerver integráció, tipp, előnézet."""
import json
import pytest

from game import Game
from player import Player


# ---------------------------------------------------------------- játékmodell

class TestGameBots:
    def test_add_bot(self):
        game = Game('g')
        game.add_player('H', 'Human')
        ok, _ = game.add_bot('Robi', 'easy')
        assert ok
        bot = game.players[1]
        assert bot.is_bot and bot.difficulty == 'easy'
        assert bot.id.startswith('bot-')

    def test_bot_ids_are_unique(self):
        game = Game('g')
        game.add_player('H', 'Human')
        game.add_bot('A', 'easy')
        game.add_bot('B', 'hard')
        assert len({p.id for p in game.players}) == 3

    def test_cannot_exceed_four_players(self):
        game = Game('g')
        game.add_player('H', 'Human')
        for i in range(3):
            assert game.add_bot(f'B{i}', 'easy')[0]
        ok, msg = game.add_bot('Extra', 'easy')
        assert not ok and 'Maximum' in msg

    def test_cannot_add_bot_after_start(self):
        game = Game('g')
        game.add_player('H', 'Human')
        game.start()
        ok, _ = game.add_bot('Late', 'easy')
        assert not ok

    def test_human_helpers(self):
        game = Game('g')
        game.add_player('H', 'Human')
        game.add_bot('Robi', 'easy')
        assert [p.name for p in game.human_players()] == ['Human']
        assert game.has_connected_human()
        game.mark_disconnected('H')
        assert not game.has_connected_human()

    def test_player_dict_marks_bots_and_hides_their_hand(self):
        game = Game('g')
        game.add_player('H', 'Human')
        game.add_bot('Robi', 'hard')
        game.start()
        states = game.get_all_states()
        assert list(states) == ['H']  # robotnak nem készül állapot
        bot = next(p for p in states['H']['players'] if p['is_bot'])
        assert bot['difficulty'] == 'hard'
        assert 'hand' not in bot
        assert bot['hand_count'] == 7

    def test_bots_do_not_vote_in_challenge(self):
        game = Game('g', challenge_mode=True)
        game.add_player('A', 'A')
        game.add_player('B', 'B')
        game.add_bot('Robi', 'easy')
        game.start()
        game.players[0].hand = list('ALMAKÖR')
        ok, _msg, _ = game.place_tiles('A', [(7, 7 + i, ch, False) for i, ch in enumerate('ALMA')])
        assert ok and game.pending_challenge
        assert game._get_voter_ids() == {'B'}

    def test_challenge_not_applied_against_bots_only(self):
        """Robotok nem szavaznak, ezért ember ellen nincs szavazás: a szótár dönt."""
        game = Game('g', challenge_mode=True)
        game.add_player('A', 'A')
        game.add_bot('Robi', 'easy')
        game.start()
        game.players[0].hand = list('XQWÜÖÓÚ')[:7]
        ok, msg, _ = game.place_tiles('A', [(7, 7, 'Ü', False), (7, 8, 'Ö', False)])
        assert not ok and 'Érvénytelen szó' in msg
        assert game.pending_challenge is None

    def test_valid_word_against_bots_is_placed_immediately(self):
        game = Game('g', challenge_mode=True)
        game.add_player('A', 'A')
        game.add_bot('Robi', 'easy')
        game.start()
        game.players[0].hand = list('ALMAKÖR')
        ok, _msg, score = game.place_tiles('A', [(7, 7 + i, ch, False) for i, ch in enumerate('ALMA')])
        assert ok and score > 0
        assert game.pending_challenge is None
        assert game.current_player().is_bot

    def test_save_roundtrip_keeps_bots(self):
        game = Game('g')
        game.add_player('H', 'Human')
        game.add_bot('Rezső', 'hard')
        game.start()
        data = json.loads(json.dumps(game.to_save_dict()))
        restored = Game.from_save_dict(data)
        bot = restored.players[1]
        assert bot.is_bot and bot.difficulty == 'hard' and bot.name == 'Rezső'
        assert not bot.disconnected

    def test_restored_bot_is_never_disconnected(self):
        game = Game('g')
        game.add_player('H', 'Human')
        game.add_bot('Rezső', 'hard')
        game.start()
        data = game.to_save_dict()
        data['players'][1]['disconnected'] = True
        assert Game.from_save_dict(data).players[1].disconnected is False

    def test_old_saves_without_bot_fields_still_load(self):
        game = Game('g')
        game.add_player('H', 'Human')
        game.start()
        data = game.to_save_dict()
        for pd in data['players']:
            pd.pop('is_bot', None)
            pd.pop('difficulty', None)
        data.pop('last_action_info', None)
        restored = Game.from_save_dict(data)
        assert restored.players[0].is_bot is False


class TestLastActionInfo:
    def _game(self):
        game = Game('g')
        game.add_player('A', 'Anna')
        game.add_player('B', 'Béla')
        game.start()
        return game

    def test_place_sets_structured_info(self):
        game = self._game()
        game.players[0].hand = list('ALMAKÖR')
        game.place_tiles('A', [(7, 7 + i, ch, False) for i, ch in enumerate('ALMA')])
        info = game.last_action_info
        assert info['type'] == 'place' and info['player'] == 'Anna'
        assert info['words'] == ['ALMA'] and info['score'] > 0
        assert game.last_action.startswith('Anna: ALMA')

    def test_pass_and_exchange_info(self):
        game = self._game()
        game.pass_turn('A')
        assert game.last_action_info == {'type': 'pass', 'player': 'Anna'}
        game.exchange_tiles('B', [0, 1])
        assert game.last_action_info == {'type': 'exchange', 'player': 'Béla', 'count': 2}

    def test_info_is_in_state(self):
        game = self._game()
        game.pass_turn('A')
        assert game.get_state('A')['last_action_info']['type'] == 'pass'


class TestHistory:
    def test_history_lists_moves_with_scores(self):
        game = Game('g')
        game.add_player('A', 'Anna')
        game.add_player('B', 'Béla')
        game.start()
        game.players[0].hand = list('ALMAKÖR')
        game.place_tiles('A', [(7, 7 + i, ch, False) for i, ch in enumerate('ALMA')])
        game.pass_turn('B')
        history = game.get_history()
        assert [h['type'] for h in history] == ['place', 'pass']
        assert history[0]['words'] == ['ALMA'] and history[0]['player'] == 'Anna'
        assert history[0]['score'] > 0
        assert len(history[0]['tiles']) == 4

    def test_state_history_has_no_tile_positions(self):
        game = Game('g')
        game.add_player('A', 'Anna')
        game.start()
        game.players[0].hand = list('ALMAKÖR')
        game.place_tiles('A', [(7, 7 + i, ch, False) for i, ch in enumerate('ALMA')])
        state = game.get_state('A')
        assert 'tiles' not in state['history'][0]

    def test_last_move_tiles_only_after_a_placement(self):
        game = Game('g')
        game.add_player('A', 'Anna')
        game.add_player('B', 'Béla')
        game.start()
        game.players[0].hand = list('ALMAKÖR')
        game.place_tiles('A', [(7, 7 + i, ch, False) for i, ch in enumerate('ALMA')])
        assert {(t['row'], t['col']) for t in game.get_state('A')['last_move_tiles']} == {
            (7, 7), (7, 8), (7, 9), (7, 10)}
        game.pass_turn('B')
        assert game.get_state('A')['last_move_tiles'] == []

    def test_history_cache_follows_new_moves(self):
        game = Game('g')
        game.add_player('A', 'Anna')
        game.add_player('B', 'Béla')
        game.start()
        assert game.get_history() == []
        game.pass_turn('A')
        assert len(game.get_history()) == 1
        game.pass_turn('B')
        assert len(game.get_history()) == 2


class TestPreview:
    def _game(self):
        game = Game('g')
        game.add_player('A', 'Anna')
        game.add_player('B', 'Béla')
        game.start()
        game.players[0].hand = list('ALMAKÖR')
        return game

    def test_valid_preview_matches_real_score(self):
        game = self._game()
        tiles = [(7, 7 + i, ch, False) for i, ch in enumerate('ALMA')]
        preview = game.preview_placement('A', tiles)
        assert preview['valid']
        assert preview['words'][0]['word'] == 'ALMA'
        ok, _msg, score = game.place_tiles('A', tiles)
        assert ok and preview['score'] == score

    def test_preview_does_not_change_state(self):
        game = self._game()
        hand_before = list(game.players[0].hand)
        game.preview_placement('A', [(7, 7 + i, ch, False) for i, ch in enumerate('ALMA')])
        assert game.players[0].hand == hand_before
        assert game.board.is_empty
        assert game.current_player().id == 'A'

    def test_invalid_word_preview_has_message(self):
        game = self._game()
        game.players[0].hand = list('XQWÜÖÓÚ')[:7]
        preview = game.preview_placement('A', [(7, 7, 'Ü', False), (7, 8, 'Ö', False)])
        assert not preview['valid'] and 'Érvénytelen szó' in preview['message']

    def test_preview_requires_own_turn(self):
        game = self._game()
        preview = game.preview_placement('B', [(7, 7, 'A', False), (7, 8, 'L', False)])
        assert not preview['valid']

    def test_preview_rejects_missing_tile(self):
        game = self._game()
        preview = game.preview_placement('A', [(7, 7, 'Z', False), (7, 8, 'L', False)])
        assert not preview['valid'] and 'Nincs' in preview['message']

    def test_preview_includes_bonus_for_seven_tiles(self):
        game = self._game()
        game.players[0].hand = list('ALMAFÁK')
        preview = game.preview_placement('A', [(7, 4 + i, ch, False) for i, ch in enumerate('ALMAFÁK')])
        if preview['valid']:
            assert preview['score'] >= 50


# --------------------------------------------------------------- szerver

@pytest.fixture(autouse=True)
def clean_state():
    import server
    server.rooms.clear()
    server.join_codes.clear()
    server.player_rooms.clear()
    server.player_names.clear()
    server.player_auth.clear()
    server._reconnect_tokens.clear()
    server._sid_to_token.clear()
    server._disconnected_players.clear()
    server.state._online_users.clear()
    server.state._sid_to_user_id.clear()
    server.state._pending_invites.clear()
    server.state.spectator_rooms.clear()
    server.rate_limiter._socket_history.clear()
    yield
    server.rooms.clear()
    server.join_codes.clear()
    server.player_rooms.clear()
    server.player_names.clear()
    server.player_auth.clear()
    server.state.spectator_rooms.clear()


@pytest.fixture
def make_client():
    import server

    def _make(name='Alice'):
        c = server.socketio.test_client(server.app)
        c.emit('set_name', {'name': name, 'is_guest': True, 'user_id': None})
        c.get_received()
        return c
    return _make


def _events(client, name):
    return [e['args'][0] if e['args'] else None for e in client.get_received() if e['name'] == name]


def _create(client, **data):
    import server
    client.emit('create_room', {'name': 'R', 'max_players': 4, **data})
    client.get_received()
    return list(server.state.rooms.values())[-1]


def _no_bg(monkeypatch):
    """A háttérfeladatokat nem indítjuk el: a tesztek közvetlenül hívják a robotlépést."""
    import server
    started = []
    monkeypatch.setattr(server.socketio, 'start_background_task',
                        lambda fn, *a, **k: started.append(fn))
    return started


class TestCreateRoomWithBots:
    def test_creates_bots(self, make_client):
        room = _create(make_client(), ai_players=['hard', 'easy'])
        assert [p.is_bot for p in room.game.players] == [False, True, True]
        assert [p.difficulty for p in room.game.players[1:]] == ['hard', 'easy']

    def test_bot_names_are_unique_and_match_difficulty(self, make_client):
        room = _create(make_client(), ai_players=['easy', 'easy', 'easy'])
        names = [p.name for p in room.game.players]
        assert len(set(names)) == 4

    def test_bots_are_limited_by_free_seats(self, make_client):
        room = _create(make_client(), max_players=2, ai_players=['easy', 'easy', 'easy'])
        assert len(room.game.players) == 2

    def test_at_most_three_bots(self, make_client):
        room = _create(make_client(), max_players=4, ai_players=['easy'] * 9)
        assert len(room.game.players) == 4

    def test_unknown_difficulties_are_ignored(self, make_client):
        room = _create(make_client(), ai_players=['nonsense', 'hard', 5, None])
        assert [p.difficulty for p in room.game.players[1:]] == ['hard']

    def test_non_list_is_ignored(self, make_client):
        room = _create(make_client(), ai_players='hard')
        assert len(room.game.players) == 1

    def test_lobby_dict_counts_bots(self, make_client):
        room = _create(make_client(), ai_players=['medium'])
        lobby = room.to_lobby_dict()
        assert lobby['bots'] == 1 and lobby['players'] == 2

    def test_bot_name_avoids_human_name_clash(self, make_client):
        room = _create(make_client('Rita'), ai_players=['medium'])
        assert room.game.players[1].name != 'Rita'

    def test_full_room_cannot_be_joined(self, make_client):
        owner = make_client('Owner')
        room = _create(owner, max_players=2, ai_players=['easy'])
        other = make_client('Other')
        other.emit('join_room', {'code': room.join_code})
        assert any('megtelt' in (e or {}).get('message', '') for e in _events(other, 'error'))


class TestBotTurns:
    def _started_solo(self, make_client, level='hard', **kw):
        client = make_client()
        room = _create(client, ai_players=[level], **kw)
        client.emit('start_game')
        client.get_received()
        return client, room

    def test_start_schedules_nothing_when_human_starts(self, make_client, monkeypatch):
        started = _no_bg(monkeypatch)
        self._started_solo(make_client)
        assert started == []  # az első játékos az ember

    def test_pass_schedules_bot_turn(self, make_client, monkeypatch):
        started = _no_bg(monkeypatch)
        client, room = self._started_solo(make_client)
        client.emit('pass_turn')
        assert len(started) == 1

    def test_bot_plays_and_turn_returns_to_human(self, make_client, monkeypatch):
        import server
        _no_bg(monkeypatch)
        client, room = self._started_solo(make_client)
        client.emit('pass_turn')
        client.get_received()
        assert server._play_bot_turn(room.id) is True
        game = room.game
        assert game.current_player().is_bot is False
        history = game.get_history()
        assert history[-1]['player'] == game.players[1].name
        states = _events(client, 'game_state')
        assert states and states[-1]['history'][-1]['n'] == len(history)

    def test_bot_move_scores_are_added(self, make_client, monkeypatch):
        import server
        _no_bg(monkeypatch)
        client, room = self._started_solo(make_client)
        client.emit('pass_turn')
        server._play_bot_turn(room.id)
        bot = room.game.players[1]
        last = room.game.get_history()[-1]
        if last['type'] == 'place':
            assert bot.score == last['score']

    def test_stale_turn_id_is_ignored(self, make_client, monkeypatch):
        import server
        _no_bg(monkeypatch)
        client, room = self._started_solo(make_client)
        client.emit('pass_turn')
        stale = room.bot_turn_id
        room.invalidate_bot_turn()
        assert server._play_bot_turn(room.id, turn_id=stale) is False

    def test_stale_turn_number_is_ignored(self, make_client, monkeypatch):
        import server
        _no_bg(monkeypatch)
        client, room = self._started_solo(make_client)
        client.emit('pass_turn')
        assert server._play_bot_turn(room.id, turn_number=room.game.turn_number - 1) is False

    def test_bot_does_not_move_on_human_turn(self, make_client, monkeypatch):
        import server
        _no_bg(monkeypatch)
        client, room = self._started_solo(make_client)
        assert server._play_bot_turn(room.id) is False

    def test_bots_wait_while_no_human_is_connected(self, make_client, monkeypatch):
        import server
        _no_bg(monkeypatch)
        client, room = self._started_solo(make_client)
        client.emit('pass_turn')
        room.game.players[0].disconnected = True
        assert server._bot_should_move(room) is False
        room.game.players[0].disconnected = False
        assert server._bot_should_move(room) is True

    def test_bots_wait_during_challenge_vote(self, make_client, monkeypatch):
        import server
        _no_bg(monkeypatch)
        a = make_client('A')
        room = _create(a, ai_players=['easy'], challenge_mode=True)
        b = make_client('B')
        b.emit('join_room', {'code': room.join_code})
        a.emit('start_game')
        a.get_received()
        game = room.game
        game.current_player_idx = 1  # a robot jön (A, robot, B)
        game.pending_challenge = object()
        assert server._bot_should_move(room) is False
        game.pending_challenge = None

    def test_full_game_with_passing_human_finishes_and_is_saved(self, make_client, monkeypatch):
        import server
        import auth
        _no_bg(monkeypatch)
        client, room = self._started_solo(make_client, level='hard')
        for _ in range(250):
            if room.game.finished:
                break
            if room.game.current_player().is_bot:
                assert server._play_bot_turn(room.id)
            else:
                server.rate_limiter._socket_history.clear()
                client.emit('pass_turn')
        assert room.game.finished
        assert room.result_saved
        assert room.game.winner is not None

    def test_bot_move_in_challenge_mode_starts_vote_for_humans(self, make_client, monkeypatch):
        import server
        started = _no_bg(monkeypatch)
        a = make_client('A')
        room = _create(a, ai_players=['hard'], challenge_mode=True)
        b = make_client('B')
        b.emit('join_room', {'code': room.join_code})
        a.emit('start_game')
        a.get_received()
        game = room.game
        # a sorrend A, robot, B: most a robot jön
        game.current_player_idx = 1
        game.turn_number += 1
        assert server._play_bot_turn(room.id) is True
        if game.pending_challenge is not None:
            # a robot lerakása szavazásra vár, a szavazók az emberek
            assert game._get_voter_ids() == {p.id for p in game.human_players()}
        else:
            # különben passzolt / cserélt (nincs lerakás)
            assert game.get_history()[-1]['type'] in ('pass', 'exchange')

    def test_bot_turn_scheduled_after_challenge_resolved(self, make_client, monkeypatch):
        import server
        started = _no_bg(monkeypatch)
        a = make_client('A')
        room = _create(a, ai_players=['easy'], challenge_mode=True)
        b = make_client('B')
        b.emit('join_room', {'code': room.join_code})
        a.emit('start_game')
        a.get_received()
        game = room.game
        game.players[0].hand = list('ALMAKÖR')
        a.emit('place_tiles', {'tiles': [{'row': 7, 'col': 7 + i, 'letter': ch, 'is_blank': False}
                                         for i, ch in enumerate('ALMA')]})
        assert game.pending_challenge
        started.clear()
        b.emit('accept_words')
        # elfogadás után a robot jön (sorrend: A, robot, B) — a következő robotlépés ütemeződik
        assert game.pending_challenge is None
        assert game.current_player().is_bot
        assert len(started) == 1

    def test_leaving_waiting_room_with_only_bots_left_removes_room(self, make_client):
        import server
        client = make_client()
        room = _create(client, ai_players=['easy', 'hard'])
        client.emit('leave_room')
        assert room.id not in server.state.rooms

    def test_disconnect_in_waiting_room_removes_bot_room(self, make_client):
        import server
        client = make_client()
        room = _create(client, ai_players=['easy'])
        client.disconnect()
        assert room.id not in server.state.rooms

    def test_bots_are_not_sent_states(self, make_client, monkeypatch):
        _no_bg(monkeypatch)
        client, room = self._started_solo(make_client)
        client.emit('pass_turn')
        states = _events(client, 'game_state')
        assert states


class TestRestoreWithBots:
    def test_expected_players_exclude_bots(self, make_client, monkeypatch):
        import auth
        import server
        from helpers import registered_set_name_payload
        _no_bg(monkeypatch)
        _, uid = auth.create_user('boter@example.com', 'Boter', 'password123')
        c = server.socketio.test_client(server.app)
        c.emit('set_name', registered_set_name_payload(uid))
        c.get_received()
        room = _create(c, ai_players=['hard'])
        c.emit('start_game')
        c.get_received()
        room.manually_saved = True
        server._save_game_to_db(room.id)
        c.emit('leave_room')
        c.get_received()
        saved = auth.get_user_active_games(uid)
        assert saved
        c.emit('restore_game', {'game_id': saved[0]['game_id']})
        events = c.get_received()
        joined = next(e['args'][0] for e in events if e['name'] == 'room_joined')
        assert joined['is_restore_lobby']
        assert joined['expected_players'] == ['Boter']
        new_room = list(server.state.rooms.values())[-1]
        assert new_room.max_players == 2
        c.emit('start_game')
        c.get_received()
        game = new_room.game
        assert [p.is_bot for p in game.players] == [False, True]
        assert not game.players[1].disconnected
        assert new_room.late_join_names == {}


class TestHintEvent:
    def _solo(self, make_client, monkeypatch):
        _no_bg(monkeypatch)
        client = make_client()
        room = _create(client, ai_players=['easy'])
        client.emit('start_game')
        client.get_received()
        return client, room

    def test_hint_in_solo_game(self, make_client, monkeypatch):
        client, room = self._solo(make_client, monkeypatch)
        client.emit('request_hint')
        results = _events(client, 'hint_result')
        assert results and results[0]['success']
        for move in results[0]['moves']:
            assert move['score'] > 0 and move['tiles'] and move['words']

    def test_hint_move_is_playable(self, make_client, monkeypatch):
        client, room = self._solo(make_client, monkeypatch)
        client.emit('request_hint')
        moves = _events(client, 'hint_result')[0]['moves']
        if moves:
            client.emit('place_tiles', {'tiles': moves[0]['tiles']})
            result = [r for r in _events(client, 'action_result')][0]
            assert result['success'] and result['score'] == moves[0]['score']

    def test_hint_refused_with_other_humans(self, make_client, monkeypatch):
        _no_bg(monkeypatch)
        a = make_client('A')
        room = _create(a, ai_players=['easy'])
        b = make_client('B')
        b.emit('join_room', {'code': room.join_code})
        a.emit('start_game')
        a.get_received()
        a.emit('request_hint')
        result = _events(a, 'hint_result')[0]
        assert not result['success'] and 'egyedül' in result['message']

    def test_hint_refused_when_not_your_turn(self, make_client, monkeypatch):
        client, room = self._solo(make_client, monkeypatch)
        room.game.current_player_idx = 1
        client.emit('request_hint')
        assert not _events(client, 'hint_result')[0]['success']

    def test_hint_refused_before_start(self, make_client, monkeypatch):
        _no_bg(monkeypatch)
        client = make_client()
        _create(client, ai_players=['easy'])
        client.emit('request_hint')
        assert not _events(client, 'hint_result')[0]['success']

    def test_hint_is_rate_limited(self, make_client, monkeypatch):
        client, room = self._solo(make_client, monkeypatch)
        for _ in range(5):
            client.emit('request_hint')
        results = _events(client, 'hint_result')
        assert any('Túl sok' in r.get('message', '') for r in results)


class TestPreviewEvent:
    def _room(self, make_client):
        client = make_client()
        room = _create(client, ai_players=['easy'])
        client.emit('start_game')
        client.get_received()
        room.game.players[0].hand = list('ALMAKÖR')
        return client, room

    def test_preview_event_returns_score(self, make_client, monkeypatch):
        _no_bg(monkeypatch)
        client, room = self._room(make_client)
        tiles = [{'row': 7, 'col': 7 + i, 'letter': ch, 'is_blank': False} for i, ch in enumerate('ALMA')]
        client.emit('preview_move', {'tiles': tiles})
        preview = _events(client, 'move_preview')[0]
        assert preview['valid'] and preview['words'][0]['word'] == 'ALMA'
        client.emit('place_tiles', {'tiles': tiles})
        result = _events(client, 'action_result')[0]
        assert result['score'] == preview['score']

    def test_preview_of_garbage_is_invalid(self, make_client, monkeypatch):
        _no_bg(monkeypatch)
        client, room = self._room(make_client)
        client.emit('preview_move', {'tiles': [{'row': 99, 'col': 0, 'letter': 'A'}]})
        assert not _events(client, 'move_preview')[0]['valid']

    def test_preview_without_room_is_silent(self, make_client):
        client = make_client()
        client.emit('preview_move', {'tiles': []})
        assert _events(client, 'move_preview') == []

    def test_preview_is_rate_limited_silently(self, make_client, monkeypatch):
        _no_bg(monkeypatch)
        client, room = self._room(make_client)
        tiles = [{'row': 7, 'col': 7, 'letter': 'A', 'is_blank': False},
                 {'row': 7, 'col': 8, 'letter': 'L', 'is_blank': False}]
        for _ in range(45):
            client.emit('preview_move', {'tiles': tiles})
        received = client.get_received()
        assert len([e for e in received if e['name'] == 'move_preview']) <= 30
        assert not [e for e in received if e['name'] == 'error']


class TestRejectedPlacementsAreNotRepeated:
    """Ha a szavazás elutasít egy robotlépést, a robot nem rakja le ugyanazt újra (végtelen ciklus ellen)."""

    def test_rejection_is_remembered_and_cleared_by_the_next_placement(self):
        game = Game('g', challenge_mode=True)
        game.add_player('A', 'Anna')
        game.add_player('B', 'Béla')
        game.start()
        game.players[0].hand = list('ALMAKÖR')
        tiles = [(7, 7 + i, ch, False) for i, ch in enumerate('ALMA')]
        game.place_tiles('A', tiles)
        game.reject_pending_by_player('B')
        assert frozenset(tiles) in game.rejected_placements
        # a következő sikeres lerakás (a tábla változik) törli
        game.place_tiles('A', tiles)
        game.accept_pending_by_player('B')
        assert game.rejected_placements == set()

    def test_choose_action_skips_avoided_moves(self):
        import random
        import ai_player
        from board import Board
        rack = list('ALMAKÖR')
        first = ai_player.choose_action(Board(), rack, 'hard', 80, rng=random.Random(1))
        assert first['action'] == 'place'
        again = ai_player.choose_action(Board(), rack, 'hard', 80, rng=random.Random(1),
                                        avoid={frozenset(first['tiles'])})
        assert again['action'] != 'place' or frozenset(again['tiles']) != frozenset(first['tiles'])

    def test_bot_plays_something_else_after_rejection(self, make_client, monkeypatch):
        import server
        _no_bg(monkeypatch)
        a = make_client('A')
        room = _create(a, ai_players=['hard'], challenge_mode=True)
        b = make_client('B')
        b.emit('join_room', {'code': room.join_code})
        a.emit('start_game')
        a.get_received()
        game = room.game
        game.current_player_idx = 1  # a robot jön
        game.turn_number += 1
        assert server._play_bot_turn(room.id)
        if game.pending_challenge is None:
            return  # a robot passzolt / cserélt: nincs mit elutasítani
        rejected = frozenset(game.pending_challenge.tiles_placed)
        # a szavazók: A és B — mindketten elutasítják
        a.emit('reject_words')
        b.emit('reject_words')
        assert rejected in game.rejected_placements
        # újra a robot jön (a lerakás elutasítva) — ne ugyanazt rakja
        assert game.current_player().is_bot
        assert server._play_bot_turn(room.id)
        if game.pending_challenge is not None:
            assert frozenset(game.pending_challenge.tiles_placed) != rejected


class TestHintFailure:
    def test_engine_error_is_reported_without_crashing(self, make_client, monkeypatch):
        import ai_player
        _no_bg(monkeypatch)
        client = make_client()
        _create(client, ai_players=['easy'])
        client.emit('start_game')
        client.get_received()

        def boom(*args, **kwargs):
            raise RuntimeError('nincs szótár')
        monkeypatch.setattr(ai_player, 'best_moves', boom)
        client.emit('request_hint')
        result = _events(client, 'hint_result')[0]
        assert result['success'] is False and 'tippet' in result['message']

    def test_bot_engine_error_falls_back_to_pass(self, make_client, monkeypatch):
        import ai_player
        import server
        _no_bg(monkeypatch)
        client = make_client()
        room = _create(client, ai_players=['easy'])
        client.emit('start_game')
        client.emit('pass_turn')

        def boom(*args, **kwargs):
            raise RuntimeError('nincs szótár')
        monkeypatch.setattr(ai_player, 'choose_action', boom)
        assert server._play_bot_turn(room.id) is True
        assert room.game.get_history()[-1]['type'] == 'pass'
        assert room.game.current_player().is_bot is False


# ------------------------------------------------------------- tipp-limit

class TestHintLimitModel:
    def test_default_limit(self):
        game = Game('g')
        assert game.hint_limit == 3 and game.hints_used == 0 and game.hints_left() == 3

    @pytest.mark.parametrize('limit', [0, 1, 3, 5, 10])
    def test_allowed_limits(self, limit):
        assert Game('g', hint_limit=limit).hint_limit == limit

    @pytest.mark.parametrize('limit', [-1, 2, 7, 99, None, 'x'])
    def test_invalid_limit_falls_back_to_default(self, limit):
        assert Game('g', hint_limit=limit).hint_limit == 3

    def test_use_hint_counts_down(self):
        game = Game('g', hint_limit=1)
        assert game.use_hint() is True
        assert game.hints_left() == 0
        assert game.use_hint() is False
        assert game.hints_used == 1

    def test_disabled_hints_cannot_be_used(self):
        game = Game('g', hint_limit=0)
        assert game.hints_left() == 0 and game.use_hint() is False

    def test_state_exposes_limit_and_remaining(self):
        game = Game('g', hint_limit=5)
        game.add_player('A', 'A')
        game.start()
        game.use_hint()
        state = game.get_state('A')
        assert state['hint_limit'] == 5 and state['hints_left'] == 4

    def test_save_roundtrip_keeps_used_hints(self):
        game = Game('g', hint_limit=5)
        game.add_player('A', 'A')
        game.start()
        game.use_hint()
        game.use_hint()
        restored = Game.from_save_dict(json.loads(json.dumps(game.to_save_dict())))
        assert restored.hint_limit == 5 and restored.hints_used == 2 and restored.hints_left() == 3

    def test_old_saves_without_hint_fields_get_the_default(self):
        game = Game('g')
        game.add_player('A', 'A')
        game.start()
        data = game.to_save_dict()
        data.pop('hint_limit')
        data.pop('hints_used')
        restored = Game.from_save_dict(data)
        assert restored.hint_limit == 3 and restored.hints_used == 0

    def test_corrupt_saved_counter_is_tolerated(self):
        game = Game('g')
        game.add_player('A', 'A')
        data = game.to_save_dict()
        data['hints_used'] = -4
        assert Game.from_save_dict(data).hints_used == 0


class TestHintLimitRoom:
    def test_default_when_not_specified(self, make_client):
        room = _create(make_client(), ai_players=['easy'])
        assert room.game.hint_limit == 3

    @pytest.mark.parametrize('limit', [0, 1, 5, 10])
    def test_room_setting(self, make_client, limit):
        room = _create(make_client(), ai_players=['easy'], hint_limit=limit)
        assert room.game.hint_limit == limit

    @pytest.mark.parametrize('bad', [2, -1, 100, 'abc', None, [], {}, 3.5])
    def test_invalid_values_fall_back_to_default(self, make_client, bad):
        room = _create(make_client(), ai_players=['easy'], hint_limit=bad)
        assert room.game.hint_limit == 3  # (a 3.5 egészre vágva 3, ami engedélyezett)

    def test_numeric_string_is_accepted(self, make_client):
        room = _create(make_client(), ai_players=['easy'], hint_limit='5')
        assert room.game.hint_limit == 5

    def test_room_joined_carries_the_limit(self, make_client):
        client = make_client()
        client.emit('create_room', {'name': 'R', 'ai_players': ['easy'], 'hint_limit': 1})
        joined = _events(client, 'room_joined')[0]
        assert joined['hint_limit'] == 1

    def test_other_players_get_the_limit_on_join(self, make_client):
        owner = make_client('Owner')
        room = _create(owner, hint_limit=5)
        other = make_client('Other')
        other.emit('join_room', {'code': room.join_code})
        assert _events(other, 'room_joined')[0]['hint_limit'] == 5


class TestHintLimitEvent:
    def _solo(self, make_client, monkeypatch, limit):
        _no_bg(monkeypatch)
        import server
        server.rate_limiter._socket_history.clear()
        client = make_client()
        room = _create(client, ai_players=['easy'], hint_limit=limit)
        client.emit('start_game')
        client.get_received()
        return client, room

    def test_disabled_hints_are_refused(self, make_client, monkeypatch):
        client, room = self._solo(make_client, monkeypatch, 0)
        client.emit('request_hint')
        result = _events(client, 'hint_result')[0]
        assert result['success'] is False and 'ki vannak kapcsolva' in result['message']
        assert room.game.hints_used == 0

    def test_hints_are_counted_and_run_out(self, make_client, monkeypatch):
        client, room = self._solo(make_client, monkeypatch, 1)
        client.emit('request_hint')
        first = _events(client, 'hint_result')[0]
        assert first['success'] and first['moves'] and first['hints_left'] == 0
        assert room.game.hints_left() == 0
        client.emit('request_hint')
        second = _events(client, 'hint_result')[0]
        assert second['success'] is False and 'Elfogytak' in second['message']
        assert second['hints_left'] == 0

    def test_state_is_refreshed_after_a_hint(self, make_client, monkeypatch):
        client, room = self._solo(make_client, monkeypatch, 3)
        client.emit('request_hint')
        states = _events(client, 'game_state')
        assert states and states[-1]['hints_left'] == 2

    def test_hint_without_moves_does_not_cost_a_hint(self, make_client, monkeypatch):
        import ai_player
        client, room = self._solo(make_client, monkeypatch, 3)
        monkeypatch.setattr(ai_player, 'best_moves', lambda *a, **k: [])
        client.emit('request_hint')
        result = _events(client, 'hint_result')[0]
        assert result['success'] and result['moves'] == [] and result['hints_left'] == 3
        assert room.game.hints_used == 0

    def test_hints_used_while_searching_are_not_double_spent(self, make_client, monkeypatch):
        """Ha a keresés közben elfogy a tipp (másik kérés), nem adunk ki többet a limitnél."""
        import ai_player
        client, room = self._solo(make_client, monkeypatch, 1)
        real = ai_player.best_moves

        def slow(*args, **kwargs):
            moves = real(*args, **kwargs)
            room.game.use_hint()   # közben egy másik kérés elhasználta
            return moves
        monkeypatch.setattr(ai_player, 'best_moves', slow)
        client.emit('request_hint')
        result = _events(client, 'hint_result')[0]
        assert result['success'] is False and 'Elfogytak' in result['message']
        assert room.game.hints_used == 1

    def test_limit_is_per_game(self, make_client, monkeypatch):
        client_a, room_a = self._solo(make_client, monkeypatch, 1)
        client_a.emit('request_hint')
        client_b, room_b = self._solo(make_client, monkeypatch, 1)
        client_b.emit('request_hint')
        assert _events(client_b, 'hint_result')[0]['success'] is True
        assert room_b.game.hints_used == 1

    def test_hint_limit_survives_a_restore(self, make_client, monkeypatch):
        import auth
        import server
        from helpers import registered_set_name_payload
        _no_bg(monkeypatch)
        _, uid = auth.create_user('hinter@example.com', 'Hinter', 'password123')
        c = server.socketio.test_client(server.app)
        c.emit('set_name', registered_set_name_payload(uid))
        c.get_received()
        room = _create(c, ai_players=['easy'], hint_limit=5)
        c.emit('start_game')
        c.get_received()
        server.rate_limiter._socket_history.clear()
        c.emit('request_hint')
        c.get_received()
        assert room.game.hints_used == 1
        room.manually_saved = True
        server._save_game_to_db(room.id)
        c.emit('leave_room')
        c.get_received()
        saved = auth.get_user_active_games(uid)
        c.emit('restore_game', {'game_id': saved[0]['game_id']})
        joined = _events(c, 'room_joined')[0]
        assert joined['hint_limit'] == 5
        c.emit('start_game')
        c.get_received()
        new_room = list(server.state.rooms.values())[-1]
        assert new_room.game.hint_limit == 5 and new_room.game.hints_used == 1
        assert new_room.game.hints_left() == 4
