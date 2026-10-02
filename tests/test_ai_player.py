"""Az AI ellenfél motorjának tesztjei (ai_player.py)."""
import random
import statistics

import pytest

import ai_player
from ai_player import Vocabulary, Move, generate_moves, choose_action, best_moves
from board import Board, CENTER
from game import Game
from player import Player
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

    def test_top_level_orders_by_equity(self):
        moves = self._moves()
        moves[0].equity = 999.0
        ordered = ai_player._ordered_candidates(moves, ai_player.MAX_LEVEL, random.Random(1))
        assert ordered[0] is moves[0]
        equities = [m.equity for m in ordered]
        assert equities == sorted(equities, reverse=True)

    def test_mean_picked_score_rises_with_every_level(self):
        """Fokozatonként nő az átlagosan választott lépés pontszáma (a skála monoton)."""
        moves = self._moves()
        means = []
        for level in ai_player.DIFFICULTIES:
            picks = [ai_player._ordered_candidates(moves, level, random.Random(seed))[0].score
                     for seed in range(300)]
            means.append(statistics.mean(picks))
        assert all(a < b for a, b in zip(means, means[1:])), means

    def test_lowest_level_plays_the_weakest_moves(self):
        moves = self._moves()
        weakest_few = sorted(m.score for m in moves)[:3]
        for seed in range(100):
            first = ai_player._ordered_candidates(moves, ai_player.MIN_LEVEL, random.Random(seed))[0]
            assert first.score in weakest_few

    def test_middle_level_picks_near_its_target(self):
        """A 6. fokozat (célpontszám ~20) sem a legjobb, sem a leggyengébb lépést nem választja."""
        moves = self._moves()
        picks = [ai_player._ordered_candidates(moves, 6, random.Random(seed))[0].score
                 for seed in range(200)]
        assert 10 < statistics.mean(picks) < 32
        assert min(picks) < max(picks)  # nem mindig ugyanazt

    def test_all_moves_kept_as_fallbacks(self):
        moves = self._moves()
        for level in ai_player.DIFFICULTIES:
            ordered = ai_player._ordered_candidates(moves, level, random.Random(2))
            assert len(ordered) == len(moves)
            assert {id(m) for m in ordered} == {id(m) for m in moves}


class TestLevels:
    def test_there_are_ten_levels(self):
        assert ai_player.DIFFICULTIES == tuple(range(1, 11))
        assert set(ai_player._PROFILES) == set(ai_player.DIFFICULTIES)

    def test_legacy_names_map_onto_the_new_scale(self):
        assert ai_player.parse_level('easy') == 3
        assert ai_player.parse_level('medium') == 6
        assert ai_player.parse_level('hard') == 10

    def test_numbers_and_numeric_strings_are_accepted(self):
        assert ai_player.parse_level(1) == 1
        assert ai_player.parse_level(10) == 10
        assert ai_player.parse_level('7') == 7
        assert ai_player.parse_level(' Hard ') == 10
        assert ai_player.parse_level(4.0) == 4

    @pytest.mark.parametrize('bad', [0, 11, -1, 2.5, True, False, None, 'nonsense', '', '0', '11',
                                     '²', [], {}, (6,)])
    def test_invalid_values_are_rejected(self, bad):
        assert ai_player.parse_level(bad) is None

    def test_normalize_falls_back_to_the_default(self):
        assert ai_player.normalize_level(None) == ai_player.DEFAULT_LEVEL
        assert ai_player.normalize_level('nonsense') == ai_player.DEFAULT_LEVEL
        assert ai_player.normalize_level(8) == 8

    def test_default_level_is_the_old_medium(self):
        assert ai_player.DEFAULT_LEVEL == ai_player.LEGACY_LEVELS['medium']

    def test_bots_store_the_normalized_level(self):
        assert Player('b', 'Robi', is_bot=True, difficulty='hard').difficulty == 10
        assert Player('b', 'Robi', is_bot=True, difficulty=4).difficulty == 4
        assert Player('b', 'Robi', is_bot=True).difficulty == ai_player.DEFAULT_LEVEL
        assert Player('h', 'Ember', difficulty=4).difficulty is None


class _ZeroRng(random.Random):
    """Mindig 0-t ad: a "véletlen" esemény biztosan bekövetkezik."""

    def random(self):
        return 0.0


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

    def test_lowest_level_sometimes_gets_stuck(self):
        """Az 1. fokozat néha "nem talál" lépést: cserél, ha a zsák engedi, különben passzol."""
        rack = list('ALMAKÖR')
        assert choose_action(Board(), rack, 1, 80, rng=_ZeroRng())['action'] == 'exchange'
        assert choose_action(Board(), rack, 1, 3, rng=_ZeroRng()) == {'action': 'pass'}

    def test_other_levels_never_get_stuck_by_chance(self):
        rack = list('ALMAKÖR')
        for level in range(2, 11):
            assert choose_action(Board(), rack, level, 80, rng=_ZeroRng())['action'] == 'place'

    def test_legacy_names_still_choose_actions(self):
        for name in ('easy', 'medium', 'hard'):
            assert choose_action(Board(), list('ALMAKÖR'), name, 80,
                                 rng=random.Random(1))['action'] == 'place'

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


