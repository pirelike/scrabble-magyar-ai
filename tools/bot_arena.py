"""Robot-aréna: a fokozatok erejének mérése bot–bot játékokkal (a kalibrációhoz).

  python tools/bot_arena.py ladder [-n 24] [-j 4]       # átlagos pont/kör fokozatonként (önjáték)
  python tools/bot_arena.py match 3 6 [-n 40] [-j 4]    # két fokozat egymás ellen

Egy játék ~1 mp; a -j a párhuzamos folyamatok száma. A `ladder` eredményét az `ai_player._PROFILES`
fölötti megjegyzésben rögzített értékekkel kell összevetni, ha a paramétereket módosítod.
"""
import argparse
import os
import random
import statistics
import sys
from concurrent.futures import ProcessPoolExecutor

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

import ai_player  # noqa: E402
from game import Game  # noqa: E402

_MAX_TURNS = 500  # biztonsági korlát


def play_game(levels, seed):
    """Egy játék a megadott fokozatú robotok között.

    Visszatér: (végső pontszámok, lerakott pontok összege és körök száma játékosonként)."""
    random.seed(seed)  # a zsák keverése
    rng = random.Random(seed + 1)
    game = Game(f'arena-{seed}')
    for i, level in enumerate(levels):
        game.add_bot(f'Bot{i + 1}', level)
    game.start()
    turns = [0] * len(levels)
    points = [0] * len(levels)
    for _ in range(_MAX_TURNS):
        if game.finished:
            break
        idx = game.current_player_idx
        player = game.current_player()
        action = ai_player.choose_action(
            game.board, list(player.hand), player.difficulty, game.bag.remaining(),
            rng=rng, avoid=game.rejected_placements)
        turns[idx] += 1
        ok = False
        if action['action'] == 'place':
            ok, _msg, score = game.place_tiles(player.id, action['tiles'])
            if ok:
                points[idx] += score
        elif action['action'] == 'exchange':
            ok, _msg = game.exchange_tiles(player.id, action['indices'])
        if not ok:
            game.pass_turn(player.id)
    return [p.score for p in game.players], points, turns


def _job(args):
    return play_game(*args)


def _run(jobs, workers):
    if workers > 1:
        with ProcessPoolExecutor(max_workers=workers) as pool:
            return list(pool.map(_job, jobs))
    return [_job(job) for job in jobs]


def ladder(games, workers, seed):
    print('fokozat  pont/kör')
    for level in ai_player.DIFFICULTIES:
        results = _run([([level, level], seed * 1000 + g) for g in range(games)], workers)
        points = sum(sum(r[1]) for r in results)
        turns = sum(sum(r[2]) for r in results)
        print(f'{level:7d}  {points / turns:8.1f}', flush=True)


def match(level_a, level_b, games, workers, seed):
    jobs = []
    for g in range(games):
        order = [level_a, level_b] if g % 2 == 0 else [level_b, level_a]
        jobs.append((order, seed * 1000 + g))
    wins, diffs = 0.0, []
    for (order, _seed), (scores, _points, _turns) in zip(jobs, _run(jobs, workers)):
        a_idx = order.index(level_a) if level_a != level_b else 0
        diff = scores[a_idx] - scores[1 - a_idx]
        diffs.append(diff)
        wins += 1.0 if diff > 0 else 0.5 if diff == 0 else 0.0
    print(f'{level_a}. fokozat vs {level_b}.: {wins:g}-{games - wins:g} '
          f'({100 * wins / games:.0f}% az elsőnek), átlagos pontkülönbség {statistics.mean(diffs):+.0f}')


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('mode', choices=('ladder', 'match'))
    parser.add_argument('levels', nargs='*', type=int, help='match: a két mérendő fokozat')
    parser.add_argument('-n', '--games', type=int, default=24, help='játékok száma (alapért.: 24)')
    parser.add_argument('-j', '--jobs', type=int, default=1, help='párhuzamos folyamatok (alapért.: 1)')
    parser.add_argument('--seed', type=int, default=1)
    args = parser.parse_args()

    if args.mode == 'ladder':
        ladder(args.games, args.jobs, args.seed)
    else:
        if len(args.levels) != 2 or not all(ai_player.parse_level(lv) for lv in args.levels):
            parser.error('a match két fokozatot (1–10) vár')
        match(args.levels[0], args.levels[1], args.games, args.jobs, args.seed)


if __name__ == '__main__':
    main()
