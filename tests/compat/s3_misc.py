import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from harness import *

def my_turn(c):
    return c.state and c.state.get('current_player') == my_id(c) and not c.state.get('finished')

def bots(sc):
    u1 = sc.client('u1', user='u1'); sc.step('set_name', wait=0.6)
    sc.step('create bots', (u1, 'create_room', {'name': 'Botos', 'max_players': 3, 'ai_players': [3, 'auto'], 'hint_limit': 3}), coarse=True)
    sc.step('start', (u1, 'start_game', {}), coarse=True, wait=0.8)
    sc.wait_until(lambda: my_turn(u1), 40, 'bot lépések az elejéig')
    sc.step('hint', (u1, 'request_hint', None), wait=2.0, coarse=True)
    hand = my_hand(u1)
    mv = first_move(hand, size=3) or first_move(hand, size=2) or first_move(hand)
    if u1.state['board'][7][7] is not None:
        mv = cross_move(u1.state['board'], hand, size=2) or cross_move(u1.state['board'], hand)
    sc.note('move', {'found': mv is not None})
    if mv:
        sc.step('preview', (u1, 'preview_move', {'tiles': mv}))
        sc.step('place (robot nem szavaz: szótár)', (u1, 'place_tiles', {'tiles': mv}), wait=0.8, coarse=True)
    else:
        sc.step('pass', (u1, 'pass_turn', None), coarse=True)
    sc.wait_until(lambda: my_turn(u1), 40, 'bot lépések')
    sc.step('hint 2', (u1, 'request_hint', None), wait=2.0, coarse=True)
    sc.step('hint 3', (u1, 'request_hint', None), wait=2.0, coarse=True)
    sc.step('hint 4 (elfogyott)', (u1, 'request_hint', None), wait=2.0, coarse=True)
    sc.step('exchange', (u1, 'exchange_tiles', {'tiles': [next(h for h in my_hand(u1))]}), wait=0.8, coarse=True)
    sc.wait_until(lambda: my_turn(u1), 40, 'bot lépések 3')
    sc.step('pass', (u1, 'pass_turn', None), coarse=True, wait=0.8)
    sc.step('save', (u1, 'save_game', None), coarse=True)
    sc.step('leave aktív játékból', (u1, 'leave_room', None), coarse=True, wait=0.8)

def second_human_game(sc, challenge=False, public=True):
    u1 = sc.client('u1', user='u1'); u2 = sc.client('u2', user='u2'); sc.step('set_name', wait=0.6)
    sc.step('create', (u1, 'create_room', {'name': 'Ketten', 'max_players': 2, 'challenge_mode': challenge, 'is_private': not public}))
    sc.step('join', (u2, 'join_room', {'code': u1.room_code}), wait=0.6)
    sc.step('start', (u1, 'start_game', {}), wait=0.7)
    return u1, u2

def reconnect(sc):
    u1, u2 = second_human_game(sc)
    token = u2.reconnect_token
    sc.note('token', {'have': bool(token)})
    sc.step('u2 kapcsolat megszakad', lambda: u2.close(), wait=0.8)
    u2b = sc.client('u2b', user='u2'); sc.step('u2b set_name', wait=0.6)
    sc.step('rejoin rossz token', (u2b, 'rejoin_room', {'token': 'hamis'}))
    sc.step('rejoin üres', (u2b, 'rejoin_room', {}))
    sc.step('rejoin jó token', (u2b, 'rejoin_room', {'token': token}), wait=0.8)
    sc.step('rejoin újra (már bent)', (u2b, 'rejoin_room', {'token': token}))
    cur = u1 if my_turn(u1) else u2b
    sc.step('pass', (cur, 'pass_turn', None))
    # takeover: ugyanaz a felhasználó második kapcsolatról átveszi
    u2c = sc.client('u2c', user='u2'); sc.step('u2c set_name', wait=0.6)
    sc.step('u2c rejoin (átvétel)', (u2c, 'rejoin_room', {'token': token}), wait=0.8)
    sc.step('logout', (u1, 'logout', None), wait=0.6)
    sc.step('u2c leave aktív játékból', (u2c, 'leave_room', None), wait=0.6)

