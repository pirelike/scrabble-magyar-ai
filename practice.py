"""Gyakorló módok: „Melyik szó érvényes?” kvíz, betűvadászat / bingó-edző és a rövid szavak listája.

Állapotmentes: a kvíz kérdései csak a szót tartalmazzák, a választ a szerver a játék saját szótárával
ellenőrzi (`check_answer`); a betűvadászat kezét (`make_rack`) a megoldásokkal együtt kapja a kliens, a
nem listázott szavakat `check_rack_word` bírálja el. Az érvénytelen kvízszavak hihető „félreírások”: egy
érvényes szó egy zsetonnyi módosítása, vagy (csapda mód) egy magánhangzó hosszúságának cseréje (a↔á...).
"""
import random
from collections import Counter
from itertools import product

import ai_player
import dictionary
from tiles import LETTERS, TILE_COUNTS, TILE_DISTRIBUTION, TILE_VALUES, VOWELS, tokenize_word, word_base_score

QUESTION_COUNT = 10
MAX_QUESTIONS = 30
QUIZ_MODES = ('mixed', '2', '3', 'tricky')
MIN_TILES, MAX_TILES = 3, 9          # a vegyes kvíz szavainak hossza zsetonban
INFLECTED_SHARE = 0.3                # az érvényes kérdések ekkora része ragozott alak
SHORT_LENGTHS = (2, 3)

RACK_SIZE = 7
BINGO_BONUS = 50                     # mind a hét zseton kirakása (mint a játékban)
RACK_KINDS = ('hunt', 'bingo')
MAX_RACK_WORDS = 200                 # a betűvadászat szólistájának felső korlátja
HUNT_MIN_WORDS, HUNT_MAX_WORDS = 12, 150
HUNT_MIN_LONGEST = 5                 # a „jó” kézben van legalább ekkora (zsetonos) szó

# Hosszú ↔ rövid magánhangzó párok: a magyar helyesírás legtipikusabb csapdái
_LENGTH_PAIRS = {'a': 'á', 'e': 'é', 'i': 'í', 'o': 'ó', 'ö': 'ő', 'u': 'ú', 'ü': 'ű'}
_LENGTH_PAIRS.update({v: k for k, v in list(_LENGTH_PAIRS.items())})

_VOWEL_TILES = [t for t in LETTERS if t[0] in VOWELS]
_CONSONANT_TILES = [t for t in LETTERS if t[0] not in VOWELS]
_short_cache = {}
_stem_cache = None


def warm_up():
    """A gyakorló módok lusta adatainak előállítása (szerverindításkor: az első kérés ne várjon másodperceket)."""
    _stems()
    for length in SHORT_LENGTHS:
        short_words(length)


def _is_valid(word):
    return word in dictionary.filter_valid({word})


def _stems():
    """A szótár tőszavai (nagybetűs), a robot szókincsének szűrésével: csak kirakható, magánhangzós szavak."""
    global _stem_cache
    if _stem_cache is None:
        _stem_cache = [w.upper() for w in dictionary.get_checker()._entries
                       if 2 <= len(w) <= 15 and tokenize_word(w) is not None and any(c in 'aáeéiíoóöőuúüű' for c in w)
                       and w.islower()]
    return _stem_cache


def short_words(length):
    """Az összes érvényes, pontosan `length` zsetonos szó pontértékkel: [{'word', 'score'}], ábécé szerint."""
    if length not in SHORT_LENGTHS:
        raise ValueError('a rövid szavak hossza 2 vagy 3 zseton')
    if length not in _short_cache:
        candidates = {''.join(combo) for combo in product(LETTERS, repeat=length)}
        candidates = {w for w in candidates
                      if len(tokenize_word(w) or ()) == length and any(c in VOWELS for c in w)}
        valid = dictionary.filter_valid(candidates)
        _short_cache[length] = sorted(
            ({'word': w, 'score': word_base_score(w)} for w in valid), key=lambda e: e['word'])
    return _short_cache[length]


