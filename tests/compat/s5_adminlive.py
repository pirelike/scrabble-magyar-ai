import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from harness import *
from http_diff import hnorm

H = lambda base: {'X-Admin-Request': '1', 'Origin': base}

def scenario(sc):
    ad = sc.client('admin', user='admin'); u1 = sc.client('u1', user='u1'); u2 = sc.client('u2', user='u2'); u3 = sc.client('u3', user='u3')
    sc.step('set_name', wait=0.7)
    def API(method, path, body=None, label=None):
        r = ad.http.request(method, sc.base + '/api/admin' + path, json=body, headers=H(sc.base))
        try: data = r.json()
        except Exception: data = r.text[:80]
        sc.note(f'{label or method + " " + path}: {r.status_code}', data)
        return data
    API('POST', '/sudo', {'password': 'secret12'}, 'sudo')
    sc.step('u1 admin_subscribe (nem admin)', (u1, 'admin_subscribe', {}))
    tok = ad.http.get(sc.base + '/api/auth/socket-token').json()['token']
    sc.step('admin_subscribe tokennel', (ad, 'admin_subscribe', {'auth_token': tok}), wait=0.8)
    sc.step('admin_subscribe újra (játékból)', (ad, 'admin_subscribe', {}), wait=0.5)
    sc.step('u1 szoba', (u1, 'create_room', {'name': 'Admin próba', 'max_players': 3, 'challenge_mode': True, 'turn_time_limit': 60}))
    sc.step('u2 u3 join', (u2, 'join_room', {'code': u1.room_code}), (u3, 'join_room', {'code': u1.room_code}), wait=0.8)
    room_id = u1.room_id
    sc.step('admin_watch_room', (ad, 'admin_watch_room', {'room_id': room_id}), wait=0.6)
    sc.step('start', (u1, 'start_game', {}), wait=0.9, coarse=True)
    API('GET', '/rooms', None, 'rooms')
    API('GET', f'/rooms/{u1.room_code}', None, 'room by code')
    API('GET', f'/rooms/{room_id}/racks', None, 'racks')
    def act(action, **kw):
        body = {'action': action, 'reason': 'admin próba'}; body.update(kw)
        r = API('POST', f'/rooms/{room_id}/action', body, f'action {action}')
        time.sleep(0.5); sc.record(f'után: {action}', coarse=True)
        return r
    act('message', message='Üzenet az adminról')
    act('message', message='')
    act('bogus')
    act('extend', seconds=60); act('extend', seconds=7)
    act('pause'); act('pause'); act('resume'); act('resume')
    cs = [u1, u2, u3]
    cur = current_client(cs)
    mv = first_move(my_hand(cur), size=3) or first_move(my_hand(cur))
    sc.step('place (szavazás)', (cur, 'place_tiles', {'tiles': mv}), wait=0.8, coarse=True)
    act('skip'); act('pause')
    act('resolve_vote', accept='x'); act('resolve_vote', accept=True)
    act('skip')
    act('reschedule_bot')
    act('save')
    API('GET', f'/rooms/{room_id}', None, 'room detail')
    act('transfer', player='Jatekos3'); act('transfer', player='Senki')
    act('kick', player='Jatekos2'); act('kick', player='Senki')
    act('kick', player='Jatekos3')
    sc.step('u1 chat a megmaradtaknak', (u1, 'send_chat', {'message': 'él még'}))
    act('end')
    API('GET', '/games?limit=5', None, 'games')
    # második szoba: érvénytelenítés
    u4 = sc.client('u4', user='u4'); u5 = sc.client('u5', user='u5'); sc.step('u4 u5', wait=0.6)
    sc.step('szoba 2', (u4, 'create_room', {'name': 'Második', 'max_players': 2}), wait=0.5, coarse=True)
    sc.step('join 2', (u5, 'join_room', {'code': u4.room_code}), wait=0.5, coarse=True)
    sc.step('start 2', (u4, 'start_game', {}), wait=0.8, coarse=True)
    rid2 = u4.room_id
    r = API('POST', f'/rooms/{rid2}/action', {'action': 'void', 'reason': 'admin próba'}, 'void')
    time.sleep(0.6); sc.record('után: void', coarse=True)
    # harmadik szoba: feloszlatás
    u6 = sc.client('u6', user='u6'); sc.step('u6', wait=0.5)
    sc.step('szoba 3', (u6, 'create_room', {'name': 'Harmadik', 'max_players': 2}), wait=0.5, coarse=True)
    rid3 = u6.room_id
    API('POST', f'/rooms/{rid3}/action', {'action': 'disband', 'reason': 'admin próba'}, 'disband')
    time.sleep(0.6); sc.record('után: disband', coarse=True)
    API('GET', '/audit?limit=60', None, 'audit')
    sc.step('admin kijelentkezik a szobából', (ad, 'logout', None), wait=0.6, coarse=True)

if __name__ == '__main__':
    run_both(scenario, 's5_adminlive', 100)
