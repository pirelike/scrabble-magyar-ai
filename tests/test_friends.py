import pytest
from unittest.mock import patch
from auth import (
    create_user, send_friend_request, accept_friend_request, 
    decline_friend_request, remove_friend, get_friends, 
    get_pending_requests, get_sent_requests, search_users, _db
)

# ---------------------------------------------------------
# 1. DB Szintű tesztek (auth.py függvények)
# ---------------------------------------------------------

def test_friend_request_flow(temp_db):
    """Teljes barátkérés küldés, elfogadás, törlés folyamat tesztelése."""
    _, user1_id = create_user('user1@test.com', 'User1', 'password123')
    _, user2_id = create_user('user2@test.com', 'User2', 'password123')
    
    # Magadnak nem küldhetsz
    success, msg = send_friend_request(user1_id, user1_id)
    assert not success
    
    # Nem létező felhasználónak nem küldhetsz
    success, msg = send_friend_request(user1_id, 9999)
    assert not success
    
    # Sikeres küldés
    success, msg = send_friend_request(user1_id, user2_id)
    assert success
    
    # Kétszer nem lehet küldeni
    success, msg = send_friend_request(user1_id, user2_id)
    assert not success
    
    # Ellenőrzés: pending requests
    pending = get_pending_requests(user2_id)
    assert len(pending) == 1
    assert pending[0]['id'] == user1_id
    
    sent = get_sent_requests(user1_id)
    assert len(sent) == 1
    assert sent[0]['id'] == user2_id
    
    # Elfogadás
    success, msg = accept_friend_request(user2_id, user1_id)
    assert success
    
    # Barátlista ellenőrzése
    friends1 = get_friends(user1_id)
    assert len(friends1) == 1
    assert friends1[0]['id'] == user2_id
    
    friends2 = get_friends(user2_id)
    assert len(friends2) == 1
    assert friends2[0]['id'] == user1_id
    
    # Törlés
    success, msg = remove_friend(user1_id, user2_id)
    assert success
    
    assert len(get_friends(user1_id)) == 0

def test_decline_friend_request(temp_db):
    """Barátkérés elutasításának tesztelése."""
    _, user1_id = create_user('user3@test.com', 'User3', 'password123')
    _, user2_id = create_user('user4@test.com', 'User4', 'password123')
    
    send_friend_request(user1_id, user2_id)
    assert len(get_pending_requests(user2_id)) == 1
    
    success, msg = decline_friend_request(user2_id, user1_id)
    assert success
    
    assert len(get_pending_requests(user2_id)) == 0
    assert len(get_friends(user1_id)) == 0

def test_search_users(temp_db):
    """Felhasználó keresés tesztelése."""
    _, u1 = create_user('alice@test.com', 'AliceSmith', 'pwd')
    _, u2 = create_user('bob@test.com', 'BobJones', 'pwd')
    _, u3 = create_user('charlie@test.com', 'CharlieAlice', 'pwd')
    
    # Keresés 'alice' névre (AliceSmith és CharlieAlice)
    results = search_users('alice', exclude_user_id=u1)
    assert len(results) == 1
    assert results[0]['id'] == u3  # u1 is excluded
    
    # Keresés email alapján
    results = search_users('bob@test', exclude_user_id=u1)
    assert len(results) == 1
    assert results[0]['id'] == u2
    
    # Rövid keresés
    assert len(search_users('a', exclude_user_id=u1)) == 0

def test_accept_nonexistent_request(temp_db):
    _, u1 = create_user('u1@test.com', 'U1', 'pwd')
    _, u2 = create_user('u2@test.com', 'U2', 'pwd')
    success, msg = accept_friend_request(u1, u2)
    assert not success

def test_decline_nonexistent_request(temp_db):
    _, u1 = create_user('u1b@test.com', 'U1B', 'pwd')
    _, u2 = create_user('u2b@test.com', 'U2B', 'pwd')
    success, msg = decline_friend_request(u1, u2)
    assert not success

