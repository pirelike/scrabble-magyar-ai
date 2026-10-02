"""Kitüntetések (érmék): a befejezett játékokból számolt, tartósan megszerzett jelvények.

A játékonkénti jelvényeket a lépésnaplóból és a végeredményből számoljuk (`evaluate_game`), az
összesítőket (első játék, győzelmek száma...) a felhasználó statisztikájából (`cumulative_badges`).
A megszerzést az `auth.grant_achievements` rögzíti (egyszer, kulcsonként).
"""
import json

from tiles import tokenize_word

HAND_SIZE = 7

# A megjelenítés sorrendje is ez; a nevek / leírások a kliens i18n `badge.<kulcs>` kulcsaiban vannak
BADGES = (
    'first_game',   # az első befejezett játék
    'first_win',    # az első győzelem
    'bingo',        # mind a 7 zseton egy lépésben (50 pont bónusz)
    'score_100',    # legalább 100 pontos lépés
    'long_word',    # legalább 8 zsetonos szó
    'joker_play',   # joker (üres zseton) lerakása
    'game_300',     # legalább 300 pontos játék
    'bot_slayer',   # győzelem erős (8–10. fokozatú) robot ellen
    'wins_10',      # 10 győzelem
    'games_25',     # 25 befejezett játék
)

BIG_MOVE_SCORE = 100
LONG_WORD_TILES = 8
HIGH_GAME_SCORE = 300
STRONG_BOT_LEVEL = 8
WINS_FOR_VETERAN = 10
GAMES_FOR_REGULAR = 25


def evaluate_game(game, name_to_user):
    """Játékonkénti jelvények. `name_to_user`: {játékosnév: user_id} (regisztrált emberek).

    Visszatér: {user_id: {jelvénykulcs, ...}}
    """
    earned = {uid: set() for uid in name_to_user.values() if uid}

    for move in game.move_log:
        uid = name_to_user.get(move['player_name'])
        if not uid or move['action_type'] not in ('place', 'challenge_accept'):
            continue
        try:
            details = json.loads(move.get('details_json') or '{}')
        except (TypeError, ValueError):
            continue
        tiles = details.get('tiles', [])
        if len(tiles) >= HAND_SIZE:
            earned[uid].add('bingo')
        if details.get('score', 0) >= BIG_MOVE_SCORE:
            earned[uid].add('score_100')
        if any(t.get('is_blank') for t in tiles):
            earned[uid].add('joker_play')
        if any(len(tokenize_word(w)) >= LONG_WORD_TILES for w in details.get('words', [])):
            earned[uid].add('long_word')

    strong_bot = any(p.is_bot and (p.difficulty or 0) >= STRONG_BOT_LEVEL for p in game.players)
    winners = {p.name for p in game.winners}
    for player in game.players:
        uid = name_to_user.get(player.name)
        if not uid or player.is_bot:
            continue
        if player.score >= HIGH_GAME_SCORE:
            earned[uid].add('game_300')
        if strong_bot and player.name in winners:
            earned[uid].add('bot_slayer')
    return earned


def cumulative_badges(games_played, games_won):
    """A felhasználó összesített statisztikájából járó jelvények."""
    badges = set()
    if games_played >= 1:
        badges.add('first_game')
    if games_won >= 1:
        badges.add('first_win')
    if games_won >= WINS_FOR_VETERAN:
        badges.add('wins_10')
    if games_played >= GAMES_FOR_REGULAR:
        badges.add('games_25')
    return badges
