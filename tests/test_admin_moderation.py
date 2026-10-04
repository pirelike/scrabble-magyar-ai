"""Admin panel: moderáció — tiltott szavak, chat napló, bejelentések (a játékosoknál is), nevek átnézése."""
import pytest

import admin
import admin_mod
import push_service
import server
from helpers import (
    client_with_session, guest_client, live_room, make_user, registered_client,
)

REASON = 'Teszt indoklás'


@pytest.fixture(autouse=True)
def no_background(monkeypatch):
    monkeypatch.setattr(server.socketio, 'start_background_task', lambda fn, *a, **k: None)


def _audit(action):
    return admin.query_audit(action=action, limit=50)['items']


def _chat_room(names=('Anna', 'Béla')):
    clients = [guest_client(n) for n in names]
    room = live_room(clients[0], clients[1:])
    for c in clients:
        c.get_received()
    return clients, room


def _messages(client):
    return [e['args'][0]['message'] for e in client.get_received() if e['name'] == 'chat_message']


class TestBannedWords:
    def test_management(self, api):
        assert api.post('/api/admin/moderation/words', {'word': 'Hülye', 'reason': REASON}).get_json()['word'] == 'hulye'
        assert [w['word'] for w in api.get('/api/admin/moderation/words').get_json()['items']] == ['hulye']
        assert api.post('/api/admin/moderation/words', {'word': 'HÜLYE', 'reason': REASON}).status_code == 409
        assert api.post('/api/admin/moderation/words', {'word': 'a', 'reason': REASON}).status_code == 400
        assert api.post('/api/admin/moderation/words', {'word': 'x'}).status_code == 400
        assert api.delete('/api/admin/moderation/words', {'word': 'hülye', 'reason': REASON}).status_code == 200
        assert api.delete('/api/admin/moderation/words', {'word': 'hülye', 'reason': REASON}).status_code == 404
        assert [r['action'] for r in _audit('mod.word')] == ['mod.word_remove', 'mod.word_add']

    def test_matching_ignores_case_and_accents(self, api):
        api.post('/api/admin/moderation/words', {'word': 'hulye', 'reason': REASON})
        for text in ('HÜLYE vagy', 'te hülye!', 'hulyeség'):
            assert admin_mod.contains_banned(text), text
        assert not admin_mod.contains_banned('hülő')
        assert admin_mod.mask_banned('Te HÜLYE vagy, hülye!') == ('Te ***** vagy, *****!', True)
        assert admin_mod.mask_banned('rendben') == ('rendben', False)

    def test_chat_messages_are_masked_by_default(self, api):
        api.post('/api/admin/moderation/words', {'word': 'hulye', 'reason': REASON})
        (anna, bela), room = _chat_room()
        anna.emit('send_chat', {'message': 'Te hülye vagy'})
        assert _messages(bela) == ['Te ***** vagy']
        assert room.chat_messages[-1]['message'] == 'Te ***** vagy'

    def test_chat_messages_can_be_dropped_instead(self, api):
        api.post('/api/admin/moderation/words', {'word': 'hulye', 'reason': REASON})
        api.patch('/api/admin/settings', {'changes': {'banned_word_action': 'drop'}, 'reason': REASON})
        (anna, bela), room = _chat_room()
        anna.emit('send_chat', {'message': 'Te hülye vagy'})
        anna.emit('send_chat', {'message': 'Szia'})
        assert _messages(bela) == ['Szia']

    def test_names_and_room_names_are_rejected(self, api, monkeypatch):
        api.post('/api/admin/moderation/words', {'word': 'hulye', 'reason': REASON})
        guest = server.socketio.test_client(server.app)
        guest.emit('set_name', {'name': 'Hülye Hugó', 'is_guest': True})
        received = guest.get_received()
        assert [e['args'][0]['message'] for e in received if e['name'] == 'error'] == ['Ez a név nem engedélyezett.']
        assert list(server.state.player_names.values()) == ['Névtelen']
        guest.emit('create_room', {'name': 'Hülye szoba'})
        assert [e['args'][0]['message'] for e in guest.get_received() if e['name'] == 'error'] == \
            ['Ez a szobanév nem engedélyezett.']
        assert server.state.rooms == {}
        from helpers import verify_email
        verify_email('uj@example.com')
        response = client_with_session(None).post('/api/auth/register', json={
            'email': 'uj@example.com', 'password': 'secret12', 'display_name': 'Hülye'})
        assert response.status_code == 400 and response.get_json()['message'] == 'Ez a név nem engedélyezett.'

    def test_a_flagged_message_alerts_the_admin_room(self, api):
        from socket_auth import create_socket_token
        api.post('/api/admin/moderation/words', {'word': 'hulye', 'reason': REASON})
        watcher = server.socketio.test_client(server.app)
        watcher.emit('admin_subscribe', {'auth_token': create_socket_token(server.app.config['SECRET_KEY'], api.user_id)})
        watcher.get_received()
        (anna, bela), room = _chat_room()
        anna.emit('send_chat', {'message': 'hülye'})
        alerts = [e['args'][0] for e in watcher.get_received() if e['name'] == 'admin_alert']
        assert alerts == [{'code': 'banned_word', 'room': room.name, 'name': 'Anna'}]