def waiting_grace(sc):
    u1 = sc.client('u1', user='u1'); sc.step('set_name', wait=0.6)
    sc.step('create', (u1, 'create_room', {'name': 'Váró', 'max_players': 2}))
    token = u1.reconnect_token
    sc.step('u1 kapcsolat megszakad', lambda: u1.close(), wait=0.8)
    u1b = sc.client('u1b', user='u1'); sc.step('u1b set_name', wait=0.6)
    sc.step('u1b rejoin', (u1b, 'rejoin_room', {'token': token}), wait=0.8)
    u2 = sc.client('u2', user='u2'); sc.step('u2 set_name', wait=0.6)
    sc.step('u2 join (kód)', (u2, 'join_room', {'code': u1b.room_code}), wait=0.6)
    sc.step('u2 disconnect váróban', lambda: u2.close(), wait=0.8)
    sc.step('get_rooms', (u1b, 'get_rooms', None))

def spectators(sc):
    u1, u2 = second_human_game(sc)
    u3 = sc.client('u3', user='u3'); g = sc.client('guest', guest_name='Néző'); sc.step('néző set_name', wait=0.6)
    room_id = u1.room_id
    sc.step('spectate nincs ilyen', (u3, 'spectate_room', {'room_id': 'nincs'}))
    sc.step('spectate üres', (u3, 'spectate_room', {}))
    sc.step('spectate', (u3, 'spectate_room', {'room_id': room_id}), wait=0.7)
    sc.step('guest spectate kóddal', (g, 'spectate_room', {'code': u1.room_code}), wait=0.7)
    sc.step('néző place', (u3, 'place_tiles', {'tiles': [{'row': 7, 'col': 7, 'letter': 'A', 'is_blank': False}]}))
    sc.step('néző chat', (u3, 'send_chat', {'message': 'szia'}))
    sc.step('néző pass', (u3, 'pass_turn', None))
    cur = u1 if my_turn(u1) else u2
    sc.step('játékos pass (nézők látják)', (cur, 'pass_turn', None))
    sc.step('chat (nézők látják?)', (u1, 'send_chat', {'message': 'üdv'}))
    sc.step('néző leave_spectate', (u3, 'leave_spectate', None), wait=0.6)
    sc.step('néző leave_spectate újra', (u3, 'leave_spectate', None))
    sc.step('owner kilép → feloszlik?', (u1, 'leave_room', None), wait=0.8)

def save_restore(sc):
    u1, u2 = second_human_game(sc, public=False)
    cur = u1 if my_turn(u1) else u2
    sc.step('pass', (cur, 'pass_turn', None))
    sc.step('save nem tulajdonos', (u2, 'save_game', None))
    sc.step('save', (u1, 'save_game', None), wait=0.6)
    r = u1.http.get(f'{u1.base}/api/auth/saved-games').json()
    sc.note('saved list', r)
    gid = r['games'][0]['game_id'] if r.get('games') else None
    sc.step('leave', (u1, 'leave_room', None), wait=0.7)
    sc.step('u2 leave', (u2, 'leave_room', None), wait=0.7)
    sc.step('restore nincs', (u1, 'restore_game', {'game_id': 99999}))
    sc.step('restore idegen (u3)', lambda: None)
    sc.step('restore', (u1, 'restore_game', {'game_id': gid}), wait=0.8)
    sc.step('u2 join mentett', (u2, 'join_room', {'code': u1.room_code}), wait=0.8)
    u3 = sc.client('u3', user='u3'); sc.step('u3 set_name', wait=0.6)
    sc.step('u3 join mentett (idegen)', (u3, 'join_room', {'code': u1.room_code}))
    sc.step('start restore', (u1, 'start_game', {}), wait=0.8)
    sc.step('saved-games után', lambda: sc.note('saved list 2', u1.http.get(f'{u1.base}/api/auth/saved-games').json()))
    sc.step('abandon', lambda: sc.note('abandon', u1.http.post(f'{u1.base}/api/game/{gid}/abandon').json()))

