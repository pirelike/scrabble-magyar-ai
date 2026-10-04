"""Admin panel: kommunikáció (közlemény, karbantartási mód, push, e-mail) és a futásidejű beállítások."""
import pytest

import admin
import auth
import config
import email_service
import push_service
import server
from helpers import (
    ADMIN_EMAIL, client_with_session, guest_client, live_room, make_user, registered_client,
)

REASON = 'Teszt indoklás'


@pytest.fixture(autouse=True)
def no_background(monkeypatch):
    monkeypatch.setattr(server.socketio, 'start_background_task', lambda fn, *a, **k: None)


def _audit(action):
    return admin.query_audit(action=action, limit=50)['items']


def _public(client=None):
    client = client or client_with_session(None)
    return client.get('/api/announcements').get_json()['items']


class TestAnnouncements:
    def test_create_and_show_publicly(self, api):
        data = {'text_hu': 'Szombaton frissítünk.', 'text_en': 'Update on Saturday.', 'kind': 'warning'}
        new_id = api.post('/api/admin/announcements', data).get_json()['id']
        items = _public()
        assert [(i['id'], i['kind'], i['text_hu'], i['text_en']) for i in items] == [
            (new_id, 'warning', 'Szombaton frissítünk.', 'Update on Saturday.')]
        assert api.get('/api/admin/announcements').get_json()['items'][0]['status'] == 'active'
        row = _audit('comm.announcement_create')[0]
        assert row['details']['text_hu'] == 'Szombaton frissítünk.' and row['details']['announcement_id'] == new_id

    def test_the_english_text_falls_back_to_hungarian(self, api):
        api.post('/api/admin/announcements', {'text_hu': 'Csak magyarul.'})
        assert _public()[0]['text_en'] == 'Csak magyarul.'

    def test_the_validity_window(self, api):
        api.post('/api/admin/announcements', {'text_hu': 'Jövőbeli', 'starts_at': '2999-01-01'})
        api.post('/api/admin/announcements', {'text_hu': 'Lejárt', 'ends_at': '2000-01-01'})
        api.post('/api/admin/announcements', {'text_hu': 'Most érvényes', 'starts_at': '2000-01-01',
                                              'ends_at': '2999-01-01'})
        assert [i['text_hu'] for i in _public()] == ['Most érvényes']
        statuses = {i['text_hu']: i['status'] for i in api.get('/api/admin/announcements').get_json()['items']}
        assert statuses == {'Jövőbeli': 'scheduled', 'Lejárt': 'expired', 'Most érvényes': 'active'}

    def test_audience(self, api):
        api.post('/api/admin/announcements', {'text_hu': 'Mindenkinek', 'audience': 'all'})
        api.post('/api/admin/announcements', {'text_hu': 'Bejelentkezetteknek', 'audience': 'registered'})
        api.post('/api/admin/announcements', {'text_hu': 'Vendégeknek', 'audience': 'guests'})
        _, token = make_user('a@example.com', 'Anna')
        assert [i['text_hu'] for i in _public()] == ['Mindenkinek', 'Vendégeknek']
        assert [i['text_hu'] for i in _public(client_with_session(token))] == ['Mindenkinek', 'Bejelentkezetteknek']

    def test_edit_and_revoke(self, api):
        new_id = api.post('/api/admin/announcements', {'text_hu': 'Régi'}).get_json()['id']
        assert api.patch(f'/api/admin/announcements/{new_id}', {'text_hu': 'Új', 'kind': 'info'}).status_code == 200
        assert _public()[0]['text_hu'] == 'Új'
        row = _audit('comm.announcement_update')[0]['details']
        assert row['before']['text_hu'] == 'Régi' and row['after']['text_hu'] == 'Új'
        assert api.delete(f'/api/admin/announcements/{new_id}').status_code == 200
        assert _public() == []
        assert api.delete(f'/api/admin/announcements/{new_id}').status_code == 409
        assert api.patch(f'/api/admin/announcements/{new_id}', {'text_hu': 'Vissza', 'active': True}).status_code == 200
        assert [i['text_hu'] for i in _public()] == ['Vissza']
        assert api.delete('/api/admin/announcements/999').status_code == 404

    @pytest.mark.parametrize('data', [
        {}, {'text_hu': '  '}, {'text_hu': 'x' * 501}, {'text_hu': 'ok', 'kind': 'riadó'},
        {'text_hu': 'ok', 'audience': 'senki'}, {'text_hu': 'ok', 'starts_at': 'holnap'},
        {'text_hu': 'ok', 'starts_at': '2999-01-02', 'ends_at': '2999-01-01'},
    ])
    def test_validation(self, api, data):
        assert api.post('/api/admin/announcements', data).status_code == 400
        assert api.get('/api/admin/announcements').get_json()['items'] == []

    def test_live_event(self, api):
        client = guest_client('Anna')
        api.post('/api/admin/announcements', {'text_hu': 'Élő', 'audience': 'all'})
        events = [e['args'][0] for e in client.get_received() if e['name'] == 'announcement']
        assert [i['text_hu'] for i in events[0]['guests']] == ['Élő'] and events[0]['registered'][0]['text_hu'] == 'Élő'

    def test_the_endpoint_is_public_and_rate_limited(self, api):
        assert client_with_session(None).get('/api/announcements').status_code == 200
        server._ip_rate_limits.clear()
        statuses = [client_with_session(None).get('/api/announcements').status_code for _ in range(61)]
        assert statuses[:60] == [200] * 60 and statuses[60] == 429


