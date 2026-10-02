"""Az AI ellenfél motorjának tesztjei (ai_player.py)."""
import random

import pytest

import ai_player
from ai_player import Vocabulary, Move, generate_moves, choose_action, best_moves
from board import Board, CENTER
from game import Game
from tiles import TILE_VALUES


@pytest.fixture
def small_vocab():
    return Vocabulary(['ALMA', 'KÖR', 'ÖRÖK', 'KARÓ', 'ALAK', 'LAKK', 'RAK', 'KAR', 'ŐZ', 'AL', 'LÁ'])


def _board_with(word, row=CENTER, col=CENTER):
    board = Board()
    for i, letter in enumerate(word):
        board.set(row, col + i, letter)
    board.is_empty = False
    return board


class TestVocabulary:
    def test_load_real_dictionary(self):
        vocab = ai_player.load_vocabulary()
        assert len(vocab) > 50000
        assert 'ALMA' in vocab
        assert 'SZÉK' in vocab

    def test_abbreviations_without_vowels_are_excluded(self):
        vocab = ai_player.load_vocabulary()
        for abbr in ('KKV', 'TB', 'KG', 'MG'):
            assert abbr not in vocab

    def test_proper_nouns_are_excluded(self):
        vocab = ai_player.load_vocabulary()
        assert 'BUDAPEST' not in vocab

    def test_has_prefix(self, small_vocab):
        assert small_vocab.has_prefix('ALM')
        assert small_vocab.has_prefix('ALMA')
        assert not small_vocab.has_prefix('XYZ')
        assert not small_vocab.has_prefix('ALMAA')

    def test_words_are_uppercase_and_sorted(self):
        vocab = Vocabulary(['alma', 'Kör', 'ŐZ'])
        assert vocab.words == sorted(vocab.words)
        assert all(w == w.upper() for w in vocab.words)


class TestGenerateMoves:
    def test_first_move_covers_center_with_two_or_more_tiles(self, small_vocab):
        moves = generate_moves(Board(), list('ALMAKÖR'), vocab=small_vocab)
        assert moves
        for move in moves:
            assert len(move.tiles) >= 2
            assert any((r, c) == (CENTER, CENTER) for r, c, _l, _b in move.tiles)

    def test_first_move_finds_longest_word(self, small_vocab):
        moves = generate_moves(Board(), list('ALMAKÖR'), vocab=small_vocab)
        assert any(m.words == ['ALMA'] for m in moves)

    def test_score_matches_game_scoring(self, small_vocab):
        """A generátor pontszáma megegyezik azzal, amit a Game ad a lerakásra."""
        game = Game('t')
        game.add_player('A', 'A')
        game.add_player('B', 'B')
        game.start()
        game.current_player().hand = list('ALMAKÖR')
        moves = generate_moves(game.board, list(game.current_player().hand), vocab=small_vocab)
        # a tábla üres, ezért a lépés érvényességét a szótár is eldönti — csak a pontszámot vetjük össze
        move = max(moves, key=lambda m: m.score)
        valid, formed, _ = game.board.validate_placement(move.tiles, skip_dictionary=True)
        assert valid
        assert move.score == sum(s for _w, _p, s in formed)

    def test_extends_existing_word_through_board_tile(self, small_vocab):
        board = _board_with('KAR')
        moves = generate_moves(board, list('ÖALK'), vocab=small_vocab, allow_blanks=False)
        # a KAR-hoz kapcsolódó lerakások: mind érint vagy bővít meglévő betűt
        assert moves
        for move in moves:
            for r, c, _l, _b in move.tiles:
                assert board.get(r, c) is None

    def test_all_formed_words_are_valid(self):
        """Fő szó: a szókincsből; keresztszó: a szótár szerint érvényes — rosszat nem generál."""
        import dictionary
        vocab = Vocabulary(['KAR', 'ARC', 'KAC', 'RAK', 'AKAR', 'CAK'])
        board = _board_with('KAR')
        moves = generate_moves(board, ['A', 'C', 'K'], vocab=vocab, allow_blanks=False)
        assert moves
        for move in moves:
            for word in move.words:
                assert word in vocab.word_set or dictionary.is_word_valid(word), word

    def test_blank_tile_gets_a_letter(self, small_vocab):
        moves = generate_moves(Board(), ['A', 'L', 'M', ''], vocab=small_vocab)
        blank_moves = [m for m in moves if any(b for _r, _c, _l, b in m.tiles)]
        assert blank_moves
        for move in blank_moves:
            for _r, _c, letter, is_blank in move.tiles:
                if is_blank:
                    assert letter != ''

    def test_blank_not_used_when_disallowed(self, small_vocab):
        moves = generate_moves(Board(), ['A', 'L', 'M', ''], vocab=small_vocab, allow_blanks=False)
        assert all(not b for m in moves for _r, _c, _l, b in m.tiles)

    def test_digraph_tile_matches_two_characters(self):
        vocab = Vocabulary(['PÖCS', 'KÖCS'])
        moves = generate_moves(Board(), ['P', 'Ö', 'CS'], vocab=vocab, allow_blanks=False)
        assert any(m.words == ['PÖCS'] and len(m.tiles) == 3 for m in moves)

    def test_empty_rack_has_no_moves(self, small_vocab):
        assert generate_moves(Board(), [], vocab=small_vocab) == []

    def test_does_not_modify_the_real_board(self, small_vocab):
        board = _board_with('KAR')
        before = [list(row) for row in board.cells]
        generate_moves(board, list('ÖALK'), vocab=small_vocab)
        assert board.cells == before
        assert board.is_empty is False

    def test_seven_tile_move_gets_bonus(self):
        vocab = Vocabulary(['ALMAFÁK'])
        moves = generate_moves(Board(), list('ALMAFÁK'), vocab=vocab, allow_blanks=False)
        assert moves
        top = max(moves, key=lambda m: m.score)
        assert len(top.tiles) == 7
        assert top.score >= 50

    def test_time_budget_stops_the_search(self, small_vocab):
        # 0 másodperces keret: nem dob hibát, részleges (akár üres) eredményt ad
        moves = generate_moves(Board(), list('ALMAKÖR'), vocab=small_vocab, seconds=0)
        assert isinstance(moves, list)

    def test_yield_function_is_called_during_long_searches(self):
        calls = []
        vocab = ai_player.get_vocabulary()
        generate_moves(Board(), list('ALMAKÖR'), vocab=vocab, yield_fn=lambda: calls.append(1))
        assert calls


