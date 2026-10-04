"""Admin panel: a napló (audit) — csak hozzáfűzhető, a művelettel egy tranzakcióban íródik, szűrhető és exportálható."""
import csv
import io
import json
import sqlite3

import pytest

import admin
import auth
import config
import server
from helpers import client_with_session, create_user_with_session

ADMIN_EMAIL = 'boss@example.com'
PASSWORD = 'secret12'
HEADERS = {'X-Admin-Request': '1', 'Origin': 'http://localhost'}


@pytest.fixture(autouse=True)
def admin_config(monkeypatch):
    monkeypatch.setattr(config, 'ADMIN_EMAILS', frozenset({ADMIN_EMAIL}))
    monkeypatch.setattr(config, 'ADMIN_IP_ALLOWLIST', frozenset())
    server._ip_rate_limits.clear()
    yield
    server._ip_rate_limits.clear()


@pytest.fixture
def admin_id():
    user_id, _ = create_user_with_session(ADMIN_EMAIL, 'Főnök', PASSWORD)
    return user_id


@pytest.fixture
def ctx(admin_id):
    return admin.AdminContext(admin_id, '203.0.113.7', 'Teszt-Böngésző/1.0')


@pytest.fixture
def admin_client(admin_id):
    token = auth.create_session(admin_id)
    auth.touch_admin_session(token)
    return client_with_session(token)


def _rows():
    return admin.query_audit(limit=200)['items']


def _log(ctx, action='user.ban', target_type='user', target_id=1, reason='Teszt ok', **details):
    with admin.action(ctx, action, target_type, target_id, reason=reason, details=details):
        pass


class TestAppendOnly:
    def test_rows_cannot_be_updated(self, ctx):
        _log(ctx)
        with pytest.raises(sqlite3.DatabaseError):
            with auth.transaction() as conn:
                conn.execute("UPDATE admin_audit SET action = 'x'")
        assert _rows()[0]['action'] == 'user.ban'

    def test_rows_cannot_be_deleted(self, ctx):
        _log(ctx)
        with pytest.raises(sqlite3.DatabaseError):
            with auth.transaction() as conn:
                conn.execute('DELETE FROM admin_audit')
        assert len(_rows()) == 1

    def test_the_log_survives_deleting_the_admin_account(self, ctx, admin_id):
        _log(ctx)
        with auth.transaction() as conn:
            conn.execute('DELETE FROM users WHERE id = ?', (admin_id,))
        rows = _rows()
        assert len(rows) == 1 and rows[0]['admin_user_id'] == admin_id and rows[0]['admin_name'] is None

    def test_the_protection_is_in_place_after_a_reinitialisation(self, ctx):
        auth.init_db()
        _log(ctx)
        with pytest.raises(sqlite3.DatabaseError):
            with auth.transaction() as conn:
                conn.execute('DELETE FROM admin_audit')