class TestMaintenance:
    def test_blocks_new_games_but_not_the_running_ones(self, api):
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela])
        assert api.post('/api/admin/maintenance', {'enabled': True, 'message_hu': 'Karbantartás!',
                                                   'message_en': 'Maintenance!'}).status_code == 200
        cili = guest_client('Cili')
        cili.emit('create_room', {'name': 'Új'})
        errors = [e['args'][0]['message'] for e in cili.get_received() if e['name'] == 'error']
        assert errors == ['A szerver karbantartás alatt van, új játék most nem indítható.']
        assert len(server.state.rooms) == 1
        anna.emit('pass_turn')                                          # a futó játék folytatódik
        assert room.game.turn_number == 1

    def test_other_ways_to_start_a_game_are_blocked_too(self, api):
        a_id, _ = make_user('a@example.com', 'Anna')
        client = registered_client(a_id, 'Anna')
        api.post('/api/admin/maintenance', {'enabled': True, 'message_hu': 'Karbantartás!'})
        for event, payload in (('start_daily', None), ('create_async_game', {'friend_ids': [1]}),
                               ('restore_game', {'game_id': 1})):
            client.emit(event, payload) if payload is not None else client.emit(event)
            errors = [e['args'][0]['message'] for e in client.get_received() if e['name'] == 'error']
            assert errors == ['A szerver karbantartás alatt van, új játék most nem indítható.'], event

    def test_the_admin_is_exempt(self, api):
        api.post('/api/admin/maintenance', {'enabled': True, 'message_hu': 'Karbantartás!'})
        boss = registered_client(api.user_id, 'Főnök')
        boss.emit('create_room', {'name': 'Admin szoba'})
        assert [e['name'] for e in boss.get_received() if e['name'] == 'room_joined']

    def test_banner_with_a_countdown(self, api):
        state = api.post('/api/admin/maintenance', {'enabled': True, 'message_hu': 'Újraindulás.',
                                                    'message_en': 'Restarting.', 'until': '2999-01-01 10:00'}).get_json()
        assert state['enabled'] and state['active']
        banner = _public()[0]
        assert banner['id'] == 'maintenance' and banner['kind'] == 'maintenance'
        assert banner['text_en'] == 'Restarting.' and banner['ends_at'] == '2999-01-01T10:00:00Z'

    def test_ends_automatically(self, api):
        api.post('/api/admin/maintenance', {'enabled': True, 'message_hu': 'Hamarosan vége', 'until': '2999-01-01'})
        import settings
        value = settings.get(settings.MAINTENANCE_KEY)
        value['until'] = '2000-01-01 00:00:00'
        with auth.transaction() as conn:
            settings.store(conn, settings.MAINTENANCE_KEY, value, api.user_id)
        settings.invalidate()
        assert settings.maintenance() is None and _public() == []
        cili = guest_client('Cili')
        cili.emit('create_room', {'name': 'Új'})
        assert [e['name'] for e in cili.get_received() if e['name'] == 'room_joined']

    def test_switch_off(self, api):
        api.post('/api/admin/maintenance', {'enabled': True, 'message_hu': 'Karbantartás!'})
        assert api.post('/api/admin/maintenance', {'enabled': False}).get_json()['active'] is False
        assert _public() == []
        rows = _audit('comm.maintenance')
        assert [r['details']['after']['enabled'] for r in rows] == [False, True]

    @pytest.mark.parametrize('data', [{}, {'enabled': 'igen'}, {'enabled': True},
                                      {'enabled': True, 'message_hu': 'ok', 'until': '2000-01-01'}])
    def test_validation(self, api, data):
        assert api.post('/api/admin/maintenance', data).status_code == 400


