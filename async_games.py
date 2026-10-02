"""Levelezős (aszinkron) játékmód: a játékosok órák vagy napok alatt lépnek.

A játék egy kész, elindított `Game`, amelynek minden résztvevője (a létrehozón kívül) lecsatlakozottként
várja az első belépését; a lecsatlakozottat nem ugorjuk át, a soron lévőnek `turn_hours` órája van,
utána automatikus passz (három egymás utáni lejárt határidő után feladja). A játék tartós otthona az
adatbázis: minden lépés után mentődik, és a szerver újraindulásakor a szobák visszaépülnek.
"""
import json
import random

from game import Game

ALLOWED_TURN_HOURS = (24, 48, 72, 168)
DEFAULT_TURN_HOURS = 48
MAX_FRIENDS = 3        # a létrehozón kívül legfeljebb ennyi játékos


def unique_name(name, taken):
    """A névből a már foglaltaktól (kis-nagybetűtől függetlenül) különböző név: Anna, Anna (2)..."""
    taken = {t.casefold() for t in taken}
    if name.casefold() not in taken:
        return name
    n = 2
    while f'{name} ({n})'.casefold() in taken:
        n += 1
    return f'{name} ({n})'


def placeholder_id(room_id, user_id):
    """A még be nem lépett játékos helyettesítő azonosítója (belépéskor a SID váltja fel)."""
    return f'async-{room_id}-{user_id}'


def build_game(room_id, creator_sid, creator_name, creator_user_id, friends, turn_hours, rng=None):
    """A levelezős játék: a létrehozó (kapcsolódva) és a meghívott barátok (lecsatlakozottként).

    friends: [{'id': user_id, 'name': megjelenítési név}, ...]
    Visszatér: (game, known_user_ids) — az utóbbi {játékosnév: user_id}.
    """
    rng = rng or random
    game = Game(room_id, hint_limit=0)
    game.async_mode = True
    game.turn_hours = turn_hours
    known = {}
    game.add_player(creator_sid, creator_name)
    known[creator_name] = creator_user_id
    for friend in friends:
        name = unique_name(friend['name'], known)
        game.add_player(placeholder_id(room_id, friend['id']), name)
        game.players[-1].disconnected = True
        known[name] = friend['id']
    game.start()
    game.current_player_idx = rng.randrange(len(game.players))   # véletlen kezdő játékos
    game._reset_deadline()
    return game, known


def summarize(row):
    """Egy `get_user_async_games` sor a lobby listájához (a mentett állapotból)."""
    try:
        state = json.loads(row['state_json'])
    except (TypeError, ValueError):
        return None
    players = state.get('players', [])
    idx = state.get('current_player_idx', 0)
    current = players[idx]['name'] if 0 <= idx < len(players) else None
    return {
        'game_id': row['game_id'],
        'room_name': row['room_name'],
        'my_name': row['my_name'],
        'players': [{'name': p['name'], 'score': p.get('score', 0)} for p in players],
        'current_player': current,
        'my_turn': current == row['my_name'] and not state.get('finished'),
        'turn_deadline': state.get('turn_deadline'),
        'turn_hours': state.get('turn_hours', 0),
        'updated_at': row['updated_at'],
    }
