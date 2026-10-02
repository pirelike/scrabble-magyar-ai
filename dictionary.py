import os
import re
import subprocess

# Szótár fájlok helye (a projektben a dict/ mappában)
_DICT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'dict')

# Beállítjuk a DICPATH-ot, hogy a pyenchant/hunspell megtalálja a szótárat
if os.path.isdir(_DICT_DIR):
    os.environ.setdefault('DICPATH', _DICT_DIR)

_checker = None
_checker_type = None
_init_attempted = False  # a szótár inicializálását csak egyszer próbáljuk meg (ne minden szónál)


def _init_checker():
    """Inicializálja a szótár-ellenőrzőt. Sorrendben próbálja: pyenchant, hunspell CLI."""
    global _checker, _checker_type, _init_attempted
    _init_attempted = True

    # 1. Próbáljuk a pyenchant-ot (cross-platform)
    try:
        import enchant
        _checker = enchant.Dict('hu_HU')
        _checker_type = 'enchant'
        print("Szótár: pyenchant (hu_HU)")
        return
    except Exception:
        pass

    # 2. Próbáljuk a hunspell CLI-t (Linux/macOS)
    try:
        dict_path = os.path.join(_DICT_DIR, 'hu_HU') if os.path.isdir(_DICT_DIR) else 'hu_HU'
        result = subprocess.run(
            ['hunspell', '-d', dict_path, '-l'],
            input='teszt',
            capture_output=True,
            text=True,
            timeout=5,
            env={**os.environ, 'DICPATH': _DICT_DIR},
        )
        _checker_type = 'cli'
        print("Szótár: hunspell CLI (hu_HU)")
        return
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass

    print("FIGYELEM: Szótár-ellenőrzés nem elérhető! Telepítsd a pyenchant csomagot és a dict/ mappa szótárfájljait.")
    _checker_type = None


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

    # A táblán a szavak nagybetűsek, de kisbetűvel kell keresni: a nagybetűs alak a
    # tulajdonneveket (BUDAPEST, DUNA) is elfogadja, a Scrabble-ban pedig azok nem érvényesek.
    if _checker_type == 'enchant':
        invalid = [w for w in words if not _checker.check(w.lower())]
        return len(invalid) == 0, invalid

    if _checker_type == 'cli':
        try:
            input_text = '\n'.join(w.lower() for w in words)
            dict_path = os.path.join(_DICT_DIR, 'hu_HU') if os.path.isdir(_DICT_DIR) else 'hu_HU'
            result = subprocess.run(
                ['hunspell', '-d', dict_path, '-l'],
                input=input_text,
                capture_output=True,
                text=True,
                timeout=5,
                env={**os.environ, 'DICPATH': _DICT_DIR},
            )
            flagged = {w.strip().lower() for w in result.stdout.strip().split('\n') if w.strip()}
            invalid = [w for w in words if w.lower() in flagged]
            return len(invalid) == 0, invalid
        except (subprocess.TimeoutExpired, FileNotFoundError):
            pass

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
    if _checker_type == 'enchant':
        return {w: bool(_checker.check(w)) for w in words}

    if _checker_type == 'cli' and words:
        try:
            dict_path = os.path.join(_DICT_DIR, 'hu_HU') if os.path.isdir(_DICT_DIR) else 'hu_HU'
            result = subprocess.run(
                ['hunspell', '-d', dict_path, '-l'],
                input='\n'.join(words),
                capture_output=True,
                text=True,
                timeout=15,
                env={**os.environ, 'DICPATH': _DICT_DIR},
            )
            flagged = {w.strip().lower() for w in result.stdout.split('\n') if w.strip()}
            return {w: w not in flagged for w in words}
        except (subprocess.TimeoutExpired, FileNotFoundError):
            pass

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


def suggest_words(word, limit=6):
    """Javaslatok egy hibás szóra (nagybetűs, a táblán kirakható szavak). Üres lista, ha nincs."""
    if not isinstance(word, str) or not _VALID_WORD_RE.match(word.upper()) \
            or len(word) > _MAX_WORD_LENGTH:
        return []
    if _checker_type is None and not _init_attempted:
        _init_checker()

    raw = []
    lowered = word.lower()
    if _checker_type == 'enchant':
        try:
            raw = list(_checker.suggest(lowered))
        except Exception:
            raw = []
    elif _checker_type == 'cli':
        try:
            dict_path = os.path.join(_DICT_DIR, 'hu_HU') if os.path.isdir(_DICT_DIR) else 'hu_HU'
            result = subprocess.run(
                ['hunspell', '-d', dict_path, '-a'],
                input=lowered + '\n',
                capture_output=True,
                text=True,
                timeout=8,
                env={**os.environ, 'DICPATH': _DICT_DIR},
            )
            for line in result.stdout.split('\n'):
                if line.startswith('&') and ':' in line:
                    raw = [s.strip() for s in line.split(':', 1)[1].split(',')]
                    break
        except (subprocess.TimeoutExpired, FileNotFoundError):
            raw = []

    suggestions = []
    for candidate in raw:
        up = candidate.upper()
        if up != word.upper() and len(up) <= _MAX_WORD_LENGTH and _VALID_WORD_RE.match(up) \
                and up not in suggestions:
            suggestions.append(up)
        if len(suggestions) >= limit:
            break
    return suggestions
