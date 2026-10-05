import json, os, random, sys
sys.path.insert(0, os.environ['PYTHON_FINAL_DIR'])
OUT = os.environ.get('GOLDEN_OUT', 'tests/golden')
out = sys.argv[1]
if os.path.exists(out): os.remove(out)
import config
config.DB_PATH = out
import auth
auth.DB_PATH = out
import dictionary, ai_player
from game import Game
dictionary.warm_up()
auth.init_db()

ok, alice = auth.create_user('alice@example.com', 'Alice', 'titok123')
ok, bob = auth.create_user('bob@example.com', 'Bób', 'Jelszó-ékezet ő')
tok = auth.create_session(alice, '1.2.3.4', 'UA')
auth.get_or_create_user_reconnect_token(alice)

def play(game, plies, rng, level=7):
    for _ in range(plies):
        if game.finished: break
        p = game.current_player()
        action = ai_player.choose_action(game.board, list(p.hand), level, game.bag.remaining(), rng=rng)
        done = False
        if action['action'] == 'place':
            done, _m, _s = game.place_tiles(p.id, action['tiles'])
        elif action['action'] == 'exchange':
            done, _m = game.exchange_tiles(p.id, action['indices'])
        if not done:
            game.pass_turn(p.id)

rng = random.Random(5)
# 1. folyamatban lévő, robotos játék (challenge mód nélkül)
g1 = Game('abc12345', turn_time_limit=90)
g1.add_player('sidA', 'Alice'); g1.add_bot('Robi', 3); g1.add_bot('Rita', 'auto')
g1.start(); play(g1, 9, rng)
# 2. szavazásos játék függő lerakással
g2 = Game('def67890', challenge_mode=True)
g2.add_player('sidA', 'Alice'); g2.add_player('sidB', 'Bób')
g2.start()
for _ in range(30):
    p = g2.current_player()
    if g2.pending_challenge: break
    action = ai_player.choose_action(g2.board, list(p.hand), 8, g2.bag.remaining(), rng=rng)
    if action['action'] == 'place':
        g2.place_tiles(p.id, action['tiles'])
    else:
        g2.pass_turn(p.id)
    if len(g2.move_log) >= 3 and g2.pending_challenge is None: pass
# 3. befejezett játék (6 passz)
g3 = Game('fin00001')
g3.add_player('sidA', 'Alice'); g3.add_player('sidB', 'Bób')
g3.start(); play(g3, 8, rng)
for _ in range(12):
    if g3.finished: break
    g3.pass_turn(g3.current_player().id)
# 4. levelezős
g4 = Game('async001', hint_limit=0); g4.async_mode = True; g4.turn_hours = 48
g4.add_player('async-async001-1', 'Alice'); g4.add_player('async-async001-2', 'Bób')
g4.start(); g4._reset_deadline(); play(g4, 4, rng)
g4.players[1].disconnected = True

states = {}
for key, g in [('robot', g1), ('vote', g2), ('finished', g3), ('async', g4)]:
    states[key] = g.to_save_dict()
    gid = auth.save_game(g.id, 'Szoba ' + key, json.dumps(states[key], ensure_ascii=False), g.challenge_mode,
                         [{'player_name': p.name, 'user_id': alice if p.name == 'Alice' else (bob if p.name == 'Bób' else None), 'score': p.score} for p in g.players],
                         owner_name='Alice', owner_token='tok-' + key, has_bots=any(p.is_bot for p in g.players), is_async=g.async_mode)
    for m in g.move_log:
        auth.add_game_move(gid, m['move_number'], m['player_name'], m['action_type'], m['details_json'], m['board_snapshot_json'])
    if g.finished:
        auth.finish_game(g.id, json.dumps(g.to_save_dict(), ensure_ascii=False),
                         [{'player_name': p.name, 'user_id': alice if p.name == 'Alice' else bob, 'final_score': p.score,
                           'is_winner': any(w.name == p.name for w in g.winners)} for p in g.players], room_name='Szoba ' + key)
json.dump({'states': states, 'session': tok, 'alice': alice, 'bob': bob}, open(os.path.splitext(out)[0] + '.json', 'w'), ensure_ascii=False)
print({k: (len(v['players']), v['finished'], v['turn_number']) for k, v in states.items()}, file=sys.stderr)
print('pending in vote game:', g2.pending_challenge is not None, file=sys.stderr)
