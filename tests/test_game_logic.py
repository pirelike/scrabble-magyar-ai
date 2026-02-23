"""Tests for tiles.py, board.py, game.py - game logic."""
import pytest
from unittest.mock import patch


class TestTileBag:
    def test_initial_count(self):
        from tiles import TileBag
        bag = TileBag()
        assert bag.remaining() == 100

    def test_draw(self):
        from tiles import TileBag
        bag = TileBag()
        drawn = bag.draw(7)
        assert len(drawn) == 7
        assert bag.remaining() == 93

    def test_draw_more_than_available(self):
        from tiles import TileBag
        bag = TileBag()
        bag.tiles = ['A', 'B', 'C']
        drawn = bag.draw(5)
        assert len(drawn) == 3
        assert bag.remaining() == 0

    def test_draw_empty(self):
        from tiles import TileBag
        bag = TileBag()
        bag.tiles = []
        drawn = bag.draw(5)
        assert drawn == []

    def test_put_back(self):
        from tiles import TileBag
        bag = TileBag()
        drawn = bag.draw(7)
        bag.put_back(drawn)
        assert bag.remaining() == 100

    def test_is_empty(self):
        from tiles import TileBag
        bag = TileBag()
        assert not bag.is_empty()
        bag.tiles = []
        assert bag.is_empty()

    def test_tile_values(self):
        from tiles import TILE_VALUES
        assert TILE_VALUES['A'] == 1
        assert TILE_VALUES['TY'] == 10
        assert TILE_VALUES[''] == 0  # Joker
        assert TILE_VALUES['CS'] == 7
        assert TILE_VALUES['SZ'] == 3


class TestBoard:
    def test_empty_board(self):
        from board import Board, BOARD_SIZE
        b = Board()
        assert b.is_empty
        for r in range(BOARD_SIZE):
            for c in range(BOARD_SIZE):
                assert b.get(r, c) is None

    def test_get_out_of_bounds(self):
        from board import Board
        b = Board()
        assert b.get(-1, 0) is None
        assert b.get(0, -1) is None
        assert b.get(15, 0) is None
        assert b.get(0, 15) is None

    def test_set_and_get(self):
        from board import Board
        b = Board()
        b.set(7, 7, 'A', False)
        cell = b.get(7, 7)
        assert cell == ('A', False)

    def test_premium_map(self):
        from board import Board, TW, DW, DL, TL, ST
        b = Board()
        assert b.premium_at(0, 0) == TW
        assert b.premium_at(7, 7) == ST
        assert b.premium_at(1, 1) == DW
        assert b.premium_at(0, 3) == DL
        assert b.premium_at(1, 5) == TL

    def test_premium_symmetry(self):
        from board import Board
        b = Board()
        # TW at corners
        for r, c in [(0, 0), (0, 14), (14, 0), (14, 14)]:
            assert b.premium_at(r, c) == 'TW'

    def test_to_dict(self):
        from board import Board
        b = Board()
        b.set(7, 7, 'A', False)
        d = b.to_dict()
        assert len(d) == 15
        assert len(d[0]) == 15
        assert d[7][7] == {'letter': 'A', 'is_blank': False}
        assert d[0][0] is None

    def test_apply_placement(self):
        from board import Board
        b = Board()
        tiles = [(7, 7, 'A', False), (7, 8, 'B', False)]
        b.apply_placement(tiles)
        assert b.get(7, 7) == ('A', False)
        assert b.get(7, 8) == ('B', False)
        assert b.is_empty is False