class TestPush:
    @pytest.fixture
    def devices(self, monkeypatch):
        sent = []
        monkeypatch.setattr(push_service, '_send_one', lambda sub, payload: sent.append((sub['endpoint'], payload)) or 'ok')
        a, _ = make_user('a@example.com', 'Anna')
        b, _ = make_user('b@example.com', 'Béla')
        make_user('c@example.com', 'Cili')                       # nincs feliratkozott eszköze
        auth.save_push_subscription(a, 'https://push.example.org/a-device-1', 'p' * 30, 'a' * 12)
        auth.save_push_subscription(a, 'https://push.example.org/a-device-2', 'p' * 30, 'a' * 12)
        auth.save_push_subscription(b, 'https://push.example.org/b-device-1', 'p' * 30, 'a' * 12)
        return sent, a, b

    def test_preview(self, api, devices):
        data = api.post('/api/admin/push/preview', {'target': 'group', 'group': 'all', 'title': 'Cím',
                                                    'body': 'Szöveg'}).get_json()
        assert data['recipients'] == 2 and data['devices'] == 3 and sorted(data['sample']) == ['Anna', 'Béla']

    def test_send_to_a_user(self, api, devices):
        sent, a, b = devices
        data = api.post('/api/admin/push', {'target': 'user', 'user_id': a, 'title': 'Cím', 'body': 'Szöveg'}).get_json()
        assert (data['recipients'], data['sent'], data['failed'], data['removed']) == (1, 2, 0, 0)
        assert {e for e, _ in sent} == {'https://push.example.org/a-device-1', 'https://push.example.org/a-device-2'}
        assert sent[0][1]['title'] == 'Cím' and sent[0][1]['body'] == 'Szöveg'
        row = _audit('comm.push')[0]['details']
        assert row == {'title': 'Cím', 'body': 'Szöveg', 'recipients': 1}

    def test_group_send_needs_the_confirmation(self, api, devices):
        body = {'target': 'group', 'group': 'all', 'title': 'Cím', 'body': 'Szöveg'}
        response = api.post('/api/admin/push', body)
        assert response.status_code == 400 and response.get_json()['needs_confirm'] is True
        assert not devices[0]
        assert api.post('/api/admin/push', {**body, 'confirm': True}).get_json()['sent'] == 3

    def test_the_result_counts_failures_and_removed_subscriptions(self, api, devices, monkeypatch):
        sent, a, b = devices
        outcomes = iter(['ok', 'gone', 'error'])
        monkeypatch.setattr(push_service, '_send_one', lambda sub, payload: next(outcomes))
        data = api.post('/api/admin/push', {'target': 'group', 'group': 'all', 'title': 'T', 'body': 'B',
                                            'confirm': True}).get_json()
        assert (data['sent'], data['removed'], data['failed']) == (1, 1, 1)
        assert auth.count_push_subscriptions(a) + auth.count_push_subscriptions(b) == 2

    def test_nobody_to_send_to_and_validation(self, api, devices):
        c = auth.get_user_by_email('c@example.com')['id']
        assert api.post('/api/admin/push', {'target': 'user', 'user_id': c, 'title': 'T', 'body': 'B'}).status_code == 409
        assert api.post('/api/admin/push', {'target': 'user', 'user_id': 999, 'title': 'T', 'body': 'B'}).status_code == 404
        assert api.post('/api/admin/push', {'target': 'senki', 'title': 'T', 'body': 'B'}).status_code == 400
        assert api.post('/api/admin/push', {'target': 'group', 'group': 'all', 'title': '', 'body': 'B'}).status_code == 400

    def test_unavailable_push(self, api, devices, monkeypatch):
        monkeypatch.setattr(push_service, 'is_available', lambda: False)
        assert api.post('/api/admin/push', {'target': 'user', 'user_id': devices[1], 'title': 'T', 'body': 'B'}
                        ).status_code == 503

    def test_the_online_group(self, api, devices):
        sent, a, b = devices
        registered_client(a, 'Anna')
        data = api.post('/api/admin/push/preview', {'target': 'group', 'group': 'online', 'title': 'T', 'body': 'B'}
                        ).get_json()
        assert data['sample'] == ['Anna']

    def test_banned_users_are_not_recipients(self, api, devices):
        sent, a, b = devices
        api.sudo()
        api.post(f'/api/admin/users/{a}/ban', {'until': '1d', 'reason': REASON})
        data = api.post('/api/admin/push/preview', {'target': 'group', 'group': 'all', 'title': 'T', 'body': 'B'}
                        ).get_json()
        assert data['sample'] == ['Béla']