def test_remove_non_friend(temp_db):
    _, u1 = create_user('u1c@test.com', 'U1C', 'pwd')
    _, u2 = create_user('u2c@test.com', 'U2C', 'pwd')
    success, msg = remove_friend(u1, u2)
    assert not success

def test_get_friends_empty(temp_db):
    _, u1 = create_user('u1d@test.com', 'U1D', 'pwd')
    assert len(get_friends(u1)) == 0

def test_duplicate_friend_request(temp_db):
    _, u1 = create_user('u1e@test.com', 'U1E', 'pwd')
    _, u2 = create_user('u2e@test.com', 'U2E', 'pwd')
    send_friend_request(u1, u2)
    success, msg = send_friend_request(u2, u1)
    assert not success
    assert "küldött" in msg.lower() or "küldtél" in msg.lower() or "van" in msg.lower() or "barátok" in msg.lower()

def test_already_friends_request(temp_db):
    _, u1 = create_user('u1f@test.com', 'U1F', 'pwd')
    _, u2 = create_user('u2f@test.com', 'U2F', 'pwd')
    send_friend_request(u1, u2)
    accept_friend_request(u2, u1)
    success, msg = send_friend_request(u1, u2)
    assert not success
    assert "barátok" in msg.lower()

# ---------------------------------------------------------
# 2. Socket.IO és HTTP Integrációs Tesztek
# ---------------------------------------------------------

@pytest.fixture
def app():
    from server import app
    app.config['TESTING'] = True
    return app

@pytest.fixture
def socketio_app():
    from server import socketio
    return socketio

@pytest.fixture
def client(app):
    return app.test_client()

def login_user(client, email, password):
    resp = client.post('/api/auth/login', json={'email': email, 'password': password})
    return resp.json.get('user')

def test_http_get_friends_authenticated(client, temp_db):
    create_user('h1@test.com', 'H1', 'pwd')
    login_user(client, 'h1@test.com', 'pwd')
    resp = client.get('/api/auth/friends')
    assert resp.status_code == 200
    assert resp.json['friends'] == []

def test_http_get_friends_unauthenticated(client, temp_db):
    resp = client.get('/api/auth/friends')
    assert resp.status_code == 401

def test_http_search_users_authenticated(client, temp_db):
    create_user('s1@test.com', 'Search1', 'pwd')
    create_user('s2@test.com', 'Search2', 'pwd')
    login_user(client, 's1@test.com', 'pwd')
    
    resp = client.get('/api/auth/search-users?q=Search')
    assert resp.status_code == 200
    assert len(resp.json.get('users', [])) == 1
    assert resp.json['users'][0]['display_name'] == 'Search2'

def test_http_search_users_short_query(client, temp_db):
    create_user('s3@test.com', 'Search3', 'pwd')
    login_user(client, 's3@test.com', 'pwd')
    resp = client.get('/api/auth/search-users?q=S')
    assert resp.status_code == 200
    assert len(resp.json.get('users', [])) == 0

# Helper for Socket.IO clients
def create_registered_client(app, socketio_app, name, user_id):
    c = socketio_app.test_client(app)
    c.emit('set_name', {'name': name, 'is_guest': False, 'user_id': user_id})
    c.get_received()
    return c

def create_guest_client(app, socketio_app, name):
    c = socketio_app.test_client(app)
    c.emit('set_name', {'name': name, 'is_guest': True, 'user_id': None})
    c.get_received()
    return c

# Socket.IO tesztek
def test_socket_send_friend_request(client, app, socketio_app, temp_db):
    from server import state as st
    
    _, u1_id = create_user('sock1@test.com', 'Sock1', 'pwd')
    _, u2_id = create_user('sock2@test.com', 'Sock2', 'pwd')
    
    sio1 = create_registered_client(app, socketio_app, 'Sock1', u1_id)
    sio2 = create_registered_client(app, socketio_app, 'Sock2', u2_id)
    
    sio1.get_received()
    sio2.get_received()
    
    sio1.emit('send_friend_request', {'friend_id': u2_id})
    
    r1 = sio1.get_received()
    assert any(e['name'] == 'friend_request_result' and e['args'][0]['success'] for e in r1)
    
    r2 = sio2.get_received()
    assert any(e['name'] == 'friend_request_received' for e in r2)

