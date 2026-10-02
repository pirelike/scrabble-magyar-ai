"""Számítógépes ellenfél (AI): lépésgenerátor és döntési stratégia.

Működés:
  * Szókincs: a beágyazott hu_HU hunspell szótár tőszavai (ragozott alakok nélkül), egy rendezett
    listában. A prefixkeresés `bisect`-tel történik, így nincs szükség nagy memóriájú trie-ra.
  * Lépésgenerálás: horgonyalapú (Appel–Jacobson) keresés vízszintes és függőleges irányban.
    A keresztszavak érvényességét a hunspell szótár dönti el (egyetlen tömeges híváson belül), így a
    robot ragozott szavakhoz is tud kapcsolódni. A fő szó tőszó; a kiválasztott lépés minden szavát
    a játék saját szótára is ellenőrzi, mielőtt a robot lerakja.
  * Nehézségi szintek: könnyű (rövid, gyenge lépések), közepes (jó lépések közül véletlen),
    nehéz (a legjobb pontszám + maradék zsetonok értékelése, jokert is használ).
"""
import os
import random
import re
import time
from bisect import bisect_left
from collections import Counter

from board import Board, BOARD_SIZE, CENTER
from dictionary import filter_valid
from tiles import LETTERS, TILE_VALUES, VOWELS

HAND_SIZE = 7
BONUS_ALL_TILES = 50

DIFFICULTIES = ('easy', 'medium', 'hard')
DEFAULT_DIFFICULTY = 'medium'

# Gondolkodási időkeret másodpercben (a keresés ennyi után a eddigi legjobbal folytatja)
_TIME_BUDGET = {'easy': 1.0, 'medium': 2.0, 'hard': 3.5}

_DIC_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'dict', 'hu_HU.dic')
_STEM_RE = re.compile(r'^[a-záéíóöőúüű]{2,15}$')
_HAS_VOWEL_RE = re.compile(r'[aáeéiíoóöőuúüű]')


class _OutOfTime(Exception):
    """A keresés időkerete lejárt."""


# --- Szókincs ---

class Vocabulary:
    """Rendezett szólista gyors prefix- és tagsági kereséssel."""

    def __init__(self, words):
        self.words = sorted(set(w.upper() for w in words))
        self.word_set = frozenset(self.words)

    def has_prefix(self, prefix):
        i = bisect_left(self.words, prefix)
        return i < len(self.words) and self.words[i].startswith(prefix)

    def __contains__(self, word):
        return word in self.word_set

    def __len__(self):
        return len(self.words)


_vocabulary = None


def load_vocabulary(path=_DIC_PATH):
    """A hunspell .dic fájl tőszavaiból szókincset épít (csak kisbetűs, a táblán kirakható szavak)."""
    stems = set()
    with open(path, encoding='utf-8', errors='replace') as fh:
        next(fh, None)  # az első sor a szavak száma
        for line in fh:
            entry = line.strip().split('\t')[0].split('/')[0]
            # A magánhangzó nélküli bejegyzések rövidítések (kkv, tb, kg): a robot nem rak le ilyet
            if _STEM_RE.match(entry) and _HAS_VOWEL_RE.search(entry):
                stems.add(entry.upper())
    return Vocabulary(stems)


def get_vocabulary():
    """A szókincs lusta betöltése (az első robotlépésnél, egyszer)."""
    global _vocabulary
    if _vocabulary is None:
        _vocabulary = load_vocabulary()
    return _vocabulary


def set_vocabulary(vocabulary):
    """Szókincs cseréje (tesztekhez / előtöltéshez)."""
    global _vocabulary
    _vocabulary = vocabulary


# --- Lépés ---

class Move:
    """Egy lehetséges lerakás: zsetonok, a képzett szavak és a pontszám."""
    __slots__ = ('tiles', 'words', 'score', 'equity')

    def __init__(self, tiles, words, score):
        self.tiles = tiles      # [(row, col, letter, is_blank), ...]
        self.words = words      # a képzett szavak (fő + mellék)
        self.score = score
        self.equity = float(score)

    def __repr__(self):
        return f'Move({self.words}, {self.score} pont, {len(self.tiles)} zseton)'


# --- Keresés ---

class _Budget:
    """Időkeret + együttműködő (eventlet) ütemezés a hosszú keresés alatt."""

    def __init__(self, seconds, yield_fn):
        self.deadline = time.monotonic() + seconds
        self.yield_fn = yield_fn
        self.ticks = 0

    def tick(self):
        self.ticks += 1
        if self.ticks & 511 == 0:
            if self.yield_fn:
                self.yield_fn()
            if time.monotonic() > self.deadline:
                raise _OutOfTime()


