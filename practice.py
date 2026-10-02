"""Gyakorló módok: „Melyik szó érvényes?” kvíz és a rövid (2–3 zsetonos) szavak listája.

A kvíz kérdései csak a szót tartalmazzák; a választ a szerver a játék saját szótárával ellenőrzi
(`check_answer`), így nincs titkos állapot. Az érvénytelen szavak hihető „félreírások”: egy érvényes szó
egy zsetonnyi módosítása, amelyet a szótár nem fogad el.
"""
import random
from itertools import product

import ai_player
import dictionary
from tiles import LETTERS, VOWELS, tokenize_word, word_base_score

QUESTION_COUNT = 10
MAX_QUESTIONS = 30
MIN_TILES, MAX_TILES = 3, 9          # a vegyes kvíz szavainak hossza zsetonban
INFLECTED_SHARE = 0.3                # az érvényes kérdések ekkora része ragozott alak
SHORT_LENGTHS = (2, 3)

_VOWEL_TILES = [t for t in LETTERS if t[0] in VOWELS]
_CONSONANT_TILES = [t for t in LETTERS if t[0] not in VOWELS]
_short_cache = {}
_stem_cache = None


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


def _fake_words(sources, count, rng, length=None):
    """`count` érvénytelen, de hihető szó a `sources` szavaiból (módosítással)."""
    fakes = []
    seen = set()
    attempts = 0
    while len(fakes) < count and attempts < 200:
        attempts += 1
        batch = []
        for _ in range(count * 4):
            fake = _mutate(rng.choice(sources), rng)
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


def make_quiz(count=QUESTION_COUNT, length=None, rng=None):
    """Kvízkérdések: a fele érvényes, a fele érvénytelen szó, véletlen sorrendben.

    length: None (vegyes hosszúság) vagy 2 / 3 (csak annyi zsetonos szavak).
    Visszatér: a szavak listája (a választ nem tartalmazza).
    """
    rng = rng or random
    count = max(2, min(int(count), MAX_QUESTIONS))
    valid_count = count // 2 + (count % 2 and rng.random() < 0.5)
    fake_count = count - valid_count

    if length is None:
        stems = [w for w in rng.sample(_stems(), min(len(_stems()), valid_count * 6))
                 if MIN_TILES <= len(tokenize_word(w) or ()) <= MAX_TILES]
        stem_set = set(stems)
        inflected_target = round(valid_count * INFLECTED_SHARE)
        pool = ai_player.get_vocabulary().words
        inflected = [w for w in rng.sample(pool, min(len(pool), inflected_target * 8 + 8))
                     if MIN_TILES <= len(tokenize_word(w) or ()) <= MAX_TILES and w not in stem_set]
        inflected = [w for w in inflected if _is_valid(w)][:inflected_target]
        valid = [w for w in stems if _is_valid(w)][:valid_count - len(inflected)] + inflected
        sources = valid or stems
        fakes = _fake_words(sources, fake_count, rng)
    else:
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
