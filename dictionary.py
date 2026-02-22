import os
import subprocess

# Szótár fájlok helye (a projektben a dict/ mappában)
_DICT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'dict')

# Beállítjuk a DICPATH-ot, hogy a pyenchant/hunspell megtalálja a szótárat
if os.path.isdir(_DICT_DIR):
    os.environ.setdefault('DICPATH', _DICT_DIR)

_checker = None
_checker_type = None


def _init_checker():
    """Inicializálja a szótár-ellenőrzőt. Sorrendben próbálja: pyenchant, hunspell CLI."""
    global _checker, _checker_type

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
        result = subprocess.run(
            ['hunspell', '-d', 'hu_HU', '-l'],
            input='teszt',
            capture_output=True,
            text=True,
            timeout=5,
        )
        _checker_type = 'cli'
        print("Szótár: hunspell CLI (hu_HU)")
        return
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass

    print("FIGYELEM: Szótár-ellenőrzés nem elérhető! Telepítsd a pyenchant csomagot és a dict/ mappa szótárfájljait.")
    _checker_type = None


def check_words(words):
    """
    Ellenőrzi a szavak helyességét a magyar szótárral.
    words: lista szavakból (nagybetűs, pl. ["ALMA", "SZÉKEK"])
    Visszatér: (all_valid, invalid_words)
    """
    global _checker, _checker_type

    if not words:
        return True, []

    if _checker_type is None:
        _init_checker()

    if _checker_type == 'enchant':
        invalid = [w for w in words if not _checker.check(w)]
        return len(invalid) == 0, invalid

    if _checker_type == 'cli':
        try:
            input_text = '\n'.join(words)
            result = subprocess.run(
                ['hunspell', '-d', 'hu_HU', '-l'],
                input=input_text,
                capture_output=True,
                text=True,
                timeout=5,
            )
            invalid = [w.strip() for w in result.stdout.strip().split('\n') if w.strip()]
            return len(invalid) == 0, invalid
        except (subprocess.TimeoutExpired, FileNotFoundError):
            pass

    # Nincs elérhető szótár — minden szót elfogadunk
    return True, []