def _mutate(word, rng):
    """Egy érvényes szó egy zsetonnyi módosítása (hihető, de többnyire érvénytelen alak)."""
    tokens = tokenize_word(word)
    if not tokens or len(tokens) < 2:
        return None
    kind = rng.choice(('replace', 'replace', 'swap', 'insert', 'delete'))
    i = rng.randrange(len(tokens))
    if kind == 'replace':
        pool = _VOWEL_TILES if tokens[i][0] in VOWELS else _CONSONANT_TILES
        tokens[i] = rng.choice([t for t in pool if t != tokens[i]])
    elif kind == 'swap' and len(tokens) >= 2:
        j = min(i, len(tokens) - 2)
        tokens[j], tokens[j + 1] = tokens[j + 1], tokens[j]
    elif kind == 'insert':
        tokens.insert(i, rng.choice(_VOWEL_TILES if rng.random() < 0.4 else _CONSONANT_TILES))
    elif kind == 'delete' and len(tokens) > 2:
        del tokens[i]
    return ''.join(tokens)


def _mutate_length(word, rng):
    """Egy magánhangzó hosszúságának cseréje (a↔á, e↔é, o↔ó, ö↔ő, u↔ú, ü↔ű, i↔í): a csapda-mód félreírása."""
    spots = [i for i, c in enumerate(word.lower()) if c in _LENGTH_PAIRS]
    if not spots:
        return None
    i = rng.choice(spots)
    return word[:i] + _LENGTH_PAIRS[word[i].lower()].upper() + word[i + 1:]


def _fake_words(sources, count, rng, length=None, mutate=_mutate):
    """`count` érvénytelen, de hihető szó a `sources` szavaiból (módosítással).

    Egy forrásszóból lehetőleg csak egy hamis szó készül (különben a kérdések egymás átírásai lennének)."""
    fakes = []
    seen = set()
    order = []
    attempts = 0
    while len(fakes) < count and attempts < 200:
        attempts += 1
        if not order:                       # kifogytak a forrásszavak: újra végigmegyünk rajtuk
            order = list(sources)
            rng.shuffle(order)
        batch = []
        for _ in range(count * 4):
            if not order:
                break
            fake = mutate(order.pop(), rng)
            tokens = tokenize_word(fake) if fake else None
            if not tokens or fake in seen or not any(t[0] in VOWELS for t in tokens):
                continue
            if length is not None and len(tokens) != length:
                continue
            if len(fake) > 15:
                continue
            seen.add(fake)
            batch.append(fake)
        valid = dictionary.filter_valid(set(batch))
        fakes.extend(w for w in batch if w not in valid)
    return fakes[:count]


def _accent_sensitive(word):
    return any(c in _LENGTH_PAIRS for c in word.lower())


def make_quiz(count=QUESTION_COUNT, mode='mixed', rng=None):
    """Kvízkérdések: a fele érvényes, a fele érvénytelen szó, véletlen sorrendben.

    mode: `mixed` (vegyes hosszúság), `2` / `3` (csak annyi zsetonos szavak) vagy `tricky` (csapdák: az
    érvénytelen szavak egy érvényes szó magánhangzójának hosszúságcseréjével készülnek).
    Visszatér: a szavak listája (a választ nem tartalmazza).
    """
    if mode not in QUIZ_MODES:
        raise ValueError('ismeretlen kvízmód')
    rng = rng or random
    count = max(2, min(int(count), MAX_QUESTIONS))
    valid_count = count // 2 + (count % 2 and rng.random() < 0.5)
    fake_count = count - valid_count

    if mode in ('mixed', 'tricky'):
        tricky = mode == 'tricky'
        stems = [w for w in rng.sample(_stems(), min(len(_stems()), valid_count * (14 if tricky else 6)))
                 if MIN_TILES <= len(tokenize_word(w) or ()) <= MAX_TILES and (not tricky or _accent_sensitive(w))]
        stem_set = set(stems)
        inflected_target = round(valid_count * INFLECTED_SHARE)
        pool = ai_player.get_vocabulary().words
        inflected = [w for w in rng.sample(pool, min(len(pool), inflected_target * (24 if tricky else 8) + 8))
                     if MIN_TILES <= len(tokenize_word(w) or ()) <= MAX_TILES and w not in stem_set
                     and (not tricky or _accent_sensitive(w))]
        inflected = [w for w in inflected if _is_valid(w)][:inflected_target]
        valid = [w for w in stems if _is_valid(w)][:valid_count - len(inflected)] + inflected
        sources = valid or stems
        fakes = _fake_words(sources, fake_count, rng, mutate=_mutate_length if tricky else _mutate)
    else:
        length = int(mode)
        words = [e['word'] for e in short_words(length)]
        valid = rng.sample(words, min(valid_count, len(words)))
        fakes = _fake_words(words, fake_count, rng, length=length)

    questions = list(dict.fromkeys(valid + fakes))
    rng.shuffle(questions)
    return questions


