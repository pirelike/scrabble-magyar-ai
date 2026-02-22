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
for letter, value, count in TILE_DISTRIBUTION:
    TILE_VALUES[letter] = value


class TileBag:
    """Betűzseton zsák kezelése."""

    def __init__(self):
        self.tiles = []
        for letter, value, count in TILE_DISTRIBUTION:
            for _ in range(count):
                self.tiles.append(letter)
        random.shuffle(self.tiles)

    def draw(self, count):
        """Húz count darab zsetont a zsákból."""
        drawn = []
        for _ in range(min(count, len(self.tiles))):
            drawn.append(self.tiles.pop())
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
