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


class TestChallengeMode:
    """Challenge mód tesztek — szavazásos rendszer."""

    def test_create_game_with_challenge_mode(self):
        from game import Game
        g = Game('test', challenge_mode=True)
        assert g.challenge_mode is True
        assert g.pending_challenge is None

    def test_challenge_mode_default_off(self):
        from game import Game
        g = Game('test')
        assert g.challenge_mode is False

    def test_challenge_mode_state_in_get_state(self):
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.start()
        state = g.get_state()
        assert state['challenge_mode'] is True
        assert state['pending_challenge'] is None

    @patch('board.check_words', return_value=(True, []))
    def test_single_player_challenge_mode_no_pending(self, mock_check):
        """Egyjátékos módban challenge mode nem hoz létre pending-et."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        success, msg, score = g.place_tiles('p1', [
            (7, 6, 'A', False), (7, 7, 'B', False)
        ])
        assert success is True
        assert g.pending_challenge is None

    @patch('board.check_words', return_value=(True, []))
    def test_multiplayer_challenge_mode_creates_pending(self, mock_check):
        """Többjátékos challenge módban pending challenge jön létre."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        success, msg, score = g.place_tiles('p1', [
            (7, 6, 'A', False), (7, 7, 'B', False)
        ])
        assert success is True
        assert g.pending_challenge is not None
        assert 'megtámadható' in msg
        assert 'A' not in g.players[0].hand
        assert 'B' not in g.players[0].hand
        # Új szavazásos mezők
        assert g.pending_challenge['voting_phase'] is False
        assert g.pending_challenge['votes'] == {}

    @patch('board.check_words', return_value=(True, []))
    def test_pending_challenge_in_state(self, mock_check):
        """A pending challenge megjelenik a game state-ben szavazás infóval."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        state = g.get_state()
        pc = state['pending_challenge']
        assert pc is not None
        assert pc['player_name'] == 'Alice'
        assert len(pc['tiles']) == 2
        assert pc['voting_phase'] is False
        assert pc['player_count'] == 2

    @patch('board.check_words', return_value=(True, []))
    def test_cannot_place_during_pending(self, mock_check):
        """Nem lehet lerakni amíg van pending challenge."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.players[1].hand = ['C', 'D', 'E', 'F', 'G', 'H', 'I']
        success, msg, score = g.place_tiles('p2', [(8, 7, 'C', False), (9, 7, 'D', False)])
        assert success is False
        assert 'megtámadási' in msg

    @patch('board.check_words', return_value=(True, []))
    def test_cannot_pass_during_pending(self, mock_check):
        """Nem lehet passzolni amíg van pending challenge."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        success, msg = g.pass_turn('p2')
        assert success is False

    # --- 2 játékos: nincs megtámadás, kötelező elfogadás ---

    @patch('board.check_words', return_value=(True, []))
    def test_2_player_no_challenge(self, mock_check):
        """2 játékosnál a challenge nem engedélyezett."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        success, result, msg = g.challenge('p2')
        assert success is False
        assert 'Két játékos' in msg

    @patch('board.check_words', return_value=(True, []))
    def test_2_player_accept_immediate(self, mock_check):
        """2 játékosnál az elfogadás azonnali."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        old_score = g.players[0].score
        success, result, msg = g.accept_pending_by_player('p2')
        assert success is True
        assert result == 'accepted'
        assert g.pending_challenge is None
        assert g.players[0].score > old_score
        assert g.board.get(7, 6) == ('A', False)

    @patch('board.check_words', return_value=(True, []))
    def test_2_player_cannot_accept_own(self, mock_check):
        """2 játékosnál a lerakó nem fogadhatja el saját lerakását."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        success, result, msg = g.accept_pending_by_player('p1')
        assert success is False

    # --- 2 játékos: elutasítás ---

    @patch('board.check_words', return_value=(True, []))
    def test_2_player_reject(self, mock_check):
        """2 játékosnál az elutasítás visszavonja a lerakást."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        old_score = g.players[0].score
        success, result, msg = g.reject_pending_by_player('p2')
        assert success is True
        assert result == 'rejected'
        assert g.pending_challenge is None
        assert g.players[0].score == old_score
        assert g.board.get(7, 6) is None
        assert g.board.get(7, 7) is None

    @patch('board.check_words', return_value=(True, []))
    def test_2_player_cannot_reject_own(self, mock_check):
        """2 játékosnál a lerakó nem utasíthatja el saját lerakását."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        success, result, msg = g.reject_pending_by_player('p1')
        assert success is False

    @patch('board.check_words', return_value=(True, []))
    def test_2_player_reject_tiles_returned(self, mock_check):
        """2 játékos elutasítás: betűk visszakerülnek a lerakó kezébe."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        hand_before = len(g.players[0].hand)
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        hand_after_place = len(g.players[0].hand)
        assert hand_after_place == hand_before - 2
        g.reject_pending_by_player('p2')
        assert len(g.players[0].hand) == hand_before

    @patch('board.check_words', return_value=(True, []))
    def test_2_player_reject_no_pending(self, mock_check):
        """Elutasítás pending nélkül sikertelen."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        success, result, msg = g.reject_pending_by_player('p2')
        assert success is False

    @patch('board.check_words', return_value=(True, []))
    def test_3_player_cannot_use_reject(self, mock_check):
        """3+ játékosnál a reject_pending_by_player nem használható."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        success, result, msg = g.reject_pending_by_player('p2')
        assert success is False
        assert '3+' in msg

    # --- accept_pending (timeout) ---

    @patch('board.check_words', return_value=(True, []))
    def test_accept_pending_timeout(self, mock_check):
        """Timeout elfogadás véglegesíti a lerakást."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        old_score = g.players[0].score
        success, result, msg = g.accept_pending()
        assert success is True
        assert result == 'accepted'
        assert g.pending_challenge is None
        assert g.players[0].score > old_score

    @patch('board.check_words', return_value=(True, []))
    def test_accept_pending_no_pending(self, mock_check):
        """Elfogadás pending nélkül sikertelen."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.start()
        success, result, msg = g.accept_pending()
        assert success is False

    # --- Saját lerakás megtámadása ---

    @patch('board.check_words', return_value=(True, []))
    def test_challenge_own_placement(self, mock_check):
        """Saját lerakást nem lehet megtámadni."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        success, result, msg = g.challenge('p1')
        assert success is False

    def test_challenge_no_pending(self):
        """Megtámadás pending nélkül sikertelen."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        success, result, msg = g.challenge('p2')
        assert success is False

    # --- 3+ játékos szavazás ---

    @patch('board.check_words', return_value=(True, []))
    def test_3_player_challenge_starts_voting(self, mock_check):
        """3 játékosnál a challenge szavazást indít."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        success, result, msg = g.challenge('p2')
        assert success is True
        assert result == 'voting'
        assert g.pending_challenge is not None
        assert g.pending_challenge['voting_phase'] is True
        assert g.pending_challenge['challenger_id'] == 'p2'

    @patch('board.check_words', return_value=(True, []))
    def test_3_player_vote_accept(self, mock_check):
        """3 játékos: szavazó elfogadja → szó marad."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.challenge('p2')
        success, result, msg = g.cast_vote('p3', 'accept')
        assert success is True
        assert result == 'vote_accepted'
        assert g.pending_challenge is None
        assert g.board.get(7, 6) == ('A', False)

    @patch('board.check_words', return_value=(True, []))
    def test_3_player_vote_reject(self, mock_check):
        """3 játékos: szavazó elutasítja → betűk visszavéve."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.challenge('p2')
        success, result, msg = g.cast_vote('p3', 'reject')
        assert success is True
        assert result == 'vote_rejected'
        assert g.pending_challenge is None
        assert g.board.get(7, 6) is None
        assert 'A' in g.players[0].hand
        assert 'B' in g.players[0].hand

    @patch('board.check_words', return_value=(True, []))
    def test_4_player_50_percent_is_yes(self, mock_check):
        """4 játékos: 50% szavazat (1 elfogad, 1 elutasít) → elfogadva."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.add_player('p4', 'Diana')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.challenge('p2')  # p2 megtámad (nem szavaz)
        # Szavazók: p3, p4 (p1=lerakó, p2=megtámadó kizárva)
        g.cast_vote('p3', 'accept')
        success, result, msg = g.cast_vote('p4', 'reject')
        assert success is True
        assert result == 'vote_accepted'  # 50% = elfogadva
        assert g.board.get(7, 6) == ('A', False)

    @patch('board.check_words', return_value=(True, []))
    def test_4_player_all_reject(self, mock_check):
        """4 játékos: mindkét szavazó elutasít → elutasítva."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.add_player('p4', 'Diana')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.challenge('p2')
        g.cast_vote('p3', 'reject')
        success, result, msg = g.cast_vote('p4', 'reject')
        assert success is True
        assert result == 'vote_rejected'
        assert g.board.get(7, 6) is None
        assert 'A' in g.players[0].hand

    @patch('board.check_words', return_value=(True, []))
    def test_voting_timeout_non_voters_accept(self, mock_check):
        """Szavazási timeout: nem szavazók elfogadásnak számítanak."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.challenge('p2')
        # p3 nem szavaz, timer lejár
        success, result, msg = g.accept_pending()
        assert success is True
        assert result == 'vote_accepted'  # Nem szavazó = elfogadás
        assert g.board.get(7, 6) == ('A', False)

    @patch('board.check_words', return_value=(True, []))
    def test_cast_vote_not_in_voting_phase(self, mock_check):
        """Szavazás nem szavazási fázisban sikertelen."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        # Szavazási fázis még nem indult
        success, result, msg = g.cast_vote('p3', 'accept')
        assert success is False

    @patch('board.check_words', return_value=(True, []))
    def test_cast_vote_duplicate(self, mock_check):
        """Duplikált szavazat sikertelen."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.add_player('p4', 'Diana')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.challenge('p2')
        g.cast_vote('p3', 'accept')
        success, result, msg = g.cast_vote('p3', 'reject')
        assert success is False
        assert 'Már szavaztál' in msg

    @patch('board.check_words', return_value=(True, []))
    def test_challenger_cannot_vote(self, mock_check):
        """A megtámadó nem szavazhat."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.challenge('p2')
        success, result, msg = g.cast_vote('p2', 'reject')
        assert success is False
        assert 'megtámadó' in msg

    @patch('board.check_words', return_value=(True, []))
    def test_placer_cannot_vote(self, mock_check):
        """A lerakó nem szavazhat."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.challenge('p2')
        success, result, msg = g.cast_vote('p1', 'accept')
        assert success is False
        assert 'lerakó' in msg

    @patch('board.check_words', return_value=(True, []))
    def test_already_in_voting_cannot_challenge_again(self, mock_check):
        """Már szavazási fázisban nem lehet újra megtámadni."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.challenge('p2')
        success, result, msg = g.challenge('p3')
        assert success is False
        assert 'Már folyamatban' in msg

    @patch('board.check_words', return_value=(True, []))
    def test_3_player_all_accept_no_challenge(self, mock_check):
        """3 játékos: mindenki elfogadja (challenge nélkül) → lerakás végleges."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.accept_pending_by_player('p2')
        success, result, msg = g.accept_pending_by_player('p3')
        assert success is True
        assert result == 'accepted'
        assert g.pending_challenge is None
        assert g.board.get(7, 6) == ('A', False)

    @patch('board.check_words', return_value=(True, []))
    def test_3_player_partial_accept_then_challenge(self, mock_check):
        """3 játékos: p2 elfogad, p3 megtámad → azonnali szavazás eredmény."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        # p2 elfogadja (phase 1)
        g.accept_pending_by_player('p2')
        # p3 megtámadja → szavazás, p2 elfogadása átvive
        # Szavazók: csak p2 (p1=lerakó, p3=megtámadó), p2 már szavazott
        success, result, msg = g.challenge('p3')
        assert success is True
        assert result == 'vote_accepted'  # p2 elfogadta → 100% elfogadás

    @patch('board.check_words', return_value=(True, []))
    def test_no_skip_turn_penalty(self, mock_check):
        """Szavazásos rendszerben nincs kör kihagyás büntetés."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.challenge('p2')
        g.cast_vote('p3', 'accept')  # Szó elfogadva
        # Senki nem hagyja ki a körét
        for p in g.players:
            assert p.skip_next_turn is False

    def test_remove_player_clears_pending(self):
        """Ha a pending játékos kilép, a pending törlődik."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        with patch('board.check_words', return_value=(True, [])):
            g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        assert g.pending_challenge is not None
        g.remove_player('p1')
        assert g.pending_challenge is None
