"""Játékelemzés: minden lépésnél megmutatja a legjobb lehetséges lépést és a kint maradt pontot.

A lépésnapló (`game.move_log`) minden lépésnél tartalmazza a játékos kezét a lépés előtt (`rack`)
és a lépés utáni tábla pillanatképét. Az elemzés a megelőző lépés pillanatképén a robot motorjával
megkeresi a kézből rakható legjobb lerakást, és összeveti a ténylegesen játszottal.
Az eredmény gyorsítótárazott (`game_analysis` tábla); a keresés tőszavakra épül, ezért a robot
szókincsének bővítésekor az `ANALYSIS_VERSION`-t növelni kell.
"""
import json

import ai_player
from board import Board

ANALYSIS_VERSION = 6   # 6: a kétjegyű betű csak a saját zsetonjával rakható ki (5: a szótár-építő második köre, 4: első kör, 3: a furcsa alakok szűrése)
SECONDS_PER_MOVE = 0.8
# Ezek a lépések számítanak a játékos körének (az elutasított lerakás nem)
TURN_TYPES = ('place', 'challenge_accept', 'exchange', 'pass')


def _details(move):
    try:
        details = json.loads(move.get('details_json') or '{}')
    except (TypeError, ValueError):
        return {}
    return details if isinstance(details, dict) else {}


def analyzable_turns(moves):
    """A lépésnapló elemezhető körei (régi játékokban nincs rögzített kéz)."""
    return [m for m in moves
            if m['action_type'] in TURN_TYPES and isinstance(_details(m).get('rack'), list)]


def _tile_dicts(tiles):
    return [{'row': r, 'col': c, 'letter': l, 'is_blank': b} for r, c, l, b in tiles]


def analyze_game(moves, seconds=SECONDS_PER_MOVE, yield_fn=None, progress=None):
    """A játék elemzése.

    moves: a lépések listája (move_number szerint rendezve), mezők: player_name, action_type,
    details_json, board_snapshot_json.
    progress: opcionális `fn(kész, összes)` visszahívás.
    Visszatér: {'version', 'moves': [...], 'players': [...]}
    """
    total = len(analyzable_turns(moves))
    board = Board()          # a tábla az aktuális lépés előtt
    entries = []
    for move in moves:
        details = _details(move)
        if move['action_type'] in TURN_TYPES and isinstance(details.get('rack'), list):
            entries.append(_analyze_turn(board, move, details, seconds, yield_fn))
            if progress:
                progress(len(entries), total)
        snapshot = move.get('board_snapshot_json')
        if snapshot:
            try:
                board = Board.from_dict(json.loads(snapshot))
            except (TypeError, ValueError, KeyError):
                pass
    return {'version': ANALYSIS_VERSION, 'moves': entries, 'players': _summarize(entries)}


def _analyze_turn(board, move, details, seconds, yield_fn):
    rack = details['rack']
    actual = details.get('score', 0) if move['action_type'] in ('place', 'challenge_accept') else 0
    best = ai_player.best_moves(board, rack, count=1, yield_fn=yield_fn, seconds=seconds)
    best_score, best_words, best_tiles = 0, [], []
    if best:
        best_score, best_words, best_tiles = best[0].score, best[0].words, _tile_dicts(best[0].tiles)
    if actual >= best_score and actual > 0:
        # A játszott lépés legalább olyan jó, mint amit a motor talált (a motor szókincse szűkebb)
        best_score, best_words, best_tiles = actual, details.get('words', []), details.get('tiles', [])
    return {
        'n': move['move_number'],
        'player': move['player_name'],
        'type': move['action_type'],
        'rack': rack,
        'score': actual,
        'words': details.get('words', []),
        'best_score': best_score,
        'best_words': best_words,
        'best_tiles': best_tiles,
        'lost': max(0, best_score - actual),
    }


def _summarize(entries):
    """Játékosonkénti összesítés: hány pontot ért el, és mennyit lehetett volna."""
    players = {}
    for e in entries:
        p = players.setdefault(e['player'], {
            'player': e['player'], 'turns': 0, 'scored': 0, 'possible': 0, 'optimal': 0,
            'biggest_miss': None,
        })
        p['turns'] += 1
        p['scored'] += e['score']
        p['possible'] += max(e['score'], e['best_score'])
        if e['lost'] == 0:
            p['optimal'] += 1
        if e['lost'] > 0 and (p['biggest_miss'] is None or e['lost'] > p['biggest_miss']['lost']):
            p['biggest_miss'] = {'n': e['n'], 'lost': e['lost'], 'best_words': e['best_words'],
                                 'best_score': e['best_score'], 'score': e['score']}
    result = []
    for p in players.values():
        p['missed'] = p['possible'] - p['scored']
        p['efficiency'] = round(100.0 * p['scored'] / p['possible'], 1) if p['possible'] else 100.0
        result.append(p)
    return result