class TestLeaveAndExchange:
    def test_blank_in_leave_is_valuable(self):
        assert ai_player.leave_value(['']) > ai_player.leave_value(['K'])

    def test_duplicates_are_penalised(self):
        assert ai_player.leave_value(['A', 'A', 'A']) < ai_player.leave_value(['A', 'E', 'K'])

    def test_empty_leave_is_zero(self):
        assert ai_player.leave_value([]) == 0.0

    def test_exchange_keeps_blank(self):
        rack = ['', 'Ű', 'Ű', 'TY', 'LY', 'ZS', 'Ú']
        indices = ai_player.choose_exchange(rack, random.Random(1))
        assert 0 not in indices
        assert 1 <= len(indices) <= 6

    def test_exchange_never_empty(self):
        assert ai_player.choose_exchange(['A', 'E', 'K'], random.Random(1))


class TestDifficultyPolicy:
    def _moves(self):
        moves = []
        for i in range(20):
            tiles = [(CENTER, CENTER + k, 'A', False) for k in range(1 + i % 6)]
            move = Move(tiles, ['AA'], 5 + i * 3)
            move.equity = float(move.score)
            moves.append(move)
        return moves

    def test_hard_orders_by_equity(self):
        moves = self._moves()
        moves[0].equity = 999.0
        ordered = ai_player._ordered_candidates(moves, 'hard', random.Random(1))
        assert ordered[0] is moves[0]
        equities = [m.equity for m in ordered]
        assert equities == sorted(equities, reverse=True)

    def test_easy_prefers_short_weak_moves(self):
        moves = self._moves()
        short_scores = sorted((m.score for m in moves if len(m.tiles) <= 4), reverse=True)
        lower_half_max = short_scores[len(short_scores) // 2]
        for seed in range(30):
            first = ai_player._ordered_candidates(moves, 'easy', random.Random(seed))[0]
            assert len(first.tiles) <= 4
            assert first.score <= lower_half_max

    def test_medium_picks_from_top_third(self):
        moves = self._moves()
        by_score = sorted(moves, key=lambda m: -m.score)
        top = set(map(id, by_score[:max(3, len(by_score) * 3 // 10)]))
        for seed in range(15):
            ordered = ai_player._ordered_candidates(moves, 'medium', random.Random(seed))
            assert id(ordered[0]) in top

    def test_all_moves_kept_as_fallbacks(self):
        moves = self._moves()
        for level in ai_player.DIFFICULTIES:
            ordered = ai_player._ordered_candidates(moves, level, random.Random(2))
            assert len(ordered) == len(moves)
            assert {id(m) for m in ordered} == {id(m) for m in moves}


class TestChooseAction:
    def test_places_valid_word_on_empty_board(self):
        action = choose_action(Board(), list('ALMAKÖR'), 'hard', 80, rng=random.Random(3))
        assert action['action'] == 'place'
        assert action['score'] > 0
        game = Game('t')
        game.add_player('A', 'A')
        game.start()
        game.current_player().hand = list('ALMAKÖR')
        ok, msg, score = game.place_tiles('A', action['tiles'])
        assert ok, msg
        assert score == action['score']

    def test_exchanges_when_no_move_and_bag_is_full(self):
        vocab = Vocabulary(['ALMA'])
        action = choose_action(Board(), ['Ű'] * 7, 'hard', 50, vocab=vocab)
        assert action['action'] == 'exchange'
        assert action['indices']

    def test_passes_when_no_move_and_bag_is_empty(self):
        vocab = Vocabulary(['ALMA'])
        assert choose_action(Board(), ['Ű'] * 7, 'hard', 0, vocab=vocab) == {'action': 'pass'}

    def test_passes_when_bag_too_small_to_exchange(self):
        vocab = Vocabulary(['ALMA'])
        assert choose_action(Board(), ['Ű'] * 7, 'medium', 3, vocab=vocab)['action'] == 'pass'

    def test_unknown_difficulty_falls_back_to_default(self):
        action = choose_action(Board(), list('ALMAKÖR'), 'nonsense', 80, rng=random.Random(1))
        assert action['action'] in ('place', 'exchange', 'pass')

    @pytest.mark.parametrize('level', ai_player.DIFFICULTIES)
    def test_every_difficulty_plays_accepted_moves(self, level):
        """Önjáték: a robot minden lépését a játék is elfogadja (szótár + pontszám)."""
        random.seed(11)
        game = Game('t')
        game.add_player('A', 'A')
        game.add_player('B', 'B')
        game.start()
        for _ in range(6):
            player = game.current_player()
            action = choose_action(game.board, list(player.hand), level, game.bag.remaining(),
                                   rng=random.Random(5))
            if action['action'] == 'place':
                ok, msg, score = game.place_tiles(player.id, action['tiles'])
                assert ok, f"{action['words']}: {msg}"
                assert score == action['score']
            elif action['action'] == 'exchange':
                ok, msg = game.exchange_tiles(player.id, action['indices'])
                assert ok, msg
            else:
                game.pass_turn(player.id)


class TestBestMoves:
    def test_returns_sorted_valid_moves(self):
        moves = best_moves(Board(), list('ALMAKÖR'), count=3)
        assert 1 <= len(moves) <= 3
        scores = [m.score for m in moves]
        assert scores == sorted(scores, reverse=True)

    def test_hint_moves_are_playable(self):
        game = Game('t')
        game.add_player('A', 'A')
        game.start()
        player = game.current_player()
        moves = best_moves(game.board, list(player.hand), count=1)
        if moves:
            ok, msg, score = game.place_tiles('A', moves[0].tiles)
            assert ok, msg
            assert score == moves[0].score

    def test_unplayable_rack_returns_nothing(self):
        assert best_moves(Board(), ['Ű'] * 7, vocab=Vocabulary(['ALMA'])) == []


def test_tile_values_are_consistent_with_move_scores():
    """Egy betűs prémium nélküli szó pontszáma a zsetonértékek összege."""
    vocab = Vocabulary(['AL'])
    moves = generate_moves(Board(), ['A', 'L'], vocab=vocab, allow_blanks=False)
    assert moves
    # AL a középső (DW) mezőt fedi: (1+1)*2
    assert max(m.score for m in moves) == (TILE_VALUES['A'] + TILE_VALUES['L']) * 2
