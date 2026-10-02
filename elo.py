"""Élő-értékszám (ELO) 2–4 játékos közötti játékokhoz.

Minden játékospárra külön számolunk (a nagyobb pontszám nyer, az egyenlő döntetlen), a változást
az ellenfelek számával osztjuk, így a játékos teljes elmozdulása legfeljebb egy K lehet, akárhányan
játszanak. A számításhoz a játék előtti értékszámok kellenek.
"""

INITIAL_RATING = 1200
PROVISIONAL_GAMES = 10   # ennyi értékelt játékig nagyobb a K (gyorsabban áll be a valódi szint)
K_PROVISIONAL = 32
K_ESTABLISHED = 20
# A ranglistára csak ennyi értékelt játék után kerülhet fel valaki
MIN_RATED_GAMES_FOR_RANKING = 3


def expected_score(rating, opponent_rating):
    """A játékos várható eredménye (0–1) az ellenféllel szemben."""
    return 1.0 / (1.0 + 10 ** ((opponent_rating - rating) / 400.0))


def k_factor(rated_games):
    return K_PROVISIONAL if rated_games < PROVISIONAL_GAMES else K_ESTABLISHED


def rating_changes(entries):
    """Értékszám-változások egy játék végén.

    entries: [(kulcs, értékszám, értékelt_játékok_száma, pontszám), ...] — csak a rangsorolt
    (regisztrált, emberi) játékosok. Kevesebb mint két játékosnál nincs változás.
    Visszatér: {kulcs: egész változás}
    """
    if len(entries) < 2:
        return {key: 0 for key, *_ in entries}
    changes = {}
    opponents = len(entries) - 1
    for key, rating, rated_games, score in entries:
        total = 0.0
        for other_key, other_rating, _other_games, other_score in entries:
            if other_key == key:
                continue
            if score > other_score:
                actual = 1.0
            elif score == other_score:
                actual = 0.5
            else:
                actual = 0.0
            total += actual - expected_score(rating, other_rating)
        changes[key] = round(k_factor(rated_games) * total / opponents)
    return changes