class TestEmail:
    @pytest.fixture
    def mails(self, monkeypatch):
        sent = []
        monkeypatch.setattr(email_service, 'send_plain_email',
                            lambda to, subject, body, wait=False: (sent.append((to, subject, body)) or (True, None)))
        monkeypatch.setattr(config, 'SMTP_CONFIGURED', True)
        return sent

    def test_to_a_user(self, api, mails):
        a, _ = make_user('a@example.com', 'Anna')
        data = api.post('/api/admin/email', {'user_id': a, 'subject': 'Üdv', 'body': 'Szöveg.'}).get_json()
        assert data == {'success': True, 'sent': True, 'console': False}
        to, subject, body = mails[0]
        assert to == 'a@example.com' and subject == 'Üdv' and body.startswith('Szia Anna!') and 'Szöveg.' in body
        assert _audit('comm.email')[0]['details'] == {'subject': 'Üdv', 'body': 'Szöveg.'}

    def test_without_smtp_it_goes_to_the_console(self, api, monkeypatch, capsys):
        monkeypatch.setattr(config, 'SMTP_CONFIGURED', False)
        a, _ = make_user('a@example.com', 'Anna')
        data = api.post('/api/admin/email', {'user_id': a, 'subject': 'Üdv', 'body': 'Szöveg.'}).get_json()
        assert data['sent'] is False and data['console'] is True
        assert 'a@example.com' in capsys.readouterr().out

    @pytest.mark.parametrize('body', [{'subject': '', 'body': 'x'}, {'subject': 'x', 'body': ''},
                                      {'subject': 'két\nsor', 'body': 'x'}, {'subject': 'x', 'body': 'y' * 5001}])
    def test_validation(self, api, mails, body):
        a, _ = make_user('a@example.com', 'Anna')
        assert api.post('/api/admin/email', {'user_id': a, **body}).status_code == 400
        assert not mails

    def test_unknown_or_deleted_user(self, api, mails):
        assert api.post('/api/admin/email', {'user_id': 999, 'subject': 'x', 'body': 'y'}).status_code == 404
        assert api.post('/api/admin/email', {'subject': 'x', 'body': 'y'}).status_code == 400

    def test_bulk_needs_sudo_confirmation_and_respects_the_rate_limit(self, api, mails, monkeypatch):
        import admin_comm
        monkeypatch.setitem(admin_comm._bulk_state, 'last', 0.0)
        make_user('a@example.com', 'Anna')
        make_user('b@example.com', 'Béla')
        body = {'group': 'all', 'subject': 'Hírlevél', 'body': 'Szöveg', 'reason': REASON}
        assert api.post('/api/admin/email/bulk', body).status_code == 401
        api.sudo()
        response = api.post('/api/admin/email/bulk', body)
        assert response.status_code == 400 and response.get_json()['needs_confirm'] is True and not mails
        data = api.post('/api/admin/email/bulk', {**body, 'confirm': True}).get_json()
        assert data['recipients'] == 3 and data['sent'] == 3                      # az admin is felhasználó
        assert {m[0] for m in mails} == {ADMIN_EMAIL, 'a@example.com', 'b@example.com'}
        assert api.post('/api/admin/email/bulk', {**body, 'confirm': True}).status_code == 429
        assert _audit('comm.email_bulk')[0]['details']['recipients'] == 3

    def test_bulk_preview(self, api):
        make_user('a@example.com', 'Anna')
        assert api.get('/api/admin/email/preview?group=all').get_json()['recipients'] == 2
        assert api.get('/api/admin/email/preview?group=online').status_code == 400