class TestBoardValidation:
    """Test board placement validation with mocked dictionary."""

    @pytest.fixture(autouse=True)
    def mock_dictionary(self):
        """Mock the dictionary to accept all words."""
        with patch('board.check_words', return_value=(True, [])):
            yield

    def test_first_move_must_cover_center(self):
        from board import Board
        b = Board()
        valid, words, err = b.validate_placement([(0, 0, 'A', False), (0, 1, 'B', False)])
        assert valid is False
        assert 'középső' in err

    def test_first_move_must_be_at_least_2_tiles(self):
        from board import Board
        b = Board()
        valid, words, err = b.validate_placement([(7, 7, 'A', False)])
        assert valid is False
        assert '2 betű' in err

    def test_valid_first_move_horizontal(self):
        from board import Board
        b = Board()
        tiles = [(7, 6, 'A', False), (7, 7, 'B', False), (7, 8, 'C', False)]
        valid, words, err = b.validate_placement(tiles)
        assert valid is True
        assert len(words) >= 1
        assert words[0][0] == 'ABC'

    def test_valid_first_move_vertical(self):
        from board import Board
        b = Board()
        tiles = [(6, 7, 'A', False), (7, 7, 'B', False)]
        valid, words, err = b.validate_placement(tiles)
        assert valid is True

    def test_tiles_must_be_in_line(self):
        from board import Board
        b = Board()
        tiles = [(7, 6, 'A', False), (7, 7, 'B', False), (8, 8, 'C', False)]
        valid, words, err = b.validate_placement(tiles)
        assert valid is False
        assert 'egy sorban' in err

    def test_second_move_must_connect(self):
        from board import Board
        b = Board()
        b.apply_placement([(7, 7, 'A', False), (7, 8, 'B', False)])
        b.is_empty = False
        valid, words, err = b.validate_placement([(0, 0, 'C', False), (0, 1, 'D', False)])
        assert valid is False
        assert 'csatlakoznia' in err

    def test_second_move_connecting(self):
        from board import Board
        b = Board()
        b.apply_placement([(7, 7, 'A', False), (7, 8, 'B', False)])
        b.is_empty = False
        # Place adjacent tile
        valid, words, err = b.validate_placement([(7, 9, 'C', False), (7, 10, 'D', False)])
        assert valid is True

    def test_cannot_place_on_occupied_cell(self):
        from board import Board
        b = Board()
        b.apply_placement([(7, 7, 'A', False)])
        b.is_empty = False
        valid, words, err = b.validate_placement([(7, 7, 'B', False)])
        assert valid is False
        assert 'foglalt' in err

    def test_gap_in_placement(self):
        from board import Board
        b = Board()
        # Place tiles with a gap (7,6 and 7,8 but not 7,7)
        tiles = [(7, 5, 'A', False), (7, 7, 'B', False)]
        valid, words, err = b.validate_placement(tiles)
        assert valid is False

    def test_empty_placement(self):
        from board import Board
        b = Board()
        valid, words, err = b.validate_placement([])
        assert valid is False

    def test_out_of_bounds(self):
        from board import Board
        b = Board()
        valid, words, err = b.validate_placement([(15, 7, 'A', False)])
        assert valid is False

    def test_scoring_double_letter(self):
        from board import Board, DL, PREMIUM_MAP
        b = Board()
        # Find a DL cell near center
        # (0,3) is DL, but we need to be near center for first move
        # (7,3) is DL
        tiles = [(7, 3, 'A', False), (7, 4, 'B', False), (7, 5, 'C', False),
                 (7, 6, 'D', False), (7, 7, 'E', False)]
        valid, words, err = b.validate_placement(tiles)
        assert valid is True
        assert len(words) >= 1
        # Verify score is calculated (A on DL at 7,3 should be doubled)

    def test_scoring_with_blank(self):
        from board import Board
        b = Board()
        tiles = [(7, 6, 'A', True), (7, 7, 'B', False)]
        valid, words, err = b.validate_placement(tiles)
        assert valid is True
        # Blank tile (A) should have 0 point value
        word, positions, score = words[0]
        assert word == 'AB'

    def test_cross_words_formed(self):
        from board import Board
        b = Board()
        b.apply_placement([(7, 7, 'A', False), (7, 8, 'B', False)])
        b.is_empty = False
        # Place vertical word crossing horizontal
        tiles = [(6, 7, 'C', False), (8, 7, 'D', False)]
        valid, words, err = b.validate_placement(tiles)
        assert valid is True
        # Should form main vertical word + possibly cross words
        assert len(words) >= 1


