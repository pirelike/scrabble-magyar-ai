"""Kétjegyű betűk (SZ, CS, GY, LY, NY, TY, ZS): csak a saját zsetonjukkal rakhatók ki, külön S + Z-ből nem."""
import pytest

from ai_player import Vocabulary, generate_moves
from board import Board, CENTER
from game import Game
from tiles import DIGRAPHS, forms_digraph

SPLITS = [(d[0], d[1]) for d in sorted(DIGRAPHS)]


def _row(letters, row=CENTER, start=CENTER):
    """Egy sor zseton: a `letters` elemei egy-egy zseton (a kétjegyű betű egy elem)."""
    return [(row, start + i, letter, False) for i, letter in enumerate(letters)]


class TestFormsDigraph:
    def test_the_seven_digraphs(self):
        assert sorted(SPLITS) == [('C', 'S'), ('G', 'Y'), ('L', 'Y'), ('N', 'Y'), ('S', 'Z'), ('T', 'Y'), ('Z', 'S')]
        for first, second in SPLITS:
            assert forms_digraph(first, second)

    def test_ordinary_pairs_and_digraph_tiles_do_not_count(self):
        assert not forms_digraph('A', 'S')
        assert not forms_digraph('S', 'A')
        assert not forms_digraph('Z', 'Z')
        assert not forms_digraph('S', 'SZ')      # a második egy kétjegyű zseton
        assert not forms_digraph('Z', 'SZ')
        assert not forms_digraph('SZ', 'Z')
        assert not forms_digraph('', 'Z')        # a szó elején nincs előző zseton


class TestBoardRejectsSplitDigraphs:
    @pytest.mark.parametrize('first,second', SPLITS)
    def test_two_new_single_tiles(self, first, second):
        valid, _words, error = Board().validate_placement(_row(['A', first, second, 'A']), skip_dictionary=True)
        assert valid is False
        assert first + second in error and 'saját zsetonjával' in error

    @pytest.mark.parametrize('digraph', sorted(DIGRAPHS))
    def test_the_digraph_tile_itself_is_fine(self, digraph):
        valid, words, _error = Board().validate_placement(_row(['A', digraph, 'A']), skip_dictionary=True)
        assert valid is True
        assert words[0][0] == 'A' + digraph + 'A'

    def test_new_tile_next_to_an_old_one(self):
        board = Board()
        board.set(CENTER, CENTER, 'S')
        board.is_empty = False
        valid, _w, error = board.validate_placement([(CENTER, CENTER + 1, 'Z', False)], skip_dictionary=True)
        assert valid is False and 'SZ' in error

    def test_cross_word_pairs_are_checked_too(self):
        board = Board()
        board.set(CENTER, CENTER, 'S')
        board.is_empty = False
        # a függőleges lerakás egyetlen betűje (a vízszintes keresztszóban) Z-vel zárul az S után
        valid, _w, error = board.validate_placement(
            [(CENTER - 1, CENTER + 1, 'A', False), (CENTER, CENTER + 1, 'Z', False)], skip_dictionary=True)
        assert valid is False and 'SZ' in error

    def test_blank_tile_counts_as_its_letter(self):
        valid, _w, error = Board().validate_placement(
            [(CENTER, CENTER, 'S', True), (CENTER, CENTER + 1, 'Z', False)], skip_dictionary=True)
        assert valid is False and 'SZ' in error

    def test_single_tile_beside_a_digraph_tile_is_allowed(self):
        # vízszsint: Z + SZ, asszony: S + SZ — az egyik zseton kétjegyű, tehát nem két külön zseton alkotja
        for letters in (['Z', 'SZ', 'I'], ['A', 'S', 'SZ', 'O', 'NY'], ['ZS', 'Z', 'A']):
            valid, _w, error = Board().validate_placement(_row(letters), skip_dictionary=True)
            assert valid is True, (letters, error)

    def test_old_pairs_do_not_block_the_game(self):
        """A régi (megengedőbb) szabállyal lerakott S + Z pár az állásban marad, a lerakás folytatható."""
        board = Board()
        board.set(CENTER, CENTER, 'S')
        board.set(CENTER, CENTER + 1, 'Z')
        board.is_empty = False
        valid, words, error = board.validate_placement(
            [(CENTER, CENTER + 2, 'A', False)], skip_dictionary=True)
        assert valid is True, error
        assert words[0][0] == 'SZA'

    def test_dictionary_still_decides_the_rest(self):
        # az S + Z elutasítása megelőzi a szótárat; a szótár-ellenőrzés is lefut, ha nincs ilyen pár
        valid, _w, error = Board().validate_placement(_row(['K', 'K', 'K']))
        assert valid is False and 'Érvénytelen szó' in error