def _contexts(grid):
    """Az üres mezők függőleges környezete: {(sor, oszlop): (fent, lent)}.
    Csak azok a mezők szerepelnek, ahol fent vagy lent áll betű (ott keresztszó keletkezik)."""
    ctx = {}
    for r in range(BOARD_SIZE):
        for c in range(BOARD_SIZE):
            if grid[r][c] is not None:
                continue
            up = ''
            rr = r - 1
            while rr >= 0 and grid[rr][c] is not None:
                up = grid[rr][c] + up
                rr -= 1
            down = ''
            rr = r + 1
            while rr < BOARD_SIZE and grid[rr][c] is not None:
                down += grid[rr][c]
                rr += 1
            if up or down:
                ctx[(r, c)] = (up, down)
    return ctx


def _allowed_letters(contexts, letters, vocab):
    """Minden keresztszavas mezőre kiszámolja, mely betűk rakhatók oda érvényes keresztszóval.

    contexts: [ctx_dict, ...] — az összes irány egyetlen szótárhívással ellenőrződik.
    Visszatér: egy {(sor, oszlop): {betűk}} dict minden irányhoz.
    """
    entries = []
    unknown = set()
    for ctx in contexts:
        for square, (up, down) in ctx.items():
            for letter in letters:
                word = up + letter + down
                entries.append((id(ctx), square, letter, word))
                if word not in vocab.word_set:
                    unknown.add(word)
    valid_unknown = filter_valid(unknown) if unknown else set()

    result = {id(ctx): {square: set() for square in ctx} for ctx in contexts}
    for ctx_id, square, letter, word in entries:
        if word in vocab.word_set or word in valid_unknown:
            result[ctx_id][square].add(letter)
    return [result[id(ctx)] for ctx in contexts]


def _anchors(grid, empty_board):
    """Horgonyok: az üres mezők, amelyek szomszédosak egy betűvel (üres táblán a közép)."""
    if empty_board:
        return [(CENTER, CENTER)]
    anchors = []
    for r in range(BOARD_SIZE):
        for c in range(BOARD_SIZE):
            if grid[r][c] is not None:
                continue
            for dr, dc in ((-1, 0), (1, 0), (0, -1), (0, 1)):
                rr, cc = r + dr, c + dc
                if 0 <= rr < BOARD_SIZE and 0 <= cc < BOARD_SIZE and grid[rr][cc] is not None:
                    anchors.append((r, c))
                    break
    return anchors


def _search(grid, allowed, anchors, rack_counts, blanks, vocab, budget, transposed, out):
    """Vízszintes keresés a `grid` soraiban (a függőleges irányt az átfordított rács adja).
    A talált lerakásokat az `out` dict-be gyűjti (kulcs: a lerakás halmaza)."""
    n = BOARD_SIZE
    words = vocab.words
    word_set = vocab.word_set
    anchor_set = set(anchors)
    nwords = len(words)

    def has_prefix(prefix):
        i = bisect_left(words, prefix)
        return i < nwords and words[i].startswith(prefix)

    for (r, anchor) in anchors:
        row = grid[r]
        counts = dict(rack_counts)
        free_blanks = blanks

        def record(placed):
            tiles = []
            for col, letter, is_blank in placed:
                tiles.append((col, r, letter, is_blank) if transposed else (r, col, letter, is_blank))
            out.setdefault(frozenset(tiles), tiles)

        def extend_right(col, prefix, placed, counts, free_blanks):
            budget.tick()
            if col >= n or row[col] is None:
                if col > anchor and len(prefix) >= 2 and prefix in word_set:
                    record(placed)
                if col >= n:
                    return
                allowed_here = allowed.get((r, col))
                for letter in list(counts):
                    if counts[letter] <= 0:
                        continue
                    if allowed_here is not None and letter not in allowed_here:
                        continue
                    new_prefix = prefix + letter
                    if not has_prefix(new_prefix):
                        continue
                    counts[letter] -= 1
                    placed.append((col, letter, False))
                    extend_right(col + 1, new_prefix, placed, counts, free_blanks)
                    placed.pop()
                    counts[letter] += 1
                if free_blanks > 0:
                    for letter in LETTERS:
                        if allowed_here is not None and letter not in allowed_here:
                            continue
                        new_prefix = prefix + letter
                        if not has_prefix(new_prefix):
                            continue
                        placed.append((col, letter, True))
                        extend_right(col + 1, new_prefix, placed, counts, free_blanks - 1)
                        placed.pop()
            else:
                new_prefix = prefix + row[col]
                if has_prefix(new_prefix):
                    extend_right(col + 1, new_prefix, placed, counts, free_blanks)

        # A horgonytól balra álló összefüggő betűk a szó fix eleje
        start = anchor
        while start > 0 and row[start - 1] is not None:
            start -= 1
        if start < anchor:
            extend_right(anchor, ''.join(row[start:anchor]), [], counts, free_blanks)
            continue

        # Egyébként a bal oldali üres (nem horgony) mezők hossza korlátozza a bal részt
        limit = 0
        cc = anchor - 1
        while cc >= 0 and row[cc] is None and (r, cc) not in anchor_set and limit < HAND_SIZE - 1:
            limit += 1
            cc -= 1

        def left_part(prefix, left, counts, free_blanks):
            budget.tick()
            placed = [(anchor - len(left) + i, letter, is_blank) for i, (letter, is_blank) in enumerate(left)]
            extend_right(anchor, prefix, placed, counts, free_blanks)
            if len(left) >= limit:
                return
            for letter in list(counts):
                if counts[letter] <= 0:
                    continue
                new_prefix = prefix + letter
                if not has_prefix(new_prefix):
                    continue
                counts[letter] -= 1
                left.append((letter, False))
                left_part(new_prefix, left, counts, free_blanks)
                left.pop()
                counts[letter] += 1
            if free_blanks > 0:
                for letter in LETTERS:
                    new_prefix = prefix + letter
                    if not has_prefix(new_prefix):
                        continue
                    left.append((letter, True))
                    left_part(new_prefix, left, counts, free_blanks - 1)
                    left.pop()

        left_part('', [], counts, free_blanks)