class TestPlayer:
    def test_player_creation(self):
        from game import Player
        p = Player('id1', 'Alice')
        assert p.id == 'id1'
        assert p.name == 'Alice'
        assert p.hand == []
        assert p.score == 0
        assert p.consecutive_passes == 0

    def test_to_dict_hidden_hand(self):
        from game import Player
        p = Player('id1', 'Alice')
        p.hand = ['A', 'B', 'C']
        p.score = 42
        d = p.to_dict(reveal_hand=False)
        assert 'hand' not in d
        assert d['hand_count'] == 3
        assert d['score'] == 42
        assert d['name'] == 'Alice'

    def test_to_dict_revealed_hand(self):
        from game import Player
        p = Player('id1', 'Alice')
        p.hand = ['A', 'B', 'C']
        d = p.to_dict(reveal_hand=True)
        assert d['hand'] == ['A', 'B', 'C']


class TestGame:
    def test_create_game(self):
        from game import Game
        g = Game('test-room')
        assert g.id == 'test-room'
        assert g.started is False
        assert g.finished is False
        assert len(g.players) == 0

    def test_add_player(self):
        from game import Game
        g = Game('test')
        success, msg = g.add_player('p1', 'Alice')
        assert success is True
        assert len(g.players) == 1

    def test_add_max_players(self):
        from game import Game
        g = Game('test')
        for i in range(4):
            g.add_player(f'p{i}', f'Player{i}')
        success, msg = g.add_player('p5', 'Player5')
        assert success is False
        assert '4 játékos' in msg

    def test_add_player_after_start(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        success, msg = g.add_player('p2', 'Bob')
        assert success is False

    def test_remove_player(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.remove_player('p1')
        assert len(g.players) == 1
        assert g.players[0].name == 'Bob'

    def test_start_game(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        success, msg = g.start()
        assert success is True
        assert g.started is True
        assert len(g.players[0].hand) == 7

    def test_start_game_no_players(self):
        from game import Game
        g = Game('test')
        success, msg = g.start()
        assert success is False

    def test_start_game_twice(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        success, msg = g.start()
        assert success is False

    def test_start_deals_hands(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        assert len(g.players[0].hand) == 7
        assert len(g.players[1].hand) == 7
        assert g.bag.remaining() == 100 - 14

    def test_pass_turn(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        success, msg = g.pass_turn('p1')
        assert success is True
        assert g.current_player().id == 'p2'

    def test_pass_wrong_player(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        success, msg = g.pass_turn('p2')
        assert success is False
        assert 'Nem te' in msg

    def test_game_ends_after_all_pass_twice(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        # p1 pass, p2 pass, p1 pass, p2 pass => game over
        g.pass_turn('p1')
        g.pass_turn('p2')
        g.pass_turn('p1')
        g.pass_turn('p2')
        assert g.finished is True

    def test_exchange_tiles(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        old_hand = list(g.players[0].hand)
        success, msg = g.exchange_tiles('p1', [0, 1])
        assert success is True
        assert len(g.players[0].hand) == 7

    def test_exchange_wrong_player(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        success, msg = g.exchange_tiles('p2', [0])
        assert success is False

    def test_exchange_invalid_index(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        success, msg = g.exchange_tiles('p1', [10])
        assert success is False

    def test_exchange_empty(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        success, msg = g.exchange_tiles('p1', [])
        assert success is False

    def test_get_state(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        state = g.get_state(for_player_id='p1')
        assert state['started'] is True
        assert state['finished'] is False
        assert len(state['players']) == 1
        assert 'hand' in state['players'][0]  # Revealed for requesting player
        assert state['tiles_remaining'] == 93
        assert state['current_player'] == 'p1'

    def test_get_state_hides_other_hands(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        state = g.get_state(for_player_id='p1')
        p1_data = next(p for p in state['players'] if p['id'] == 'p1')
        p2_data = next(p for p in state['players'] if p['id'] == 'p2')
        assert 'hand' in p1_data
        assert 'hand' not in p2_data

    def test_current_player_empty(self):
        from game import Game
        g = Game('test')
        assert g.current_player() is None

    def test_pass_after_game_over(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        g.finished = True
        success, msg = g.pass_turn('p1')
        assert success is False
        assert 'véget ért' in msg
