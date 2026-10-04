import random


# Magyar Scrabble betűkészlet: (betű, pontérték, darabszám)
TILE_DISTRIBUTION = [
    ('', 0, 2),     # Üres zseton (joker)
    ('A', 1, 6),
    ('E', 1, 6),
    ('K', 1, 6),
    ('T', 1, 5),
    ('Á', 1, 4),
    ('L', 1, 4),
    ('N', 1, 4),
    ('R', 1, 4),
    ('I', 1, 3),
    ('M', 1, 3),
    ('O', 1, 3),
    ('S', 1, 3),
    ('B', 2, 3),
    ('D', 2, 3),
    ('G', 2, 3),
    ('Ó', 2, 3),
    ('É', 3, 3),
    ('H', 3, 2),
    ('SZ', 3, 2),
    ('V', 3, 2),
    ('F', 4, 2),
    ('GY', 4, 2),
    ('J', 4, 2),
    ('Ö', 4, 2),
    ('P', 4, 2),
    ('U', 4, 2),
    ('Ü', 4, 2),
    ('Z', 4, 2),
    ('C', 5, 1),
    ('Í', 5, 1),
    ('NY', 5, 1),
    ('CS', 7, 1),
    ('Ő', 7, 1),
    ('Ú', 7, 1),
    ('Ű', 7, 1),
    ('LY', 8, 1),
    ('ZS', 8, 1),
    ('TY', 10, 1),
]

# Pontérték szótár
TILE_VALUES = {}
# Darabszám szótár (üres zseton kulcsa: '')
TILE_COUNTS = {}
for letter, value, count in TILE_DISTRIBUTION:
    TILE_VALUES[letter] = value
    TILE_COUNTS[letter] = count

# A táblán használható betűk (üres zseton nélkül)
LETTERS = [letter for letter, _value, _count in TILE_DISTRIBUTION if letter]
DIGRAPHS = frozenset(letter for letter in LETTERS if len(letter) == 2)
VOWELS = frozenset('AÁEÉIÍOÓÖŐUÚÜŰ')


def forms_digraph(first, second):
    """Két szomszédos zseton egy-egy betűje kétjegyű betűt adna-e (S + Z = SZ)? Ez nem megengedett: a kétjegyű
    betűt (SZ, CS, GY, LY, NY, TY, ZS) csak a saját zsetonjával lehet kirakni."""
    return len(first) == 1 and len(second) == 1 and first + second in DIGRAPHS


def tokenize_word(word):
    """Szó felbontása zsetonokra (a kétkarakteres betűk: SZ, CS, ... egy zseton).

    A felbontás nem egyértelmű (pl. S+Z vagy SZ): a kevesebb zsetont használót adja
    (a magyar helyesírásban a kétjegyű betű egy betű), egyenlőség esetén a több pontot érőt.
    Visszatér: a betűk listája, vagy None, ha a szó olyan karaktert tartalmaz, amihez nincs zseton.
    """
    word = word.upper()
    n = len(word)
    best = [None] * (n + 1)   # best[i] = (zsetonok száma, -pontszám, [betűk]) a word[:i] felbontására
    best[0] = (0, 0, [])
    for i in range(n):
        if best[i] is None:
            continue
        count, neg_score, parts = best[i]
        for size in (1, 2):
            piece = word[i:i + size]
            if len(piece) != size or piece not in TILE_VALUES:
                continue
            candidate = (count + 1, neg_score - TILE_VALUES[piece], parts + [piece])
            current = best[i + size]
            if current is None or candidate[:2] < current[:2]:
                best[i + size] = candidate
    return best[n][2] if best[n] is not None else None


def word_base_score(word):
    """A szó betűértékeinek összege prémium mezők nélkül (None, ha nem rakható ki)."""
    tokens = tokenize_word(word)
    if tokens is None:
        return None
    return sum(TILE_VALUES[t] for t in tokens)


class TileBag:
    """Betűzseton zsák kezelése."""

    def __init__(self):
        self.tiles = [
            letter
            for letter, value, count in TILE_DISTRIBUTION
            for _ in range(count)
        ]
        random.shuffle(self.tiles)

    def draw(self, count):
        """Húz count darab zsetont a zsákból."""
        n = min(count, len(self.tiles))
        if n <= 0:
            # Fontos: a `del self.tiles[-0:]` a teljes zsákot törölné.
            return []
        drawn = self.tiles[-n:]
        del self.tiles[-n:]
        return drawn

    def put_back(self, tiles):
        """Visszateszi a zsetonokat és újrakeveri."""
        self.tiles.extend(tiles)
        random.shuffle(self.tiles)

    def remaining(self):
        """Hátralévő zsetonok száma."""
        return len(self.tiles)

    def is_empty(self):
        return len(self.tiles) == 0