class TestStrengthOrdering:
    @staticmethod
    def _final_scores(levels, seed):
        random.seed(seed)  # a zsák keverése
        rng = random.Random(seed)
        game = Game('s')
        for i, level in enumerate(levels):
            game.add_bot(f'B{i}', level)
        game.start()
        for _ in range(200):
            if game.finished:
                break
            player = game.current_player()
            action = choose_action(game.board, list(player.hand), player.difficulty,
                                   game.bag.remaining(), rng=rng)
            ok = False
            if action['action'] == 'place':
                ok = game.place_tiles(player.id, action['tiles'])[0]
            elif action['action'] == 'exchange':
                ok = game.exchange_tiles(player.id, action['indices'])[0]
            if not ok:
                game.pass_turn(player.id)
        return [p.score for p in game.players]

    def test_a_much_stronger_level_outscores_a_weak_one(self):
        weak = strong = 0
        for seed in range(3):
            a, b = self._final_scores([2, 9], seed)
            weak, strong = weak + a, strong + b
        assert strong > weak * 2


class TestInflectedVocabulary:
    """A robot szókincse a tőszavak mellett a gyakori ragozott alakokat is tartalmazza."""

    @pytest.fixture(scope='class')
    def vocab(self):
        return ai_player.load_vocabulary()

    @pytest.fixture(scope='class')
    def stems_only(self):
        return ai_player.load_vocabulary(inflect=False)

    def test_common_inflected_forms_are_present(self, vocab, stems_only):
        # a tővégi a/e nyúlása is (alma → almában, almához)
        for word in ('ALMÁT', 'ALMÁK', 'ALMÁBAN', 'ALMÁHOZ', 'SZÉKEK', 'ASZTALNAK', 'HÁZBAN', 'KÖNYVET'):
            assert word in vocab, word
            assert word not in stems_only, word
        assert 'ALMA' in vocab and 'SZÉK' in vocab   # a tőszavak megmaradnak

    def test_all_forms_are_valid_in_the_game_dictionary(self, vocab, stems_only):
        import dictionary
        extra = sorted(set(vocab.words) - set(stems_only.words))
        assert len(extra) > 100_000
        step = max(1, len(extra) // 5000)
        sample = set(extra[::step])
        assert dictionary.filter_valid(sample) == sample

    def test_only_whitelisted_short_forms_are_generated(self, vocab, stems_only):
        extra = set(vocab.words) - set(stems_only.words)
        assert max(len(w) for w in extra) <= ai_player.INFLECT_MAX_FORM
        assert all(w == w.upper() for w in extra)

    def test_size_stays_bounded(self, vocab):
        assert 250_000 < len(vocab) < 500_000

    def test_membership_and_prefix_use_the_sorted_list(self, vocab):
        assert 'ALMÁKBAN' not in vocab         # két végződés egymásra: nincs felvéve
        assert vocab.has_prefix('ALMÁ')
        assert not vocab.has_prefix('ALMÁÁ')
        assert vocab.word_set.__contains__('ALMA')
        assert 'QWXZ' not in vocab.word_set

    def test_robot_can_play_an_inflected_word(self, vocab, stems_only):
        """Ha a legjobb lépés ragozott alak, a robot lerakja, és a játék szótára is elfogadja."""
        rack = list('ALMÁKTX')
        with_forms = {m.words[0] for m in generate_moves(Board(), rack, vocab=vocab, allow_blanks=False)}
        without = {m.words[0] for m in generate_moves(Board(), rack, vocab=stems_only, allow_blanks=False)}
        assert 'ALMÁK' in with_forms and 'ALMÁK' not in without

    def test_search_stays_fast_with_the_larger_vocabulary(self, vocab):
        import time
        board = _board_with('ALMA')
        started = time.monotonic()
        generate_moves(board, list('KÖRTEZS'), vocab=vocab, seconds=5)
        assert time.monotonic() - started < 2.0


class TestVocabularyLetters:
    def test_digraph_words_with_y_are_kept(self):
        vocab = ai_player.load_vocabulary()
        for word in ('KÖNYV', 'MEGGY', 'KÖNYVET'):
            assert word in vocab, word

    def test_words_without_tiles_are_excluded(self):
        from tiles import tokenize_word
        vocab = ai_player.load_vocabulary()
        assert 'ADYAS' not in vocab                      # d+y: nincs "dy" zseton
        assert all(tokenize_word(w) is not None for w in vocab.words)
