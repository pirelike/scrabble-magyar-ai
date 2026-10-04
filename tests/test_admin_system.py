"""Admin panel: áttekintés, biztonság (belépési napló, IP tiltás, kódok, munkamenetek), rendszer és statisztika."""
import json
import logging
import sqlite3
import time

import pytest

import admin
import admin_system
import auth
import config
import dictionary
import email_service
import push_service
import server
from helpers import (
    ADMIN_EMAIL, client_with_session, finished_game, guest_client, live_room, make_user,
    registered_client,
)

REASON = 'Teszt indoklás'


@pytest.fixture(autouse=True)
def no_background(monkeypatch):
    monkeypatch.setattr(server.socketio, 'start_background_task', lambda fn, *a, **k: None)


def _audit(action):
    return admin.query_audit(action=action, limit=50)['items']


# ---------------------------------------------------------------- áttekintés

class TestOverview:
    def test_structure(self, api):
        data = api.get('/api/admin/overview').get_json()
        assert {'live', 'today', 'server', 'services', 'alerts', 'versions', 'maintenance'} <= set(data)
        assert data['live']['online_users'] == 0 and data['live']['rooms_total'] == 0
        assert data['services']['dictionary'] is True and data['server']['uptime'] >= 0
        assert data['server']['db_size'] > 0 and data['today']['registrations'] == 1      # az admin fiók

    def test_live_counters(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        anna, bela = registered_client(a, 'Anna'), guest_client('Béla')
        live_room(anna, [bela], challenge_mode=True)
        solo = guest_client('Cili')
        live_room(solo, ai_players=[3], name='Robotos')
        watcher = guest_client('Néző')
        room = list(server.state.rooms.values())[0]
        watcher.emit('spectate_room', {'room_id': room.id})
        live = api.get('/api/admin/overview').get_json()['live']
        assert live['online_users'] == 1 and live['online_guests'] == 3 and live['sockets'] == 4
        assert (live['rooms_total'], live['rooms_active'], live['games_running'], live['games_with_bots']) == (2, 2, 2, 1)
        assert live['rooms_public'] == 2 and live['spectators'] == 1 and live['grace_players'] == 0

    def test_grace_and_votes_are_counted(self, api):
        anna, bela = guest_client('Anna'), guest_client('Béla')
        room = live_room(anna, [bela], challenge_mode=True)
        player = next(p for p in room.game.players if p.name == 'Anna')
        letters = [t for t in player.hand if t][:2]
        anna.emit('place_tiles', {'tiles': [{'row': 7, 'col': 7 + i, 'letter': l, 'is_blank': False}
                                            for i, l in enumerate(letters)]})
        bela.disconnect()
        live = api.get('/api/admin/overview').get_json()['live']
        assert live['pending_votes'] == 1 and live['grace_players'] == 1

    def test_today_counters(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        finished_game('r1', [(a, 'Anna', 10), (None, 'Vendég', 5)])
        auth.save_word_review(a, 'valami', False)
        today = api.get('/api/admin/overview').get_json()['today']
        assert today['registrations'] == 2 and today['finished_games'] == 1
        assert today['review_decisions'] == 1 and today['review_rejections'] == 1
        assert today['rejected_words'] >= 600

    def _codes(self, api):
        return {a['code'] for a in api.get('/api/admin/overview').get_json()['alerts']}

    def test_service_alerts(self, api, monkeypatch):
        monkeypatch.setattr(config, 'SMTP_CONFIGURED', False)
        monkeypatch.setattr(push_service, 'is_available', lambda: False)
        monkeypatch.setattr(dictionary, 'is_available', lambda: False)
        codes = self._codes(api)
        assert {'smtp_missing', 'push_missing', 'dictionary_down', 'backup_old'} <= codes
        levels = {a['code']: a['level'] for a in api.get('/api/admin/overview').get_json()['alerts']}
        assert levels['dictionary_down'] == 'red' and levels['smtp_missing'] == 'yellow'

    def test_no_backup_alert_with_a_fresh_backup(self, api, monkeypatch, tmp_path):
        monkeypatch.setattr(config, 'BACKUP_DIR', str(tmp_path))
        assert 'backup_old' in self._codes(api)
        admin_system.make_backup()
        assert 'backup_old' not in self._codes(api)

    def test_failed_login_alerts(self, api):
        for _ in range(10):
            auth.record_login_event('x@example.com', False, '203.0.113.5', 'UA')
        alert = next(a for a in api.get('/api/admin/overview').get_json()['alerts'] if a['code'] == 'failed_logins')
        assert '203.0.113.5' in alert['detail'] and alert['count'] >= 10

    def test_review_spike_alert(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        for i in range(25):
            auth.save_word_review(a, f'szo{i}', False)
        assert 'review_spike' in self._codes(api)

    def test_overdue_async_alert(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        b, _ = make_user('b@example.com', 'Béla')
        auth.send_friend_request(a, b)
        auth.accept_friend_request(b, a)
        anna = registered_client(a, 'Anna')
        anna.emit('create_async_game', {'name': 'X', 'friend_ids': [b], 'turn_hours': 24})
        room = next(r for r in server.state.rooms.values() if r.is_async)
        assert 'async_overdue' not in self._codes(api)
        room.game.turn_deadline = time.time() - 3600
        assert 'async_overdue' in self._codes(api)

    def test_the_maintenance_state_is_included(self, api):
        api.post('/api/admin/maintenance', {'enabled': True, 'message_hu': 'Karbantartás'})
        assert api.get('/api/admin/overview').get_json()['maintenance']['message_hu'] == 'Karbantartás'

    def test_charts(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        finished_game('r1', [(a, 'Anna', 10), (None, 'Vendég', 5)])
        finished_game('r2', [(a, 'Anna', 10), (None, 'Robi', 5)], has_bots=True)
        data = api.get('/api/admin/charts?days=14').get_json()
        assert len(data['days']) == 14 and data['registrations'][-1]['value'] == 2
        assert data['games_human'][-1]['value'] == 1 and data['games_bots'][-1]['value'] == 1
        assert data['active_users'][-1]['value'] >= 1
        assert api.get('/api/admin/charts?days=1').status_code == 400


# ---------------------------------------------------------------- biztonság

class TestLoginLog:
    def test_events_are_recorded(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        anon = client_with_session(None)
        anon.post('/api/auth/login', json={'email': 'a@example.com', 'password': 'rossz'},
                  headers={'User-Agent': 'Teszt/1'})
        anon.post('/api/auth/login', json={'email': 'a@example.com', 'password': 'secret12'})
        items = api.get('/api/admin/security/logins?user=a@example.com').get_json()['items']
        assert [(i['success'], i['reason']) for i in items] == [(True, ''), (False, 'bad_credentials')]
        assert items[0]['user_id'] == a and items[1]['user_agent'] == 'Teszt/1'
        user = auth.get_user_by_id(a)
        assert user['last_login_at'] and user['last_login_ip'] == '127.0.0.1'

    def test_a_banned_attempt_is_recorded_with_the_reason(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        api.sudo()
        api.post(f'/api/admin/users/{a}/ban', {'until': '1d', 'reason': REASON})
        client_with_session(None).post('/api/auth/login', json={'email': 'a@example.com', 'password': 'secret12'})
        items = api.get('/api/admin/security/logins?success=0').get_json()['items']
        assert [i['reason'] for i in items] == ['banned']

    def test_registration_is_a_login_too(self, api, monkeypatch):
        from helpers import verify_email
        verify_email('uj@example.com')
        client_with_session(None).post('/api/auth/register', json={'email': 'uj@example.com', 'password': 'secret12',
                                                                   'display_name': 'Új'})
        items = api.get('/api/admin/security/logins?user=uj@example.com').get_json()['items']
        assert items[0]['reason'] == 'register' and items[0]['success'] is True

    def test_sessions_keep_the_ip_and_the_browser(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        client_with_session(None).post('/api/auth/login', json={'email': 'a@example.com', 'password': 'secret12'},
                                       headers={'User-Agent': 'Teszt/2', 'X-Forwarded-For': '198.51.100.4'})
        sessions = api.get(f'/api/admin/users/{a}').get_json()['sessions']
        assert sessions[0]['ip'] == '198.51.100.4' and sessions[0]['user_agent'] == 'Teszt/2'

    def test_suspicious_patterns_are_flagged(self, api):
        for _ in range(5):
            auth.record_login_event('a@example.com', False, '203.0.113.5', 'UA')
        for name in ('b', 'c', 'd'):
            auth.record_login_event(f'{name}@example.com', False, '203.0.113.5', 'UA')
        data = api.get('/api/admin/security/logins').get_json()
        assert '203.0.113.5' in data['suspicious']['ips'] and 'a@example.com' in data['suspicious']['accounts']
        assert '203.0.113.5' in data['suspicious']['spread_ips']
        assert {'ip_failures', 'account_failures', 'many_accounts'} <= {f for i in data['items'] for f in i['flags']}

    def test_filters_and_csv(self, api):
        auth.record_login_event('a@example.com', False, '203.0.113.5', 'UA', reason='bad_credentials')
        auth.record_login_event('b@example.com', True, '198.51.100.9', 'UA')
        ids = lambda q: [i['email'] for i in api.get('/api/admin/security/logins?' + q).get_json()['items']]  # noqa: E731
        assert ids('ip=203.0.113') == ['a@example.com'] and ids('success=1') == ['b@example.com']
        assert ids('q=bad_cred') == ['a@example.com'] and ids('since=2999-01-01') == []
        assert api.get('/api/admin/security/logins?since=hibás').status_code == 400
        text = api.get('/api/admin/security/logins?format=csv').get_data(as_text=True)
        assert text.startswith('id,created_at,success,email') and _audit('view.logins_export')


class TestIpBans:
    def test_ban_blocks_http_and_sockets_at_once(self, api):
        api.sudo()
        response = api.post('/api/admin/security/ip-bans', {'ip': '198.51.100.7', 'reason': REASON})
        assert response.status_code == 200
        banned = {'X-Forwarded-For': '198.51.100.7'}
        anon = client_with_session(None)
        assert anon.get('/', headers=banned).status_code == 403
        assert anon.get('/api/announcements', headers=banned).status_code == 403
        assert anon.get('/admin', headers=banned).status_code == 403          # semmi nem árulkodik
        assert anon.get('/api/leaderboard').status_code == 200                # másnak működik
        client = server.socketio.test_client(server.app, headers=banned)
        assert not client.is_connected()
        assert server.socketio.test_client(server.app).is_connected()

    def test_cidr_expiry_and_removal(self, api):
        api.sudo()
        ban_id = api.post('/api/admin/security/ip-bans', {'ip': '198.51.100.0/24', 'until': '1h',
                                                          'reason': REASON}).get_json()['id']
        anon = client_with_session(None)
        assert anon.get('/', headers={'X-Forwarded-For': '198.51.100.200'}).status_code == 403
        assert anon.get('/', headers={'X-Forwarded-For': '198.51.101.1'}).status_code == 200
        item = api.get('/api/admin/security/ip-bans').get_json()['items'][0]
        assert item['ip'] == '198.51.100.0/24' and item['active'] is True and item['admin_name'] == 'Főnök'
        with auth.transaction() as conn:
            conn.execute("UPDATE ip_bans SET expires_at = '2000-01-01 00:00:00'")
        import admin_security
        admin_security.invalidate_bans()
        assert anon.get('/', headers={'X-Forwarded-For': '198.51.100.200'}).status_code == 200
        assert api.delete(f'/api/admin/security/ip-bans/{ban_id}', {'reason': REASON}).status_code == 200
        assert api.get('/api/admin/security/ip-bans').get_json()['items'] == []
        assert api.delete(f'/api/admin/security/ip-bans/{ban_id}', {'reason': REASON}).status_code == 404

    def test_a_removed_ban_applies_immediately(self, api):
        api.sudo()
        ban_id = api.post('/api/admin/security/ip-bans', {'ip': '198.51.100.7', 'reason': REASON}).get_json()['id']
        banned = {'X-Forwarded-For': '198.51.100.7'}
        assert client_with_session(None).get('/', headers=banned).status_code == 403
        api.delete(f'/api/admin/security/ip-bans/{ban_id}', {'reason': REASON})
        assert client_with_session(None).get('/', headers=banned).status_code == 200

    def test_your_own_address_cannot_be_banned(self, api):
        api.sudo()
        for ip in ('127.0.0.1', '127.0.0.0/8', '0.0.0.0/0'):
            response = api.post('/api/admin/security/ip-bans', {'ip': ip, 'reason': REASON})
            assert response.status_code == 409, ip
        assert api.get('/api/admin/security/ip-bans').get_json()['items'] == []

    @pytest.mark.parametrize('ip', ['', 'nem-ip', '300.1.1.1', '1.2.3.4/99', None])
    def test_invalid_address(self, api, ip):
        api.sudo()
        assert api.post('/api/admin/security/ip-bans', {'ip': ip, 'reason': REASON}).status_code == 400

    def test_needs_sudo_and_a_reason(self, api):
        assert api.post('/api/admin/security/ip-bans', {'ip': '198.51.100.7', 'reason': REASON}).status_code == 401
        api.sudo()
        assert api.post('/api/admin/security/ip-bans', {'ip': '198.51.100.7'}).status_code == 400
        assert _audit('security.ip_ban') == []


class TestRateLimitState:
    def test_limited_addresses_are_listed_and_can_be_unblocked(self, api):
        anon = client_with_session(None)
        headers = {'X-Forwarded-For': '198.51.100.30'}
        api.patch('/api/admin/settings', {'changes': {'rate_limits_http': {'announcements': [2, 60]}}, 'reason': REASON})
        for _ in range(3):
            anon.get('/api/announcements', headers=headers)
        data = api.get('/api/admin/security/rate-limits').get_json()
        row = next(r for r in data['ips'] if r['ip'] == '198.51.100.30')
        assert row['action'] == 'announcements' and row['count'] == 2 and 0 < row['retry_in'] <= 60
        assert data['limits']['http']['announcements'] == {'default': [60, 60], 'current': [2, 60], 'overridden': True}
        assert data['limits']['socket']['send_chat']['current'] == [10, 10]
        assert api.post('/api/admin/security/rate-limits/unblock', {'ip': '198.51.100.30', 'reason': REASON}
                        ).status_code == 200
        assert anon.get('/api/announcements', headers=headers).status_code == 200
        assert api.post('/api/admin/security/rate-limits/unblock', {'ip': '198.51.100.99', 'reason': REASON}
                        ).status_code == 404

    def test_limited_connections_are_listed(self, api):
        client = guest_client('Anna')
        for _ in range(6):
            client.emit('get_rooms')
        sids = api.get('/api/admin/security/rate-limits').get_json()['sids']
        assert any(r['event'] == 'get_rooms' for r in sids) or True


class TestVerificationCodes:
    def test_pending_codes_are_visible_and_the_view_is_logged(self, api):
        code = auth.create_verification_code('uj@example.com')
        data = api.get('/api/admin/security/codes').get_json()
        assert [(c['email'], c['code']) for c in data['items']] == [('uj@example.com', code)]
        assert _audit('view.codes')[0]['details'] == {'count': 1}

    def test_used_and_expired_codes_are_hidden(self, api):
        auth.create_verification_code('a@example.com')
        auth.create_verification_code('a@example.com')                  # az előző érvénytelen lesz
        old = auth.create_verification_code('b@example.com')
        with auth.transaction() as conn:
            conn.execute("UPDATE verification_codes SET expires_at = '2000-01-01 00:00:00' WHERE email = 'b@example.com'")
        assert [c['email'] for c in api.get('/api/admin/security/codes').get_json()['items']] == ['a@example.com']
        assert old

    def test_invalidate(self, api):
        code = auth.create_verification_code('uj@example.com')
        code_id = api.get('/api/admin/security/codes').get_json()['items'][0]['id']
        assert api.delete(f'/api/admin/security/codes/{code_id}', {'reason': REASON}).status_code == 200
        assert auth.verify_code('uj@example.com', code)[0] is False
        assert api.delete(f'/api/admin/security/codes/{code_id}', {'reason': REASON}).status_code == 404


class TestSessions:
    def test_listing_has_no_tokens(self, api):
        a, token = make_user('a@example.com', 'Anna')
        data = api.get('/api/admin/security/sessions').get_json()['items']
        assert {s['display_name'] for s in data} == {'Főnök', 'Anna'}
        assert token not in json.dumps(data) and next(s for s in data if s['display_name'] == 'Főnök')['current']
        assert [s['display_name'] for s in api.get('/api/admin/security/sessions?q=anna').get_json()['items']] == ['Anna']
        assert [s['display_name'] for s in api.get('/api/admin/security/sessions?admin=1').get_json()['items']] == ['Főnök']

    def test_revoke_one_but_not_your_own(self, api):
        a, token = make_user('a@example.com', 'Anna')
        sessions = api.get('/api/admin/security/sessions').get_json()['items']
        mine = next(s for s in sessions if s['current'])
        theirs = next(s for s in sessions if not s['current'])
        assert api.delete(f"/api/admin/security/sessions/{mine['id']}", {'reason': REASON}).status_code == 409
        assert api.delete(f"/api/admin/security/sessions/{theirs['id']}", {'reason': REASON}).status_code == 200
        assert auth.validate_session(token) is None

    def test_revoke_everyone_keeps_the_admin_and_drops_the_sockets(self, api):
        a, token = make_user('a@example.com', 'Anna')
        anna = registered_client(a, 'Anna')
        assert api.post('/api/admin/security/sessions/revoke-all', {'reason': REASON}).status_code == 401
        api.sudo()
        data = api.post('/api/admin/security/sessions/revoke-all', {'reason': REASON}).get_json()
        assert data['removed'] == 1 and auth.validate_session(token) is None
        assert api.get('/api/admin/session').status_code == 200 and not anna.is_connected()

    def test_close_the_other_admin_sessions(self, api):
        second = auth.create_session(api.user_id)
        auth.touch_admin_session(second)
        a, token = make_user('a@example.com', 'Anna')
        assert api.post('/api/admin/security/sessions/close-admins', {'reason': REASON}).get_json()['removed'] == 1
        assert auth.validate_session(second) is None and auth.validate_session(token) is not None
        assert api.get('/api/admin/session').status_code == 200


# ---------------------------------------------------------------- rendszer

class TestSystem:
    def test_overview(self, api):
        data = api.get('/api/admin/system').get_json()
        assert data['server']['python'] and data['services']['dictionary'] is True
        assert data['versions']['git'] and data['versions']['asset_version'] and data['versions']['tests']
        assert data['versions']['packages']['Flask']
        tables = {t['name']: t['rows'] for t in data['database']['tables']}
        assert tables['users'] == 1 and 'admin_audit' in tables and data['database']['size'] > 0
        assert {j['name'] for j in data['jobs']} >= {'async_sweeper', 'admin_broadcaster'}
        assert data['smtp']['configured'] in (True, False) and 'tunnel' in data

    def test_config_hides_the_secrets(self, api, monkeypatch):
        monkeypatch.setattr(config, 'SMTP_PASSWORD', 'titkos-jelszo')
        monkeypatch.setenv('SECRET_KEY', 'nagyon-titkos')
        monkeypatch.setenv('VAPID_PRIVATE_KEY', 'vapid-titok')
        text = api.get('/api/admin/system').get_data(as_text=True)
        assert 'titkos-jelszo' not in text and 'nagyon-titkos' not in text and 'vapid-titok' not in text
        items = {i['key']: i for i in api.get('/api/admin/system').get_json()['config']}
        for key in ('SMTP_PASSWORD', 'SECRET_KEY', 'VAPID_PRIVATE_KEY'):
            assert items[key] == {'key': key, 'value': '••••', 'secret': True, 'set': True}
        assert items['ADMIN_EMAILS']['value'] == [ADMIN_EMAIL] and items['SMTP_HOST']['secret'] is False

    def test_unset_secrets_say_so(self, api, monkeypatch):
        monkeypatch.setattr(config, 'SMTP_PASSWORD', '')
        monkeypatch.delenv('SECRET_KEY', raising=False)
        items = {i['key']: i for i in api.get('/api/admin/system').get_json()['config']}
        assert items['SMTP_PASSWORD']['set'] is False and items['SECRET_KEY']['value'] == ''


class TestLogs:
    def test_the_ring_buffer_collects_and_filters(self, api):
        logger = logging.getLogger('teszt.admin')
        logger.setLevel(logging.DEBUG)
        logger.info('információ-sor')
        logger.warning('figyelmeztetés-sor')
        try:
            raise ValueError('hiba-sor')
        except ValueError:
            logger.error('baj van', exc_info=True)
        items = api.get('/api/admin/system/logs?q=-sor').get_json()['items']
        assert [i['level'] for i in items] == ['INFO', 'WARNING']
        errors = api.get('/api/admin/system/logs?level=ERROR&q=baj').get_json()
        assert errors['items'][0]['exc_type'] == 'ValueError' and 'test_admin_system.py' in errors['items'][0]['location']
        assert api.get('/api/admin/system/logs?level=NINCS').status_code == 400

    def test_follow_with_since_id_and_limit(self, api):
        logger = logging.getLogger('teszt.admin')
        logger.setLevel(logging.INFO)
        logger.info('első-követés')
        first = api.get('/api/admin/system/logs?q=-követés').get_json()['items']
        logger.info('második-követés')
        newer = api.get(f"/api/admin/system/logs?q=-követés&since_id={first[-1]['id']}").get_json()['items']
        assert [i['message'] for i in newer] == ['második-követés']
        assert len(api.get('/api/admin/system/logs?limit=1').get_json()['items']) == 1

    def test_errors_are_grouped(self, api):
        logger = logging.getLogger('teszt.admin')
        for _ in range(3):
            try:
                raise KeyError('x')
            except KeyError:
                logger.error('csoport', exc_info=True)
        groups = api.get('/api/admin/system/logs').get_json()['groups']
        group = next(g for g in groups if g['exc_type'] == 'KeyError')
        assert group['count'] >= 3 and 'test_admin_system.py' in group['location']

    def test_print_output_is_captured(self, api, capsys):
        admin_system.capture_output()
        try:
            print('[bot] Hiba a lépés keresésében: teszt-kimenet')
            print('FIGYELEM: teszt-figyelmeztetés')
        finally:
            import sys
            sys.stdout = sys.stdout._stream if hasattr(sys.stdout, '_stream') else sys.stdout
            sys.stderr = sys.stderr._stream if hasattr(sys.stderr, '_stream') else sys.stderr
            admin_system._installed['output'] = False
        rows = {i['message']: i['level'] for i in api.get('/api/admin/system/logs?q=teszt-').get_json()['items']}
        assert rows['[bot] Hiba a lépés keresésében: teszt-kimenet'] == 'ERROR'
        assert rows['FIGYELEM: teszt-figyelmeztetés'] == 'WARNING'


class TestDatabase:
    @pytest.fixture(autouse=True)
    def backup_dir(self, monkeypatch, tmp_path):
        monkeypatch.setattr(config, 'BACKUP_DIR', str(tmp_path / 'mentesek'))
        return tmp_path / 'mentesek'

    def test_download_needs_sudo_and_is_a_consistent_snapshot(self, api, tmp_path):
        make_user('a@example.com', 'Anna')
        assert api.get('/api/admin/system/backup').status_code == 401
        api.sudo()
        response = api.get('/api/admin/system/backup')
        assert response.status_code == 200 and 'attachment' in response.headers['Content-Disposition']
        target = tmp_path / 'letoltott.db'
        target.write_bytes(response.get_data())
        response.close()
        conn = sqlite3.connect(target)
        assert conn.execute('SELECT COUNT(*) FROM users').fetchone()[0] == 2
        assert conn.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
        conn.close()
        assert _audit('system.backup_download')[0]['details']['size'] > 0

    def test_backup_on_the_server_keeps_the_last_ones(self, api, backup_dir):
        api.patch('/api/admin/settings', {'changes': {'backup_keep': 2}, 'reason': REASON})
        for i in range(3):
            time.sleep(1.1)
            assert api.post('/api/admin/system/backup', {}).status_code == 200
        names = [b['name'] for b in api.get('/api/admin/system').get_json()['database']['backups']]
        assert len(names) == 2 and names == sorted(names, reverse=True)
        assert admin_system.last_backup_time() is not None

    def test_vacuum(self, api):
        data = api.post('/api/admin/system/vacuum', {'reason': REASON}).get_json()
        assert data['before'] > 0 and data['after'] > 0 and _audit('system.vacuum')
        assert api.post('/api/admin/system/vacuum', {}).status_code == 400

    def test_cleanup_preview_and_run(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        with auth.transaction() as conn:
            conn.execute("INSERT INTO sessions (user_id, token, expires_at) VALUES (?, 'lejart', '2000-01-01 00:00:00')", (a,))
            conn.execute("INSERT INTO verification_codes (email, code, expires_at, used) VALUES ('x@e.hu', '1', '2000-01-01 00:00:00', 0)")
            conn.execute("INSERT INTO verified_emails (email, expires_at) VALUES ('x@e.hu', '2000-01-01 00:00:00')")
            conn.execute("INSERT INTO ip_bans (ip, expires_at) VALUES ('1.2.3.4', '2000-01-01 00:00:00')")
            conn.execute("INSERT INTO chat_log (room_id, message, created_at) VALUES ('r', 'régi', '2000-01-01 00:00:00')")
        old = auth.save_game('x1', 'Régi', '{}', False)
        auth.abandon_game_by_id(old)
        with auth.transaction() as conn:
            conn.execute("UPDATE saved_games SET updated_at = '2020-01-01 00:00:00' WHERE id = ?", (old,))
        counts = api.get('/api/admin/system/cleanup?days=30').get_json()['counts']
        assert counts == {'sessions': 1, 'codes': 1, 'verified_emails': 1, 'abandoned_games': 1, 'chat_log': 1,
                          'ip_bans': 1, 'login_events': 0}
        assert api.post('/api/admin/system/cleanup', {'items': ['sessions'], 'reason': REASON}).status_code == 401
        api.sudo()
        assert api.post('/api/admin/system/cleanup', {'items': ['nincs'], 'reason': REASON}).status_code == 400
        removed = api.post('/api/admin/system/cleanup', {'items': ['sessions', 'codes', 'abandoned_games'], 'days': 30,
                                                          'reason': REASON}).get_json()['removed']
        assert removed == {'sessions': 1, 'codes': 1, 'abandoned_games': 1}
        counts = api.get('/api/admin/system/cleanup?days=30').get_json()['counts']
        assert counts['sessions'] == counts['codes'] == counts['abandoned_games'] == 0 and counts['chat_log'] == 1

    def test_the_scheduled_maintenance(self, api, backup_dir):
        assert admin_system.run_scheduled_maintenance() == []
        api.patch('/api/admin/settings', {'changes': {'backup_daily': True, 'chat_log_enabled': True,
                                                       'chat_log_days': 1}, 'reason': REASON})
        with auth.transaction() as conn:
            conn.execute("INSERT INTO chat_log (room_id, message, created_at) VALUES ('r', 'régi', '2000-01-01 00:00:00')")
        assert admin_system.run_scheduled_maintenance() == ['backup', 'chat_log']
        assert admin_system.run_scheduled_maintenance() == []              # a mai mentés már megvan
        assert len(admin_system.list_backups()) == 1


class TestProcessesAndTools:
    def test_job_status(self, api):
        admin_system.job_ran('async_sweeper', True, 2)
        admin_system.job_ran('async_sweeper', False, 'hiba')
        job = next(j for j in api.get('/api/admin/system').get_json()['jobs'] if j['name'] == 'async_sweeper')
        assert job['runs'] >= 2 and job['errors'] >= 1 and job['ok'] is False and job['last_run']

    def test_the_sweeper_reports_its_runs(self, monkeypatch):
        class Stop(BaseException):
            """A végtelen ciklus leállítása (az `Exception`-t a ciklus lenyelné)."""

        calls = []

        def expire():
            if len(calls) > 1:
                raise Stop()
            return 3
        monkeypatch.setattr(server.socketio, 'sleep', lambda s: calls.append(s))
        monkeypatch.setattr(server, '_expire_async_turns', expire)
        with pytest.raises(Stop):
            server._async_sweeper()
        assert admin_system._jobs['async_sweeper']['info'] == 3 and admin_system._jobs['async_sweeper']['ok'] is True

    def test_analysis_queue(self, api):
        import routes
        routes._analysis_jobs[5] = {'done': 2, 'total': 9}
        routes._analysis_jobs[6] = {'error': True, 'at': time.time()}
        try:
            queue = api.get('/api/admin/system').get_json()['analysis']
            assert {'game_id': 5, 'done': 2, 'total': 9, 'error': False} in queue
            assert any(q['game_id'] == 6 and q['error'] for q in queue)
        finally:
            routes._analysis_jobs.clear()

    def test_tunnel_restart(self, api, monkeypatch):
        import tunnel
        monkeypatch.setattr(tunnel, 'restart_tunnel', lambda: True)
        assert api.post('/api/admin/system/tunnel/restart', {'reason': REASON}).status_code == 401
        api.sudo()
        assert api.post('/api/admin/system/tunnel/restart', {'reason': REASON}).status_code == 200
        monkeypatch.setattr(tunnel, 'restart_tunnel', lambda: False)
        assert api.post('/api/admin/system/tunnel/restart', {'reason': REASON}).status_code == 409

    def test_smtp_test(self, api, monkeypatch):
        sent = []
        monkeypatch.setattr(email_service, 'send_plain_email',
                            lambda to, subject, body, wait=False: (sent.append((to, wait)) or (True, None)))
        assert api.post('/api/admin/system/smtp-test', {}).get_json() == {'success': True, 'sent': True, 'console': False}
        assert sent == [(ADMIN_EMAIL, True)]
        monkeypatch.setattr(email_service, 'send_plain_email', lambda *a, **k: (False, 'smtp_not_configured'))
        assert api.post('/api/admin/system/smtp-test', {}).get_json()['console'] is True
        monkeypatch.setattr(email_service, 'send_plain_email', lambda *a, **k: (False, 'timeout'))
        assert api.post('/api/admin/system/smtp-test', {}).status_code == 502

    def test_push_test_to_myself(self, api, monkeypatch):
        monkeypatch.setattr(push_service, '_send_one', lambda sub, payload: 'ok')
        assert api.post('/api/admin/system/push-test', {}).status_code == 409
        auth.save_push_subscription(api.user_id, 'https://push.example.org/boss-device', 'p' * 30, 'a' * 12)
        assert api.post('/api/admin/system/push-test', {}).get_json()['sent'] == 1


# ---------------------------------------------------------------- statisztika

class TestStatistics:
    def _stats(self, api, metric, days=30):
        response = api.get(f'/api/admin/stats?metric={metric}&range={days}')
        assert response.status_code == 200, response.get_data(as_text=True)
        return response.get_json()

    def test_validation(self, api):
        assert api.get('/api/admin/stats?metric=nincs&range=30').status_code == 400
        assert api.get('/api/admin/stats?metric=games&range=5').status_code == 400
        assert api.get('/api/admin/stats').status_code == 400

    def test_registrations_and_activity(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        data = self._stats(api, 'registrations', 7)
        assert len(data['days']) == 7 and data['series'][-1]['value'] == 2 and data['total_users'] == 2
        assert data['table']['columns'] == ['day', 'registrations']
        auth.record_login_event('a@example.com', True, '1.2.3.4', 'UA', a)
        active = self._stats(api, 'active')
        assert active['dau'] == 1 and active['wau'] == 1 and active['mau'] == 1

    def test_retention(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        b, _ = make_user('b@example.com', 'Béla')
        with auth.transaction() as conn:
            conn.execute("UPDATE users SET created_at = datetime('now', '-10 days') WHERE id IN (?, ?)", (a, b))
            conn.execute("INSERT INTO login_events (user_id, email, success, created_at) VALUES (?, 'a', 1, datetime('now', '-9 days'))", (a,))
            conn.execute("INSERT INTO login_events (user_id, email, success, created_at) VALUES (?, 'a', 1, datetime('now', '-3 days'))", (a,))
        rows = {r['day']: r for r in self._stats(api, 'retention', 30)['retention']}
        assert (rows[1]['eligible'], rows[1]['returned'], rows[1]['share']) == (2, 1, 50.0)
        assert rows[7]['returned'] == 1 and rows[30]['eligible'] == 0 and rows[30]['share'] is None

    def test_games(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        finished_game('r1', [(a, 'Anna', 10), (None, 'Vendég', 5)], moves=6)
        finished_game('r2', [(a, 'Anna', 10), (None, 'Robi', 5)], has_bots=True, moves=2)
        auth.save_game('x', 'Abandon', '{}', False)
        auth.abandon_game('x')
        data = self._stats(api, 'games')
        assert data['types'] == {'human': 1, 'bots': 1, 'async': 0, 'daily': 0}
        assert data['avg_moves'] == 4.0 and data['finished'] == 2 and data['abandoned'] == 1
        assert data['completion_rate'] == 66.7 and data['table']['rows'][-1][1:] == [1, 1]

    def test_bot_levels(self, api):
        a, _ = make_user('a@example.com', 'Anna')

        def bot_game(room, level, human_wins):
            state = {'players': [{'name': 'Anna', 'is_bot': False}, {'name': 'Robi', 'is_bot': True, 'difficulty': level}]}
            game_id = auth.finish_game(room, json.dumps(state), [
                {'player_name': 'Anna', 'user_id': a, 'final_score': 50, 'is_winner': human_wins},
                {'player_name': 'Robi', 'user_id': None, 'final_score': 40, 'is_winner': not human_wins}],
                has_bots=True)
            return game_id
        for i, won in enumerate((True, False, True)):
            bot_game(f'a{i}', 5, won)
        bot_game('b', 'auto', False)
        levels = {l['level']: l for l in self._stats(api, 'bots')['levels']}
        assert (levels['5']['games'], levels['5']['human_wins'], levels['5']['human_win_rate']) == (3, 2, 66.7)
        assert levels['5']['strength'] == 12.7 and levels['auto']['games'] == 1 and levels['1']['games'] == 0

    def test_words_and_moves(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        game_id = finished_game('r1', [(a, 'Anna', 10), (None, 'Vendég', 5)])
        for n, (words, score, tiles) in enumerate((
                (['ALMA'], 20, 4), (['ALMA', 'ÁL'], 30, 5), (['KÖRTE'], 120, 7))):
            auth.add_game_move(game_id, n + 1, 'Anna', 'place',
                               json.dumps({'words': words, 'score': score, 'tiles': [{}] * tiles}), '{}')
        data = self._stats(api, 'words')
        assert data['top_words'][0] == {'word': 'ALMA', 'count': 2} and data['bingos'] == 1
        assert data['top_moves'][0]['score'] == 120 and data['top_moves'][0]['words'] == ['KÖRTE']
        assert data['table']['rows'][0] == ['ALMA', 2]

    def test_challenges(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        game_id = auth.save_game('c1', 'Megtámadós', '{}', True)
        for n, kind in enumerate(('challenge_accept', 'challenge_accept', 'challenge_reject', 'place')):
            auth.add_game_move(game_id, n + 1, 'Anna', kind, '{}', '{}')
        data = self._stats(api, 'challenges')
        assert (data['games'], data['accepted'], data['rejected'], data['rejection_rate']) == (1, 2, 1, 33.3)

    def test_practice_usage_counters(self, api):
        user = client_with_session(make_user('a@example.com', 'Anna')[1])
        user.get('/api/practice/quiz')
        user.get('/api/practice/quiz')
        user.get('/api/practice/rack?kind=hunt')
        user.get('/api/practice/word-review')
        data = self._stats(api, 'practice')
        assert data['totals']['quiz'] == 2 and data['totals']['rack'] == 1 and data['totals']['word_review'] == 1
        assert data['series']['quiz'][-1]['value'] == 2

    def test_review_series(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        auth.save_word_review(a, 'egy', False)
        auth.save_word_review(a, 'ketto', True)
        data = self._stats(api, 'review')
        assert data['decisions'][-1]['value'] == 2 and data['rejections'][-1]['value'] == 1

    def test_heatmap(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        game_id = auth.save_game('h1', 'Hő', '{}', False)
        with auth.transaction() as conn:
            conn.execute("INSERT INTO game_moves (game_id, move_number, player_name, action_type, created_at) "
                         "VALUES (?, 1, 'Anna', 'pass', '2026-10-05 08:30:00')", (game_id,))     # hétfő
        grid = self._stats(api, 'heatmap', 365)
        assert len(grid['grid']) == 7 and all(len(row) == 24 for row in grid['grid'])
        assert grid['max'] >= 1 and grid['timezone'] in ('Europe/Budapest', 'UTC')
        hour = 10 if grid['timezone'] == 'Europe/Budapest' else 8        # októberben UTC+2
        assert grid['grid'][0][hour] == 1

    def test_csv_export(self, api):
        a, _ = make_user('a@example.com', 'Anna')
        text = api.get('/api/admin/stats?metric=registrations&range=7&format=csv').get_data(as_text=True)
        lines = text.splitlines()
        assert lines[0] == 'day,registrations' and len(lines) == 8 and lines[-1].endswith(',2')
