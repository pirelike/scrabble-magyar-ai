"""Valódi szerver az admin felület böngészős kipróbálásához (a `test_admin_browser.py` indítja, nem pytest-teszt).

Ideiglenes adatbázis és mentési mappa (`ADMIN_SMOKE_DIR`), néhány felhasználó, befejezett játék lépésekkel, élő szobák
(egyik robottal), bejelentések, közlemény; az admin: boss@example.com / secret12. A `READY` sor jelzi, hogy áll a szerver.
"""
import json
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WORK = os.environ['ADMIN_SMOKE_DIR']
os.environ['SCRABBLE_DB_PATH'] = os.path.join(WORK, 'smoke.db')
os.environ['SCRABBLE_BACKUP_DIR'] = os.path.join(WORK, 'backups')
os.environ['ADMIN_EMAILS'] = 'boss@example.com'
PORT = int(os.environ['ADMIN_SMOKE_PORT'])
sys.path.insert(0, ROOT)
os.chdir(ROOT)

import admin  # noqa: E402
import admin_comm  # noqa: E402
import admin_mod  # noqa: E402
import ai_player  # noqa: E402
import auth  # noqa: E402
import daily  # noqa: E402
import dictionary  # noqa: E402
import practice  # noqa: E402
import routes  # noqa: E402
import server  # noqa: E402
from socket_auth import create_socket_token  # noqa: E402

routes._rate_limiter.check_ip = lambda ip, action: True   # a kipróbálás sok kérést küld egy IP-ről


def make_user(email, name):
    ok, user_id = auth.create_user(email, name, 'secret12')
    assert ok, user_id
    return user_id


boss = make_user('boss@example.com', 'Főnök')
anna, bela, cili, dani = (make_user('anna@example.com', 'Anna'), make_user('bela@example.com', 'Béla'),
                          make_user('cili@example.com', 'Cili'), make_user('dani@example.com', 'Dani'))
with auth.transaction() as conn:
    conn.execute('UPDATE users SET banned_until = ?, ban_reason = ? WHERE id = ?', (auth.PERMANENT_UNTIL, 'Sértegetés', dani))
    conn.execute("UPDATE users SET chat_muted_until = '2099-01-01 00:00:00' WHERE id = ?", (cili,))
    for i in range(12):
        conn.execute('INSERT INTO login_events (user_id, email, ip, user_agent, success, reason) VALUES (?, ?, ?, ?, ?, ?)',
                     (None if i % 2 else anna, 'anna@example.com', '203.0.113.9', 'Mozilla/5.0 Teszt', 0 if i % 2 else 1,
                      'rossz jelszó' if i % 2 else ''))


def board(*tiles):
    cells = [[None] * 15 for _ in range(15)]
    for row, col, letter in tiles:
        cells[row][col] = {'letter': letter, 'is_blank': False}
    return cells


def finished(room, results, moves=4):
    best = max(score for _uid, _name, score in results)
    players = [{'player_name': name, 'user_id': uid, 'final_score': score, 'is_winner': score == best, 'resigned': False}
               for uid, name, score in results]
    state = {'players': [{'name': name, 'resigned': False, 'is_bot': False} for _u, name, _s in results], 'finished': True}
    game_id = auth.finish_game(room, json.dumps(state), players, room_name='Játék ' + room, has_bots=False)
    word = 'ALMA'
    for n in range(1, moves + 1):
        tiles = [{'row': 7, 'col': 6 + i, 'letter': ch, 'is_blank': False} for i, ch in enumerate(word[:n])]
        auth.add_game_move(game_id, n, results[(n - 1) % len(results)][1], 'place',
                           json.dumps({'rack': ['A', 'L', 'M', 'A', 'K', 'E', 'T'], 'score': 6 * n, 'words': [word[:n]], 'tiles': tiles}),
                           json.dumps(board(*[(7, 6 + i, ch) for i, ch in enumerate(word[:n])])))
    return game_id


finished('g1', [(anna, 'Anna', 310), (bela, 'Béla', 250)])
finished('g2', [(anna, 'Anna', 280), (cili, 'Cili', 300)])
admin_comm.create_announcement(admin.AdminContext(boss, '127.0.0.1', 'smoke'),
                               {'text_hu': 'Szerdán karbantartás.', 'text_en': 'Maintenance on Wednesday.',
                                'kind': 'warning', 'audience': 'all', 'reason': 'smoke'})


def player_client(user_id, name):
    client = server.socketio.test_client(server.app)
    client.emit('set_name', {'name': name, 'is_guest': False, 'user_id': user_id,
                             'auth_token': create_socket_token(server.app.config['SECRET_KEY'], user_id)})
    client.get_received()
    return client


CLIENTS = []
for owner, others, options in (((anna, 'Anna'), [(bela, 'Béla')], {'name': 'Esti játék', 'max_players': 4,
                                                                     'turn_time_limit': 90, 'challenge_mode': True}),
                               ((cili, 'Cili'), [], {'name': 'Robotos játék', 'max_players': 2, 'ai_players': [6]})):
    owner_client = player_client(*owner)
    owner_client.emit('create_room', options)
    owner_client.get_received()
    room = list(server.state.rooms.values())[-1]
    CLIENTS.append(owner_client)
    for uid, name in others:
        client = player_client(uid, name)
        client.emit('join_room', {'code': room.join_code})
        client.get_received()
        CLIENTS.append(client)
    owner_client.emit('start_game')
    owner_client.get_received()
CLIENTS[0].emit('send_chat', {'message': 'Sziasztok!'})
CLIENTS[1].emit('send_chat', {'message': 'Te hülye vagy'})
first_room = list(server.state.rooms.values())[0]
admin_mod.create_report(anna, 'Anna', bela, 'Béla', 'chat', 'Te hülye vagy', 'Sértő', first_room)
admin_mod.create_report(bela, 'Béla', anna, 'Anna', 'player', '', 'Csal', first_room)

auth.cleanup_expired()
dictionary.warm_up()
server.apply_runtime_settings()
server.admin_system.capture_output()
try:
    ai_player.get_vocabulary()
    practice.warm_up()
    daily.ensure_puzzle()
except Exception as error:     # a feladvány hiánya nem akadályozza a kipróbálást
    print('warm-up:', error)
server.socketio.start_background_task(server._admin_broadcaster)
print('READY', flush=True)
server.socketio.run(server.app, host='127.0.0.1', port=PORT, debug=False, use_reloader=False)