class TestRecordingActions:
    def test_one_row_with_everything(self, ctx, admin_id):
        with admin.action(ctx, 'user.ban', 'user', 42, reason='  Sértő üzenetek  ',
                          details={'until': '2030-01-01'}) as act:
            act.details['before'] = {'banned': False}
            act.details['after'] = {'banned': True}
        rows = _rows()
        assert len(rows) == 1
        row = rows[0]
        assert row['action'] == 'user.ban'
        assert row['target_type'] == 'user' and row['target_id'] == '42'
        assert row['admin_user_id'] == admin_id and row['admin_name'] == 'Főnök'
        assert row['ip'] == '203.0.113.7' and row['user_agent'] == 'Teszt-Böngésző/1.0'
        assert row['reason'] == 'Sértő üzenetek'
        assert row['details'] == {'reason': 'Sértő üzenetek', 'until': '2030-01-01',
                                  'before': {'banned': False}, 'after': {'banned': True}}
        assert row['created_at']

    def test_the_reason_cannot_be_overridden_by_details(self, ctx):
        with admin.action(ctx, 'user.ban', 'user', 1, reason='Valódi ok', details={'reason': 'Hamis ok'}):
            pass
        assert _rows()[0]['reason'] == 'Valódi ok'

    def test_the_operation_and_the_log_share_one_transaction(self, ctx, admin_id):
        with admin.action(ctx, 'user.rename', 'user', admin_id, reason='Névjavítás') as act:
            act.conn.execute('UPDATE users SET display_name = ? WHERE id = ?', ('Új név', admin_id))
        assert auth.get_user_by_id(admin_id)['display_name'] == 'Új név'
        assert len(_rows()) == 1

    def test_a_failing_log_write_rolls_the_operation_back(self, ctx, admin_id, monkeypatch):
        def broken(*args, **kwargs):
            raise sqlite3.OperationalError('a napló nem írható')
        monkeypatch.setattr(admin, 'record', broken)
        with pytest.raises(sqlite3.OperationalError):
            with admin.action(ctx, 'user.rename', 'user', admin_id, reason='Névjavítás') as act:
                act.conn.execute('UPDATE users SET display_name = ? WHERE id = ?', ('Új név', admin_id))
        assert auth.get_user_by_id(admin_id)['display_name'] == 'Főnök'

    def test_a_failing_operation_leaves_no_log_row(self, ctx, admin_id):
        with pytest.raises(RuntimeError):
            with admin.action(ctx, 'user.rename', 'user', admin_id, reason='Névjavítás') as act:
                act.conn.execute('UPDATE users SET display_name = ? WHERE id = ?', ('Új név', admin_id))
                raise RuntimeError('menet közben hiba')
        assert auth.get_user_by_id(admin_id)['display_name'] == 'Főnök'
        assert _rows() == []

    @pytest.mark.parametrize('reason', [None, '', '   ', 'ab', 5, ['x'], 'x' * 501])
    def test_the_reason_is_mandatory_and_the_operation_never_starts(self, ctx, admin_id, reason):
        entered = []
        with pytest.raises(admin.AdminError) as error:
            with admin.action(ctx, 'user.ban', 'user', 1, reason=reason):
                entered.append(True)
        assert error.value.status == 400
        assert not entered
        assert _rows() == []

    def test_view_events_need_no_reason(self, ctx):
        with auth.transaction() as conn:
            admin.record(conn, ctx, 'view.user', 'user', 7)
        row = _rows()[0]
        assert row['action'] == 'view.user' and row['reason'] is None and row['details'] is None

    def test_a_long_user_agent_is_truncated(self, admin_id):
        long_ctx = admin.AdminContext(admin_id, '1.2.3.4', 'A' * 1000)
        _log(long_ctx)
        assert len(_rows()[0]['user_agent']) == admin.MAX_USER_AGENT_LEN

    def test_unserialisable_values_do_not_break_the_log(self, ctx):
        import datetime
        _log(ctx, when=datetime.datetime(2030, 1, 2, 3, 4, 5))
        assert '2030' in _rows()[0]['details']['when']


