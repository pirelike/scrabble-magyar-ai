"""Napi feladvány: naponta egy közös táblaállás és kéz, a cél a lehető legtöbb pont egyetlen lépéssel.

A feladványt a nap dátumából indított, determinisztikus bot–bot játék adja (néhány lépés után a soron
lévő robot keze + a tábla), a "legjobb lépést" a robot motorja keresi meg. Az első kérésnél készül el,
és az adatbázisban rögzül, így mindenkinek ugyanaz. Játék közben az ellenőrzést és a pontozást a
játék saját logikája végzi (egy játékos, egy lerakás után vége).
"""
import random
from collections import Counter
from datetime import datetime, timezone

import ai_player
import auth
from board import Board
from game import Game, HAND_SIZE
from tiles import TILE_DISTRIBUTION

try:
    from zoneinfo import ZoneInfo
    _TZ = ZoneInfo('Europe/Budapest')
except Exception:  # nincs időzóna-adatbázis (pl. Windows tzdata nélkül): UTC
    _TZ = timezone.utc

MIN_BEST_SCORE = 30       # a legjobb lépés legalább ennyit érjen (különben unalmas a feladvány)
MAX_BEST_SCORE = 250      # és ne legyen túl szerencsés / abszurd
PLIES = (8, 16)           # ennyi lépést játszanak le a robotok a feladvány előtt
MAX_ATTEMPTS = 40
SEARCH_SECONDS = 4.0


def today_str():
    """A mai dátum (magyar idő szerint) ÉÉÉÉ-HH-NN formában."""
    return datetime.now(_TZ).strftime('%Y-%m-%d')


def previous_date(date_str):
    from datetime import date, timedelta
    d = date.fromisoformat(date_str) - timedelta(days=1)
    return d.isoformat()


def is_valid_date(text):
    from datetime import date
    try:
        return date.fromisoformat(text).isoformat() == text
    except (TypeError, ValueError):
        return False


def _tile_dicts(tiles):
    return [{'row': r, 'col': c, 'letter': l, 'is_blank': b} for r, c, l, b in tiles]


def _try_generate(rng, yield_fn=None):
    """Egy próbálkozás: lejátszott játék, majd a soron lévő robot keze a feladvány."""
    game = Game('daily')
    levels = [rng.randint(5, 8), rng.randint(5, 8)]
    for i, level in enumerate(levels):
        game.add_bot(f'Bot{i + 1}', level)
    game.bag.tiles.sort()
    rng.shuffle(game.bag.tiles)   # a zsák sorrendje csak a dátumtól függ
    game.start()

    for _ in range(rng.randint(*PLIES)):
        if game.finished:
            return None
        player = game.current_player()
        action = ai_player.choose_action(game.board, list(player.hand), player.difficulty,
                                         game.bag.remaining(), rng=rng, yield_fn=yield_fn)
        ok = False
        if action['action'] == 'place':
            ok, _msg, _score = game.place_tiles(player.id, action['tiles'])
        if not ok:
            # Csere helyett passz: a zsák visszakeverése a globális véletlent használná, a feladvány
            # viszont csak a dátumtól függhet
            game.pass_turn(player.id)

    rack = list(game.current_player().hand)
    if game.finished or len(rack) < HAND_SIZE or game.board.is_empty:
        return None
    best = ai_player.best_moves(game.board, rack, count=1, seconds=SEARCH_SECONDS, yield_fn=yield_fn)
    if not best or not (MIN_BEST_SCORE <= best[0].score <= MAX_BEST_SCORE):
        return None
    return {
        'board': game.board.to_dict(),
        'rack': rack,
        'best_score': best[0].score,
        'best': {'tiles': _tile_dicts(best[0].tiles), 'words': best[0].words, 'score': best[0].score},
    }


def generate_puzzle(date_str, yield_fn=None):
    """A dátumhoz tartozó feladvány előállítása (determinisztikus a dátumból)."""
    seed = int(date_str.replace('-', '')) * 1000
    for attempt in range(MAX_ATTEMPTS):
        puzzle = _try_generate(random.Random(seed + attempt), yield_fn)
        if puzzle:
            return puzzle
    raise RuntimeError(f'nem sikerült feladványt készíteni ({date_str})')


def ensure_puzzle(date_str=None, yield_fn=None):
    """A nap feladványa: az adatbázisból, ennek híján elkészíti és elmenti."""
    date_str = date_str or today_str()
    puzzle = auth.get_daily_puzzle(date_str)
    if puzzle:
        return puzzle
    generated = generate_puzzle(date_str, yield_fn)
    auth.save_daily_puzzle(date_str, generated['board'], generated['rack'],
                           generated['best_score'], generated['best'])
    return auth.get_daily_puzzle(date_str)   # egyidejű kérésnél az először mentett marad


def _remaining_tiles(board, rack, date_str):
    """A zsákban maradt (a táblán és a kézben nem látható) zsetonok, a dátumtól függő sorrendben."""
    counts = Counter({letter: count for letter, _value, count in TILE_DISTRIBUTION})
    for row in board.cells:
        for cell in row:
            if cell is not None:
                counts['' if cell[1] else cell[0]] -= 1
    counts.subtract(rack)
    tiles = sorted(letter for letter, n in counts.items() for _ in range(max(0, n)))
    random.Random(date_str).shuffle(tiles)
    return tiles


def reset_game(game, puzzle):
    """A játékot a feladvány kezdőállapotára állítja (indításkor és újrapróbálkozáskor)."""
    player = game.players[0]
    game.board = Board.from_dict(puzzle['board'])
    player.hand = list(puzzle['rack'])
    player.score = 0
    game.bag.tiles = _remaining_tiles(game.board, player.hand, puzzle['date'])
    game.current_player_idx = 0
    game.finished = False
    game.winners = []
    game.move_log = []
    game.turn_number = 0
    game.scoreless_turns = 0
    game.pending_challenge = None
    game.last_action = None
    game.last_action_info = None
    game.puzzle = {'date': puzzle['date'], 'score': None}


def build_game(room_id, sid, name, puzzle):
    """Egyjátékos játék a feladvány állásából."""
    game = Game(room_id, hint_limit=0)
    game.add_player(sid, name)
    game.start()
    reset_game(game, puzzle)
    return game


def record_result(puzzle, user_id, score, tiles, words):
    """Egy beküldött lépés feldolgozása: ranglista (csak regisztrált), új rekord, helyezés.

    Visszatér: {'score', 'best_score' (a feladvány legjobbja), 'is_best', 'recorded', 'attempts',
    'my_best', 'rank'}; vendégnél `recorded` hamis.
    """
    date_str = puzzle['date']
    best_score = puzzle['best_score']
    if score > best_score:
        auth.raise_daily_best(date_str, score, {'tiles': tiles, 'words': words, 'score': score})
        best_score = score
    result = {
        'score': score, 'best_score': best_score, 'is_best': score >= best_score,
        'recorded': False, 'attempts': None, 'my_best': None, 'rank': None,
    }
    if user_id:
        rec = auth.record_daily_score(date_str, user_id, score)
        result.update(recorded=rec['recorded'], attempts=rec['attempts'], my_best=rec['best'])
        _entries, me = auth.get_daily_leaderboard(date_str, 1, user_id)
        result['rank'] = me['rank'] if me else None
    return result