def generate_moves(board, rack, vocab=None, allow_blanks=True, max_tiles=HAND_SIZE,
                   seconds=3.0, yield_fn=None):
    """Az összes (a szókincs alapján) lehetséges lerakás a megadott kézzel.

    A fő szó tőszó (szókincs), a keresztszavak a hunspell szerint érvényesek. A fő szavakat a
    hívónak még ellenőriznie kell a játék szótárával (lásd `_first_valid`).
    board: Board; rack: a kéz betűi ('' = üres zseton).
    Visszatér: Move-ok listája (pontszámmal), rendezetlenül.
    """
    vocab = vocab or get_vocabulary()
    rack_counts = Counter(t for t in rack if t)
    blanks = sum(1 for t in rack if not t) if allow_blanks else 0
    if not rack_counts and not blanks:
        return []

    grid = [[cell[0] if cell else None for cell in row] for row in board.cells]
    transposed_grid = [list(col) for col in zip(*grid)]
    empty_board = board.is_empty

    letters = set(LETTERS) if blanks else set(rack_counts)
    budget = _Budget(seconds, yield_fn)
    found = {}

    try:
        ctx_h = _contexts(transposed_grid)   # vízszintes irány keresztszavai = oszlopok az átfordított rácson
        ctx_v = _contexts(grid)              # függőleges irány keresztszavai
        # Vízszintes keresés: keresztszavak függőlegesek (ctx_v); függőleges keresésnél fordítva
        allowed_v, allowed_h = _allowed_letters([ctx_v, ctx_h], letters, vocab)

        _search(grid, allowed_v, _anchors(grid, empty_board), rack_counts, blanks,
                vocab, budget, False, found)
        if not empty_board:
            t_anchors = _anchors(transposed_grid, False)
            _search(transposed_grid, allowed_h, t_anchors, rack_counts, blanks,
                    vocab, budget, True, found)
    except _OutOfTime:
        pass

    # Pontozás a játék saját táblaszámításával (privát másolaton, a valódi tábla érintetlen)
    scoring = Board()
    scoring.cells = [list(row) for row in board.cells]
    scoring.is_empty = board.is_empty

    moves = []
    for tiles in found.values():
        if len(tiles) > max_tiles:
            continue
        valid, formed, _err = scoring.validate_placement(tiles, skip_dictionary=True)
        if not valid:
            continue
        score = sum(s for _w, _p, s in formed)
        if len(tiles) == HAND_SIZE:
            score += BONUS_ALL_TILES
        moves.append(Move(tiles, [w for w, _p, _s in formed], score))
    return moves


# --- Értékelés és döntés ---

def leave_value(leave):
    """A lerakás után kézben maradó zsetonok (heurisztikus) értéke."""
    if not leave:
        return 0.0
    value = 0.0
    counts = Counter(leave)
    value += 12.0 * counts.get('', 0)
    for letter, count in counts.items():
        if letter and count > 1:
            value -= 2.5 * (count - 1)
        if letter and TILE_VALUES.get(letter, 0) >= 7:
            value -= 1.0 * count
    vowels = sum(c for t, c in counts.items() if t and t[0] in VOWELS)
    consonants = sum(c for t, c in counts.items() if t and t[0] not in VOWELS)
    ideal_vowels = round(0.42 * (vowels + consonants))
    value -= 1.5 * abs(vowels - ideal_vowels)
    return value


