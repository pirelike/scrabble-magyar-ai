import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from harness import *

def scenario(sc):
    u1 = sc.client('u1', user='u1'); u2 = sc.client('u2', user='u2'); u3 = sc.client('u3', user='u3')
    sc.step('set_name', wait=0.6)
    sc.step('create', (u1, 'create_room', {'name': 'Játék', 'max_players': 3, 'challenge_mode': True}))
    sc.step('join', (u2, 'join_room', {'code': u1.room_code}), (u3, 'join_room', {'code': u1.room_code}), wait=0.6)
    sc.step('start', (u1, 'start_game', {}), wait=0.6)
    cs = [u1, u2, u3]
    cur = current_client(cs)
    others = [c for c in cs if c is not cur]
    sc.relabel({cur: 'cur', others[0]: 'o1', others[1]: 'o2'})
    sc.step('nem soros: place', (others[0], 'place_tiles', {'tiles': [{'row': 7, 'col': 7, 'letter': 'A', 'is_blank': False}]}))
    sc.step('nem soros: pass', (others[0], 'pass_turn', None))
    sc.step('nem soros: exchange', (others[0], 'exchange_tiles', {'tiles': ['A']}))
    sc.step('place: üres', (cur, 'place_tiles', {'tiles': []}))
    sc.step('place: nem lista', (cur, 'place_tiles', {'tiles': 'x'}))
    sc.step('place: hibás elem', (cur, 'place_tiles', {'tiles': [{'row': 'a', 'col': 7, 'letter': 'A'}]}))
    sc.step('place: táblán kívül', (cur, 'place_tiles', {'tiles': [{'row': 99, 'col': 7, 'letter': 'A', 'is_blank': False}]}))
    sc.step('place: nincs a kézben', (cur, 'place_tiles', {'tiles': [{'row': 7, 'col': 7, 'letter': 'X', 'is_blank': False}]}))
    hand = my_hand(cur)
    letter = next(h for h in hand if h)
    sc.step('place: egy zseton középen (túl rövid?)', (cur, 'place_tiles', {'tiles': [{'row': 7, 'col': 7, 'letter': letter, 'is_blank': False}]}))
    move = first_move(hand, size=3) or first_move(hand, size=2) or first_move(hand, size=4) or first_move(hand)
    sc.note('first_move_found', {'ok': move is not None, 'size': len(move)})
    bad = [dict(t) for t in move]; 
    for t in bad: t['row'] = 3
    sc.step('preview: nem a közepén', (cur, 'preview_move', {'tiles': bad}))
    sc.step('preview: jó', (cur, 'preview_move', {'tiles': move}))
    sc.step('preview: üres', (cur, 'preview_move', {'tiles': []}))
    diag = [dict(t) for t in move]
    if len(diag) > 1: diag[1]['row'] += 1
    sc.step('place: nem egy vonalban', (cur, 'place_tiles', {'tiles': diag}))
    sc.step('place: jó (szavazás indul)', (cur, 'place_tiles', {'tiles': move}), wait=0.6)
    sc.step('pending: a lerakó nem fogadhat el', (cur, 'accept_words', None))
    sc.step('pending: lerakó withdraw', (cur, 'withdraw_words', None), wait=0.6)
    sc.step('újra lerak', (cur, 'place_tiles', {'tiles': move}), wait=0.6)
    sc.step('mindenki elfogad', (others[0], 'accept_words', None), (others[1], 'accept_words', None), wait=0.8)
    sc.note('scores', [(p['name'], p['score']) for p in u1.state['players']])
    # második kör
    cur2 = current_client(cs)
    others2 = [c for c in cs if c is not cur2]
    sc.note('current2', {'role': cur2.label})
    board = cur2.state['board']
    m2 = cross_move(board, my_hand(cur2), size=2) or cross_move(board, my_hand(cur2), size=1) or cross_move(board, my_hand(cur2))
    sc.note('cross_move_found', {'ok': m2 is not None, 'size': len(m2) if m2 else 0})
    if m2:
        sc.step('r2: preview', (cur2, 'preview_move', {'tiles': m2}))
        sc.step('r2: place', (cur2, 'place_tiles', {'tiles': m2}), wait=0.6)
        sc.step('r2: megtámad', (others2[0], 'reject_words', None), wait=0.6)
        sc.step('r2: megtámadó szavaz?', (others2[0], 'accept_words', None))
        sc.step('r2: a harmadik elutasít', (others2[1], 'reject_words', None), wait=1.0)
        sc.note('scores2', [(p['name'], p['score']) for p in u1.state['players']])
    cur3 = current_client(cs)
    sc.note('current3', {'role': cur3.label})
    sc.step('r3: exchange hibás', (cur3, 'exchange_tiles', {'tiles': ['Q']}))
    sc.step('r3: exchange üres', (cur3, 'exchange_tiles', {'tiles': []}))
    h3 = my_hand(cur3)
    sc.step('r3: exchange jó', (cur3, 'exchange_tiles', {'tiles': [h3[0]]}), wait=0.6)
    sc.step('chat', (u1, 'send_chat', {'message': 'Szia mindenkinek'}))
    sc.step('chat: túl hosszú', (u1, 'send_chat', {'message': 'x' * 300}))
    sc.step('chat: üres', (u1, 'send_chat', {'message': '   '}))
    sc.step('chat: spam', *[(u2, 'send_chat', {'message': f'üzenet {i}'}) for i in range(12)], wait=0.8)
    # passzok a játék végéig (6 egymás utáni pont nélküli kör)
    for i in range(8):
        cc = current_client(cs)
        sc.step(f'pass {i}', (cc, 'pass_turn', None), wait=0.5)
        if u1.state.get('finished'):
            break
    sc.note('finished', {'finished': u1.state.get('finished'), 'winner': u1.state.get('winner'), 'winners': u1.state.get('winners')})
    sc.step('befejezett: pass', (u1, 'pass_turn', None))
    sc.step('befejezett: chat', (u1, 'send_chat', {'message': 'jó játék'}))

if __name__ == '__main__':
    run_both(scenario, 's2_game', 140)