class TestQuery:
    @pytest.fixture(autouse=True)
    def sample(self, ctx, admin_id):
        other_id, _ = create_user_with_session('masodik@example.com', 'Moderátor', PASSWORD)
        other = admin.AdminContext(other_id, '198.51.100.5', 'Másik')
        _log(ctx, 'user.ban', 'user', 5, 'Spam')
        _log(ctx, 'user.unban', 'user', 5, 'Tévedés volt')
        _log(ctx, 'room.disband', 'room', 'ABC123', 'Elakadt szoba')
        _log(other, 'dict.reject_add', 'word', 'xyzzy', '100% biztos és_más')
        _log(other, 'users.export', 'user', 6, 'Adatkérés')
        self.admin_id, self.other_id = admin_id, other_id

    def test_newest_first_and_total(self):
        result = admin.query_audit()
        assert result['total'] == 5
        assert [r['action'] for r in result['items']] == [
            'users.export', 'dict.reject_add', 'room.disband', 'user.unban', 'user.ban']

    def test_filter_by_admin_id_name_and_email(self):
        assert {r['action'] for r in admin.query_audit(admin=str(self.other_id))['items']} == \
            {'dict.reject_add', 'users.export'}
        assert admin.query_audit(admin='moder')['total'] == 2
        assert admin.query_audit(admin='BOSS@')['total'] == 3
        assert admin.query_audit(admin='nincs ilyen')['total'] == 0

    def test_filter_by_action_prefix(self):
        assert admin.query_audit(action='user.')['total'] == 2   # a `users.export` nem illeszkedik
        assert admin.query_audit(action='user')['total'] == 3
        assert admin.query_audit(action='room.disband')['total'] == 1

    def test_like_wildcards_in_filters_are_literal(self):
        assert admin.query_audit(action='%')['total'] == 0
        assert admin.query_audit(action='user_ban')['total'] == 0
        assert admin.query_audit(q='%')['total'] == 1       # csak a „100%” indoklás
        assert admin.query_audit(q='és_más')['total'] == 1
        assert admin.query_audit(q='_')['total'] == 1

    def test_filter_by_target(self):
        assert admin.query_audit(target_type='user')['total'] == 3
        assert admin.query_audit(target_type='user', target_id='5')['total'] == 2
        assert admin.query_audit(target_id='ABC123')['total'] == 1

    def test_free_text_search(self):
        assert admin.query_audit(q='spam')['total'] == 1           # az indoklásban
        assert admin.query_audit(q='198.51.100')['total'] == 2     # az IP-ben
        assert admin.query_audit(q='xyzzy')['total'] == 1          # a célpontban

    def test_date_filters(self):
        today = auth.format_ts(auth.utcnow())[:10]
        assert admin.query_audit(since=today)['total'] == 5
        assert admin.query_audit(until=today)['total'] == 5        # a végnap egésze benne van
        assert admin.query_audit(since='2999-01-01')['total'] == 0
        assert admin.query_audit(until='2000-01-01')['total'] == 0
        assert admin.query_audit(since=today + ' 00:00', until=today + ' 23:59:59')['total'] == 5

    def test_combined_filters(self):
        assert admin.query_audit(admin='boss', action='user.', q='spam')['total'] == 1

    def test_paging(self):
        first = admin.query_audit(limit=2, offset=0)
        second = admin.query_audit(limit=2, offset=2)
        third = admin.query_audit(limit=2, offset=4)
        assert [len(p['items']) for p in (first, second, third)] == [2, 2, 1]
        assert first['total'] == second['total'] == third['total'] == 5
        ids = [r['id'] for p in (first, second, third) for r in p['items']]
        assert ids == sorted(ids, reverse=True) and len(set(ids)) == 5

    def test_limits_are_clamped(self):
        assert admin.query_audit(limit=10 ** 6)['limit'] == admin.AUDIT_MAX_LIMIT
        assert admin.query_audit(limit=0)['limit'] == 1
        assert admin.query_audit(limit='szemét')['limit'] == admin.AUDIT_DEFAULT_LIMIT
        assert admin.query_audit(offset=-5)['offset'] == 0
        assert admin.query_audit(limit=10 ** 6, max_limit=admin.AUDIT_EXPORT_MAX)['limit'] == admin.AUDIT_EXPORT_MAX

    @pytest.mark.parametrize('kwargs', [{'since': 'tegnap'}, {'until': '2030-13-45'}, {'action': 'x' * 101},
                                        {'q': ['lista']}, {'admin': 5}])
    def test_invalid_filters(self, kwargs):
        with pytest.raises(admin.AdminError) as error:
            admin.query_audit(**kwargs)
        assert error.value.status == 400


class TestCsv:
    def test_roundtrip_with_special_characters(self, ctx):
        _log(ctx, 'user.note', 'user', 1, 'Vessző, "idézőjel"\nés új sor', extra={'k': 'é'})
        rows = list(csv.reader(io.StringIO(admin.audit_csv(_rows()))))
        assert rows[0] == list(admin.AUDIT_CSV_FIELDS)
        assert len(rows) == 2
        record = dict(zip(rows[0], rows[1]))
        assert record['reason'] == 'Vessző, "idézőjel"\nés új sor'
        assert record['admin_name'] == 'Főnök'
        assert json.loads(record['details_json'])['extra'] == {'k': 'é'}

    @pytest.mark.parametrize('value', ['=HYPERLINK("http://x")', '+1+1', '-2', '@SUM(A1)'])
    def test_formula_injection_is_neutralised(self, ctx, value):
        _log(ctx, reason=value + ' ok')
        record = dict(zip(*list(csv.reader(io.StringIO(admin.audit_csv(_rows()))))))
        assert record['reason'].startswith("'")

    def test_cell_helper_covers_tab_and_carriage_return_too(self):
        assert admin._csv_cell('\tx') == "'\tx"
        assert admin._csv_cell('\rx') == "'\rx"
        assert admin._csv_cell(None) == ''
        assert admin._csv_cell(5) == '5'

    def test_user_agent_is_neutralised_too(self, admin_id):
        _log(admin.AdminContext(admin_id, '1.1.1.1', '=cmd|calc'))
        record = dict(zip(*list(csv.reader(io.StringIO(admin.audit_csv(_rows()))))))
        assert record['user_agent'] == "'=cmd|calc"