def _remaining_after(rack, move):
    left = list(rack)
    for _r, _c, letter, is_blank in move.tiles:
        left.remove('' if is_blank else letter)
    return left


def _rate(moves, rack, bag_remaining):
    """Kitölti a lépések `equity` értékét (nehéz szinthez)."""
    for move in moves:
        leave = _remaining_after(rack, move)
        if bag_remaining == 0:
            # Végjáték: a kézben maradó zsetonok pontja levonódik, a kiürítés bónuszt ér
            move.equity = move.score - sum(TILE_VALUES.get(t, 0) for t in leave) * 0.8
        else:
            move.equity = move.score + leave_value(leave)


def _first_valid(candidates, wanted=1, batch=8):
    """A rendezett jelöltek közül az első `wanted` darabot adja, amelynek minden szava érvényes
    a játék szótárában."""
    chosen = []
    for start in range(0, len(candidates), batch):
        chunk = candidates[start:start + batch]
        valid = filter_valid({w for m in chunk for w in m.words})
        for move in chunk:
            if len(chosen) < wanted and all(w in valid for w in move.words):
                chosen.append(move)
        if len(chosen) >= wanted:
            break
    return chosen


def _ordered_candidates(moves, difficulty, rng):
    """A jelöltek preferencia szerinti sorrendje a nehézségi szint alapján."""
    if difficulty == 'hard':
        return sorted(moves, key=lambda m: (-m.equity, rng.random()))

    by_score = sorted(moves, key=lambda m: -m.score)
    if difficulty == 'easy':
        # Rövid szavak, a gyengébb lépések közül véletlen
        short = [m for m in by_score if len(m.tiles) <= 4] or by_score
        half = short[len(short) // 2:] or short
        pool = list(half)
        rng.shuffle(pool)
        rest = [m for m in by_score if m not in pool]
        return pool + rest

    # közepes: a legjobb ~30% közül véletlen, majd a többi pontszám szerint
    top = max(3, len(by_score) * 3 // 10)
    pool = by_score[:top]
    rng.shuffle(pool)
    return pool + by_score[top:]


def choose_exchange(rack, rng=None):
    """Cserére kiválasztott kéz-indexek: a jokert és a jó betűket megtartja."""
    rng = rng or random
    def keep_worth(tile):
        if tile == '':
            return 100
        value = TILE_VALUES.get(tile, 0)
        base = 6 if value <= 1 else (3 if value <= 3 else 0)
        return base + (2 if tile[0] in VOWELS else 0)

    order = sorted(range(len(rack)), key=lambda i: (-keep_worth(rack[i]), rng.random()))
    keep, seen = [], Counter()
    for i in order:
        if len(keep) >= 3:
            break
        if seen[rack[i]] == 0:
            keep.append(i)
            seen[rack[i]] += 1
    exchange = [i for i in range(len(rack)) if i not in keep]
    return exchange or [0]


def choose_action(board, rack, difficulty, bag_remaining, rng=None, vocab=None, yield_fn=None,
                  avoid=None):
    """Eldönti, mit lép a robot.

    avoid: kerülendő lerakások (a tiles halmaza) — pl. a szavazással elutasítottak.

    Visszatér egy dict-et:
      {'action': 'place', 'tiles': [...], 'words': [...], 'score': n}
      {'action': 'exchange', 'indices': [...]}
      {'action': 'pass'}
    """
    rng = rng or random
    if difficulty not in DIFFICULTIES:
        difficulty = DEFAULT_DIFFICULTY

    moves = generate_moves(
        board, rack, vocab=vocab,
        allow_blanks=(difficulty != 'easy'),
        seconds=_TIME_BUDGET[difficulty], yield_fn=yield_fn,
    )
    if avoid:
        moves = [m for m in moves if frozenset(m.tiles) not in avoid]
    if difficulty == 'hard' or bag_remaining == 0:
        _rate(moves, rack, bag_remaining)
    candidates = _ordered_candidates(moves, difficulty, rng)
    best = _first_valid(candidates)

    if best:
        move = best[0]
        return {'action': 'place', 'tiles': move.tiles, 'words': move.words, 'score': move.score}
    if bag_remaining >= HAND_SIZE:
        return {'action': 'exchange', 'indices': choose_exchange(rack, rng)}
    return {'action': 'pass'}


def best_moves(board, rack, count=3, vocab=None, yield_fn=None):
    """A `count` legjobb (szótár szerint érvényes) lépés tippként, pontszám szerint rendezve."""
    moves = generate_moves(board, rack, vocab=vocab, allow_blanks=True,
                           seconds=_TIME_BUDGET['hard'], yield_fn=yield_fn)
    moves.sort(key=lambda m: -m.score)
    return _first_valid(moves, wanted=count)
