"""Szótár-építő eszközök: véletlen szavak mintavétele átnézésre (AI-val vagy kézzel) és az elutasított
szavak tartós listájának (dict/hu_rejected.txt) kezelése.

A hu_HU szótár helyesírás-ellenőrzésre készült, ezért a Scrabble-ban sok olyan szót is elfogad, amelyet senki
sem tekintene rendes szónak. Az alkalmazásban a gyakorló módok között a „Szótár-építő” ad véletlen szavakat az
embereknek; ez az eszköz ugyanezt teszi tömegesen, hogy egy AI-modell is átnézhessen ezreket:

    python tools/word_review.py sample 5000 --seed 1 --out sample.txt   # átnézendő szavak, soronként egy
    # ... az átnézés után a „nem rendes szó” ítéletek egy fájlba, soronként egy szó ...
    python tools/word_review.py apply rejected.txt                      # felvétel a dict/hu_rejected.txt-be
    python tools/word_review.py stats

A `sample` csak olyan szavakat ad, amelyeket a szótár most elfogad (a tőszavak és a gyakori ragozott alakok
7:3 arányban, mint az alkalmazásban). Az `apply` ellenőriz: csak a szótár által éppen elfogadott szó kerülhet
a listára (az elírt szavakat jelzi és kihagyja). A listán lévő szavakat a szótár pontos alakban zárja ki, és a
robot szókincséből is kimaradnak. Az ítéletek az alkalmazás Szótár-építőjében emberileg is felülvizsgálhatók.
"""
import argparse
import os
import random
import sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..')
sys.path.insert(0, ROOT)

import dictionary  # noqa: E402
import word_review  # noqa: E402

REJECTED_PATH = os.path.join(ROOT, 'dict', 'hu_rejected.txt')

DEFAULT_HEADER = """\
# Elutasított szavak: a hu_HU szótár elfogadná őket, de átnézés szerint nem rendes (használatos) szavak.
# A szótár a pontos szóalakot zárja ki (a ragozott alakokat nem), a robot szókincséből is kimaradnak.
# Karbantartás: tools/word_review.py (sample → átnézés → apply). Soronként egy kisbetűs szó, `#` megjegyzés.
"""


def read_words(path):
    """Szavak egy fájlból (soronként egy, `#` megjegyzés): kisbetűs halmaz."""
    return dictionary.load_rejected(path)


def split_header(path):
    """A fájl elején álló megjegyzés-blokk (a szavak előtt); hiányzó fájlnál az alapértelmezett fejléc."""
    header = []
    try:
        with open(path, encoding='utf-8') as fh:
            for line in fh:
                if line.strip() and not line.lstrip().startswith('#'):
                    break
                header.append(line)
    except OSError:
        return DEFAULT_HEADER
    return ''.join(header).rstrip('\n') + '\n' if any(h.strip() for h in header) else DEFAULT_HEADER


def write_rejected(path, words):
    header = split_header(path)
    with open(path, 'w', encoding='utf-8') as fh:
        fh.write(header)
        fh.write(''.join(f'{w}\n' for w in sorted(words)))


def cmd_sample(args):
    exclude = set(read_words(args.rejected))
    for path in args.exclude or ():
        exclude |= read_words(path)
    rng = random.Random(args.seed)
    words = word_review.sample_words(args.count, rng, exclude=exclude)
    text = ''.join(f'{w.lower()}\n' for w in words)
    if args.out:
        with open(args.out, 'w', encoding='utf-8') as fh:
            fh.write(text)
        print(f'{len(words)} szó -> {args.out}')
    else:
        sys.stdout.write(text)


def cmd_apply(args):
    proposed = read_words(args.file)
    current = set(read_words(args.rejected))
    checker = dictionary.get_checker()
    added, already, unknown = [], [], []
    for word in sorted(proposed):
        if word in current:
            already.append(word)
        elif word_review.normalize(word) is None or not checker.check(word):
            unknown.append(word)            # elírás vagy a szótár úgysem fogadja el: felesleges a listára tenni
        else:
            added.append(word)
    if added and not args.dry_run:
        write_rejected(args.rejected, current | set(added))
    print(f'felvéve: {len(added)}, már a listán volt: {len(already)}, a szótár úgysem fogadja el: {len(unknown)}'
          + (' (próbafuttatás, a lista nem módosult)' if args.dry_run else ''))
    if unknown:
        print('kihagyva (nincs a szótárban):', ', '.join(unknown))


def cmd_stats(args):
    print(f'elutasított szavak a listán ({os.path.relpath(args.rejected)}): {len(read_words(args.rejected))}')


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--rejected', default=REJECTED_PATH, help='az elutasított szavak listája')
    sub = parser.add_subparsers(dest='command', required=True)

    sample = sub.add_parser('sample', help='véletlen szavak átnézésre')
    sample.add_argument('count', type=int)
    sample.add_argument('--seed', type=int, default=None, help='a véletlen mag (reprodukálható minta)')
    sample.add_argument('--out', help='kimeneti fájl (alapból a standard kimenet)')
    sample.add_argument('--exclude', action='append', help='már átnézett szavak fájlja, ezek kimaradnak (többször is adható)')
    sample.set_defaults(func=cmd_sample)

    apply = sub.add_parser('apply', help='elutasított szavak felvétele a listára')
    apply.add_argument('file', help='a „nem rendes szó” ítéletek, soronként egy szó')
    apply.add_argument('--dry-run', action='store_true', help='csak az összesítő, a lista nem módosul')
    apply.set_defaults(func=cmd_apply)

    stats = sub.add_parser('stats', help='a lista mérete')
    stats.set_defaults(func=cmd_stats)

    args = parser.parse_args(argv)
    if args.command != 'stats' and not dictionary.warm_up():
        parser.error('a szótár nem tölthető be')
    args.func(args)


if __name__ == '__main__':
    main()