def daily(sc):
    u1 = sc.client('u1', user='u1'); g = sc.client('guest', guest_name='Vendeg'); sc.step('set_name', wait=0.6)
    sc.step('start_daily', (u1, 'start_daily', None), wait=1.2)
    sc.step('start_daily újra (szobában)', (u1, 'start_daily', None), wait=0.8)
    st = u1.state
    sc.note('puzzle', {'has': bool(st and st.get('puzzle'))})
    if st:
        mv = cross_move(st['board'], my_hand(u1), size=2) or cross_move(st['board'], my_hand(u1))
        sc.note('move', {'found': mv is not None})
        if mv:
            sc.step('daily place', (u1, 'place_tiles', {'tiles': mv}), wait=1.0)
    sc.step('daily pass (tiltott)', (u1, 'pass_turn', None))
    sc.step('retry_daily', (u1, 'retry_daily', None), wait=1.0)
    sc.step('reveal_daily', (u1, 'reveal_daily', None), wait=1.0)
    sc.step('reveal újra', (u1, 'reveal_daily', None), wait=0.6)
    sc.step('leave', (u1, 'leave_room', None), wait=0.6)
    sc.step('guest start_daily', (g, 'start_daily', None), wait=1.2)
    sc.step('guest reveal', (g, 'reveal_daily', None), wait=0.8)
    sc.note('http daily', u1.http.get(f'{u1.base}/api/daily').json())
    sc.note('http daily lb', u1.http.get(f'{u1.base}/api/daily/leaderboard').json())

def friends(sc):
    u1 = sc.client('u1', user='u1'); u2 = sc.client('u2', user='u2'); u3 = sc.client('u3', user='u3'); sc.step('set_name', wait=0.7)
    sc.step('friend req self', (u1, 'send_friend_request', {'friend_id': 1}))
    sc.step('friend req nincs', (u1, 'send_friend_request', {'friend_id': 999}))
    sc.step('friend req bad', (u1, 'send_friend_request', {'friend_id': 'x'}))
    sc.step('friend req u2', (u1, 'send_friend_request', {'friend_id': 2}))
    sc.step('friend req dup', (u1, 'send_friend_request', {'friend_id': 2}))
    sc.step('u2 accept bad', (u2, 'accept_friend_request', {'requester_id': 3}))
    sc.step('u2 accept', (u2, 'accept_friend_request', {'requester_id': 1}), wait=0.6)
    sc.step('friend req u3', (u1, 'send_friend_request', {'friend_id': 3}))
    sc.step('u3 decline', (u3, 'decline_friend_request', {'requester_id': 1}))
    sc.step('invite: nincs szoba', (u1, 'invite_to_room', {'friend_id': 2}))
    sc.step('create', (u1, 'create_room', {'name': 'Meghívós', 'max_players': 2}))
    sc.step('invite nem barát', (u1, 'invite_to_room', {'friend_id': 3}))
    sc.step('invite', (u1, 'invite_to_room', {'friend_id': 2}), wait=0.6)
    sc.step('invite újra', (u1, 'invite_to_room', {'friend_id': 2}))
    inv = None
    for e, d in u2.events:
        pass
    sc.step('respond bad', (u2, 'respond_invite', {'invite_id': 999, 'accept': True}))
    sc.step('remove friend', (u1, 'remove_friend', {'friend_id': 2}), wait=0.6)
    sc.step('remove friend again', (u1, 'remove_friend', {'friend_id': 2}))

