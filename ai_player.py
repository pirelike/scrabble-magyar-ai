"""Számítógépes ellenfél (AI): lépésgenerátor és döntési stratégia.

Működés:
  * Szókincs: a beágyazott hu_HU szótár tőszavai (ragozott alakok nélkül), egy rendezett
    listában. A prefixkeresés `bisect`-tel történik, így nincs szükség nagy memóriájú trie-ra.
  * Lépésgenerálás: horgonyalapú (Appel–Jacobson) keresés vízszintes és függőleges irányban.
    A keresztszavak érvényességét a szótár dönti el (egyetlen tömeges híváson belül), így a
    robot ragozott szavakhoz is tud kapcsolódni. A fő szó tőszó; a kiválasztott lépés minden szavát
    a játék saját szótára is ellenőrzi, mielőtt a robot lerakja.
  * Nehézségi szintek: 10 fokozat (1 = újonc … 10 = mester). Az alsó fokozatokon a robot minden
    körben egy véletlen célpontszámot vesz fel, és a hozzá legközelebbi pontszámú lépést rakja le;
    a felső fokozatokon a legjobban értékelt (pont + maradék zsetonok) lépést választja, egyre
    kisebb zajjal. A fokozatok erejét bot–bot mérés kalibrálta (lásd `_PROFILES`).
"""
import math
import os
import random
import re
import time
from bisect import bisect_left
from collections import Counter

from board import Board, BOARD_SIZE, CENTER
import dictionary
from dictionary import filter_valid
from tiles import LETTERS, TILE_VALUES, VOWELS

HAND_SIZE = 7
BONUS_ALL_TILES = 50

MIN_LEVEL = 1
MAX_LEVEL = 10
DEFAULT_LEVEL = 6
DIFFICULTIES = tuple(range(MIN_LEVEL, MAX_LEVEL + 1))
DEFAULT_DIFFICULTY = DEFAULT_LEVEL

# A korábbi háromfokozatú skála (könnyű / közepes / nehéz) helye az új skálán: a régi mentések és
# kliensek ugyanilyen erős robotot kapnak (egyenrangú ellenfelekkel mérve 38 / 53 / 49% volt).
LEGACY_LEVELS = {'easy': 3, 'medium': 6, 'hard': 10}
_LEVEL_TEXTS = {str(level): level for level in DIFFICULTIES}

# Gondolkodási időkeret másodpercben (a keresés ennyi után az eddigi legjobbal folytatja). A robot
# erejét a lépéskiválasztás szabja meg, nem az időkeret: a keresés a legtöbb táblán ennél jóval
# hamarabb véget ér, így a szintek ereje nem függ a szerver sebességétől.
_TIME_BUDGET = 1.5
_HINT_TIME_BUDGET = 3.5

# Fokozatonkénti erősség. Kétféle választási mód van:
#   mu    — célpontszámos: a körönkénti célpontszám átlaga (lognormális szórással), a robot a hozzá
#           legközelebbi pontszámú lépést rakja le;
#   sigma — értékeléses: a legjobb (pont + maradék zsetonok értéke) lépés, az értékelésre rakott
#           Gauss-zajjal (0 = mindig a legjobb);
#   pass_p — esély, hogy a robot "nem talál" lépést, és passzol / cserél.
# Mért átlagos pontszám körönként (bot–bot önjáték, a passzok is számítanak, ragozott alakokkal a
# szókincsben): 4,3 · 6,1 · 7,5 · 10,3 · 13,0 · 16,1 · 19,3 · 23,2 · 25,3 · 28,0 (`LEVEL_STRENGTH`).
# Újramérés: `python tools/bot_arena.py ladder`; utána a `LEVEL_STRENGTH` értékeit is frissíteni kell.
_PROFILES = {
    1: {'mu': 2, 'pass_p': 0.15},
    2: {'mu': 5},
    3: {'mu': 8},
    4: {'mu': 12},
    5: {'mu': 16},
    6: {'mu': 20},
    7: {'mu': 26},
    8: {'sigma': 8},
    9: {'sigma': 4},
    10: {'sigma': 0},
}
_TARGET_CV = 0.5  # a célpontszám relatív szórása

# "Igazodik hozzám" mód: a robot az ember utolsó köreinek átlagához állítja az erejét. A mért
# átlagok (fent) között lineárisan interpolálunk, a tört fokozatot a két szomszédos fokozat
# véletlenszerű keverése valósítja meg (pl. 5,3 → 70%-ban 5., 30%-ban 6. fokozat).
ADAPTIVE = 'auto'
LEVEL_STRENGTH = (4.3, 6.1, 7.5, 10.3, 13.0, 16.1, 19.3, 23.2, 25.3, 28.0)  # pont/kör fokozatonként
ADAPT_WINDOW = 6          # az ember utolsó ennyi köre számít
ADAPT_START_LEVEL = 5.0   # induló fokozat, amíg nincs adat (kevés adatnál efelé húz)