class TestSettings:
    def _patch(self, api, changes, reason=REASON):
        return api.patch('/api/admin/settings', {'changes': changes, 'reason': reason})

    def test_listing(self, api):
        data = api.get('/api/admin/settings').get_json()
        items = {i['key']: i for i in data['items']}
        assert items['grace_disconnect'] == {**items['grace_disconnect'], 'value': 120, 'default': 120,
                                             'overridden': False, 'min': 15, 'max': 1800, 'type': 'int',
                                             'changed_by': None, 'changed_at': None, 'changed_by_name': None,
                                             'group': 'timing'}
        assert items['room_creation']['choices'] == ['everyone', 'registered', 'none']
        assert 'maintenance' not in items and set(data['groups']) >= {'access', 'bots', 'timing'}

    def test_change_reset_and_the_audit_trail(self, api):
        assert self._patch(api, {'grace_disconnect': 60, 'chat_max_length': 100}).status_code == 200
        items = {i['key']: i for i in api.get('/api/admin/settings').get_json()['items']}
        assert items['grace_disconnect']['value'] == 60 and items['grace_disconnect']['overridden'] is True
        assert items['grace_disconnect']['default'] == 120 and items['grace_disconnect']['changed_by_name'] == 'Főnök'
        assert items['grace_disconnect']['changed_at']
        row = _audit('settings.update')[0]['details']
        assert row['before'] == {'grace_disconnect': 120, 'chat_max_length': 200}
        assert row['after'] == {'grace_disconnect': 60, 'chat_max_length': 100}
        assert self._patch(api, {'grace_disconnect': None}).status_code == 200
        item = next(i for i in api.get('/api/admin/settings').get_json()['items'] if i['key'] == 'grace_disconnect')
        assert item['value'] == 120 and item['overridden'] is False and item['changed_by'] is None

    @pytest.mark.parametrize('changes', [
        {'grace_disconnect': 5}, {'grace_disconnect': '60'}, {'grace_disconnect': True}, {'registration_open': 1},
        {'room_creation': 'bárki'}, {'default_hint_limit': 4}, {'bot_think_multiplier': 9.0}, {'nincs_ilyen': 1},
        {'maintenance': {'enabled': True}}, {'rate_limits_http': {'login': [0, 5]}}, {'rate_limits_http': 'sok'},
        {'rate_limits_socket': {'create_room': [3]}},
    ])
    def test_validation_is_all_or_nothing(self, api, changes):
        changes = {**changes, 'chat_max_length': 100}
        assert self._patch(api, changes).status_code == 400
        assert api.get('/api/admin/settings').get_json()['items'][0]
        item = next(i for i in api.get('/api/admin/settings').get_json()['items'] if i['key'] == 'chat_max_length')
        assert item['overridden'] is False

    def test_a_reason_and_a_change_are_required(self, api):
        assert api.patch('/api/admin/settings', {'changes': {'chat_max_length': 100}}).status_code == 400
        assert self._patch(api, {}).status_code == 400
        assert api.patch('/api/admin/settings', {'reason': REASON}).status_code == 400

    # --- hatás futás közben ---

    def test_registration_can_be_closed(self, api, monkeypatch):
        monkeypatch.setattr('routes.send_verification_email', lambda e, c: None)
        self._patch(api, {'registration_open': False})
        anon = client_with_session(None)
        for path, body in (('/api/auth/request-code', {'email': 'uj@example.com'}),
                           ('/api/auth/register', {'email': 'uj@example.com', 'password': 'secret12',
                                                   'display_name': 'Új'})):
            response = anon.post(path, json=body)
            assert response.status_code == 403 and response.get_json()['message'] == \
                'A regisztráció jelenleg le van zárva.'
        self._patch(api, {'registration_open': None})
        assert anon.post('/api/auth/request-code', json={'email': 'uj@example.com'}).status_code == 200

    def test_guest_mode_can_be_turned_off(self, api):
        self._patch(api, {'guest_allowed': False})
        client = server.socketio.test_client(server.app)
        client.emit('set_name', {'name': 'Vendég', 'is_guest': True})
        assert [e['args'][0]['message'] for e in client.get_received() if e['name'] == 'error'] == \
            ['A vendég mód jelenleg ki van kapcsolva.']
        assert server.state.player_names == {}
        a, _ = make_user('a@example.com', 'Anna')
        assert registered_client(a, 'Anna')                      # a regisztrált belépés nem érintett
        assert list(server.state.player_names.values()) == ['Anna']

    def test_room_creation_policy(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        self._patch(api, {'room_creation': 'registered'})
        guest = guest_client('Vendég')
        guest.emit('create_room', {'name': 'X'})
        assert [e['args'][0]['message'] for e in guest.get_received() if e['name'] == 'error'] == \
            ['Szobát csak regisztrált felhasználó hozhat létre.']
        member = registered_client(a, 'Anna')
        member.emit('create_room', {'name': 'X'})
        assert [e['name'] for e in member.get_received() if e['name'] == 'room_joined']
        self._patch(api, {'room_creation': 'none'})
        other, _ = make_user('b@example.com', 'Béla')
        blocked = registered_client(other, 'Béla')
        blocked.emit('create_room', {'name': 'Y'})
        assert [e['args'][0]['message'] for e in blocked.get_received() if e['name'] == 'error'] == \
            ['Új szoba létrehozása jelenleg le van tiltva.']

    def test_bot_settings(self, api):
        self._patch(api, {'max_bots': 1})
        client = guest_client('Anna')
        client.emit('create_room', {'name': 'X', 'max_players': 4, 'ai_players': [3, 5, 7]})
        client.get_received()
        assert sum(p.is_bot for p in list(server.state.rooms.values())[-1].game.players) == 1
        client.emit('leave_room')
        self._patch(api, {'bots_enabled': False})
        client.emit('create_room', {'name': 'Y', 'max_players': 4, 'ai_players': [3]})
        client.get_received()
        assert sum(p.is_bot for p in list(server.state.rooms.values())[-1].game.players) == 0

    def test_default_hint_limit(self, api):
        self._patch(api, {'default_hint_limit': 10})
        client = guest_client('Anna')
        client.emit('create_room', {'name': 'X'})
        joined = next(e['args'][0] for e in client.get_received() if e['name'] == 'room_joined')
        assert joined['hint_limit'] == 10

    def test_feature_switches(self, api):
        a, token = make_user('a@example.com', 'Anna')
        user = client_with_session(token)
        self._patch(api, {'feature_practice': False, 'feature_word_review': False, 'feature_daily': False,
                          'feature_async': False})
        assert user.get('/api/practice/quiz').status_code == 503
        assert user.get('/api/practice/short-words?length=2').status_code == 503
        assert user.get('/api/practice/word-review').status_code == 503
        assert user.get('/api/daily').status_code == 503
        assert user.get('/api/async/games').status_code == 503
        client = registered_client(a, 'Anna')
        client.emit('start_daily')
        assert [e['args'][0]['message'] for e in client.get_received() if e['name'] == 'error'] == \
            ['Ez a funkció jelenleg ki van kapcsolva.']
        self._patch(api, {'feature_practice': None, 'feature_word_review': None})
        assert user.get('/api/practice/quiz').status_code == 200
        assert user.get('/api/practice/word-review').status_code == 200

    def test_grace_periods(self, api, monkeypatch):
        timers, slept = [], []
        monkeypatch.setattr(server.socketio, 'start_background_task', lambda fn, *a, **k: timers.append(fn))
        monkeypatch.setattr(server.time, 'sleep', lambda seconds: slept.append(seconds))
        self._patch(api, {'grace_disconnect': 45, 'grace_waiting_owner': 90})
        owner, friend = guest_client('Owner'), guest_client('Friend')
        owner.emit('create_room', {'name': 'X'})
        owner.get_received()
        code = list(server.state.rooms.values())[-1].join_code
        friend.emit('join_room', {'code': code})
        friend.disconnect()
        owner.disconnect()
        for timer in timers:
            timer()
        assert slept == [45, 90]

    def test_chat_length_limit(self, api):
        self._patch(api, {'chat_max_length': 20})
        anna, bela = guest_client('Anna'), guest_client('Béla')
        live_room(anna, [bela])
        bela.get_received()
        anna.emit('send_chat', {'message': 'x' * 21})
        anna.emit('send_chat', {'message': 'x' * 20})
        assert [len(e['args'][0]['message']) for e in bela.get_received() if e['name'] == 'chat_message'] == [20]

    def test_chat_rate_limit(self, api):
        self._patch(api, {'chat_rate_count': 2, 'chat_rate_window': 60})
        anna, bela = guest_client('Anna'), guest_client('Béla')
        live_room(anna, [bela])
        bela.get_received()
        for text in ('egy', 'kettő', 'három'):                         # a harmadik már a sebességkorlát alatt
            anna.emit('send_chat', {'message': text})
        assert [e['args'][0]['message'] for e in bela.get_received() if e['name'] == 'chat_message'] == ['egy', 'kettő']

    def test_rate_limit_overrides(self, api):
        self._patch(api, {'rate_limits_http': {'announcements': [2, 60]}})
        anon = client_with_session(None)
        server._ip_rate_limits.clear()
        assert [anon.get('/api/announcements').status_code for _ in range(3)] == [200, 200, 429]
        self._patch(api, {'rate_limits_http': None})
        server._ip_rate_limits.clear()
        assert [anon.get('/api/announcements').status_code for _ in range(3)] == [200, 200, 200]

    def test_spectator_limit(self, api):
        self._patch(api, {'max_spectators': 1})
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela])
        watchers = [guest_client(f'Néző{i}') for i in range(2)]
        for watcher in watchers:
            watcher.emit('spectate_room', {'room_id': room.id})
        assert len(room.spectators) == 1
        assert [e['args'][0]['message'] for e in watchers[1].get_received() if e['name'] == 'error'] == \
            ['A szobának nem fér több megfigyelője.']

    def test_bot_think_time(self, api, monkeypatch):
        delays = []
        monkeypatch.setattr(server.socketio, 'start_background_task', lambda fn, *a, **k: delays.append(fn))
        monkeypatch.setattr(server.random, 'uniform', lambda lo, hi: 2.0)
        slept = []
        monkeypatch.setattr(server.socketio, 'sleep', lambda seconds: slept.append(seconds))
        self._patch(api, {'bot_think_multiplier': 0.5})
        client = guest_client('Anna')
        room = live_room(client, ai_players=[3])
        room.game.pass_turn(room.game.current_player().id)
        monkeypatch.setattr(server, '_play_bot_turn', lambda *a: None)
        server._schedule_bot_turn(room.id)
        delays[-1]()
        assert slept[-1] == 1.0
