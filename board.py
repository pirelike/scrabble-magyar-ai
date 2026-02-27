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
_PREMIUM_QUARTER = [
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
        for rr, cc in [(r, c), (r, 14 - c), (14 - r, c), (14 - r, 14 - c)]:
            premium[(rr, cc)] = ptype
    return premium


PREMIUM_MAP = _build_premium_map()


class Board:
    """15x15 Scrabble tábla."""

    def __init__(self):
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

    def _has_letter(self, row, col):
        if 0 <= row < BOARD_SIZE and 0 <= col < BOARD_SIZE:
            return self.cells[row][col] is not None
        return False

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

    # --- Word extraction ---

    def _find_word_bounds(self, fixed, start, end, horizontal):
        """Kiterjeszti a szó határait a meglévő betűkkel."""
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
        """Kinyeri a szót és kiszámítja a pontszámát."""
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

    # --- Placement validation (broken into phases) ---

    def _validate_positions(self, tiles_placed):
        """Ellenőrzi a pozíciók érvényességét és a cellák foglaltságát.
        Visszatér: (ok, error) — ok=False esetén error az üzenet.
        """
        for r, c, letter, is_blank in tiles_placed:
            if not (0 <= r < BOARD_SIZE and 0 <= c < BOARD_SIZE):
                return False, f"A ({r},{c}) pozíció a táblán kívül van."
            if self.cells[r][c] is not None:
                return False, f"A ({r},{c}) mező már foglalt."
        return True, ""

    def _validate_alignment(self, positions):
        """Ellenőrzi, hogy a betűk egy sorban vagy oszlopban vannak-e.
        Visszatér: (ok, horizontal, error)
        """
        rows = set(r for r, c in positions)
        cols = set(c for r, c in positions)

        if len(rows) > 1 and len(cols) > 1:
            return False, True, "A betűknek egy sorban vagy egy oszlopban kell lenniük."
        horizontal = len(rows) == 1 or len(positions) == 1
        return True, horizontal, ""

    def _determine_direction_single(self, r, c):
        """Egy betűnél meghatározza az irányt a szomszédok alapján.
        Visszatér: (horizontal, has_neighbor)
        """
        has_h = self._has_letter(r, c - 1) or self._has_letter(r, c + 1)
        has_v = self._has_letter(r - 1, c) or self._has_letter(r + 1, c)
        if not self.is_empty and not has_h and not has_v:
            return True, False  # nincs szomszéd — hiba lesz
        return has_h or not has_v, True

    def _get_main_bounds(self, positions, horizontal):
        """Kiszámítja a fő irány fix tengelyét és határait."""
        if horizontal:
            rows = set(r for r, c in positions)
            fixed = list(rows)[0] if len(rows) == 1 else positions[0][0]
            start = min(c for r, c in positions)
            end = max(c for r, c in positions)
        else:
            cols = set(c for r, c in positions)
            fixed = list(cols)[0] if len(cols) == 1 else positions[0][1]
            start = min(r for r, c in positions)
            end = max(r for r, c in positions)
        return fixed, start, end

    def _check_continuity(self, fixed, start, end, horizontal):
        """Ellenőrzi, hogy a sor/oszlop folytonos-e (nincs üres rés)."""
        for i in range(start, end + 1):
            r, c = (fixed, i) if horizontal else (i, fixed)
            if self.cells[r][c] is None:
                return False
        return True

    def _check_adjacency(self, positions, new_positions):
        """Ellenőrzi, hogy a lerakott betűk csatlakoznak-e meglévőkhöz."""
        for r, c in positions:
            for dr, dc in [(-1, 0), (1, 0), (0, -1), (0, 1)]:
                nr, nc = r + dr, c + dc
                if (nr, nc) not in new_positions and self._has_letter(nr, nc):
                    return True
        return False

    def _collect_words(self, positions, new_positions, horizontal, fixed, start, end):
        """Összegyűjti az összes képzett szót (fő + mellékszavak)."""
        formed_words = []

        if end > start:
            formed_words.append(
                self._extract_word(fixed, start, end, horizontal, new_positions)
            )

        for r, c in positions:
            if horizontal:
                cs, ce = self._find_word_bounds(c, r, r, False)
                if ce > cs:
                    formed_words.append(
                        self._extract_word(c, cs, ce, False, new_positions)
                    )
            else:
                cs, ce = self._find_word_bounds(r, c, c, True)
                if ce > cs:
                    formed_words.append(
                        self._extract_word(r, cs, ce, True, new_positions)
                    )

        return formed_words

    def validate_placement(self, tiles_placed, skip_dictionary=False):
        """Ellenőrzi a lerakott zsetonokat.
        Visszatér: (valid, formed_words, error_message)
        """
        if not tiles_placed:
            return False, [], "Legalább egy zsetont le kell rakni."

        ok, err = self._validate_positions(tiles_placed)
        if not ok:
            return False, [], err

        positions = [(r, c) for r, c, _, _ in tiles_placed]
        new_positions = set(positions)

        ok, horizontal, err = self._validate_alignment(positions)
        if not ok:
            return False, [], err

        # Ideiglenesen lerakjuk a betűket
        for r, c, letter, is_blank in tiles_placed:
            self.cells[r][c] = (letter, is_blank)

        try:
            # Első lépés ellenőrzés
            if self.is_empty:
                if (CENTER, CENTER) not in new_positions:
                    return False, [], "Az első szónak a középső mezőt (csillagot) kell fednie."
                if len(tiles_placed) < 2:
                    return False, [], "Az első szónak legalább 2 betűből kell állnia."

            # Egy betűnél irány meghatározása szomszédok alapján
            if len(tiles_placed) == 1:
                horizontal, has_neighbor = self._determine_direction_single(*positions[0])
                if not self.is_empty and not has_neighbor:
                    return False, [], "A betűnek csatlakoznia kell meglévő szóhoz."

            # Fő irány határai
            fixed, start, end = self._get_main_bounds(positions, horizontal)
            start, end = self._find_word_bounds(fixed, start, end, horizontal)

            # Folytonosság
            if not self._check_continuity(fixed, start, end, horizontal):
                return False, [], "A betűknek folytonos sort kell alkotniuk."

            # Csatlakozás meglévőkhöz
            if not self.is_empty and not self._check_adjacency(positions, new_positions):
                return False, [], "A szónak csatlakoznia kell meglévő betűkhöz."

            # Szavak gyűjtése
            formed_words = self._collect_words(
                positions, new_positions, horizontal, fixed, start, end
            )

            if not formed_words:
                return False, [], "Legalább egy szót kell alkotni."

            # Szótár-ellenőrzés
            if not skip_dictionary:
                word_strings = [w for w, _, _ in formed_words]
                all_valid, invalid = check_words(word_strings)
                if not all_valid:
                    return False, [], f"Érvénytelen szó(k): {', '.join(invalid)}"

            return True, formed_words, ""

        finally:
            for r, c, _, _ in tiles_placed:
                self.cells[r][c] = None

    def apply_placement(self, tiles_placed):
        """Véglegesen lerakja a betűket."""
        for r, c, letter, is_blank in tiles_placed:
            self.cells[r][c] = (letter, is_blank)
        self.is_empty = False