class TestAuditEndpoint:
    def test_list(self, admin_client, ctx):
        _log(ctx, 'user.ban', 'user', 5, 'Spam')
        _log(ctx, 'room.disband', 'room', 'ABC123', 'Elakadt')
        data = admin_client.get('/api/admin/audit').get_json()
        assert data['success'] is True and data['total'] == 2
        assert data['items'][0]['action'] == 'room.disband'
        assert data['items'][0]['details']['reason'] == 'Elakadt'

    def test_filters_and_paging_come_from_the_query_string(self, admin_client, ctx):
        for number in range(7):
            _log(ctx, 'user.ban', 'user', number, 'Ok ' + str(number))
        _log(ctx, 'room.disband', 'room', 'ABC123', 'Elakadt')
        data = admin_client.get('/api/admin/audit?action=user.&limit=3&offset=3').get_json()
        assert data['total'] == 7 and len(data['items']) == 3
        assert all(item['action'] == 'user.ban' for item in data['items'])
        assert admin_client.get('/api/admin/audit?q=Elakadt').get_json()['total'] == 1

    def test_list_is_capped_at_the_page_maximum(self, admin_client):
        assert admin_client.get('/api/admin/audit?limit=100000').get_json()['limit'] == admin.AUDIT_MAX_LIMIT

    def test_bad_filter_is_a_400(self, admin_client):
        response = admin_client.get('/api/admin/audit?since=tegnap')
        assert response.status_code == 400
        assert response.get_json()['success'] is False

    def test_unknown_format(self, admin_client):
        assert admin_client.get('/api/admin/audit?format=xml').status_code == 400

    def test_listing_is_not_logged(self, admin_client, ctx):
        _log(ctx)
        admin_client.get('/api/admin/audit')
        admin_client.get('/api/admin/audit?action=user.')
        assert admin.query_audit()['total'] == 1

    def test_csv_export_is_a_download_and_is_logged(self, admin_client, ctx):
        _log(ctx, 'user.ban', 'user', 5, 'Spam')
        response = admin_client.get('/api/admin/audit?format=csv&action=user.')
        assert response.status_code == 200
        assert response.headers['Content-Type'].startswith('text/csv')
        assert 'attachment' in response.headers['Content-Disposition']
        rows = list(csv.reader(io.StringIO(response.get_data(as_text=True))))
        assert len(rows) == 2 and rows[1][rows[0].index('action')] == 'user.ban'
        export = admin.query_audit(action='view.')['items']
        assert len(export) == 1
        assert export[0]['details']['format'] == 'csv'
        assert export[0]['details']['rows'] == 1
        assert export[0]['details']['filters'] == {'action': 'user.'}

    def test_json_export_is_a_download_and_is_logged(self, admin_client, ctx):
        _log(ctx)
        response = admin_client.get('/api/admin/audit?download=1')
        assert 'attachment' in response.headers['Content-Disposition']
        assert response.get_json()['total'] == 1
        assert [r['details']['format'] for r in admin.query_audit(action='view.')['items']] == ['json']

    def test_export_ignores_the_page_size_of_the_listing(self, admin_client, ctx):
        for number in range(admin.AUDIT_MAX_LIMIT + 5):
            _log(ctx, target_id=number)
        data = admin_client.get('/api/admin/audit?download=1').get_json()
        assert len(data['items']) == admin.AUDIT_MAX_LIMIT + 5

    def test_the_log_cannot_be_changed_through_the_api(self, admin_client, ctx):
        _log(ctx)
        for method in ('post', 'put', 'patch', 'delete'):
            response = getattr(admin_client, method)('/api/admin/audit', headers=HEADERS)
            assert response.status_code == 405, method
        for method in ('put', 'patch', 'delete'):
            assert getattr(admin_client, method)('/api/admin/audit/1', headers=HEADERS).status_code == 404
        assert admin.query_audit()['total'] == 1