def test_socket_send_friend_request_guest(client, app, socketio_app, temp_db):
    _, u2_id = create_user('sock3@test.com', 'Sock3', 'pwd')
    
    sio1 = create_guest_client(app, socketio_app, 'Guest1')
    
    sio1.get_received()
    sio1.emit('send_friend_request', {'target_id': u2_id})
    r1 = sio1.get_received()
    # It just returns or error, does not emit friend_request_result with true
    assert not any(e['name'] == 'friend_request_result' and e['args'][0]['success'] for e in r1)

def test_socket_accept_friend_request(client, app, socketio_app, temp_db):
    _, u1_id = create_user('sock4@test.com', 'Sock4', 'pwd')
    _, u2_id = create_user('sock5@test.com', 'Sock5', 'pwd')
    send_friend_request(u1_id, u2_id)
    
    sio1 = create_registered_client(app, socketio_app, 'Sock4', u1_id)
    sio2 = create_registered_client(app, socketio_app, 'Sock5', u2_id)
    
    sio1.get_received()
    sio2.get_received()
    
    sio2.emit('accept_friend_request', {'requester_id': u1_id})
    
    r2 = sio2.get_received()
    assert any(e['name'] == 'friend_request_result' and e['args'][0]['success'] for e in r2)
    
    r1 = sio1.get_received()
    assert any(e['name'] == 'friend_request_accepted' for e in r1)

def test_socket_decline_friend_request(client, app, socketio_app, temp_db):
    _, u1_id = create_user('sock6@test.com', 'Sock6', 'pwd')
    _, u2_id = create_user('sock7@test.com', 'Sock7', 'pwd')
    send_friend_request(u1_id, u2_id)
    
    sio2 = create_registered_client(app, socketio_app, 'Sock7', u2_id)
    
    sio2.get_received()
    sio2.emit('decline_friend_request', {'requester_id': u1_id})
    
    r2 = sio2.get_received()
    assert any(e['name'] == 'friend_request_result' and e['args'][0]['success'] for e in r2)

def test_socket_remove_friend(client, app, socketio_app, temp_db):
    _, u1_id = create_user('sock8@test.com', 'Sock8', 'pwd')
    _, u2_id = create_user('sock9@test.com', 'Sock9', 'pwd')
    send_friend_request(u1_id, u2_id)
    accept_friend_request(u2_id, u1_id)
    
    sio1 = create_registered_client(app, socketio_app, 'Sock8', u1_id)
    
    sio1.get_received()
    sio1.emit('remove_friend', {'friend_id': u2_id})
    
    r1 = sio1.get_received()
    assert any(e['name'] == 'friend_request_result' and e['args'][0]['success'] for e in r1)

def test_invite_friend_success(client, app, socketio_app, temp_db):
    from server import state as st
    
    _, u1_id = create_user('inv1@test.com', 'Inv1', 'pwd')
    _, u2_id = create_user('inv2@test.com', 'Inv2', 'pwd')
    send_friend_request(u1_id, u2_id)
    accept_friend_request(u2_id, u1_id)
    
    sio1 = create_registered_client(app, socketio_app, 'Inv1', u1_id)
    sio2 = create_registered_client(app, socketio_app, 'Inv2', u2_id)
    
    sio1.emit('create_room', {'name': 'testroom', 'max_players': 4})
    r1 = sio1.get_received()
    
    sio1.emit('invite_to_room', {'friend_id': u2_id})
    
    r1_invite = sio1.get_received()
    assert any(e['name'] == 'invite_sent' and e['args'][0]['success'] for e in r1_invite)
    
    r2 = sio2.get_received()
    assert any(e['name'] == 'game_invite' for e in r2)