def async_game(sc):
    u1 = sc.client('u1', user='u1'); u2 = sc.client('u2', user='u2'); sc.step('set_name', wait=0.7)
    sc.step('async nincs barát', (u1, 'create_async_game', {'name': 'Levél', 'friend_ids': [2], 'turn_hours': 24}))
    sc.step('friend req', (u1, 'send_friend_request', {'friend_id': 2}))
    sc.step('friend accept', (u2, 'accept_friend_request', {'requester_id': 1}), wait=0.6)
    sc.step('async bad hours', (u1, 'create_async_game', {'name': 'Levél', 'friend_ids': [2], 'turn_hours': 5}))
    sc.step('async no friends', (u1, 'create_async_game', {'name': 'Levél', 'friend_ids': [], 'turn_hours': 24}))
    sc.step('async self', (u1, 'create_async_game', {'name': 'Levél', 'friend_ids': [1], 'turn_hours': 24}))
    sc.step('async create', (u1, 'create_async_game', {'name': 'Levél', 'friend_ids': [2], 'turn_hours': 24}), wait=1.0)
    r = u1.http.get(f'{u1.base}/api/async/games').json()
    sc.note('async list', r)
    gid = r['games'][0]['game_id'] if r.get('games') else None
    sc.note('states', {'u1': bool(u1.state), 'u2': bool(u2.state)})
    cur = next((c for c in (u1, u2) if c.state and my_turn(c)), None)
    if cur:
        mv = first_move(my_hand(cur), size=3) or first_move(my_hand(cur))
        sc.step('async place', (cur, 'place_tiles', {'tiles': mv}), wait=0.8)
    sc.step('async chat', (u1, 'send_chat', {'message': 'levél'}))
    sc.step('u1 leave (nézet bezár)', (u1, 'leave_room', None), wait=0.6)
    sc.step('u2 list', lambda: sc.note('async list u2', u2.http.get(f'{u2.base}/api/async/games').json()))
    sc.step('u1 open', (u1, 'open_async_game', {'game_id': gid}), wait=1.0)
    sc.step('open bad', (u2, 'open_async_game', {'game_id': 99999}))
    sc.step('resign (nem szobában)', lambda: None)
    sc.step('u1 resign', (u1, 'resign_game', None), wait=1.0)

def reports(sc):
    u1, u2 = second_human_game(sc)
    sc.step('report chat', (u1, 'report_content', {'kind': 'chat', 'name': 'Jatekos2', 'message': 'csúnya', 'reason': 'Sértő'}))
    sc.step('report player', (u1, 'report_content', {'kind': 'player', 'name': 'Jatekos2', 'message': '', 'reason': 'Csal'}))
    sc.step('report bad kind', (u1, 'report_content', {'kind': 'zzz', 'name': 'Jatekos2', 'reason': 'x'}))
    sc.step('report self', (u1, 'report_content', {'kind': 'player', 'name': 'Jatekos1', 'reason': 'x'}))
    sc.step('report nincs név', (u1, 'report_content', {'kind': 'player', 'name': 'Senki', 'reason': 'x'}))
    sc.step('set_visibility', (u1, 'set_visibility', {'hidden': True}))
    sc.step('set_visibility 2', (u1, 'set_visibility', {'hidden': False}))
    g = sc.client('guest', guest_name='Vendég2'); sc.step('guest', wait=0.5)
    sc.step('guest report', (g, 'report_content', {'kind': 'player', 'name': 'Jatekos1', 'reason': 'x'}))
    sc.step('hint (2 ember)', (u1, 'request_hint', None))
    sc.step('admin_subscribe nem admin', (u1, 'admin_subscribe', {}))
    sc.step('admin_watch nem admin', (u1, 'admin_watch_room', {'room_id': u1.room_id}))

SCENARIOS = {'bots': bots, 'reconnect': reconnect, 'waiting': waiting_grace, 'spectators': spectators, 'saverestore': save_restore,
             'daily': daily, 'friends': friends, 'async': async_game, 'reports': reports}

if __name__ == '__main__':
    run_both(SCENARIOS[sys.argv[1]], 's3_' + sys.argv[1], 100)
