import os, hashlib, json, random, sys, time
sys.path.insert(0, os.environ['PYTHON_FINAL_DIR'])
OUT = os.environ.get('GOLDEN_OUT', 'tests/golden')
import dictionary, ai_player
from game import Game

dictionary.warm_up()
vocab = ai_player.get_vocabulary()
cases = []
rng = random.Random(2026)

def play_game(seed, max_plies):
    r = random.Random(seed)
    game = Game('g%d' % seed)
    game.add_bot('A', 6); game.add_bot('B', 6)
    game.start()
    snaps = []
    for ply in range(max_plies):
        if game.finished: break
        player = game.current_player()
        action = ai_player.choose_action(game.board, list(player.hand), 10, game.bag.remaining(), rng=r, seconds=None) if False else None
        action = ai_player.choose_action(game.board, list(player.hand), 6 + (ply % 5), game.bag.remaining(), rng=r)
        snaps.append((game.board.to_dict(), list(player.hand), game.board.is_empty))
        ok = False
        if action['action'] == 'place':
            ok, _m, _s = game.place_tiles(player.id, action['tiles'])
        if not ok:
            game.pass_turn(player.id)
    return snaps

for seed in range(6):
    snaps = play_game(100 + seed, 22)
    picks = [0, 1, 2, 4, 7, 10, 14, 18]
    for i in picks:
        if i < len(snaps):
            cases.append(snaps[i])

from board import Board
out = []
t0 = time.time()
for idx, (bd, rack, empty) in enumerate(cases):
    variants = [rack]
    if rack:
        v = list(rack); v[rng.randrange(len(v))] = ''; variants.append(v)
        if idx % 4 == 0 and len(rack) >= 2:
            v2 = list(rack); v2[0] = ''; v2[1] = ''; variants.append(v2)
    for rk in variants:
        board = Board.from_dict(bd)
        board.is_empty = empty and not any(any(c for c in row) for row in bd)
        moves = ai_player.generate_moves(board, rk, vocab=vocab, allow_blanks=True, seconds=120.0)
        lines = []
        for m in moves:
            tiles = sorted(list(t) for t in m.tiles)
            lines.append('|'.join([';'.join(f'{r},{c},{l},{int(b)}' for r, c, l, b in tiles), ','.join(m.words), str(m.score)]))
        lines.sort()
        digest = hashlib.sha256('\n'.join(lines).encode()).hexdigest()
        out.append({'board': bd, 'rack': rk, 'count': len(lines), 'sha256': digest, 'lines': lines})
print('cases', len(out), 'moves', sum(o['count'] for o in out), 'time', time.time() - t0, file=sys.stderr)
if len(sys.argv) > 1:   # opcionális: a teljes lépéslisták (a részletes összevetéshez)
    json.dump(out, open(os.path.join(sys.argv[1], 'moves_full.json'), 'w'), ensure_ascii=False)
for o in out:
    o.pop('lines')
json.dump(out, open(os.path.join(OUT, 'moves.json'), 'w'), ensure_ascii=False)
