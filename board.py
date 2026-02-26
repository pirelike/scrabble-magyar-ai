from tiles import TILE_VALUES
from dictionary import check_words

BOARD_SIZE = 15
CENTER = 7  # 0-indexed

# Premium mező típusok
NONE = '.'
DL = 'DL'  # Dupla betű (világoskék)
TL = 'TL'  # Tripla betű (sötétkék)
DW = 'DW'  # Dupla szó (rózsaszín)
TW = 'TW'  # Tripla szó (piros)
ST = 'ST'  # Csillag (középső, = DW)

# Standard Scrabble tábla premium mező elrendezés
# Csak a negyedét definiáljuk, tükrözzük
_PREMIUM_QUARTER = [
    # (sor, oszlop, típus) - 0-indexed, bal felső negyed
    (0, 0, TW), (0, 3, DL), (0, 7, TW),
    (1, 1, DW), (1, 5, TL),
    (2, 2, DW), (2, 6, DL),
    (3, 0, DL), (3, 3, DW), (3, 7, DL),
    (4, 4, DW),
    (5, 1, TL), (5, 5, TL),
    (6, 2, DL), (6, 6, DL),
    (7, 0, TW), (7, 3, DL), (7, 7, ST),
]


def _build_premium_map():
    """Felépíti a teljes 15x15 premium mező térképet szimmetria alapján."""
    premium = {}
    for r, c, ptype in _PREMIUM_QUARTER:
        # Tükrözés mind a 4 negyedbe
        for rr, cc in [(r, c), (r, 14 - c), (14 - r, c), (14 - r, 14 - c)]:
            premium[(rr, cc)] = ptype
    return premium


PREMIUM_MAP = _build_premium_map()


