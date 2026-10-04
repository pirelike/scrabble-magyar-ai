import os
import re

# Szótár fájlok helye (a projektben a dict/ mappában)
_DICT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'dict')

_checker = None
_checker_type = None  # 'builtin', ha a beágyazott szótár betöltődött
_init_attempted = False  # a szótár inicializálását csak egyszer próbáljuk meg (ne minden szónál)

# Elutasított szavak: a szótár elfogadná őket, de emberi (vagy AI-) átnézés szerint nem rendes szavak.
# Két forrásból állnak össze, mindkettő pontos szóalakot (kisbetűs) zár ki, a ragozott alakokat nem:
#  - `dict/hu_rejected.txt`: a szótár-építő átnézések tartós listája (a szótárral együtt töltődik be);
#  - a felhasználói szavazatok (`word_review.py`): futás közben változik, az adatbázisból épül fel.
_REJECTED_PATH = os.path.join(_DICT_DIR, 'hu_rejected.txt')
_rejected_listed = frozenset()
_rejected_voted = set()
# Admin felülbírálatok (kisbetűs szavak): `allow` — a szó a szavazatok / a tartós lista ellenére érvényes (de a
# szótárban nem szereplő szót nem teszi érvényessé), `reject` — a szó érvénytelen, `additions` — saját szavak,
# amelyek a szótárban nem szerepelnek, de a játékban elfogadottak
_override_allow = frozenset()
_override_reject = frozenset()
_additions = frozenset()
_rejected_version = 0  # minden változásnál nő: a szótárból számolt gyorsítótárak ebből látják, hogy elavultak

# Magánhangzó nélküli tételek a szótárban: betűnevek (B, CS), rövidítések (KG, DB, SMS, TV, PDF)
# és indulatszavak. A Scrabble-ban a rövidítések és betűnevek nem érvényesek, az indulatszavak igen.
_HAS_VOWEL_RE = re.compile('[aáeéiíoóöőuúüű]')
_VOWELLESS_INTERJECTIONS = frozenset({'brr', 'brrr', 'hm', 'hmm', 'hmmm', 'khm', 'khmm', 'pszt'})


def load_checker():
    """A beágyazott hu_HU szótár ellenőrzője, a ténylegesen használt "kockázatos" alakok listájával
    (dict/hu_attested.txt; ha hiányzik, a kockázatos levezetések is érvényesek)."""
    from affix_checker import AffixChecker
    attested = os.path.join(_DICT_DIR, 'hu_attested.txt')
    return AffixChecker(os.path.join(_DICT_DIR, 'hu_HU.aff'), os.path.join(_DICT_DIR, 'hu_HU.dic'),
                        attested if os.path.exists(attested) else None)


def load_rejected(path=_REJECTED_PATH):
    """Az elutasított szavak listája (soronként egy szó, `#` megjegyzés): kisbetűs szavak halmaza.
    Hiányzó fájlnál üres."""
    words = set()
    try:
        with open(path, encoding='utf-8') as fh:
            for line in fh:
                word = line.split('#', 1)[0].strip().lower()
                if word:
                    words.add(word)
    except OSError:
        pass
    return frozenset(words)


def _init_checker():
    """Betölti a beágyazott hu_HU szótárat (dict/hu_HU.aff + .dic). Nincs külső függőség:
    sem a pyenchant/libenchant, sem a hunspell program nem szükséges."""
    global _checker, _checker_type, _init_attempted, _rejected_listed, _rejected_version
    _init_attempted = True
    try:
        _checker = load_checker()
        _checker_type = 'builtin'
        _rejected_listed = load_rejected()
        _rejected_version += 1
        print(f"Szótár: beágyazott hu_HU ({_checker.entry_count} szótő, {len(_rejected_listed)} elutasított szó)")
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
    if word in _override_reject:
        return False
    if word not in _override_allow and (word in _rejected_listed or word in _rejected_voted):
        return False
    if not _HAS_VOWEL_RE.search(word) and word not in _VOWELLESS_INTERJECTIONS:
        return False
    if word in _additions:
        return True
    return _checker.check(word)


# --- Elutasított szavak ---

def _ensure_init():
    if _checker_type is None and not _init_attempted:
        _init_checker()


def rejected_version():
    """Az elutasított szavak állapotának verziója (a számolt gyorsítótárak érvénytelenítéséhez)."""
    _ensure_init()
    return _rejected_version


def listed_rejected():
    """A `dict/hu_rejected.txt` szavai (kisbetűs halmaz)."""
    _ensure_init()
    return _rejected_listed


def is_rejected(word):
    """Igaz, ha a (bármilyen kis/nagybetűs) szót elutasították — akkor is, ha a szótár elfogadná."""
    _ensure_init()
    word = word.lower()
    if word in _override_reject:
        return True
    return word not in _override_allow and (word in _rejected_listed or word in _rejected_voted)


def voted_rejected():
    """A szavazatok alapján elutasított szavak (kisbetűs halmaz)."""
    _ensure_init()
    return frozenset(_rejected_voted)


def admin_lists():
    """Az admin felülbírálatok: (allow, reject, additions) kisbetűs halmazok."""
    _ensure_init()
    return _override_allow, _override_reject, _additions


def set_admin_lists(allow, reject, additions):
    """Az admin felülbírálatok és saját szavak teljes cseréje (indításkor és minden módosítás után)."""
    global _override_allow, _override_reject, _additions, _rejected_version
    _ensure_init()
    _override_allow = frozenset(w.lower() for w in allow)
    _override_reject = frozenset(w.lower() for w in reject)
    _additions = frozenset(w.lower() for w in additions)
    _rejected_version += 1
    _valid_cache.clear()


def reload_rejected(path=None):
    """A tartós lista (`dict/hu_rejected.txt`) újraolvasása a fájlból (az admin panel módosítása után)."""
    global _rejected_listed, _rejected_version
    _ensure_init()
    _rejected_listed = load_rejected(path or _REJECTED_PATH)
    _rejected_version += 1
    _valid_cache.clear()
    return len(_rejected_listed)


def rejected_path():
    return _REJECTED_PATH


def clear_valid_cache():
    """A szavak érvényességének gyorsítótára (a következő ellenőrzés újraszámol). Visszatér: a törölt tételek száma."""
    count = len(_valid_cache)
    _valid_cache.clear()
    return count


def set_voted_rejected(words):
    """A szavazatok alapján elutasított szavak teljes cseréje (indításkor, az adatbázisból)."""
    global _rejected_voted, _rejected_version
    _ensure_init()
    _rejected_voted = {w.lower() for w in words}
    _rejected_version += 1
    _valid_cache.clear()


def mark_voted_rejected(word, rejected):
    """Egy szó felvétele az elutasítottak közé (vagy kivétele belőle) a szavazatok alapján."""
    global _rejected_version
    _ensure_init()
    word = word.lower()
    if (word in _rejected_voted) == bool(rejected):
        return
    if rejected:
        _rejected_voted.add(word)
    else:
        _rejected_voted.discard(word)
    _rejected_version += 1
    _valid_cache.pop(word, None)


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
