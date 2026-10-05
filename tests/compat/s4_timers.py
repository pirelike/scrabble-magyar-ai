import os, sys, threading; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from harness import *

def my_turn(c):
    return c.state and c.state.get('current_player') == my_id(c) and not c.state.get('finished')

def scenario(sc):
    admin = requests.Session()
    admin.post(sc.base + '/api/auth/login', json={'email': 'admin@example.com', 'password': 'secret12'})
    r = admin.patch(sc.base + '/api/admin/settings', headers={'X-Admin-Request': '1', 'Origin': sc.base},
                    json={'changes': {'grace_disconnect': 15, 'grace_waiting_owner': 60}, 'reason': 'időzítő próba'})
    sc.note('settings', r.json())
    u1 = sc.client('u1', user='u1'); u2 = sc.client('u2', user='u2'); sc.step('set_name', wait=0.6)
    sc.step('create', (u1, 'create_room', {'name': 'Időzítős', 'max_players': 2, 'challenge_mode': True, 'turn_time_limit': 60}))
    sc.step('join', (u2, 'join_room', {'code': u1.room_code}), wait=0.6)
    sc.step('start', (u1, 'start_game', {}), wait=0.7)
    cs = [u1, u2]
    cur = current_client(cs); oth = u2 if cur is u1 else u1
    sc.relabel({cur: 'cur', oth: 'oth'})
    mv = first_move(my_hand(cur), size=3) or first_move(my_hand(cur))
    sc.step('place (szavazás)', (cur, 'place_tiles', {'tiles': mv}), wait=0.7)
    sc.step('várakozás 33 s: automatikus elfogadás', wait=33, coarse=True)
    sc.note('after accept', {'turn': u1.state['turn_number'], 'pending': u1.state['pending_challenge'] is not None})
    sc.step('várakozás 63 s: időlimit lejár', wait=63, coarse=True)
    sc.note('after timeout', {'turn': u1.state['turn_number']})
    sc.step('oth lecsatlakozik', lambda: oth.close(), wait=1.0, coarse=True)
    sc.step('várakozás 17 s: türelmi idő lejár', wait=17, coarse=True)
    sc.note('state', {'players': [(p['name'], p['disconnected']) for p in cur.state['players']]} if cur.state else None)
    sc.step('cur lecsatlakozik (tulajdonos)', lambda: cur.close(), wait=1.0, coarse=True)
    sc.step('várakozás 17 s: mentés + feloszlatás', wait=17, coarse=True)
    u3 = sc.client('u3', user='u3'); u4 = sc.client('u4', user='u4'); sc.step('u3 u4 set_name', wait=0.6)
    sc.step('get_rooms', (u4, 'get_rooms', None))
    sc.note('saved', u3.http.get(sc.base + '/api/auth/saved-games').json())
    sc.step('u3 váró szoba', (u3, 'create_room', {'name': 'Váró', 'max_players': 2}))
    sc.step('u3 lecsatlakozik', lambda: u3.close(), wait=1.0, coarse=True)
    sc.step('várakozás 62 s: a váró szoba megszűnik', wait=62, coarse=True)
    sc.step('get_rooms', (u4, 'get_rooms', None))

if __name__ == '__main__':
    out = {}
    def go(tag, port):
        sc = Scenario(f'http://localhost:{port}')
        try: scenario(sc)
        finally: sc.close()
        out[tag] = sc.transcript
    ts = [threading.Thread(target=go, args=(t, p)) for t, p in (('py', PY_PORT), ('rs', RS_PORT))]
    [t.start() for t in ts]; [t.join() for t in ts]
    base = OUT
    for tag in out: json.dump(out[tag], open(base + f's4_timers.{tag}.json', 'w'), ensure_ascii=False, indent=1)
    lines = compare(out['py'], out['rs'], 80)
    print(f'== timers: {len(out["py"])} lépés, eltérés: {len(lines)} sor'); print('\n'.join(l[:300] for l in lines))