def check_answer(word, answer):
    """Egy kvíz-válasz kiértékelése.

    Visszatér: {'valid', 'correct', 'score', 'tiles', 'suggestions'} — vagy None, ha a szó alakilag
    érvénytelen (nem magyar betűk, túl hosszú).
    """
    word = (word or '').strip().upper()
    if not 2 <= len(word) <= 15 or tokenize_word(word) is None:
        return None
    valid = _is_valid(word)
    result = {
        'word': word,
        'valid': valid,
        'correct': bool(answer) == valid,
        'score': word_base_score(word),
        'tiles': tokenize_word(word),
        'suggestions': [],
    }
    if not valid:
        result['suggestions'] = [s for s in dictionary.suggest_words(word, limit=3)]
    return result


# --- Betűvadászat / bingó-edző ---
#
# A kéz 7 betűzseton (joker nélkül). A szerver a kézzel együtt kiadja az összes kirakható szót is
# (a robot szókincséből, a szótárral megerősítve), így a kliens azonnal tud pontozni; a szókincsben nem
# szereplő, de érvényes szavakat `check_rack_word` bírálja el.

def _rack_pool():
    return [letter for letter, _value, count in TILE_DISTRIBUTION if letter for _ in range(count)]


def _count_ok(tokens):
    """A zsetonok darabszáma belefér-e a játék zsákjába (pl. nincs négy Ó)."""
    return all(n <= TILE_COUNTS[t] for t, n in Counter(tokens).items())


def rack_words(rack):
    """Az összes szó, amely a kéz zsetonjaiból kirakható (legalább 2 zseton).

    Visszatér: [{'word', 'score', 'tiles'}] pontszám (majd ábécé) szerint; a pont a zsetonértékek összege
    (+50, ha mind a hét zseton fogy). Egy szó legjobb kirakását számoljuk (S+Z helyett SZ is lehet)."""
    vocab = ai_player.get_vocabulary()
    counts = Counter(rack)
    best = {}

    def walk(prefix, used, score):
        if used >= 2 and prefix in vocab:
            total = score + (BINGO_BONUS if used == RACK_SIZE else 0)
            if prefix not in best or total > best[prefix][0]:
                best[prefix] = (total, used)
        if used >= len(rack):
            return
        for tile in list(counts):
            if counts[tile] <= 0:
                continue
            nxt = prefix + tile
            if not vocab.has_prefix(nxt):
                continue
            counts[tile] -= 1
            walk(nxt, used + 1, score + TILE_VALUES[tile])
            counts[tile] += 1

    walk('', 0, 0)
    valid = dictionary.filter_valid(set(best)) if best else set()
    words = [{'word': w, 'score': best[w][0], 'tiles': best[w][1]} for w in valid]
    words.sort(key=lambda e: (-e['score'], e['word']))
    return words


def _draw_rack(rng):
    pool = _rack_pool()
    while True:
        rack = rng.sample(pool, RACK_SIZE)
        vowels = sum(1 for t in rack if t[0] in VOWELS)
        if 2 <= vowels <= 4:
            return rack


BINGO_STEM_SHARE = 0.75      # a bingó-kezek ekkora része szótári tőszóból indul (a ragozott alakok gyakran furcsák)