# Ragozott alakok a szókincsben: szótő + egy végződés a hunspell szabályaival. A teljes szabályrendszer
# több tízmillió alakot adna, ezért csak a leggyakoribb esetragokat, a birtokos és az igei végződéseket
# vesszük fel, a rövidebb (legfeljebb INFLECT_MAX_STEM betűs) szótövekhez: ~250 000 alak, ~35 MB.
_INFLECT_ADDS = frozenset(
    # többes szám, tárgyrag (a magánhangzó-nyúlásos alakokkal) és esetragok
    'k ak ek ok ök ák ék t at et ot öt át ét ban ben ba be ból ből ra re ról ről on en ön hoz hez höz '
    'tól től nak nek val vel ig ért nál nél ul ül ként '
    # birtokos személyjelek
    'm d a e ja je unk ünk uk ük om em öm od ed öd '
    # igei és képzett alakok
    'ik sz ol el öl tok tek tök ott ett ött tam tem tál tél tunk tünk tak ni va ve ó ő ás és'.split())
INFLECT_MAX_STEM = 6
INFLECT_MAX_FORM = 12

_DIC_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'dict', 'hu_HU.dic')
_STEM_RE = re.compile(r'^[a-záéíóöőúüű]{2,15}$')
_HAS_VOWEL_RE = re.compile(r'[aáeéiíoóöőuúüű]')


class _OutOfTime(Exception):
    """A keresés időkerete lejárt."""


def parse_level(value):
    """A robot fokozata (1–10) a megadott értékből, vagy None, ha érvénytelen.

    Elfogadja az egész számot, a számot szövegként ("6"), és a korábbi neveket
    ('easy' / 'medium' / 'hard')."""
    if isinstance(value, bool):
        return None
    if isinstance(value, float) and value.is_integer():
        value = int(value)
    if isinstance(value, int):
        return value if MIN_LEVEL <= value <= MAX_LEVEL else None
    if isinstance(value, str):
        text = value.strip().lower()
        return LEGACY_LEVELS.get(text) or _LEVEL_TEXTS.get(text)
    return None


def normalize_level(value):
    """Mint a `parse_level`, de érvénytelen vagy hiányzó érték esetén az alapértelmezett fokozat."""
    level = parse_level(value)
    return level if level is not None else DEFAULT_LEVEL


def parse_difficulty(value):
    """A robot beállított nehézsége: fokozat (1–10) vagy `ADAPTIVE` ("igazodik hozzám"); None, ha
    érvénytelen."""
    if isinstance(value, str) and value.strip().lower() in (ADAPTIVE, 'adaptive'):
        return ADAPTIVE
    return parse_level(value)


def normalize_difficulty(value):
    """Mint a `parse_difficulty`, de érvénytelen érték esetén az alapértelmezett fokozat."""
    difficulty = parse_difficulty(value)
    return difficulty if difficulty is not None else DEFAULT_LEVEL


def level_for_average(average):
    """Az átlagos pont/körnek megfelelő (tört) fokozat a mért `LEVEL_STRENGTH` skálán."""
    strength = LEVEL_STRENGTH
    if average <= strength[0]:
        return float(MIN_LEVEL)
    if average >= strength[-1]:
        return float(MAX_LEVEL)
    for i in range(len(strength) - 1):
        if average <= strength[i + 1]:
            return (i + 1) + (average - strength[i]) / (strength[i + 1] - strength[i])
    return float(MAX_LEVEL)


def adaptive_level(average, turns, rng=None):
    """Az "igazodik hozzám" robot fokozata (egész szám) az ember legutóbbi köreinek alapján.

    average: az ember átlagos pont/köre (None, ha még nem lépett); turns: a figyelembe vett körök
    száma. Kevés adatnál az induló fokozat felé húz, `ADAPT_WINDOW` körtől a mért átlagot követi.
    A tört fokozatot a két szomszédos fokozat véletlenszerű keverése adja."""
    rng = rng or random
    if average is None or turns <= 0:
        target = ADAPT_START_LEVEL
    else:
        weight = min(turns, ADAPT_WINDOW) / ADAPT_WINDOW
        target = ADAPT_START_LEVEL * (1 - weight) + level_for_average(average) * weight
    low = math.floor(target)
    level = low + 1 if rng.random() < target - low else low
    return max(MIN_LEVEL, min(MAX_LEVEL, level))


# --- Szókincs ---

class _SortedMembership:
    """Tagsági vizsgálat egy rendezett listán (`bisect`): a külön halmaz memóriáját spórolja meg,
    ami a ~400 000 szavas szókincsnél jelentős (a keresés így is milliszekundumos)."""
    __slots__ = ('_words',)

    def __init__(self, words):
        self._words = words

    def __contains__(self, word):
        words = self._words
        i = bisect_left(words, word)
        return i < len(words) and words[i] == word


class Vocabulary:
    """Rendezett szólista gyors prefix- és tagsági kereséssel."""

    def __init__(self, words):
        self.words = sorted(set(w.upper() for w in words))
        self.word_set = _SortedMembership(self.words)

    def has_prefix(self, prefix):
        i = bisect_left(self.words, prefix)
        return i < len(self.words) and self.words[i].startswith(prefix)

    def __contains__(self, word):
        return word in self.word_set

    def __len__(self):
        return len(self.words)


