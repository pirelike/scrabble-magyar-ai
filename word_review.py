"""Szótár-építő: a játék szótárának emberi átnézése.

A hu_HU szótár helyesírás-ellenőrzésre készült, ezért a Scrabble-ban sok olyan szót is elfogad, amelyet
senki sem tekintene rendes szónak. A szótár-építő véletlen szavakat mutat (a játék által éppen elfogadottakat),
a felhasználó pedig eldönti, rendes szó-e. A „nem szó” döntés azonnal kizárja a szót a játékból
(`dictionary.mark_voted_rejected`): a lerakás, a robot, a kvíz és a szólisták sem fogadják el többé.

A döntések az adatbázisban élnek (`word_reviews`, felhasználónként egy szavazat szavanként); egy szót
akkor zár ki a szótár, ha a „nem szó” szavazatok száma legalább `config.WORD_REJECT_THRESHOLD`-dal több
a „rendes szó” szavazatoknál. Az elutasítás pontos szóalakra vonatkozik (a ragozott alakokat nem érinti).
A kizárt szavak indításkor az adatbázisból töltődnek vissza (`refresh`); az AI-val vagy kézzel átnézett,
tartós lista a `dict/hu_rejected.txt` (lásd `tools/word_review.py`).
"""
import random

import ai_player
import auth
import config
import dictionary
import practice
import settings
from tiles import tokenize_word

BATCH_SIZE = 20
MAX_BATCH = 50
INFLECTED_SHARE = 0.3        # a mutatott szavak ekkora része ragozott alak (a többi szótári tő)
MIN_LENGTH, MAX_LENGTH = 2, 15

_pools = None        # (a szókincs, a tőszavak, a ragozott alakok): a szókincshez kötve, egyszer épül fel


def threshold():
    """A kizáráshoz szükséges „nem szó” többlet: az admin panelről állítható, felülbírálat nélkül a konfiguráció."""
    return settings.get('word_reject_threshold')


def refresh():
    """A szavazatok alapján elutasított szavak és az admin szótári felülbírálatai (engedélyezések, tiltások, saját
    szavak) betöltése az adatbázisból (szerverindításkor és az admin módosításai után)."""
    dictionary.set_voted_rejected(auth.get_voted_rejected_words(threshold()))
    dictionary.set_admin_lists(*auth.get_admin_word_lists())


def normalize(word):
    """A szó nagybetűs, ellenőrzött alakja, vagy None, ha nem lehet magyar szó a táblán."""
    if not isinstance(word, str):
        return None
    word = word.strip().upper()
    if not MIN_LENGTH <= len(word) <= MAX_LENGTH or tokenize_word(word) is None:
        return None
    return word


def _apply(word):
    """A szó elutasított állapotának újraszámolása a szavazatokból (a szótárba is átvezetve).
    Visszatér: elutasított-e most a szó."""
    good, bad = auth.get_word_review_votes(word.lower())
    rejected = bad - good >= threshold()
    dictionary.mark_voted_rejected(word, rejected)
    return rejected


def record_vote(user_id, word, valid):
    """A felhasználó döntése egy szóról (`valid`: rendes szó-e). Csak olyan szóra lehet szavazni, amelyet
    a szótár éppen elfogad. Visszatér: a szó elutasított-e most, vagy None, ha a szó nem szavazható."""
    word = normalize(word)
    if word is None or not dictionary.is_word_valid(word):
        return None
    auth.save_word_review(user_id, word.lower(), valid)
    return _apply(word)


def undo_vote(user_id, word):
    """A felhasználó saját döntésének visszavonása. Visszatér: (volt-e mit visszavonni, elutasított-e
    most a szó)."""
    word = normalize(word)
    if word is None or not auth.delete_word_review(user_id, word.lower()):
        return False, False
    return True, _apply(word)


def stats(user_id):
    """A felhasználó összesítője: összes / rendes / nem szó döntés és a ma átnézettek száma."""
    return auth.get_word_review_stats(user_id)


# --- Mintavétel ---

def _word_pools():
    """A mintavétel két készlete: a szótár tőszavai és a (tőszónak nem számító) gyakori ragozott alakok."""
    global _pools
    vocabulary = ai_player.get_vocabulary()
    if _pools is None or _pools[0] is not vocabulary:
        stems = practice.stem_words()
        stem_set = set(stems)
        _pools = (vocabulary, stems, [w for w in vocabulary.words if w not in stem_set])
    return _pools[1], _pools[2]


def _draw(pool, want, taken, rng):
    """`want` darab, a `taken`-ben még nem szereplő, a szótár szerint érvényes szó a `pool`-ból."""
    found = []
    for _ in range(12):
        if len(found) >= want:
            break
        sample = rng.sample(pool, min(len(pool), (want - len(found)) * 4 + 8))
        sample = [w for w in sample if w.lower() not in taken and MIN_LENGTH <= len(w) <= MAX_LENGTH]
        valid = dictionary.filter_valid(set(sample))
        for word in sample:
            if word in valid and word.lower() not in taken and len(found) < want:
                taken.add(word.lower())
                found.append(word)
    return found


def sample_words(count=BATCH_SIZE, rng=None, exclude=()):
    """`count` véletlen szó átnézésre: a szótár tőszavaiból és gyakori ragozott alakjaiból (a robot
    szókincse), csak olyanok, amelyeket a szótár most elfogad és az `exclude`-ban (kisbetűs) nem szerepelnek.
    Visszatér: nagybetűs szavak, véletlen sorrendben (kevesebb is lehet, ha elfogytak a szavak)."""
    rng = rng or random
    count = max(1, int(count))
    taken = set(exclude)
    inflected_count = round(count * INFLECTED_SHARE)
    stems, inflected_pool = _word_pools()
    words = _draw(stems, count - inflected_count, taken, rng)
    words += _draw(inflected_pool, count - len(words), taken, rng)
    rng.shuffle(words)
    return words


def next_words(count=BATCH_SIZE, rng=None):
    """Átnézésre váró szavak (legfeljebb `MAX_BATCH`): olyanok, amelyeket még senki sem nézett át (az
    elfogadottak sem térnek vissza)."""
    return sample_words(min(max(1, int(count)), MAX_BATCH), rng, exclude=auth.get_reviewed_words())