def _bingo_rack(rng):
    """Olyan kéz, amelyből biztosan kirakható egy 7 zsetonos szó (a szótár egy véletlen szavából)."""
    stems = _stems()
    vocabulary = ai_player.get_vocabulary().words
    for _ in range(400):
        pool = stems if rng.random() < BINGO_STEM_SHARE else vocabulary
        for word in rng.sample(pool, 300):
            if not 7 <= len(word) <= 14:
                continue
            tokens = tokenize_word(word)
            if tokens and len(tokens) == RACK_SIZE and _count_ok(tokens) and _is_valid(word):
                rng.shuffle(tokens)
                return tokens
    raise RuntimeError('nem sikerült bingó-kezet készíteni')


def make_rack(kind='hunt', rng=None):
    """Egy gyakorló kéz a megoldásokkal.

    kind: `hunt` (betűvadászat: sok szó, legalább egy 5+ zsetonos) vagy `bingo` (biztosan van 7 zsetonos szó).
    Visszatér: {'kind', 'rack': [zsetonok], 'words': [{'word', 'score', 'tiles'}], 'total_score'}; bingónál a
    `words` csak a 7 zsetonos (bingó) szavakat tartalmazza."""
    if kind not in RACK_KINDS:
        raise ValueError('ismeretlen kéztípus')
    rng = rng or random
    if kind == 'bingo':
        rack = _bingo_rack(rng)
        words = [w for w in rack_words(rack) if w['tiles'] == RACK_SIZE]
        return {'kind': kind, 'rack': rack, 'words': words, 'total_score': sum(w['score'] for w in words)}
    best = None
    for _ in range(40):
        rack = _draw_rack(rng)
        words = rack_words(rack)
        if not words:
            continue
        longest = max(w['tiles'] for w in words)
        if HUNT_MIN_WORDS <= len(words) <= HUNT_MAX_WORDS and longest >= HUNT_MIN_LONGEST:
            best = (rack, words)
            break
        if best is None or len(words) > len(best[1]):
            best = (rack, words)
    rack, words = best
    words = words[:MAX_RACK_WORDS]
    return {'kind': kind, 'rack': rack, 'words': words, 'total_score': sum(w['score'] for w in words)}


def _assign(word, counts, score=0):
    """A szó kirakása a kéz zsetonjaiból (a kétjegyű betű egy vagy két zseton is lehet):
    (legjobb pontszám, felhasznált zsetonok) vagy None."""
    if not word:
        return score, []
    best = None
    for size in (1, 2):
        piece = word[:size]
        if len(piece) != size or counts.get(piece, 0) <= 0:
            continue
        counts[piece] -= 1
        rest = _assign(word[size:], counts, score + TILE_VALUES[piece])
        counts[piece] += 1
        if rest is not None and (best is None or rest[0] > best[0]):
            best = (rest[0], [piece] + rest[1])
    return best


def check_rack_word(rack, word):
    """Egy beírt szó bírálata a kézhez.

    Visszatér: {'ok', 'reason', 'word', 'tiles', 'score', 'bingo'}; a `reason`: `too_short`,
    `invalid_chars`, `not_in_rack`, `not_a_word` (ok=False esetén)."""
    word = (word or '').strip().upper()
    result = {'ok': False, 'reason': None, 'word': word, 'tiles': [], 'score': 0, 'bingo': False}
    if len(word) < 2:
        result['reason'] = 'too_short'
        return result
    if len(word) > 15 or tokenize_word(word) is None:
        result['reason'] = 'invalid_chars'
        return result
    assigned = _assign(word, dict(Counter(rack)))
    if assigned is None:
        result['reason'] = 'not_in_rack'
        return result
    if not _is_valid(word):
        result['reason'] = 'not_a_word'
        return result
    score, tiles = assigned
    bingo = len(tiles) == RACK_SIZE
    result.update(ok=True, tiles=tiles, score=score + (BINGO_BONUS if bingo else 0), bingo=bingo)
    return result