_vocabulary = None


def load_vocabulary(path=_DIC_PATH, inflect=True):
    """A hunspell .dic fájl tőszavaiból (és azok gyakori ragozott alakjaiból, ha `inflect`) építi a
    szókincset — csak kisbetűs, a táblán kirakható szavak."""
    stems = set()
    with open(path, encoding='utf-8', errors='replace') as fh:
        next(fh, None)  # az első sor a szavak száma
        for line in fh:
            entry = line.strip().split('\t')[0].split('/')[0]
            # A magánhangzó nélküli bejegyzések rövidítések (kkv, tb, kg): a robot nem rak le ilyet
            if _STEM_RE.match(entry) and _HAS_VOWEL_RE.search(entry):
                stems.add(entry.upper())
    words = set(stems)
    checker = dictionary.get_checker() if inflect else None
    if checker is not None:
        short = [s.lower() for s in stems if len(s) <= INFLECT_MAX_STEM]
        forms = checker.inflected_forms(short, _INFLECT_ADDS, INFLECT_MAX_FORM)
        words.update(f.upper() for f in forms if _STEM_RE.match(f) and _HAS_VOWEL_RE.search(f))
    return Vocabulary(words)


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
    """Időkeret + együttműködő (gevent) ütemezés a hosszú keresés alatt."""

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
    """Kitölti a lépések `equity` értékét (az értékeléses fokozatokhoz)."""
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


def _uses_equity(level):
    """Értékeléses (nem célpontszámos) fokozat: a lépések `equity` értékét ki kell tölteni."""
    return 'sigma' in _PROFILES[level]


def _ordered_candidates(moves, level, rng):
    """A jelöltek preferencia szerinti sorrendje a fokozat alapján (az első a legkedvezőbb).

    Célpontszámos fokozatnál a körönként sorsolt célpontszámhoz legközelebbi pontszámú lépés az első;
    értékeléses fokozatnál a legnagyobb (zajjal terhelt) `equity`. Az összes lépés megmarad
    tartaléknak arra az esetre, ha a legkedvezőbbet a játék szótára elutasítja."""
    profile = _PROFILES[normalize_level(level)]
    if 'mu' in profile:
        target = profile['mu'] * math.exp(rng.gauss(-_TARGET_CV ** 2 / 2, _TARGET_CV))
        return sorted(moves, key=lambda m: (abs(m.score - target), rng.random()))
    sigma = profile['sigma']
    if sigma:
        return sorted(moves, key=lambda m: -(m.equity + rng.gauss(0, sigma)))
    return sorted(moves, key=lambda m: (-m.equity, rng.random()))


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

    difficulty: a robot fokozata (1–10); a korábbi nevek ('easy' / 'medium' / 'hard') is érvényesek,
    érvénytelen érték esetén az alapértelmezett fokozat érvényes.
    avoid: kerülendő lerakások (a tiles halmaza) — pl. a szavazással elutasítottak.

    Visszatér egy dict-et:
      {'action': 'place', 'tiles': [...], 'words': [...], 'score': n}
      {'action': 'exchange', 'indices': [...]}
      {'action': 'pass'}
    """
    rng = rng or random
    level = normalize_level(difficulty)

    pass_p = _PROFILES[level].get('pass_p', 0)
    if pass_p and rng.random() < pass_p:
        # "Nem talál" lépést (a legalsó fokozat néha elakad, mint a kezdő játékosok)
        if bag_remaining >= HAND_SIZE:
            return {'action': 'exchange', 'indices': choose_exchange(rack, rng)}
        return {'action': 'pass'}

    moves = generate_moves(board, rack, vocab=vocab, allow_blanks=True,
                           seconds=_TIME_BUDGET, yield_fn=yield_fn)
    if avoid:
        moves = [m for m in moves if frozenset(m.tiles) not in avoid]
    if _uses_equity(level):
        _rate(moves, rack, bag_remaining)
    candidates = _ordered_candidates(moves, level, rng)
    best = _first_valid(candidates)

    if best:
        move = best[0]
        return {'action': 'place', 'tiles': move.tiles, 'words': move.words, 'score': move.score}
    if bag_remaining >= HAND_SIZE:
        return {'action': 'exchange', 'indices': choose_exchange(rack, rng)}
    return {'action': 'pass'}


def best_moves(board, rack, count=3, vocab=None, yield_fn=None, seconds=None):
    """A `count` legjobb (szótár szerint érvényes) lépés tippként, pontszám szerint rendezve.
    seconds: a keresés időkerete (alapértelmezés: a tipp időkerete)."""
    moves = generate_moves(board, rack, vocab=vocab, allow_blanks=True,
                           seconds=seconds or _HINT_TIME_BUDGET, yield_fn=yield_fn)
    moves.sort(key=lambda m: -m.score)
    return _first_valid(moves, wanted=count)
