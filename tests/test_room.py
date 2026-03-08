"""Tests for room.py - Room class and generate_join_code."""
import pytest
from unittest.mock import MagicMock


class TestRoom:
    def _make_room(self, **kwargs):
        from room import Room
        game = MagicMock()
        game.players = []
        game.started = False
        game.finished = False
        game.challenge_mode = False
        defaults = dict(
            room_id='room-1',
            game=game,
            owner_sid='sid-owner',
            owner_name='Owner',
            name='TestRoom',
            max_players=4,
            join_code='123456',
            owner_token='token-owner',
        )
        defaults.update(kwargs)
        return Room(**defaults)

    def test_init_defaults(self):
        room = self._make_room()
        assert room.id == 'room-1'
        assert room.owner == 'sid-owner'
        assert room.owner_name == 'Owner'
        assert room.name == 'TestRoom'
        assert room.max_players == 4
        assert room.join_code == '123456'
        assert room.is_private is False
        assert room.is_restored is False
        assert room.chat_messages == []

    def test_init_private(self):
        room = self._make_room(is_private=True)
        assert room.is_private is True

    def test_add_chat_message(self):
        room = self._make_room()
        room.add_chat_message('Alice', 'Hello')
        assert len(room.chat_messages) == 1
        assert room.chat_messages[0] == {'name': 'Alice', 'message': 'Hello'}

    def test_add_chat_message_multiple(self):
        room = self._make_room()
        room.add_chat_message('Alice', 'Hi')
        room.add_chat_message('Bob', 'Hey')
        assert len(room.chat_messages) == 2
        assert room.chat_messages[0]['name'] == 'Alice'
        assert room.chat_messages[1]['name'] == 'Bob'

    def test_chat_message_limit(self):
        room = self._make_room()
        for i in range(110):
            room.add_chat_message('User', f'msg-{i}')
        assert len(room.chat_messages) == 100
        # The oldest messages should be trimmed
        assert room.chat_messages[0]['message'] == 'msg-10'
        assert room.chat_messages[-1]['message'] == 'msg-109'

    def test_invalidate_challenge_timer(self):
        room = self._make_room()
        assert room.challenge_timer_id == 0
        new_id = room.invalidate_challenge_timer()
        assert new_id == 1
        assert room.challenge_timer_id == 1
        new_id2 = room.invalidate_challenge_timer()
        assert new_id2 == 2

    def test_transfer_ownership(self):
        room = self._make_room()
        assert room.owner == 'sid-owner'
        assert room.owner_name == 'Owner'
        room.transfer_ownership('sid-new', 'NewOwner')
        assert room.owner == 'sid-new'
        assert room.owner_name == 'NewOwner'

    def test_to_lobby_dict(self):
        from unittest.mock import MagicMock
        game = MagicMock()
        game.players = [MagicMock(), MagicMock()]
        game.started = True
        game.finished = False
        game.challenge_mode = True
        room = self._make_room(game=game)
        room.is_restored = True

        d = room.to_lobby_dict()
        assert d['id'] == 'room-1'
        assert d['name'] == 'TestRoom'
        assert d['players'] == 2
        assert d['max_players'] == 4
        assert d['started'] is True
        assert d['finished'] is False
        assert d['owner'] == 'Owner'
        assert d['challenge_mode'] is True
        assert d['is_restored'] is True

    def test_to_lobby_dict_no_code(self):
        """Lobby dict should not expose the join code."""
        room = self._make_room()
        d = room.to_lobby_dict()
        assert 'join_code' not in d
        assert 'code' not in d


class TestGenerateJoinCode:
    def test_generates_6_digit_code(self):
        from room import generate_join_code
        code = generate_join_code(set())
        assert len(code) == 6
        assert code.isdigit()

    def test_avoids_existing_codes(self):
        from room import generate_join_code
        existing = {f'{i:06d}' for i in range(999990)}
        code = generate_join_code(existing)
        assert code not in existing
        assert len(code) == 6

    def test_unique_codes(self):
        from room import generate_join_code
        codes = set()
        for _ in range(50):
            code = generate_join_code(codes)
            assert code not in codes
            codes.add(code)
        assert len(codes) == 50
