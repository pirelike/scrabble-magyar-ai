import os
import re

# Szótár fájlok helye (a projektben a dict/ mappában)
_DICT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'dict')

_checker = None
_checker_type = None  # 'builtin', ha a beágyazott szótár betöltődött
_init_attempted = False  # a szótár inicializálását csak egyszer próbáljuk meg (ne minden szónál)

# Magánhangzó nélküli tételek a szótárban: betűnevek (B, CS), rövidítések (KG, DB, SMS, TV, PDF)
# és indulatszavak. A Scrabble-ban a rövidítések és betűnevek nem érvényesek, az indulatszavak igen.
_HAS_VOWEL_RE = re.compile('[aáeéiíoóöőuúüű]')
_VOWELLESS_INTERJECTIONS = frozenset({'brr', 'brrr', 'hm', 'hmm', 'hmmm', 'khm', 'khmm', 'pszt'})


def _init_checker():
    """Betölti a beágyazott hu_HU szótárat (dict/hu_HU.aff + .dic). Nincs külső függőség:
    sem a pyenchant/libenchant, sem a hunspell program nem szükséges."""
    global _checker, _checker_type, _init_attempted
    _init_attempted = True
    try:
        from affix_checker import AffixChecker
        _checker = AffixChecker(os.path.join(_DICT_DIR, 'hu_HU.aff'), os.path.join(_DICT_DIR, 'hu_HU.dic'))
        _checker_type = 'builtin'
        print(f"Szótár: beágyazott hu_HU ({_checker.entry_count} szótő)")
    except (OSError, ValueError) as exc:
        _checker = None
        _checker_type = None
        print(f"FIGYELEM: A szótár nem tölthető be ({exc}) — a szavak ellenőrzése ki van kapcsolva! "
              "Ellenőrizd a dict/hu_HU.aff és dict/hu_HU.dic fájlokat.")


def get_checker():
    """A beépített szóellenőrző (a ragozott alakok előállításához), vagy None, ha nem töltődött be."""
    if _checker_type is None and not _init_attempted:
        _init_checker()
    return _checker if _checker_type == 'builtin' else None


def warm_up():
    """Betölti a szótárat (szerverindításkor, hogy az első lerakásnál ne kelljen várni)."""
    if _checker_type is None and not _init_attempted:
        _init_checker()
    return is_available()


def is_available():
    """Igaz, ha működik a szótár-ellenőrzés (különben minden szó elfogadásra kerül)."""
    if _checker_type is None and not _init_attempted:
        _init_checker()
    return _checker_type == 'builtin'


def _lookup(word):
    """Egyetlen kisbetűs szó keresése a szótárban (ragozott alakokkal együtt)."""
    if not _HAS_VOWEL_RE.search(word) and word not in _VOWELLESS_INTERJECTIONS:
        return False
    return _checker.check(word)


# Érvényes magyar szó karakterek (nagybetűk + ékezetes betűk)
_VALID_WORD_RE = re.compile(r'^[A-ZÁÉÍÓÖŐÚÜŰ]+$')
_MAX_WORD_LENGTH = 15  # A tábla 15x15, szó nem lehet hosszabb


def check_words(words):
    """
    Ellenőrzi a szavak helyességét a magyar szótárral.
    words: lista szavakból (nagybetűs, pl. ["ALMA", "SZÉKEK"])
    Visszatér: (all_valid, invalid_words)
    """
    if not words:
        return True, []

    # Input sanitizálás: csak érvényes magyar betűkből álló szavakat engedünk
    for word in words:
        if not isinstance(word, str):
            return False, [str(word)]
        if len(word) > _MAX_WORD_LENGTH or not _VALID_WORD_RE.match(word):
            return False, [word]

    if _checker_type is None and not _init_attempted:
        _init_checker()

    # A táblán a szavak nagybetűsek, de kisbetűvel kell keresni: a nagy kezdőbetűs alak a
    # tulajdonneveket (BUDAPEST, DUNA) is elfogadná, a Scrabble-ban pedig azok nem érvényesek.
    if _checker_type == 'builtin':
        invalid = [w for w in words if not _lookup(w.lower())]
        return len(invalid) == 0, invalid

    # Nincs elérhető szótár — minden szót elfogadunk
    return True, []