class TestLiveChat:
    def test_every_room_in_one_place(self, api):
        (anna, bela), room1 = _chat_room()
        (cili, dani), room2 = _chat_room(('Cili', 'Dani'))
        anna.emit('send_chat', {'message': 'első szoba'})
        cili.emit('send_chat', {'message': 'második szoba'})
        api.post(f'/api/admin/rooms/{room1.id}/action', {'action': 'message', 'message': 'Rendszerüzenet'})
        data = api.get('/api/admin/moderation/chat').get_json()
        assert [(m['name'], m['message']) for m in data['items']] == [
            ('Anna', 'első szoba'), ('Cili', 'második szoba'), ('Rendszer', 'Rendszerüzenet')]
        assert data['items'][2]['system'] is True and data['items'][0]['room_name'] == room1.name
        assert [m['message'] for m in api.get('/api/admin/moderation/chat?q=második').get_json()['items']] == ['második szoba']
        assert _audit('view.chat')[0]['details']['source'] == 'live'

    def test_banned_words_are_highlighted(self, api):
        api.post('/api/admin/moderation/words', {'word': 'hulye', 'reason': REASON})
        api.patch('/api/admin/settings', {'changes': {'banned_word_action': 'drop'}, 'reason': REASON})
        (anna, bela), room = _chat_room()
        room.add_chat_message('Anna', 'hülye ez')                  # (egy korábbi, szűrés előtti üzenet)
        items = api.get('/api/admin/moderation/chat').get_json()['items']
        assert items[0]['flagged'] is True


class TestPersistentChatLog:
    def test_off_by_default(self, api):
        (anna, bela), room = _chat_room()
        anna.emit('send_chat', {'message': 'Szia'})
        assert api.get('/api/admin/moderation/chat?source=log').get_json()['total'] == 0

    def test_logging_and_searching(self, api):
        api.patch('/api/admin/settings', {'changes': {'chat_log_enabled': True}, 'reason': REASON})
        a, _ = make_user('a@example.com', 'Anna')
        anna = registered_client(a, 'Anna')
        bela = guest_client('Béla')
        room = live_room(anna, [bela])
        anna.emit('send_chat', {'message': 'Szia Béla'})
        bela.emit('send_chat', {'message': 'Szia Anna'})
        data = api.get('/api/admin/moderation/chat?source=log').get_json()
        assert [(m['name'], m['user_id'], m['message']) for m in data['items']] == [
            ('Béla', None, 'Szia Anna'), ('Anna', a, 'Szia Béla')]
        assert data['items'][0]['room_id'] == room.id
        ids = lambda q: [m['name'] for m in api.get('/api/admin/moderation/chat?source=log&' + q).get_json()['items']]  # noqa: E731
        assert ids('q=Béla') == ['Anna'] and ids(f'user={a}') == ['Anna'] and ids('user=béla') == ['Béla']
        assert ids(f'room={room.id}') == ['Béla', 'Anna'] and ids('since=2999-01-01') == []
        assert api.get('/api/admin/moderation/chat?source=log&since=hibás').status_code == 400
        assert _audit('view.chat')

    def test_the_stored_text_is_the_filtered_one_and_csv(self, api):
        api.post('/api/admin/moderation/words', {'word': 'hulye', 'reason': REASON})
        api.patch('/api/admin/settings', {'changes': {'chat_log_enabled': True}, 'reason': REASON})
        (anna, bela), room = _chat_room()
        anna.emit('send_chat', {'message': 'hülye'})
        text = api.get('/api/admin/moderation/chat?source=log&format=csv').get_data(as_text=True)
        assert '*****' in text and 'hülye' not in text


