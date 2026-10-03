"""A ténylegesen használt "kockázatos" szóalakok listájának előállítása (dict/hu_attested.txt).

A beépített szóellenőrző (affix_checker) néhány levezetést csak akkor fogad el, ha az alak a
használatban ténylegesen előfordul: melléknév + birtokos személyjel (KEDVESEM, de TAROM), -ék családi
többes (SZOMSZÉDÉK, de ÉJÉK), -ul/-ül (FELESÉGÜL, de BLÖKIÜL), -s foglalkozásnév és -né képző. Ez a
szkript egy szógyakorisági listából kiválogatja azokat az alakokat, amelyek csak ilyen levezetéssel
érvényesek, és legalább `--min-count`-szor előfordulnak. A feliratkorpusz zaját két szűrő csökkenti:
- elírás: kimarad az alak, ha egy legalább `TYPO_RATIO`-szor gyakoribb, kockázat nélkül érvényes szó
  ékezetek nélkül azonos vele, egy betű beszúrásával kapható belőle (KORUL ← KÖRÜL, TAROM ← TARTOM),
  vagy egy megkettőzött betűje a hiba (PROFII ← PROFI);
- tulajdonnév: kimarad, ha tulajdonnévből képzett (ALISA ← Ali, JULISA ← Juli).

Forrás: Hermit Dave, FrequencyWords (OpenSubtitles 2018, magyar), CC BY-SA 4.0
https://github.com/hermitdave/FrequencyWords — content/2018/hu/hu_full.txt (soronként: "szó darabszám").

Használat:
    python tools/build_attested.py hu_full.txt [--min-count 2] [--out dict/hu_attested.txt]
"""
import argparse
import os
import re
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))

from affix_checker import AffixChecker  # noqa: E402
from tiles import tokenize_word  # noqa: E402

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..')
DEFAULT_OUT = os.path.join(ROOT, 'dict', 'hu_attested.txt')
WORD_RE = re.compile(r'^[a-záéíóöőúüű]{2,15}$')
VOWEL_RE = re.compile(r'[aáeéiíoóöőuúüű]')
TYPO_RATIO = 10
_LETTERS = 'aábcdeéfghiíjklmnoóöőpqrstuúüűvwxyz'
_STRIP_ACCENTS = str.maketrans('áéíóöőúüű', 'aeiooouuu')

HEADER = """\
# A beépített szóellenőrző "kockázatos" levezetéseinek (melléknév + birtokos személyjel, -ék,
# -ul/-ül, -s foglalkozásnév, -né) ténylegesen használt alakjai — lásd affix_checker.py.
# Előállítva: tools/build_attested.py (legalább {min_count} előfordulás, elírás- és névszűrővel; {count} szó).
# Forrás: Hermit Dave, FrequencyWords (OpenSubtitles 2018, magyar), CC BY-SA 4.0
#   https://github.com/hermitdave/FrequencyWords
# Ez a fájl (a forrásból származtatott adat) szintén CC BY-SA 4.0 licencű.
"""


def candidates(path, min_count):
    """(szó, darabszám) a gyakorisági listából: kisbetűs, magyar betűs, a táblán kirakható szavak."""
    with open(path, encoding='utf-8', errors='replace') as fh:
        for line in fh:
            parts = line.split()
            if len(parts) != 2 or not parts[1].isdigit():
                continue
            word, count = parts[0], int(parts[1])
            if count < min_count:
                continue
            if not WORD_RE.match(word) or not VOWEL_RE.search(word) or tokenize_word(word.upper()) is None:
                continue
            yield word, count


def load_counts(path, min_count):
    """{szó: darabszám} a legalább `min_count`-szor előforduló kisbetűs szavakra."""
    counts = {}
    with open(path, encoding='utf-8', errors='replace') as fh:
        for line in fh:
            parts = line.split()
            if len(parts) == 2 and parts[1].isdigit() and int(parts[1]) >= min_count:
                counts[parts[0]] = int(parts[1])
    return counts


def is_typo(checker, word, count, frequent, by_skeleton):
    """Egy sokkal gyakoribb, kockázat nélkül érvényes szó elírása-e (ékezet hiánya, kimaradt vagy
    megkettőzött betű)?"""
    limit = TYPO_RATIO * count
    neighbours = set(by_skeleton.get(word.translate(_STRIP_ACCENTS), ()))
    neighbours.update(word[:i] + ch + word[i:] for i in range(len(word) + 1) for ch in _LETTERS)
    # megkettőzött betű (PROFII ← PROFI)
    neighbours.update(word[:i] + word[i + 1:] for i in range(1, len(word)) if word[i] == word[i - 1])
    neighbours.discard(word)
    return any(frequent.get(n, 0) >= limit and checker._compute(n, False) for n in neighbours)


def root_of(checker, word):
    """A (kockázatos) levezetés szótöve."""
    roots = []
    original = checker._stem_ok

    def spy(stem, *args, **kwargs):
        ok = original(stem, *args, **kwargs)
        if ok:
            roots.append(stem)
        return ok
    checker._stem_ok = spy
    try:
        checker._compute(word, True)
    finally:
        del checker._stem_ok
    return roots[0] if roots else None


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('frequency_list')
    parser.add_argument('--min-count', type=int, default=2)
    parser.add_argument('--out', default=DEFAULT_OUT)
    parser.add_argument('--with-counts', action='store_true', help='a darabszámot is kiírja (elemzéshez)')
    args = parser.parse_args()

    checker = AffixChecker(os.path.join(ROOT, 'dict', 'hu_HU.aff'), os.path.join(ROOT, 'dict', 'hu_HU.dic'))
    frequent = load_counts(args.frequency_list, TYPO_RATIO * args.min_count)
    by_skeleton = {}
    for w in frequent:
        by_skeleton.setdefault(w.translate(_STRIP_ACCENTS), []).append(w)

    attested, typos, names = {}, 0, 0
    for i, (word, count) in enumerate(candidates(args.frequency_list, args.min_count)):
        if checker.needs_attestation(word):
            root = root_of(checker, word)
            if root and root[:1].isupper():
                names += 1
            elif is_typo(checker, word, count, frequent, by_skeleton):
                typos += 1
            else:
                attested[word] = count
        if i % 100_000 == 0:
            print(f'{i} szó, {len(attested)} találat', file=sys.stderr)

    with open(args.out, 'w', encoding='utf-8') as out:
        out.write(HEADER.format(min_count=args.min_count, count=len(attested)))
        for word in sorted(attested):
            out.write(f'{word} {attested[word]}\n' if args.with_counts else f'{word}\n')
    print(f'{len(attested)} szó -> {args.out} (kiszűrve: {typos} elírás, {names} tulajdonnévből képzett)',
          file=sys.stderr)


if __name__ == '__main__':
    main()