# --- Tömeges ellenőrzés és javaslatok (AI ellenfél, szótár-böngésző) ---

_VALID_CACHE_LIMIT = 200_000
_valid_cache = {}  # {kisbetűs szó: bool}


def _remember(word, valid):
    if len(_valid_cache) >= _VALID_CACHE_LIMIT:
        _valid_cache.clear()
    _valid_cache[word] = valid


def _raw_check_many(words):
    """Szótárellenőrzés a már szűrt (kisbetűs, érvényes karakterű) szavakra.
    Visszatér: {szó: bool}. Szótár híján minden szót érvényesnek vesz (mint a check_words)."""
    if _checker_type == 'builtin':
        return {w: _lookup(w) for w in words}
    return {w: True for w in words}


def filter_valid(words):
    """Visszaadja a megadott (nagybetűs) szavak közül az érvényeseket, egyetlen szótárhívással.

    Az eredményt gyorsítótárazza. Hibás alakú (nem magyar betűs, túl hosszú) szó sosem érvényes.
    Visszatér: az érvényes szavak halmaza, az eredeti (nagybetűs) alakban.
    """
    if _checker_type is None and not _init_attempted:
        _init_checker()

    valid = set()
    unknown = {}
    for word in set(words):
        if not isinstance(word, str) or len(word) > _MAX_WORD_LENGTH or not _VALID_WORD_RE.match(word):
            continue
        key = word.lower()
        cached = _valid_cache.get(key)
        if cached is None:
            unknown[key] = word
        elif cached:
            valid.add(word)

    if unknown:
        for key, ok in _raw_check_many(list(unknown)).items():
            _remember(key, ok)
            if ok:
                valid.add(unknown[key])
    return valid


def is_word_valid(word):
    """Egyetlen (nagybetűs) szó ellenőrzése."""
    return word in filter_valid([word])


_SUGGEST_ALPHABET = 'aábcdeéfghiíjklmnoóöőpqrstuúüűvwxyz'
_ACCENT_BASE = str.maketrans('áéíóöőúüű', 'aeiooouuu')


def _edit_candidates(word):
    """A szóból egyetlen szerkesztéssel (betűcsere, törlés, beszúrás, felcserélés) kapható alakok,
    jóság szerint rendezve: ékezetcsere, két betű felcserélése, más betűcsere, törlés, beszúrás."""
    ranked = {}

    def add(candidate, rank):
        if candidate != word and candidate not in ranked:
            ranked[candidate] = rank

    n = len(word)
    for i in range(n):
        for letter in _SUGGEST_ALPHABET:
            if letter != word[i]:
                same_base = letter.translate(_ACCENT_BASE) == word[i].translate(_ACCENT_BASE)
                add(word[:i] + letter + word[i + 1:], 0 if same_base else 2)
    for i in range(n - 1):
        add(word[:i] + word[i + 1] + word[i] + word[i + 2:], 1)
    for i in range(n):
        add(word[:i] + word[i + 1:], 3)
    for i in range(n + 1):
        for letter in _SUGGEST_ALPHABET:
            add(word[:i] + letter + word[i:], 4)
    return sorted(ranked, key=lambda c: (ranked[c], c))


def suggest_words(word, limit=6):
    """Javaslatok egy hibás szóra (nagybetűs, a táblán kirakható szavak). Üres lista, ha nincs.
    A javaslatok a szóhoz egy betűnyi szerkesztéssel kapható, érvényes szavak."""
    if not isinstance(word, str) or not _VALID_WORD_RE.match(word.upper()) \
            or len(word) > _MAX_WORD_LENGTH:
        return []
    if _checker_type is None and not _init_attempted:
        _init_checker()
    if _checker_type != 'builtin':
        return []

    suggestions = []
    for candidate in _edit_candidates(word.lower()):
        if 2 <= len(candidate) <= _MAX_WORD_LENGTH and _lookup(candidate):
            suggestions.append(candidate.upper())
            if len(suggestions) >= limit:
                break
    return suggestions
