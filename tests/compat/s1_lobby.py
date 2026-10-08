import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from harness import *

def scenario(sc):
    g = sc.client('guest', guest_name='Vendeg')
    sc.step('guest set_name', wait=0.5)
    sc.step('guest get_rooms', (g, 'get_rooms', {}))
    sc.step('guest create_room (tiltott)', (g, 'create_room', {'name': 'Szoba', 'max_players': 2}))
    u1 = sc.client('u1', user='u1'); sc.step('u1 set_name', wait=0.5)
    bad = sc.client('bad', guest_name='Rossz')
    bad.emit('set_name', {'name': 'Rossz', 'is_guest': False, 'auth_token': 'hamis'}); sc.step('hamis token')
    for name, opts in [('üres név', {'name': '', 'max_players': 2}), ('max_players 9', {'name': 'Teszt', 'max_players': 9}),
                       ('rossz időlimit', {'name': 'Teszt', 'max_players': 2, 'turn_time_limit': 45}),
                       ('rossz hint', {'name': 'Teszt', 'max_players': 2, 'hint_limit': 4}),
                       ('rossz robot', {'name': 'Teszt', 'max_players': 2, 'ai_players': ['zzz']}),
                       ('túl sok robot', {'name': 'Teszt', 'max_players': 4, 'ai_players': [1, 2, 3, 4]}),
                       ('hosszú név', {'name': 'x' * 60, 'max_players': 2})]:
        sc.step('u1 create_room: ' + name, (u1, 'create_room', opts))
        for s in sc.transcript[-1:]: pass
    sc.step('u1 create_room privát+challenge', (u1, 'create_room', {'name': 'Privát', 'max_players': 3, 'is_private': True, 'challenge_mode': True, 'turn_time_limit': 60}))
    code = u1.room_code
    sc.step('u1 create_room újra (már szobában)', (u1, 'create_room', {'name': 'Második', 'max_players': 2}))
    u2 = sc.client('u2', user='u2'); sc.step('u2 set_name', wait=0.5)
    sc.step('u2 get_rooms (privát nem látszik)', (u2, 'get_rooms', {}))
    sc.step('u2 join rossz kód', (u2, 'join_room', {'code': '000000'}))
    sc.step('u2 join üres', (u2, 'join_room', {}))
    sc.step('u2 join kóddal', (u2, 'join_room', {'code': code}))
    sc.step('u2 join újra', (u2, 'join_room', {'code': code}))
    u3 = sc.client('u3', user='u3'); sc.step('u3 set_name', wait=0.5)
    sc.step('u3 join', (u3, 'join_room', {'code': code}))
    u4 = sc.client('u4', user='u4'); sc.step('u4 set_name', wait=0.5)
    sc.step('u4 join (tele)', (u4, 'join_room', {'code': code}))
    sc.step('u2 start (nem tulajdonos)', (u2, 'start_game', {}))
    sc.step('u2 leave', (u2, 'leave_room', {}))
    sc.step('u1 leave (átadás)', (u1, 'leave_room', {}))
    sc.step('u3 start (új tulajdonos, 1 játékos)', (u3, 'start_game', {}))
    sc.step('u3 leave', (u3, 'leave_room', {}))
    sc.step('guest join_room nem létező id', (g, 'join_room', {'room_id': 'nincs'}))
    # nyilvános szoba, rate limit
    sc.step('u4 create nyilvános', (u4, 'create_room', {'name': 'Nyilvános', 'max_players': 2}))
    sc.step('u2 get_rooms (látja)', (u2, 'get_rooms', {}))
    rooms_id = u4.room_id
    sc.step('u2 join id alapján', (u2, 'join_room', {'room_id': rooms_id}))
    sc.step('rate limit: 6x set_name', *[(u2, 'set_name', {'name': 'Jatekos2', 'is_guest': True}) for _ in range(7)])
    sc.step('u4 start 2 játékos', (u4, 'start_game', {}), wait=0.8)

if __name__ == '__main__':
    run_both(scenario, 's1_lobby', 100)