def test_invite_non_friend_rejected(client, app, socketio_app, temp_db):
    _, u1_id = create_user('inv3@test.com', 'Inv3', 'pwd')
    _, u2_id = create_user('inv4@test.com', 'Inv4', 'pwd')
    
    sio1 = create_registered_client(app, socketio_app, 'Inv3', u1_id)
    
    sio1.emit('create_room', {'name': 'testroom', 'max_players': 4})
    sio1.get_received()
    
    sio1.emit('invite_to_room', {'friend_id': u2_id})
    
    r1 = sio1.get_received()
    invite_res = next((e for e in r1 if e['name'] == 'invite_sent'), None)
    assert invite_res
    assert not invite_res['args'][0]['success']
    assert "barát" in invite_res['args'][0]['message'].lower()

def test_respond_invite_accept(client, app, socketio_app, temp_db):
    _, u1_id = create_user('inv5@test.com', 'Inv5', 'pwd')
    _, u2_id = create_user('inv6@test.com', 'Inv6', 'pwd')
    send_friend_request(u1_id, u2_id)
    accept_friend_request(u2_id, u1_id)
    
    sio1 = create_registered_client(app, socketio_app, 'Inv5', u1_id)
    sio2 = create_registered_client(app, socketio_app, 'Inv6', u2_id)
    
    sio1.emit('create_room', {'name': 'testroom', 'max_players': 4})
    sio1.get_received()
    
    sio1.emit('invite_to_room', {'friend_id': u2_id})
    sio1.get_received()
    
    r2 = sio2.get_received()
    invite_event = next(e for e in r2 if e['name'] == 'game_invite')
    invite_id = invite_event['args'][0]['invite_id']
    
    sio2.emit('respond_invite', {'invite_id': invite_id, 'accept': True})
    
    r2_after = sio2.get_received()
    assert any(e['name'] == 'invite_accepted' for e in r2_after)

def test_online_status_flow(client, app, socketio_app, temp_db):
    from server import state as st
    
    _, u1_id = create_user('onl1@test.com', 'Onl1', 'pwd')
    
    assert not st.is_user_online(u1_id)
    
    # Belép
    sio1 = create_registered_client(app, socketio_app, 'Onl1', u1_id)
    
    assert st.is_user_online(u1_id)
    
    # Kilép (test client disconnet nem fut le auto, szimuláljuk)
    sids = list(st._online_users[u1_id])
    st.unregister_player(sids[0])
    
    assert not st.is_user_online(u1_id)

def test_multiple_sids_one_user(client, app, socketio_app, temp_db):
    from server import state as st
    
    _, u1_id = create_user('onl2@test.com', 'Onl2', 'pwd')
    
    sio1 = create_registered_client(app, socketio_app, 'Onl2', u1_id)
    sio2 = create_registered_client(app, socketio_app, 'Onl2', u1_id)
    
    sids = list(st._online_users[u1_id])
    assert len(sids) == 2
    assert st.is_user_online(u1_id)
    
    # Egyik kilép
    st.unregister_player(sids[0])
    
    # Még online a másik miatt
    assert st.is_user_online(u1_id)
    
    # Másik is kilép
    st.unregister_player(sids[1])
    assert not st.is_user_online(u1_id)


def test_online_status_cleared_on_real_disconnect(app, socketio_app, temp_db):
    """handle_disconnect() eltávolítja a felhasználót az _online_users-ből.

    Regressziós teszt: korábban a _sid_to_user_id és _online_users soha nem
    tisztult le disconnect-kor (unregister_player nem volt meghívva), ezért
    böngészőzárás után a játékos örökre online-nak látszott a barátok számára
    és meghívható maradt.
    """
    from server import state as st

    _, u1_id = create_user('disc_real@test.com', 'DiscReal', 'pwd')

    c = create_registered_client(app, socketio_app, 'DiscReal', u1_id)
    assert st.is_user_online(u1_id), "set_name után online kell legyen"

    # Valódi disconnect — lefuttatja a szerver handle_disconnect handler-t
    c.disconnect()

    assert not st.is_user_online(u1_id), (
        "Disconnectelés után nem szabad online-nak látszani; "
        "a handle_disconnect-nek hívnia kell remove_online_user(sid)-t"
    )
    assert u1_id not in st._online_users, "_online_users-ben sem maradhat bejegyzés"
    # _sid_to_user_id is tisztának kell lennie
    for sid, uid in st._sid_to_user_id.items():
        assert uid != u1_id, "_sid_to_user_id-ban sem maradhat a felhasználó"


