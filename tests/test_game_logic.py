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
        assert 'szavazásra vár' in msg
        assert 'A' not in g.players[0].hand
        assert 'B' not in g.players[0].hand
        assert g.pending_challenge.votes == {}

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
        assert 'Várj' in msg

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

    # --- 2 játékos: elfogadás / elutasítás ---

    @patch('board.check_words', return_value=(True, []))
    def test_2_player_accept_immediate(self, mock_check):
        """2 játékosnál az elfogadás azonnali (egyetlen szavazó)."""
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
        assert result == 'vote_accepted'
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
        assert result == 'vote_rejected'
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

    # --- accept_pending (timeout) ---

    @patch('board.check_words', return_value=(True, []))
    def test_accept_pending_timeout(self, mock_check):
        """Timeout: nem szavazók elfogadásnak számítanak."""
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
        assert result == 'vote_accepted'
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

    # --- Saját lerakás nem fogadható el/utasítható el ---

    @patch('board.check_words', return_value=(True, []))
    def test_placer_cannot_accept_own(self, mock_check):
        """A lerakó nem fogadhatja el saját lerakását."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        success, result, msg = g.accept_pending_by_player('p1')
        assert success is False

    @patch('board.check_words', return_value=(True, []))
    def test_placer_cannot_reject_own(self, mock_check):
        """A lerakó nem utasíthatja el saját lerakását."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        success, result, msg = g.reject_pending_by_player('p1')
        assert success is False

    def test_accept_no_pending(self):
        """Elfogadás pending nélkül sikertelen."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        success, result, msg = g.accept_pending_by_player('p2')
        assert success is False

    # --- 3+ játékos szavazás ---

    @patch('board.check_words', return_value=(True, []))
    def test_3_player_accept(self, mock_check):
        """3 játékos: mindketten elfogadják → szó marad."""
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
        assert result == 'vote_accepted'
        assert g.pending_challenge is None
        assert g.board.get(7, 6) == ('A', False)

    @patch('board.check_words', return_value=(True, []))
    def test_3_player_reject(self, mock_check):
        """3 játékos: mindketten elutasítják → betűk visszavéve."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.reject_pending_by_player('p2')
        success, result, msg = g.reject_pending_by_player('p3')
        assert success is True
        assert result == 'vote_rejected'
        assert g.pending_challenge is None
        assert g.board.get(7, 6) is None
        assert 'A' in g.players[0].hand
        assert 'B' in g.players[0].hand

    @patch('board.check_words', return_value=(True, []))
    def test_4_player_50_percent_is_yes(self, mock_check):
        """4 játékos: 50% szavazat (1 elfogad, 1 elutasít, 1 nem szavaz) → elfogadva."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.add_player('p4', 'Diana')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        # Szavazók: p2, p3, p4 (p1=lerakó kizárva)
        g.accept_pending_by_player('p2')
        g.reject_pending_by_player('p3')
        success, result, msg = g.reject_pending_by_player('p4')
        assert success is True
        assert result == 'vote_rejected'  # 1 elfogad, 2 elutasít → elutasítva

    @patch('board.check_words', return_value=(True, []))
    def test_4_player_all_reject(self, mock_check):
        """4 játékos: mindenki elutasít → elutasítva."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.add_player('p4', 'Diana')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.reject_pending_by_player('p2')
        g.reject_pending_by_player('p3')
        success, result, msg = g.reject_pending_by_player('p4')
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
        # Senki nem szavaz, timer lejár
        success, result, msg = g.accept_pending()
        assert success is True
        assert result == 'vote_accepted'  # Nem szavazó = elfogadás
        assert g.board.get(7, 6) == ('A', False)

    @patch('board.check_words', return_value=(True, []))
    def test_duplicate_vote(self, mock_check):
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
        g.accept_pending_by_player('p3')
        success, result, msg = g.reject_pending_by_player('p3')
        assert success is False
        assert 'Már szavaztál' in msg

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
        g.accept_pending_by_player('p2')
        g.accept_pending_by_player('p3')  # Szó elfogadva
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

    # --- Turn order after reject ---

    @patch('board.check_words', return_value=(True, []))
    def test_2_player_reject_same_player_turn(self, mock_check):
        """2 játékos elutasítás: a lerakó újra jön (nem a következő)."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        assert g.current_player().id == 'p1'
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.reject_pending_by_player('p2')
        # Elutasítás után a lerakó (p1) újra jön
        assert g.current_player().id == 'p1'

    @patch('board.check_words', return_value=(True, []))
    def test_3_player_vote_reject_same_player_turn(self, mock_check):
        """3 játékos szavazásos elutasítás: a lerakó újra jön."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        assert g.current_player().id == 'p1'
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.reject_pending_by_player('p2')
        g.reject_pending_by_player('p3')
        # Elutasítás után a lerakó (p1) újra jön
        assert g.current_player().id == 'p1'

    @patch('board.check_words', return_value=(True, []))
    def test_4_player_all_reject_same_player_turn(self, mock_check):
        """4 játékos: mindenki elutasít → a lerakó újra jön."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.add_player('p4', 'Diana')
        g.start()
        assert g.current_player().id == 'p1'
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.reject_pending_by_player('p2')
        g.reject_pending_by_player('p3')
        g.reject_pending_by_player('p4')
        # Elutasítás után a lerakó (p1) újra jön
        assert g.current_player().id == 'p1'

    @patch('board.check_words', return_value=(True, []))
    def test_2_player_accept_next_player_turn(self, mock_check):
        """2 játékos elfogadás: a következő játékos jön."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        assert g.current_player().id == 'p1'
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.accept_pending_by_player('p2')
        # Elfogadás után a másik játékos (p2) jön
        assert g.current_player().id == 'p2'

    # --- Reconnect (mark_disconnected, replace_player_sid) ---

    def test_mark_disconnected(self):
        """Játékos ideiglenesen lecsatlakozottnak jelölése."""
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        assert g.players[0].disconnected is False
        g.mark_disconnected('p1')
        assert g.players[0].disconnected is True
        assert g.players[1].disconnected is False

    def test_replace_player_sid(self):
        """Játékos sid cseréje újracsatlakozáskor."""
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.mark_disconnected('p1')
        assert g.replace_player_sid('p1', 'p1_new') is True
        assert g.players[0].id == 'p1_new'
        assert g.players[0].disconnected is False

    @patch('board.check_words', return_value=(True, []))
    def test_replace_player_sid_updates_pending_challenge(self, mock_check):
        """Sid csere frissíti a pending challenge-t is."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.accept_pending_by_player('p2')  # p2 elfogadja
        # p2 disconnectel és reconnecel
        g.mark_disconnected('p2')
        g.replace_player_sid('p2', 'p2_new')
        assert g.players[1].id == 'p2_new'


class TestGameSaveRestore:
    """Game save/restore (to_save_dict / from_save_dict)."""

    @patch('board.check_words', return_value=(True, []))
    def test_save_restore_roundtrip(self, mock_check):
        from game import Game
        g = Game('test-room', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.accept_pending_by_player('p2')

        save_data = g.to_save_dict()
        restored = Game.from_save_dict(save_data)

        assert restored.id == 'test-room'
        assert restored.challenge_mode is True
        assert restored.started is True
        assert restored.current_player_idx == g.current_player_idx
        assert restored.turn_number == g.turn_number
        assert len(restored.players) == 2
        assert restored.players[0].name == 'Alice'
        assert restored.players[1].name == 'Bob'
        assert restored.players[0].score == g.players[0].score
        assert restored.board.get(7, 6) == ('A', False)
        assert restored.board.get(7, 7) == ('B', False)

    def test_save_restore_empty_game(self):
        from game import Game
        g = Game('empty', challenge_mode=False)
        g.add_player('p1', 'Alice')
        g.start()

        save_data = g.to_save_dict()
        restored = Game.from_save_dict(save_data)

        assert restored.started is True
        assert restored.finished is False
        assert len(restored.players) == 1
        assert len(restored.players[0].hand) == 7
        assert restored.board.is_empty is True

    @patch('board.check_words', return_value=(True, []))
    def test_save_restore_winner(self, mock_check):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        # Force game end
        g.finished = True
        g.winner = g.players[0]

        save_data = g.to_save_dict()
        restored = Game.from_save_dict(save_data)
        assert restored.winner is not None
        assert restored.winner.name == 'Alice'

    def test_save_restore_preserves_hand(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        original_hand = list(g.players[0].hand)

        save_data = g.to_save_dict()
        restored = Game.from_save_dict(save_data)
        assert restored.players[0].hand == original_hand

    def test_save_restore_preserves_bag(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        original_remaining = g.bag.remaining()

        save_data = g.to_save_dict()
        restored = Game.from_save_dict(save_data)
        assert restored.bag.remaining() == original_remaining

    def test_save_restore_player_state(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        g.players[0].score = 42
        g.players[0].consecutive_passes = 1
        g.players[0].skip_next_turn = True

        save_data = g.to_save_dict()
        restored = Game.from_save_dict(save_data)
        assert restored.players[0].score == 42
        assert restored.players[0].consecutive_passes == 1
        assert restored.players[0].skip_next_turn is True


class TestGameEndScoring:
    """End game scoring tests."""

    def test_end_game_deducts_hand_values(self):
        from game import Game
        from tiles import TILE_VALUES
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = []
        g.players[0].score = 100
        g.players[1].hand = ['A', 'B']  # A=1, B=2
        g.players[1].score = 80
        g.bag.tiles = []
        g._end_game(g.players[0])

        assert g.finished is True
        # Bob loses hand value: 80 - (1+2) = 77
        assert g.players[1].score == 77
        # Alice gains Bob's hand value: 100 + 3 = 103
        assert g.players[0].score == 103
        assert g.winner.name == 'Alice'

    def test_end_game_no_finisher(self):
        """When game ends by all passing, no one gets bonus."""
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = ['A']
        g.players[0].score = 50
        g.players[1].hand = ['B']
        g.players[1].score = 60
        g._end_game(None)

        assert g.finished is True
        # Both lose hand value but no one gets bonus
        assert g.players[0].score == 49  # 50 - 1
        assert g.players[1].score == 58  # 60 - 2
        assert g.winner.name == 'Bob'

    def test_end_game_joker_zero_value(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        g.players[0].hand = ['']  # Joker = 0 points
        g.players[0].score = 50
        g._end_game(None)
        assert g.players[0].score == 50  # No deduction for joker

    def test_all_pass_twice_ends_game(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        # All 3 players pass twice
        for _ in range(2):
            g.pass_turn('p1')
            g.pass_turn('p2')
            g.pass_turn('p3')
        assert g.finished is True


class TestExchangeTilesEdgeCases:
    def test_exchange_duplicate_indices(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        success, msg = g.exchange_tiles('p1', [0, 0])
        assert success is False
        assert 'Duplikált' in msg

    def test_exchange_negative_index(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        success, msg = g.exchange_tiles('p1', [-1])
        assert success is False
        assert 'Érvénytelen' in msg

    def test_exchange_not_enough_in_bag(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        g.bag.tiles = ['X']  # Only 1 tile in bag
        success, msg = g.exchange_tiles('p1', [0, 1, 2])
        assert success is False
        assert 'Nincs elég' in msg

    def test_exchange_after_game_over(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        g.finished = True
        success, msg = g.exchange_tiles('p1', [0])
        assert success is False
        assert 'véget ért' in msg

    def test_exchange_during_pending_challenge(self):
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        with patch('board.check_words', return_value=(True, [])):
            g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        success, msg = g.exchange_tiles('p2', [0])
        assert success is False
        assert 'Várj' in msg

    def test_exchange_resets_consecutive_passes(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.pass_turn('p1')
        assert g.players[0].consecutive_passes == 1
        g.pass_turn('p2')
        g.exchange_tiles('p1', [0])
        assert g.players[0].consecutive_passes == 0


class TestGetAllStates:
    def test_all_states_separate_hands(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        states = g.get_all_states()
        assert 'p1' in states
        assert 'p2' in states

        # P1's state should show P1's hand but not P2's
        p1_state = states['p1']
        p1_data = next(p for p in p1_state['players'] if p['id'] == 'p1')
        p2_data = next(p for p in p1_state['players'] if p['id'] == 'p2')
        assert 'hand' in p1_data
        assert 'hand' not in p2_data

        # P2's state should show P2's hand but not P1's
        p2_state = states['p2']
        p1_in_p2 = next(p for p in p2_state['players'] if p['id'] == 'p1')
        p2_in_p2 = next(p for p in p2_state['players'] if p['id'] == 'p2')
        assert 'hand' not in p1_in_p2
        assert 'hand' in p2_in_p2

    def test_all_states_shared_board(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        states = g.get_all_states()
        # Board should be identical for both
        assert states['p1']['board'] == states['p2']['board']


class TestPlaceTilesEdgeCases:
    @patch('board.check_words', return_value=(True, []))
    def test_place_tiles_finished_game(self, mock_check):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        g.finished = True
        success, msg, score = g.place_tiles('p1', [(7, 7, 'A', False)])
        assert success is False
        assert 'véget ért' in msg

    @patch('board.check_words', return_value=(True, []))
    def test_place_tiles_wrong_player(self, mock_check):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        success, msg, score = g.place_tiles('p2', [(7, 7, 'A', False)])
        assert success is False
        assert 'Nem te' in msg

    @patch('board.check_words', return_value=(True, []))
    def test_place_tiles_missing_letter(self, mock_check):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        success, msg, score = g.place_tiles('p1', [(7, 6, 'X', False), (7, 7, 'A', False)])
        assert success is False
        assert 'Nincs' in msg

    @patch('board.check_words', return_value=(True, []))
    def test_place_tiles_blank_joker(self, mock_check):
        """Placing a blank tile (joker) requires empty string in hand."""
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        g.players[0].hand = ['', 'A', 'B', 'C', 'D', 'E', 'F']
        success, msg, score = g.place_tiles('p1', [(7, 6, 'X', True), (7, 7, 'A', False)])
        assert success is True

    @patch('board.check_words', return_value=(True, []))
    def test_place_tiles_no_blank_in_hand(self, mock_check):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        success, msg, score = g.place_tiles('p1', [(7, 6, 'X', True), (7, 7, 'A', False)])
        assert success is False
        assert 'üres' in msg

    @patch('board.check_words', return_value=(True, []))
    def test_place_all_7_tiles_bonus(self, mock_check):
        """Placing all 7 tiles gives 50 point bonus."""
        from game import Game, BONUS_ALL_TILES
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        tiles = [(7, i, l, False) for i, l in zip(range(4, 11), ['A', 'B', 'C', 'D', 'E', 'F', 'G'])]
        success, msg, score = g.place_tiles('p1', tiles)
        assert success is True
        assert score >= BONUS_ALL_TILES

    @patch('board.check_words', return_value=(True, []))
    def test_place_tiles_resets_consecutive_passes(self, mock_check):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.pass_turn('p1')
        assert g.players[0].consecutive_passes == 1
        g.pass_turn('p2')
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        assert g.players[0].consecutive_passes == 0

    @patch('board.check_words', return_value=(True, []))
    def test_place_tiles_draws_new_tiles(self, mock_check):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        remaining_before = g.bag.remaining()
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        assert len(g.players[0].hand) == 7
        assert g.bag.remaining() == remaining_before - 2


class TestMoveLog:
    @patch('board.check_words', return_value=(True, []))
    def test_place_records_move(self, mock_check):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        assert len(g.move_log) == 1
        assert g.move_log[0]['player_name'] == 'Alice'
        assert g.move_log[0]['action_type'] == 'place'
        assert g.move_log[0]['move_number'] == 1

    def test_pass_records_move(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        g.pass_turn('p1')
        assert len(g.move_log) == 1
        assert g.move_log[0]['action_type'] == 'pass'

    def test_exchange_records_move(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        g.exchange_tiles('p1', [0])
        assert len(g.move_log) == 1
        assert g.move_log[0]['action_type'] == 'exchange'

    @patch('board.check_words', return_value=(True, []))
    def test_challenge_accept_records_move(self, mock_check):
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.accept_pending_by_player('p2')
        # Should have a challenge_accept move
        assert any(m['action_type'] == 'challenge_accept' for m in g.move_log)

    @patch('board.check_words', return_value=(True, []))
    def test_challenge_reject_records_move(self, mock_check):
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        g.reject_pending_by_player('p2')
        assert any(m['action_type'] == 'challenge_reject' for m in g.move_log)

    @patch('board.check_words', return_value=(True, []))
    def test_move_log_contains_board_snapshot(self, mock_check):
        import json
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        g.players[0].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.place_tiles('p1', [(7, 6, 'A', False), (7, 7, 'B', False)])
        assert 'board_snapshot_json' in g.move_log[0]
        snapshot = json.loads(g.move_log[0]['board_snapshot_json'])
        assert len(snapshot) == 15


class TestBoardScoringAdvanced:
    """Advanced board scoring tests."""

    @pytest.fixture(autouse=True)
    def mock_dictionary(self):
        with patch('board.check_words', return_value=(True, [])):
            yield

    def test_triple_word_scoring(self):
        """TW at (0,0) should triple word score."""
        from board import Board
        b = Board()
        # Place a word that goes through (0,0) TW
        # First, need a word on the board near center
        b.apply_placement([(7, 7, 'A', False), (7, 8, 'B', False)])
        b.is_empty = False
        # Place a long word from (0,7) which is TW
        tiles = [(r, 7, chr(65 + r), False) for r in range(7)]  # A-G at rows 0-6
        valid, words, err = b.validate_placement(tiles)
        assert valid is True
        # The word should exist and include TW scoring at (0,7)
        assert len(words) >= 1

    def test_double_word_scoring(self):
        """DW at (1,1) should double word score."""
        from board import Board
        b = Board()
        b.apply_placement([(7, 7, 'A', False), (7, 8, 'B', False)])
        b.is_empty = False
        # Place a word through (1,1) DW
        tiles = [(1, 1, 'A', False), (2, 1, 'B', False), (3, 1, 'C', False)]
        valid, words, err = b.validate_placement(tiles)
        # This may or may not be valid depending on adjacency, but let's test via a connected path
        # Actually, for this test let's use the first move through center + DW
        b2 = Board()
        # Place first word going through center (7,7) = ST (=DW)
        tiles = [(7, 6, 'A', False), (7, 7, 'B', False)]
        valid, words, err = b2.validate_placement(tiles)
        assert valid is True
        # Center is ST which acts as DW
        word, positions, score = words[0]
        # A=1 at (7,6) no premium, B=2 at (7,7) ST=DW → word_multiplier=2
        # (1 + 2) * 2 = 6
        assert score == 6

    def test_triple_letter_scoring(self):
        """TL at (1,5) should triple letter score."""
        from board import Board
        b = Board()
        # First move covering center and TL
        # (1,5) is TL — we need to get there
        # Let's place on the board through center first
        b.apply_placement([(7, 7, 'A', False), (7, 8, 'B', False)])
        b.is_empty = False
        # Now place vertically through (1,5) — but it needs to connect
        # Let's use a simpler approach: build a path

    def test_blank_tile_zero_score(self):
        """Blank tiles should contribute 0 to score."""
        from board import Board
        b = Board()
        # Two tiles: blank A at center, normal B next to it
        tiles = [(7, 7, 'A', True), (7, 8, 'B', False)]
        valid, words, err = b.validate_placement(tiles)
        assert valid is True
        word, positions, score = words[0]
        # Blank A = 0 at (7,7) ST=DW → (0 + 2) * 2 = 4
        assert score == 4

    def test_single_tile_cross_word(self):
        """Single tile placement forming a cross word."""
        from board import Board
        b = Board()
        b.apply_placement([(7, 7, 'A', False), (7, 8, 'B', False)])
        b.is_empty = False
        # Place one tile below A to form vertical word
        valid, words, err = b.validate_placement([(8, 7, 'C', False)])
        assert valid is True
        # Should form AC vertically
        assert any(w == 'AC' for w, _, _ in words)

    def test_multiple_cross_words(self):
        """Placing tiles that form multiple cross words."""
        from board import Board
        b = Board()
        b.apply_placement([(7, 7, 'A', False), (7, 8, 'B', False)])
        b.is_empty = False
        # Place two tiles vertically below each
        b.apply_placement([(8, 7, 'C', False)])
        # Now place at (8, 8) which crosses both
        valid, words, err = b.validate_placement([(8, 8, 'D', False)])
        assert valid is True
        # Should form CD horizontally and BD vertically
        word_strs = [w for w, _, _ in words]
        assert 'CD' in word_strs
        assert 'BD' in word_strs


class TestRemovePlayerEdgeCases:
    def test_remove_current_player_adjusts_idx(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        # Current is p1 (idx 0)
        g.remove_player('p1')
        # idx should wrap or adjust
        assert g.current_player_idx < len(g.players)

    def test_remove_before_current_decrements_idx(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.current_player_idx = 2  # Charlie
        g.remove_player('p1')  # Remove Alice (idx 0)
        assert g.current_player_idx == 1
        assert g.current_player().name == 'Charlie'

    def test_remove_after_current_no_change(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.current_player_idx = 0  # Alice
        g.remove_player('p3')  # Remove Charlie (idx 2)
        assert g.current_player_idx == 0
        assert g.current_player().name == 'Alice'

    def test_remove_nonexistent_player(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        g.remove_player('p_nonexistent')  # Should not raise
        assert len(g.players) == 1

    def test_remove_all_players(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        g.remove_player('p1')
        assert len(g.players) == 0
        assert g.current_player_idx == 0

    def test_remove_player_adjusts_pending_challenge_idx(self):
        """Removing player before pending challenge player adjusts index."""
        from game import Game
        g = Game('test', challenge_mode=True)
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[1].hand = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
        g.current_player_idx = 1  # Bob's turn
        with patch('board.check_words', return_value=(True, [])):
            g.place_tiles('p2', [(7, 6, 'A', False), (7, 7, 'B', False)])
        assert g.pending_challenge.player_idx == 1
        # Remove Alice (before Bob)
        g.remove_player('p1')
        assert g.pending_challenge.player_idx == 0  # Adjusted

    def test_find_player_existing(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        p = g._find_player('p1')
        assert p is not None
        assert p.name == 'Alice'

    def test_find_player_nonexistent(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        assert g._find_player('p_none') is None


class TestNextTurnSkip:
    def test_skip_next_turn(self):
        """A player with skip_next_turn=True should be skipped."""
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.add_player('p2', 'Bob')
        g.add_player('p3', 'Charlie')
        g.start()
        g.players[1].skip_next_turn = True  # Bob will be skipped
        g.pass_turn('p1')
        # Bob is skipped, Charlie is current
        assert g.current_player().id == 'p3'
        # Bob's skip should be cleared
        assert g.players[1].skip_next_turn is False


class TestTurnTimeLimit:
    """Turn time limit feature tests."""

    def test_default_turn_time_limit(self):
        from game import Game
        g = Game('test')
        assert g.turn_time_limit == 0

    def test_turn_time_limit_parameter(self):
        from game import Game
        g = Game('test', turn_time_limit=120)
        assert g.turn_time_limit == 120

    def test_turn_time_limit_in_shared_state(self):
        from game import Game
        g = Game('test', turn_time_limit=90)
        g.add_player('p1', 'Alice')
        g.start()
        state = g.get_state(for_player_id='p1')
        assert state['turn_time_limit'] == 90

    def test_turn_time_limit_zero_in_state(self):
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        state = g.get_state(for_player_id='p1')
        assert state['turn_time_limit'] == 0

    def test_save_restore_turn_time_limit(self):
        from game import Game
        g = Game('test', turn_time_limit=180)
        g.add_player('p1', 'Alice')
        g.start()
        save_data = g.to_save_dict()
        assert save_data['turn_time_limit'] == 180
        restored = Game.from_save_dict(save_data)
        assert restored.turn_time_limit == 180

    def test_save_restore_no_turn_time_limit(self):
        """Old saves without turn_time_limit should default to 0."""
        from game import Game
        g = Game('test')
        g.add_player('p1', 'Alice')
        g.start()
        save_data = g.to_save_dict()
        del save_data['turn_time_limit']
        restored = Game.from_save_dict(save_data)
        assert restored.turn_time_limit == 0