class Board:
    """15x15 Scrabble tábla."""

    def __init__(self):
        # None = üres mező, egyébként (betű, is_blank) tuple
        self.cells = [[None] * BOARD_SIZE for _ in range(BOARD_SIZE)]
        self.is_empty = True

    def get(self, row, col):
        """Visszaadja a mező tartalmát: None vagy (betű, is_blank)."""
        if 0 <= row < BOARD_SIZE and 0 <= col < BOARD_SIZE:
            return self.cells[row][col]
        return None

    def set(self, row, col, letter, is_blank=False):
        """Betűt helyez a mezőre."""
        self.cells[row][col] = (letter, is_blank)

    def premium_at(self, row, col):
        """Visszaadja a premium mező típusát."""
        return PREMIUM_MAP.get((row, col), NONE)

    def to_dict(self):
        """Szerializálja a táblát JSON-kompatibilis formátumba."""
        result = []
        for r in range(BOARD_SIZE):
            row = []
            for c in range(BOARD_SIZE):
                cell = self.cells[r][c]
                if cell is None:
                    row.append(None)
                else:
                    row.append({'letter': cell[0], 'is_blank': cell[1]})
            result.append(row)
        return result

    def _find_word_bounds(self, fixed, start, end, horizontal):
        """Kiterjeszti a szó határait a meglévő betűkkel.
        horizontal=True: fixed=row, start/end=col tartomány
        horizontal=False: fixed=col, start/end=row tartomány
        """
        if horizontal:
            while start > 0 and self.cells[fixed][start - 1] is not None:
                start -= 1
            while end < 14 and self.cells[fixed][end + 1] is not None:
                end += 1
        else:
            while start > 0 and self.cells[start - 1][fixed] is not None:
                start -= 1
            while end < 14 and self.cells[end + 1][fixed] is not None:
                end += 1
        return start, end

    def _extract_word(self, fixed, start, end, horizontal, new_positions):
        """Kinyeri a szót és kiszámítja a pontszámát.
        horizontal=True: fixed=row, start..end=col tartomány
        horizontal=False: fixed=col, start..end=row tartomány
        """
        word = ""
        word_positions = []
        letter_score = 0
        word_multiplier = 1

        for i in range(start, end + 1):
            r, c = (fixed, i) if horizontal else (i, fixed)
            cell = self.cells[r][c]
            letter, is_blank = cell
            word += letter
            word_positions.append((r, c))

            tile_value = 0 if is_blank else TILE_VALUES.get(letter, 0)

            if (r, c) in new_positions:
                premium = self.premium_at(r, c)
                if premium == DL:
                    tile_value *= 2
                elif premium == TL:
                    tile_value *= 3
                elif premium in (DW, ST):
                    word_multiplier *= 2
                elif premium == TW:
                    word_multiplier *= 3

            letter_score += tile_value

        total = letter_score * word_multiplier
        return (word, word_positions, total)

    def validate_placement(self, tiles_placed, skip_dictionary=False):
        """
        Ellenőrzi a lerakott zsetonokat.
        tiles_placed: lista [(row, col, letter, is_blank), ...]
        skip_dictionary: ha True, kihagyja a szótár-ellenőrzést (challenge módhoz)

        Visszatér: (valid, formed_words, error_message)
        formed_words: lista [(word_str, [(row, col), ...], score), ...]
        """
        if not tiles_placed:
            return False, [], "Legalább egy zsetont le kell rakni."

        # Pozíció validálás és foglaltság ellenőrzés
        for r, c, letter, is_blank in tiles_placed:
            if not (0 <= r < BOARD_SIZE and 0 <= c < BOARD_SIZE):
                return False, [], f"A ({r},{c}) pozíció a táblán kívül van."
            if self.cells[r][c] is not None:
                return False, [], f"A ({r},{c}) mező már foglalt."

        positions = [(r, c) for r, c, _, _ in tiles_placed]
        new_positions = set(positions)
        rows = set(r for r, c in positions)
        cols = set(c for r, c in positions)

        # Egy sorban vagy egy oszlopban kell lenniük
        if len(rows) > 1 and len(cols) > 1:
            return False, [], "A betűknek egy sorban vagy egy oszlopban kell lenniük."

        horizontal = len(rows) == 1 or len(tiles_placed) == 1

        # Ideiglenesen lerakjuk a betűket
        for r, c, letter, is_blank in tiles_placed:
            self.cells[r][c] = (letter, is_blank)

        try:
            # Ha ez az első lépés, a középső mezőt kell fednie
            if self.is_empty:
                if (CENTER, CENTER) not in new_positions:
                    return False, [], "Az első szónak a középső mezőt (csillagot) kell fednie."
                if len(tiles_placed) < 2:
                    return False, [], "Az első szónak legalább 2 betűből kell állnia."

            # Meghatározzuk az irányt (1 betűnél mindkét irányt ellenőrizzük)
            if len(tiles_placed) == 1:
                r, c = positions[0]
                has_h_neighbor = (self._has_letter(r, c - 1) or self._has_letter(r, c + 1))
                has_v_neighbor = (self._has_letter(r - 1, c) or self._has_letter(r + 1, c))
                if not self.is_empty and not has_h_neighbor and not has_v_neighbor:
                    return False, [], "A betűnek csatlakoznia kell meglévő szóhoz."
                horizontal = has_h_neighbor or not has_v_neighbor

            # Fő irány: határok kiszámítása (egyszer, újrahasználjuk)
            if horizontal:
                main_fixed = list(rows)[0] if len(rows) == 1 else positions[0][0]
                main_start = min(c for r, c in positions)
                main_end = max(c for r, c in positions)
            else:
                main_fixed = list(cols)[0] if len(cols) == 1 else positions[0][1]
                main_start = min(r for r, c in positions)
                main_end = max(r for r, c in positions)

            main_start, main_end = self._find_word_bounds(
                main_fixed, main_start, main_end, horizontal
            )

            # Folytonosság ellenőrzése
            for i in range(main_start, main_end + 1):
                r, c = (main_fixed, i) if horizontal else (i, main_fixed)
                if self.cells[r][c] is None:
                    return False, [], "A betűknek folytonos sort kell alkotniuk."

            # Nem első lépésnél: csatlakozik-e meglévő betűhöz?
            if not self.is_empty:
                touches_existing = False
                for r, c in positions:
                    for dr, dc in [(-1, 0), (1, 0), (0, -1), (0, 1)]:
                        nr, nc = r + dr, c + dc
                        if (nr, nc) not in new_positions and self._has_letter(nr, nc):
                            touches_existing = True
                            break
                    if touches_existing:
                        break
                if not touches_existing:
                    return False, [], "A szónak csatlakoznia kell meglévő betűkhöz."

            # Összegyűjtjük az összes képzett szót
            formed_words = []

            # Fő szó (határok már kiszámítva)
            if main_end > main_start:
                word_info = self._extract_word(
                    main_fixed, main_start, main_end, horizontal, new_positions
                )
                formed_words.append(word_info)

            # Mellékszavak (keresztirányban)
            for r, c in positions:
                if horizontal:
                    cross_start, cross_end = self._find_word_bounds(c, r, r, False)
                    if cross_end > cross_start:
                        word_info = self._extract_word(c, cross_start, cross_end, False, new_positions)
                        formed_words.append(word_info)
                else:
                    cross_start, cross_end = self._find_word_bounds(r, c, c, True)
                    if cross_end > cross_start:
                        word_info = self._extract_word(r, cross_start, cross_end, True, new_positions)
                        formed_words.append(word_info)

            if not formed_words:
                return False, [], "Legalább egy szót kell alkotni."

            # Szótár-ellenőrzés (kihagyható challenge módban)
            if not skip_dictionary:
                word_strings = [w for w, _, _ in formed_words]
                all_valid, invalid = check_words(word_strings)
                if not all_valid:
                    inv_str = ', '.join(invalid)
                    return False, [], f"Érvénytelen szó(k): {inv_str}"

            return True, formed_words, ""

        finally:
            # Visszavonjuk az ideiglenes lerakást
            for r, c, _, _ in tiles_placed:
                self.cells[r][c] = None

    def _has_letter(self, row, col):
        if 0 <= row < BOARD_SIZE and 0 <= col < BOARD_SIZE:
            return self.cells[row][col] is not None
        return False

    def apply_placement(self, tiles_placed):
        """Véglegesen lerakja a betűket."""
        for r, c, letter, is_blank in tiles_placed:
            self.cells[r][c] = (letter, is_blank)
        self.is_empty = False