class TestPlayerReports:
    @pytest.fixture
    def users(self):
        a, _ = make_user('a@example.com', 'Anna')
        b, _ = make_user('b@example.com', 'Béla')
        anna, bela = registered_client(a, 'Anna'), registered_client(b, 'Béla')
        room = live_room(anna, [bela])
        anna.get_received()
        bela.get_received()
        return anna, a, bela, b, room

    def _result(self, client):
        return [e['args'][0] for e in client.get_received() if e['name'] == 'report_result']

    def test_report_a_player(self, api, users, monkeypatch):
        anna, a, bela, b, room = users
        pushes = []
        monkeypatch.setattr(push_service, 'notify_background', lambda uid, kind, **p: pushes.append((uid, kind, p)))
        bela.emit('send_chat', {'message': 'sértő szöveg'})
        anna.get_received()
        anna.emit('report_content', {'kind': 'player', 'name': 'Béla', 'reason': 'Zaklat'})
        assert self._result(anna) == [{'success': True, 'message': 'Bejelentés elküldve. Köszönjük!'}]
        data = api.get('/api/admin/reports').get_json()
        assert data['total'] == 1 and data['new'] == 1
        report = data['items'][0]
        assert (report['reporter_name'], report['reported_name'], report['kind'], report['status']) == (
            'Anna', 'Béla', 'player', 'new')
        assert report['reporter_user_id'] == a and report['reported_user_id'] == b and report['reason'] == 'Zaklat'
        assert pushes == [(api.user_id, 'report', {'room': room.name})]

    def test_report_a_chat_message_with_a_snapshot(self, api, users):
        anna, a, bela, b, room = users
        bela.emit('send_chat', {'message': 'sértő szöveg'})
        anna.emit('report_content', {'kind': 'chat', 'name': 'Béla', 'message': 'sértő szöveg', 'reason': ''})
        report_id = api.get('/api/admin/reports').get_json()['items'][0]['id']
        report = api.get(f'/api/admin/reports/{report_id}').get_json()['report']
        assert report['message'] == 'sértő szöveg' and report['room_name'] == room.name
        assert report['snapshot']['chat'] == [{'name': 'Béla', 'message': 'sértő szöveg'}]
        assert {p['name'] for p in report['snapshot']['players']} == {'Anna', 'Béla'}
        assert _audit('view.report')[0]['target_id'] == str(report_id)

    @pytest.mark.parametrize('payload', [
        {'kind': 'player', 'name': 'Senki'}, {'kind': 'player', 'name': 'Anna'}, {'kind': 'rossz', 'name': 'Béla'},
        {'kind': 'chat', 'name': 'Béla', 'message': ''}, {'kind': 'chat', 'name': 'Béla', 'message': 'x' * 201},
        {'kind': 'player', 'name': 'Béla', 'reason': 'x' * 301}, {'kind': 'player'},
    ])
    def test_invalid_reports_are_rejected(self, api, users, payload):
        anna = users[0]
        anna.emit('report_content', payload)
        assert self._result(anna)[0]['success'] is False
        assert api.get('/api/admin/reports').get_json()['total'] == 0

    def test_guests_and_outsiders_cannot_report(self, api, users):
        guest = guest_client('Vendég')
        guest.emit('report_content', {'kind': 'player', 'name': 'Béla'})
        assert self._result(guest) == [{'success': False, 'message': 'Nem vagy szobában.'}]
        anna, a, bela, b, room = users
        solo_guest = guest_client('Cili')
        live_room(solo_guest, name='Másik')
        solo_guest.get_received()
        solo_guest.emit('report_content', {'kind': 'player', 'name': 'Béla'})
        assert self._result(solo_guest)[0]['success'] is False

    def test_a_guest_in_a_room_cannot_report(self, api):
        anna_client, bela = guest_client('Anna'), guest_client('Béla')
        live_room(anna_client, [bela])
        anna_client.get_received()
        anna_client.emit('report_content', {'kind': 'player', 'name': 'Béla'})
        assert self._result(anna_client) == [{'success': False,
                                              'message': 'Bejelentést csak regisztrált felhasználó tehet.'}]

    def test_rate_limit(self, api, users):
        anna = users[0]
        for _ in range(4):
            anna.emit('report_content', {'kind': 'player', 'name': 'Béla'})
        results = self._result(anna)
        assert [r['success'] for r in results] == [True, True, True, False]
        assert results[3]['message'] == 'Túl sok kérés, várj egy kicsit.'

    def test_the_admin_room_gets_a_live_event(self, api, users):
        from socket_auth import create_socket_token
        anna = users[0]
        watcher = server.socketio.test_client(server.app)
        watcher.emit('admin_subscribe', {'auth_token': create_socket_token(server.app.config['SECRET_KEY'], api.user_id)})
        watcher.get_received()
        anna.emit('report_content', {'kind': 'player', 'name': 'Béla'})
        events = [e['args'][0] for e in watcher.get_received() if e['name'] == 'admin_report']
        assert len(events) == 1 and events[0]['reported'] == 'Béla'

    def test_handling(self, api, users):
        anna = users[0]
        anna.emit('report_content', {'kind': 'player', 'name': 'Béla'})
        report_id = api.get('/api/admin/reports').get_json()['items'][0]['id']
        assert api.patch(f'/api/admin/reports/{report_id}', {'status': 'handled'}).status_code == 400
        assert api.patch(f'/api/admin/reports/{report_id}', {'status': 'nincs', 'reason': REASON}).status_code == 400
        assert api.patch(f'/api/admin/reports/{report_id}', {'status': 'handled', 'reason': 'Figyelmeztettem'}
                         ).status_code == 200
        report = api.get(f'/api/admin/reports/{report_id}').get_json()['report']
        assert report['status'] == 'handled' and report['handler_note'] == 'Figyelmeztettem'
        assert report['handled_by'] == api.user_id and report['handled_at']
        assert api.get('/api/admin/reports?status=new').get_json()['items'] == []
        assert len(api.get('/api/admin/reports?status=handled').get_json()['items']) == 1
        assert api.patch(f'/api/admin/reports/{report_id}', {'status': 'new', 'reason': REASON}).status_code == 200
        assert api.get(f'/api/admin/reports/{report_id}').get_json()['report']['handled_by'] is None
        assert api.get('/api/admin/reports?status=rossz').status_code == 400
        assert api.get('/api/admin/reports/999').status_code == 404
        assert _audit('mod.report_update')[0]['details']['before'] == {'status': 'handled'}


class TestNamesReview:
    def test_recent_names_and_renames(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        b, _ = make_user('b@example.com', 'Hülye Béla')
        api.post('/api/admin/moderation/words', {'word': 'hulye', 'reason': REASON})
        api.patch(f'/api/admin/users/{a}', {'display_name': 'Annácska', 'reason': REASON})
        items = api.get('/api/admin/moderation/names').get_json()['items']
        kinds = {(i['display_name'], i['kind']): i['flagged'] for i in items}
        assert kinds[('Hülye Béla', 'registered')] is True and kinds[('Annácska', 'registered')] is False
        assert kinds[('Annácska', 'renamed')] is False
        assert api.patch(f'/api/admin/users/{b}', {'display_name': 'Béla', 'reason': REASON}).status_code == 200
        assert [i['display_name'] for i in api.get('/api/admin/moderation/names?limit=1').get_json()['items']]