def test_online_status_restored_on_rejoin(app, socketio_app, temp_db):
    """rejoin_room visszaveszi a felhasználót az _online_users-be.

    Regressziós teszt: korábban a rejoin handler nem állította vissza az
    online tracking-et, ezért a visszacsatlakozó játékos offline-nak látszott
    a barátai számára a játék teljes hátralévő ideje alatt.
    """
    from server import state as st

    _, u1_id = create_user('rejoin_onl@test.com', 'RejoinOnl', 'pwd')
    _, u2_id = create_user('rejoin_p2@test.com', 'RejoinP2', 'pwd')

    c1 = create_registered_client(app, socketio_app, 'RejoinOnl', u1_id)
    c2 = create_registered_client(app, socketio_app, 'RejoinP2', u2_id)

    # c1 szobát hoz létre, elkéri a reconnect tokent a room_joined eventből
    c1.emit('create_room', {'name': 'RejoinRoom', 'max_players': 2})
    received = c1.get_received()
    token = next(r for r in received if r['name'] == 'room_joined')['args'][0]['reconnect_token']
    code = next(r for r in received if r['name'] == 'room_code')['args'][0]['code']

    c2.emit('join_room', {'code': code})
    c2.get_received()
    c1.get_received()

    c1.emit('start_game')
    c1.get_received()
    c2.get_received()

    assert st.is_user_online(u1_id), "Játék elején online kell legyen"

    # c1 lecsatlakozik aktív játék közben → grace period indul,
    # de remove_online_user(sid) azonnal meghívódik (a fix után)
    c1.disconnect()
    assert not st.is_user_online(u1_id), "Disconnect után offline kell legyen"

    # c1 visszacsatlakozik tokennel (új socket kapcsolat)
    c1_new = socketio_app.test_client(app)
    c1_new.emit('rejoin_room', {'token': token})
    received_new = c1_new.get_received()

    rejoin_ok = any(r['name'] == 'room_joined' for r in received_new)
    assert rejoin_ok, "A rejoin_room eseménynek room_joined választ kell adnia"

    assert st.is_user_online(u1_id), (
        "Visszacsatlakozás után ismét online-nak kell látszani; "
        "a rejoin_room handler-nek vissza kell venni a felhasználót _online_users-be"
    )

    c1_new.disconnect()
    c2.disconnect()


def test_friend_presence_change_emitted_on_login(app, socketio_app, temp_db):
    _, u1_id = create_user('pres_login_1@test.com', 'PresLogin1', 'pwd')
    _, u2_id = create_user('pres_login_2@test.com', 'PresLogin2', 'pwd')
    send_friend_request(u1_id, u2_id)
    accept_friend_request(u2_id, u1_id)

    observer = create_registered_client(app, socketio_app, 'PresLogin1', u1_id)
    observer.get_received()

    subject = create_registered_client(app, socketio_app, 'PresLogin2', u2_id)
    events = observer.get_received()

    assert any(
        e['name'] == 'friend_presence_changed' and
        e['args'][0]['friend_id'] == u2_id and
        e['args'][0]['online'] is True
        for e in events
    )

    subject.disconnect()
    observer.disconnect()


def test_friend_presence_change_emitted_on_logout(app, socketio_app, temp_db):
    _, u1_id = create_user('pres_logout_1@test.com', 'PresLogout1', 'pwd')
    _, u2_id = create_user('pres_logout_2@test.com', 'PresLogout2', 'pwd')
    send_friend_request(u1_id, u2_id)
    accept_friend_request(u2_id, u1_id)

    observer = create_registered_client(app, socketio_app, 'PresLogout1', u1_id)
    subject = create_registered_client(app, socketio_app, 'PresLogout2', u2_id)
    observer.get_received()
    subject.get_received()

    subject.disconnect()
    events = observer.get_received()

    assert any(
        e['name'] == 'friend_presence_changed' and
        e['args'][0]['friend_id'] == u2_id and
        e['args'][0]['online'] is False
        for e in events
    )

    observer.disconnect()