class TestGameLevel:
    def _game(self, hand):
        game = Game('t')
        game.add_player('a', 'A')
        game.add_player('b', 'B')
        game.start()
        game.current_player().hand = list(hand)
        return game

    def test_place_tiles_refuses_split_digraph(self):
        game = self._game(['S', 'Z', 'Ó', 'K', 'A', 'L', 'M'])
        player = game.current_player()
        ok, message, _score = game.place_tiles(player.id, _row(['S', 'Z', 'Ó']))
        assert ok is False and 'SZ' in message
        assert game.board.is_empty and len(player.hand) == 7

    def test_place_tiles_accepts_the_digraph_tile(self):
        game = self._game(['SZ', 'Ó', 'K', 'A', 'L', 'M', 'T'])
        player = game.current_player()
        ok, message, score = game.place_tiles(player.id, _row(['SZ', 'Ó']))
        assert ok is True, message
        assert score == (3 + 2) * 2     # a középső csillag dupla szó

    def test_preview_explains_why(self):
        game = self._game(['S', 'Z', 'Ó', 'K', 'A', 'L', 'M'])
        player = game.current_player()
        preview = game.preview_placement(player.id, _row(['S', 'Z', 'Ó']))
        assert preview['valid'] is False and 'SZ' in preview['message']

    def test_challenge_mode_applies_the_rule_without_a_dictionary(self):
        game = Game('t', challenge_mode=True)
        game.add_player('a', 'A')
        game.add_player('b', 'B')
        game.start()
        player = game.current_player()
        player.hand = ['S', 'Z', 'Ó', 'K', 'A', 'L', 'M']
        ok, message, _score = game.place_tiles(player.id, _row(['S', 'Z', 'Ó']))
        assert ok is False and 'SZ' in message and game.pending_challenge is None


class TestRobotRespectsTheRule:
    def test_no_split_digraph_move_is_generated(self):
        vocab = Vocabulary(['SZÓ', 'ÓZ', 'KÓ'])
        moves = generate_moves(Board(), ['S', 'Z', 'Ó', 'K'], vocab=vocab, allow_blanks=False)
        assert not any(m.words == ['SZÓ'] for m in moves)
        assert any(m.words == ['KÓ'] for m in moves)

    def test_digraph_tile_move_is_still_found(self):
        vocab = Vocabulary(['SZÓ', 'ÓZ'])
        moves = generate_moves(Board(), ['SZ', 'Ó', 'S', 'Z'], vocab=vocab, allow_blanks=False)
        szo = [m for m in moves if m.words == ['SZÓ']]
        assert szo and all(len(m.tiles) == 2 and m.tiles[0][2] in ('SZ', 'Ó') for m in szo)

    def test_blank_cannot_complete_a_digraph_either(self):
        vocab = Vocabulary(['SZÓ'])
        moves = generate_moves(Board(), ['S', '', 'Ó'], vocab=vocab, allow_blanks=True)
        # a joker SZ-ként (egy zseton) rendben van, Z-ként az S mellett nem
        szo = [m for m in moves if m.words == ['SZÓ']]
        assert szo and all(len(m.tiles) == 2 for m in szo)


class TestSocketFlow:
    """A szerver eseményei: a lerakás és az előnézet is ugyanazt az üzenetet adja."""

    @staticmethod
    def _started_game():
        from server import app, socketio, rooms
        c1 = socketio.test_client(app)
        c1.emit('set_name', {'name': 'Player1', 'is_guest': True, 'user_id': None})
        c1.emit('create_room', {'name': 'Room', 'max_players': 4})
        code = next(r for r in c1.get_received() if r['name'] == 'room_code')['args'][0]['code']
        c2 = socketio.test_client(app)
        c2.emit('set_name', {'name': 'Player2', 'is_guest': True, 'user_id': None})
        c2.emit('join_room', {'code': code})
        c1.emit('start_game')
        c1.get_received()
        c2.get_received()
        game = list(rooms.values())[0].game
        return c1, c2, game

    def test_place_and_preview_refuse_split_digraph_and_keep_the_hand(self):
        c1, c2, game = self._started_game()
        hand = ['S', 'Z', 'Ó', 'K', 'A', 'L', 'M']
        game.players[0].hand = list(hand)
        tiles = [{'row': 7, 'col': 7, 'letter': 'S', 'is_blank': False},
                 {'row': 7, 'col': 8, 'letter': 'Z', 'is_blank': False},
                 {'row': 7, 'col': 9, 'letter': 'Ó', 'is_blank': False}]

        c1.emit('preview_move', {'tiles': tiles})
        preview = next(r for r in c1.get_received() if r['name'] == 'move_preview')['args'][0]
        assert preview['valid'] is False and 'saját zsetonjával' in preview['message']

        c1.emit('place_tiles', {'tiles': tiles})
        result = next(r for r in c1.get_received() if r['name'] == 'action_result')['args'][0]
        assert result['success'] is False and 'SZ' in result['message']
        assert game.players[0].hand == hand and game.board.is_empty
        c1.disconnect()
        c2.disconnect()
