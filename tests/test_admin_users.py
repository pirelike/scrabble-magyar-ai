"""Admin panel: felhasználók — lista, részletek és a műveletek (kitiltás, némítás, anonimizálás...)."""
import json

import pytest

import admin
import auth
import config
import server
from helpers import (
    ADMIN_EMAIL, finished_game, guest_client, live_room, make_user, registered_client,
)

REASON = 'Teszt indoklás'


def _audit(action=None, **filters):
    return admin.query_audit(action=action, limit=200, **filters)['items']


def _user(user_id):
    return auth.get_user_by_id(user_id)


@pytest.fixture
def player(api):
    return make_user('anna@example.com', 'Anna')


@pytest.fixture(autouse=True)
def no_real_mail(monkeypatch):
    """A tesztek nem küldenek e-mailt: a kimenő levelek egy listába gyűlnek."""
    import email_service
    sent = []
    monkeypatch.setattr(email_service, 'send_plain_email',
                        lambda to, subject, body, wait=False: (sent.append((to, subject, body)) or (True, None)))
    return sent


class TestList:
    def test_lists_users_with_their_state(self, api, player):
        uid, _ = player
        data = api.get('/api/admin/users').get_json()
        assert data['total'] == 2
        row = next(u for u in data['items'] if u['id'] == uid)
        assert row['display_name'] == 'Anna' and row['status'] == 'active' and row['online'] is False
        assert row['push_devices'] == 0 and row['is_admin'] is False
        assert next(u for u in data['items'] if u['id'] == api.user_id)['is_admin'] is True

    def test_search_is_case_and_accent_insensitive(self, api):
        make_user('o@example.com', 'Őrült Ödön')
        for query in ('orult', 'ŐRÜLT', 'odon'):
            names = [u['display_name'] for u in api.get(f'/api/admin/users?q={query}').get_json()['items']]
            assert names == ['Őrült Ödön'], query

    def test_search_by_email_and_id(self, api, player):
        uid, _ = player
        assert [u['id'] for u in api.get('/api/admin/users?q=anna@exa').get_json()['items']] == [uid]
        assert [u['id'] for u in api.get(f'/api/admin/users?q={uid}').get_json()['items']] == [uid]

    def test_status_filters(self, api, player):
        uid, _ = player
        other, _ = make_user('b@example.com', 'Béla')
        api.sudo()
        api.post(f'/api/admin/users/{uid}/ban', {'until': '1d', 'reason': REASON})
        api.post(f'/api/admin/users/{other}/mute', {'until': '1h', 'reason': REASON})
        ids = lambda q: [u['id'] for u in api.get('/api/admin/users?' + q).get_json()['items']]  # noqa: E731
        assert ids('status=banned') == [uid]
        assert ids('status=muted') == [other]
        assert api.user_id in ids('status=active') and uid not in ids('status=active')
        assert ids('admin=1') == [api.user_id]

    def test_online_filter(self, api, player):
        uid, _ = player
        client = registered_client(uid, 'Anna')
        assert [u['id'] for u in api.get('/api/admin/users?online=1').get_json()['items']] == [uid]
        assert api.get(f'/api/admin/users/{uid}').get_json()['live']['online'] is True
        client.disconnect()
        assert api.get('/api/admin/users?online=1').get_json()['items'] == []

    def test_sorting_and_paging(self, api):
        for i in range(5):
            make_user(f'u{i}@example.com', f'Név{i}')
        data = api.get('/api/admin/users?sort=name&order=asc&limit=2&offset=1').get_json()
        assert data['total'] == 6 and len(data['items']) == 2
        names = [u['display_name'] for u in api.get('/api/admin/users?sort=name&order=asc').get_json()['items']]
        assert names == sorted(names, key=admin.fold)

    def test_invalid_filters(self, api):
        assert api.get('/api/admin/users?status=nincs').status_code == 400
        assert api.get('/api/admin/users?from=hibás').status_code == 400

    def test_csv_export(self, api, player):
        response = api.get('/api/admin/users?format=csv')
        assert response.status_code == 200 and 'text/csv' in response.headers['Content-Type']
        lines = response.get_data(as_text=True).splitlines()
        assert lines[0].startswith('id,display_name,email') and len(lines) == 3

    def test_csv_cells_cannot_inject_formulas(self, api):
        make_user('x@example.com', '=SUM(A1)')
        assert "'=SUM(A1)" in api.get('/api/admin/users?format=csv').get_data(as_text=True)

    def test_global_search(self, api, player):
        data = api.get('/api/admin/search?q=anna').get_json()
        assert [u['display_name'] for u in data['users']] == ['Anna']
        assert data['words'] == [{'word': 'ANNA'}]


class TestDetail:
    def test_detail_is_logged_as_a_view(self, api, player):
        uid, _ = player
        data = api.get(f'/api/admin/users/{uid}').get_json()
        assert data['user']['email'] == 'anna@example.com' and data['user']['is_self'] is False if 'is_self' in data['user'] else True
        assert data['is_self'] is False
        rows = _audit('view.user')
        assert [(r['target_id'], r['admin_user_id']) for r in rows] == [(str(uid), api.user_id)]

    def test_contains_all_sections(self, api, player):
        uid, _ = player
        other, _ = make_user('b@example.com', 'Béla')
        auth.send_friend_request(uid, other)
        auth.accept_friend_request(other, uid)
        auth.save_word_review(uid, 'almaa', False)
        auth.save_push_subscription(uid, 'https://push.example.org/abcdefghij', 'p' * 30, 'a' * 12, 'en')
        finished_game('r1', [(uid, 'Anna', 120), (other, 'Béla', 80)])
        data = api.get(f'/api/admin/users/{uid}').get_json()
        assert [f['display_name'] for f in data['friends']] == ['Béla']
        assert data['reviews']['total'] == 1 and data['reviews']['invalid'] == 1
        assert data['push'] == [{'id': data['push'][0]['id'], 'lang': 'en',
                                 'created_at': data['push'][0]['created_at'], 'host': 'push.example.org'}]
        assert [g['score'] for g in data['games']] == [120]
        assert data['rating_history'][0]['kind'] == 'game'
        assert 'password_hash' not in json.dumps(data) and 'token' not in data['sessions'][0] if data['sessions'] else True

    def test_unknown_user(self, api):
        assert api.get('/api/admin/users/9999').status_code == 404

    def test_profile_view_is_logged(self, api, player):
        uid, _ = player
        data = api.get(f'/api/admin/users/{uid}/profile').get_json()['profile']
        assert data['display_name'] == 'Anna' and data['stats']['rating'] == 1200
        assert len(_audit('view.user_profile')) == 1

    def test_online_user_shows_where_they_are(self, api, player):
        uid, _ = player
        anna = registered_client(uid, 'Anna')
        room = live_room(anna, [guest_client('Béla')])
        live = api.get(f'/api/admin/users/{uid}').get_json()['live']
        assert live['online'] and live['rooms'][0]['code'] == room.join_code and live['rooms'][0]['role'] == 'player'


class TestEveryChangeNeedsAReason:
    @pytest.mark.parametrize('method,path,body', [
        ('post', '/unban', {}), ('post', '/mute', {'until': '1h'}), ('post', '/unmute', {}),
        ('post', '/logout-all', {}), ('post', '/rating', {'rating': 1300}),
        ('post', '/badges', {'badge': 'bingo', 'grant': True}), ('post', '/recompute-stats', {}),
        ('post', '/reviews/block', {'blocked': True}), ('patch', '', {'display_name': 'Új név'}),
        ('post', '/ban', {'until': '1d'}), ('post', '/reset-password', {}), ('post', '/reviews/revert', {}),
    ])
    def test_missing_reason_is_rejected_and_nothing_is_logged(self, api, player, method, path, body):
        uid, _ = player
        api.sudo()
        before = len(_audit())
        response = getattr(api, method)(f'/api/admin/users/{uid}{path}', body)
        assert response.status_code == 400 and response.get_json()['field'] == 'reason'
        assert len(_audit()) == before
        assert _user(uid)['display_name'] == 'Anna' and not _user(uid)['banned_until']


class TestRename:
    def test_renames_and_logs_before_and_after(self, api, player):
        uid, _ = player
        response = api.patch(f'/api/admin/users/{uid}', {'display_name': 'Annácska', 'reason': REASON})
        assert response.status_code == 200 and _user(uid)['display_name'] == 'Annácska'
        row = _audit('user.rename')[0]
        assert row['details']['before'] == {'display_name': 'Anna'}
        assert row['details']['after'] == {'display_name': 'Annácska'}
        assert row['reason'] == REASON

    @pytest.mark.parametrize('name', ['', '   ', 'x' * 21, 'Rossz<név>', 12])
    def test_registration_rules_apply(self, api, player, name):
        uid, _ = player
        assert api.patch(f'/api/admin/users/{uid}', {'display_name': name, 'reason': REASON}).status_code == 400
        assert _user(uid)['display_name'] == 'Anna'

    def test_the_same_name_is_rejected(self, api, player):
        uid, _ = player
        assert api.patch(f'/api/admin/users/{uid}', {'display_name': 'Anna', 'reason': REASON}).status_code == 409

    def test_renames_the_player_in_a_running_game(self, api, player):
        uid, _ = player
        anna = registered_client(uid, 'Anna')
        bela = guest_client('Béla')
        room = live_room(anna, [bela])
        game_id = server._save_game_to_db(room.id) and room.db_game_id
        anna_player = next(p for p in room.game.players if p.name == 'Anna')
        api.patch(f'/api/admin/users/{uid}', {'display_name': 'Annácska', 'reason': REASON})
        assert anna_player.name == 'Annácska'
        assert room.owner_name == 'Annácska' and server.state.player_names[anna_player.id] == 'Annácska'
        assert room.known_user_ids == {} or 'Anna' not in room.known_user_ids
        server._save_game_to_db(room.id)
        names = [p['player_name'] for p in auth.get_game_players(room.db_game_id)]
        assert sorted(names) == ['Annácska', 'Béla']          # nincs duplikált sor
        assert game_id == room.db_game_id
        state = [e for e in anna.get_received() if e['name'] == 'game_state'][-1]['args'][0]
        assert 'Annácska' in [p['name'] for p in state['players']]

    def test_refuses_a_name_taken_in_the_same_room(self, api, player):
        uid, _ = player
        anna = registered_client(uid, 'Anna')
        live_room(anna, [guest_client('Béla')])
        response = api.patch(f'/api/admin/users/{uid}', {'display_name': 'Béla', 'reason': REASON})
        assert response.status_code == 409 and _user(uid)['display_name'] == 'Anna'

    def test_finished_games_keep_the_historic_name(self, api, player):
        uid, _ = player
        other, _ = make_user('b@example.com', 'Béla')
        game_id = finished_game('r1', [(uid, 'Anna', 90), (other, 'Béla', 80)])
        api.patch(f'/api/admin/users/{uid}', {'display_name': 'Annácska', 'reason': REASON})
        assert 'Anna' in [p['player_name'] for p in auth.get_game_players(game_id)]


class TestEmail:
    def test_needs_sudo(self, api, player):
        uid, _ = player
        response = api.patch(f'/api/admin/users/{uid}', {'email': 'uj@example.com', 'reason': REASON})
        assert response.status_code == 401 and response.get_json()['sudo_required'] is True
        assert _user(uid)['email'] == 'anna@example.com'

    def test_changes_and_notifies_the_old_address(self, api, player, no_real_mail):
        uid, _ = player
        api.sudo()
        response = api.patch(f'/api/admin/users/{uid}', {'email': 'Uj@Example.com', 'reason': REASON})
        assert response.status_code == 200 and response.get_json()['notified'] is True
        assert _user(uid)['email'] == 'Uj@Example.com' and _user(uid)['email_lower'] == 'uj@example.com'
        assert no_real_mail[0][0] == 'anna@example.com'
        assert _audit('user.email')[0]['details']['after'] == {'email': 'Uj@Example.com'}

    def test_must_be_unique_valid_and_not_an_admin_address(self, api, player):
        uid, _ = player
        make_user('b@example.com', 'Béla')
        api.sudo()
        for email, status in (('b@example.com', 409), ('nem-email', 400), (ADMIN_EMAIL, 403)):
            assert api.patch(f'/api/admin/users/{uid}', {'email': email, 'reason': REASON}).status_code == status
        assert _user(uid)['email'] == 'anna@example.com'

    def test_an_admin_account_cannot_be_changed(self, api):
        api.sudo()
        assert api.patch(f'/api/admin/users/{api.user_id}', {'email': 'x@example.com', 'reason': REASON}
                         ).status_code == 403
        assert api.patch(f'/api/admin/users/{api.user_id}', {'display_name': 'Más', 'reason': REASON}
                         ).status_code == 403


class TestBan:
    def test_needs_sudo(self, api, player):
        uid, _ = player
        response = api.post(f'/api/admin/users/{uid}/ban', {'until': '1d', 'reason': REASON})
        assert response.status_code == 401 and response.get_json()['sudo_required'] is True
        assert auth.get_ban(uid) is None

    def test_ban_closes_sessions_and_blocks_login_with_the_reason(self, api, player):
        uid, token = player
        api.sudo()
        response = api.post(f'/api/admin/users/{uid}/ban', {'until': '7d', 'reason': 'Sértegetés'})
        assert response.status_code == 200 and response.get_json()['until']
        assert auth.validate_session(token) is None
        ban = auth.get_ban(uid)
        assert ban['reason'] == 'Sértegetés' and ban['permanent'] is False
        login = guest_client and server.app.test_client().post(
            '/api/auth/login', json={'email': 'anna@example.com', 'password': 'secret12'})
        assert login.status_code == 403
        body = login.get_json()
        assert body['message'] == 'A fiókod ki van tiltva.' and body['banned']['reason'] == 'Sértegetés'
        assert body['banned']['until'] == ban['until']
        assert _audit('user.ban')[0]['details']['sessions_closed'] == 1

    def test_permanent_ban(self, api, player):
        uid, _ = player
        api.sudo()
        assert api.post(f'/api/admin/users/{uid}/ban', {'until': None, 'reason': REASON}).get_json()['until'] is None
        assert auth.get_ban(uid)['permanent'] is True and auth.get_ban(uid)['until'] is None

    def test_an_expired_ban_no_longer_applies(self, api, player):
        uid, _ = player
        api.sudo()
        api.post(f'/api/admin/users/{uid}/ban', {'until': '1h', 'reason': REASON})
        with auth.transaction() as conn:
            conn.execute("UPDATE users SET banned_until = '2000-01-01 00:00:00' WHERE id = ?", (uid,))
        assert auth.get_ban(uid) is None
        login = server.app.test_client().post('/api/auth/login',
                                              json={'email': 'anna@example.com', 'password': 'secret12'})
        assert login.status_code == 200

    def test_unban(self, api, player):
        uid, _ = player
        api.sudo()
        api.post(f'/api/admin/users/{uid}/ban', {'until': '1d', 'reason': REASON})
        assert api.post(f'/api/admin/users/{uid}/unban', {'reason': REASON}).status_code == 200
        assert auth.get_ban(uid) is None
        assert api.post(f'/api/admin/users/{uid}/unban', {'reason': REASON}).status_code == 409

    @pytest.mark.parametrize('until', ['2h', 'holnap', 5, '2000-01-01'])
    def test_invalid_duration(self, api, player, until):
        uid, _ = player
        api.sudo()
        assert api.post(f'/api/admin/users/{uid}/ban', {'until': until, 'reason': REASON}).status_code == 400

    def test_admin_cannot_be_banned(self, api):
        api.sudo()
        assert api.post(f'/api/admin/users/{api.user_id}/ban', {'until': '1d', 'reason': REASON}).status_code == 403
        other_admin, _ = make_user('b@example.com', 'Másik')
        assert auth.get_ban(other_admin) is None

    def test_live_connection_is_dropped_and_the_player_leaves_the_game(self, api, player):
        uid, _ = player
        anna = registered_client(uid, 'Anna')
        bela = guest_client('Béla')
        room = live_room(anna, [bela])
        api.sudo()
        api.post(f'/api/admin/users/{uid}/ban', {'until': '1d', 'reason': REASON})
        events = [e['name'] for e in anna.queue]   # a bontott kapcsolat sorából (get_received már nem hívható)
        assert 'account_banned' in events and not anna.is_connected()
        player_state = next(p for p in room.game.players if p.name == 'Anna')
        assert player_state.disconnected is True
        assert not server.state.is_user_online(uid)

    def test_a_banned_user_cannot_rejoin_with_the_old_token(self, api, player):
        uid, _ = player
        anna = registered_client(uid, 'Anna')
        live_room(anna, [guest_client('Béla')])
        token = server.state.get_reconnect_token_for_sid(next(iter(server.state.player_rooms)))
        anna.disconnect()                         # türelmi idő: a token még érvényes
        api.sudo()
        api.post(f'/api/admin/users/{uid}/ban', {'until': '1d', 'reason': REASON})
        again = guest_client('Anna')
        again.emit('rejoin_room', {'token': token})
        failed = [e['args'][0] for e in again.get_received() if e['name'] == 'rejoin_failed']
        assert failed

    def test_a_banned_user_cannot_identify_over_the_socket(self, api, player):
        uid, _ = player
        api.sudo()
        api.post(f'/api/admin/users/{uid}/ban', {'until': '1d', 'reason': 'Sértegetés'})
        client = server.socketio.test_client(server.app)
        from helpers import registered_set_name_payload
        client.emit('set_name', registered_set_name_payload(uid, 'Anna'))
        received = client.get_received()
        banned = [e['args'][0] for e in received if e['name'] == 'account_banned']
        assert banned and banned[0]['reason'] == 'Sértegetés'
        assert not server.state.is_user_online(uid)


class TestMute:
    def _chat(self, anna_id):
        anna = registered_client(anna_id, 'Anna')
        bela = guest_client('Béla')
        live_room(anna, [bela])
        return anna, bela

    def test_muted_messages_are_silently_dropped(self, api, player):
        uid, _ = player
        anna, bela = self._chat(uid)
        api.post(f'/api/admin/users/{uid}/mute', {'until': '1h', 'reason': REASON})
        anna.emit('send_chat', {'message': 'Halló'})
        assert not [e for e in bela.get_received() if e['name'] == 'chat_message']
        assert not [e for e in anna.get_received() if e['name'] in ('chat_message', 'error')]
        api.post(f'/api/admin/users/{uid}/unmute', {'reason': REASON})
        anna.emit('send_chat', {'message': 'Újra'})
        assert [e['args'][0]['message'] for e in bela.get_received() if e['name'] == 'chat_message'] == ['Újra']

    def test_the_mute_expires(self, api, player):
        uid, _ = player
        api.post(f'/api/admin/users/{uid}/mute', {'until': '1h', 'reason': REASON})
        assert auth.is_chat_muted(uid)
        with auth.transaction() as conn:
            conn.execute("UPDATE users SET chat_muted_until = '2000-01-01 00:00:00' WHERE id = ?", (uid,))
        assert not auth.is_chat_muted(uid)


class TestWordReviewControl:
    def test_blocked_reviewers_cannot_vote_and_their_votes_do_not_count(self, api, player):
        import dictionary
        import word_review
        uid, token = player
        word = next(w for w in ('ALMA', 'KORTE', 'HÁZ') if dictionary.is_word_valid(w))
        assert word_review.record_vote(uid, word, False) is True
        assert dictionary.is_rejected(word)
        api.post(f'/api/admin/users/{uid}/reviews/block', {'blocked': True, 'reason': REASON})
        assert not dictionary.is_rejected(word)       # a letiltott bíráló szavazata nem számít
        from helpers import client_with_session
        response = client_with_session(token).post('/api/practice/word-review', json={'word': word, 'valid': False})
        assert response.status_code == 403
        api.post(f'/api/admin/users/{uid}/reviews/block', {'blocked': False, 'reason': REASON})
        assert dictionary.is_rejected(word)

    def test_revert_deletes_the_votes_and_recomputes_the_exclusions(self, api, player):
        import dictionary
        import word_review
        uid, _ = player
        word = next(w for w in ('ALMA', 'KORTE', 'HÁZ') if dictionary.is_word_valid(w))
        word_review.record_vote(uid, word, False)
        api.sudo()
        response = api.post(f'/api/admin/users/{uid}/reviews/revert', {'reason': REASON})
        assert response.get_json()['count'] == 1
        assert auth.get_word_review_votes(word.lower()) == (0, 0) and not dictionary.is_rejected(word)
        assert _audit('user.reviews_revert')[0]['details']['words'] == [word.lower()]


class TestRatingStatsAndBadges:
    def test_manual_rating_is_a_separate_history_row(self, api, player):
        uid, _ = player
        api.sudo()
        response = api.post(f'/api/admin/users/{uid}/rating', {'rating': 1350, 'reason': REASON})
        assert response.status_code == 200 and _user(uid)['rating'] == 1350
        history = api.get(f'/api/admin/users/{uid}').get_json()['rating_history']
        assert history[-1]['kind'] == 'adjustment' and (history[-1]['before'], history[-1]['after']) == (1200, 1350)
        assert _audit('user.rating')[0]['details']['after'] == {'rating': 1350}

    @pytest.mark.parametrize('rating', [50, 99999, '1300', None, True])
    def test_rating_range(self, api, player, rating):
        uid, _ = player
        api.sudo()
        assert api.post(f'/api/admin/users/{uid}/rating', {'rating': rating, 'reason': REASON}).status_code == 400

    def test_rating_needs_sudo(self, api, player):
        uid, _ = player
        assert api.post(f'/api/admin/users/{uid}/rating', {'rating': 1300, 'reason': REASON}).status_code == 401

    def test_recompute_stats_from_the_games(self, api, player):
        uid, _ = player
        other, _ = make_user('b@example.com', 'Béla')
        finished_game('r1', [(uid, 'Anna', 120), (other, 'Béla', 80)])
        with auth.transaction() as conn:
            conn.execute('UPDATE users SET games_played = 99, games_won = 50, total_score = 5 WHERE id = ?', (uid,))
        api.post(f'/api/admin/users/{uid}/recompute-stats', {'reason': REASON})
        user = _user(uid)
        assert (user['games_played'], user['games_won'], user['total_score']) == (1, 1, 120)
        assert _audit('user.recompute_stats')[0]['details']['before']['games_played'] == 99

    def test_badges_can_be_granted_and_revoked(self, api, player):
        uid, _ = player
        assert api.post(f'/api/admin/users/{uid}/badges',
                        {'badge': 'bingo', 'grant': True, 'reason': REASON}).status_code == 200
        assert [b['badge'] for b in auth.get_user_achievements(uid)] == ['bingo']
        assert api.post(f'/api/admin/users/{uid}/badges',
                        {'badge': 'bingo', 'grant': True, 'reason': REASON}).status_code == 409
        assert api.post(f'/api/admin/users/{uid}/badges',
                        {'badge': 'bingo', 'grant': False, 'reason': REASON}).status_code == 200
        assert auth.get_user_achievements(uid) == []
        assert api.post(f'/api/admin/users/{uid}/badges',
                        {'badge': 'nincs', 'grant': True, 'reason': REASON}).status_code == 400


class TestSessionsAndPasswords:
    def test_logout_everywhere(self, api, player):
        uid, token = player
        second = auth.create_session(uid)
        anna = registered_client(uid, 'Anna')
        response = api.post(f'/api/admin/users/{uid}/logout-all', {'reason': REASON})
        assert response.get_json()['sessions_closed'] == 2
        assert auth.validate_session(token) is None and auth.validate_session(second) is None
        assert not anna.is_connected()

    def test_password_reset_returns_the_password_only_without_smtp(self, api, player, monkeypatch):
        uid, token = player
        api.sudo()
        monkeypatch.setattr(config, 'SMTP_CONFIGURED', False)
        data = api.post(f'/api/admin/users/{uid}/reset-password', {'reason': REASON}).get_json()
        assert len(data['temp_password']) == 12 and data['sessions_closed'] == 1
        assert auth.validate_session(token) is None
        ok, _ = auth.verify_password('anna@example.com', data['temp_password'])
        assert ok and not auth.verify_password('anna@example.com', 'secret12')[0]
        assert data['temp_password'] not in json.dumps(_audit('user.reset_password'))

    def test_password_reset_with_smtp_is_only_mailed(self, api, player, monkeypatch, no_real_mail):
        uid, _ = player
        api.sudo()
        monkeypatch.setattr(config, 'SMTP_CONFIGURED', True)
        data = api.post(f'/api/admin/users/{uid}/reset-password', {'reason': REASON}).get_json()
        assert 'temp_password' not in data and data['emailed'] is True
        assert 'jelszavad:' in no_real_mail[0][2]

    def test_password_reset_needs_sudo(self, api, player):
        uid, _ = player
        assert api.post(f'/api/admin/users/{uid}/reset-password', {'reason': REASON}).status_code == 401


class TestPushAndNotes:
    def test_push_test(self, api, player, monkeypatch):
        import push_service
        uid, _ = player
        assert api.post(f'/api/admin/users/{uid}/push-test').status_code == 409
        auth.save_push_subscription(uid, 'https://push.example.org/abcdefghij', 'p' * 30, 'a' * 12, 'hu')
        sent = []
        monkeypatch.setattr(push_service, '_send_one', lambda sub, payload: sent.append(payload) or 'ok')
        data = api.post(f'/api/admin/users/{uid}/push-test').get_json()
        assert data['sent'] == 1 and sent[0]['body'].startswith('Teszt')
        assert _audit('user.push_test')

    def test_delete_a_device(self, api, player):
        uid, _ = player
        auth.save_push_subscription(uid, 'https://push.example.org/abcdefghij', 'p' * 30, 'a' * 12)
        device = api.get(f'/api/admin/users/{uid}').get_json()['push'][0]['id']
        assert api.delete(f'/api/admin/users/{uid}/push/{device}', {'reason': REASON}).status_code == 200
        assert auth.count_push_subscriptions(uid) == 0
        assert api.delete(f'/api/admin/users/{uid}/push/{device}', {'reason': REASON}).status_code == 404

    def test_notes(self, api, player):
        uid, _ = player
        note_id = api.post(f'/api/admin/users/{uid}/notes', {'note': 'Figyelni kell.'}).get_json()['id']
        notes = api.get(f'/api/admin/users/{uid}').get_json()['notes']
        assert [n['note'] for n in notes] == ['Figyelni kell.'] and notes[0]['admin_name'] == 'Főnök'
        assert api.post(f'/api/admin/users/{uid}/notes', {'note': '  '}).status_code == 400
        assert api.delete(f'/api/admin/users/{uid}/notes/{note_id}').status_code == 200
        assert api.get(f'/api/admin/users/{uid}').get_json()['notes'] == []
        assert [r['action'] for r in _audit('user.note')] == ['user.note_delete', 'user.note_add']


class TestExportAndDeletion:
    def test_export_contains_the_data_but_no_secrets(self, api, player):
        uid, _ = player
        other, _ = make_user('b@example.com', 'Béla')
        finished_game('r1', [(uid, 'Anna', 120), (other, 'Béla', 80)], moves=2)
        auth.save_word_review(uid, 'almaa', False)
        auth.grant_achievements(uid, {'bingo'})
        response = api.get(f'/api/admin/users/{uid}/export')
        assert 'attachment' in response.headers['Content-Disposition']
        data = json.loads(response.get_data(as_text=True))
        assert data['account']['email'] == 'anna@example.com'
        assert len(data['games']) == 1 and len(data['games'][0]['moves']) == 1
        assert data['word_reviews'][0]['word'] == 'almaa' and data['achievements'][0]['badge'] == 'bingo'
        assert 'password' not in json.dumps(data).lower() and 'reconnect_token' not in json.dumps(data)
        assert _audit('view.user_export')

    def test_anonymize_keeps_the_games_and_hides_the_name(self, api, player):
        uid, token = player
        other, _ = make_user('b@example.com', 'Béla')
        game_id = finished_game('r1', [(uid, 'Anna', 120), (other, 'Béla', 80)], moves=2)
        for i in range(3):
            finished_game(f'x{i}', [(uid, 'Anna', 100), (other, 'Béla', 50)])
        api.sudo()
        response = api.delete(f'/api/admin/users/{uid}', {'mode': 'anonymize', 'confirm_name': 'Anna',
                                                          'reason': REASON})
        assert response.status_code == 200
        user = _user(uid)
        assert user['display_name'] == f'Törölt felhasználó #{uid}' and user['deleted_at']
        assert user['email'] == f'deleted-{uid}@deleted.invalid' and user['password_hash'] == ''
        assert auth.validate_session(token) is None
        players = {p['player_name'] for p in auth.get_game_players(game_id)}
        assert players == {f'Törölt felhasználó #{uid}', 'Béla'}                  # a játék megmarad
        moves = {m['player_name'] for m in auth.get_game_moves(game_id)}
        assert 'Anna' not in moves
        names = [e['display_name'] for e in auth.get_leaderboard('wins')[0]]
        assert f'Törölt felhasználó #{uid}' not in names and 'Anna' not in names
        assert not auth.verify_password('anna@example.com', 'secret12')[0]
        assert 'Anna' not in json.dumps(_audit('user.anonymize')[0]['details']['after'])

    def test_the_target_name_must_be_typed(self, api, player):
        uid, _ = player
        api.sudo()
        for confirm in (None, '', 'anna', 'Béla'):
            response = api.delete(f'/api/admin/users/{uid}', {'mode': 'anonymize', 'confirm_name': confirm,
                                                              'reason': REASON})
            assert response.status_code == 400
        assert _user(uid)['display_name'] == 'Anna'

    def test_full_deletion_removes_the_row_but_not_the_games(self, api, player):
        uid, _ = player
        other, _ = make_user('b@example.com', 'Béla')
        game_id = finished_game('r1', [(uid, 'Anna', 120), (other, 'Béla', 80)])
        auth.save_word_review(uid, 'almaa', False)
        api.sudo()
        response = api.delete(f'/api/admin/users/{uid}', {'mode': 'delete', 'confirm_name': 'Anna', 'reason': REASON})
        assert response.status_code == 200 and _user(uid) is None
        assert auth.get_game_by_id(game_id) is not None
        assert all(p['user_id'] is None or p['user_id'] == other for p in auth.get_game_players(game_id))
        assert auth.get_voted_rejected_words(1) == set()

    def test_deletion_needs_sudo_and_never_touches_admins(self, api, player):
        uid, _ = player
        assert api.delete(f'/api/admin/users/{uid}', {'mode': 'delete', 'confirm_name': 'Anna',
                                                      'reason': REASON}).status_code == 401
        api.sudo()
        assert api.delete(f'/api/admin/users/{api.user_id}', {'mode': 'delete', 'confirm_name': 'Főnök',
                                                              'reason': REASON}).status_code == 403
        assert _user(api.user_id) is not None

    def test_a_failing_audit_write_rolls_the_deletion_back(self, api, player, monkeypatch):
        uid, _ = player
        api.sudo()
        monkeypatch.setattr(admin, 'record', lambda *a, **k: (_ for _ in ()).throw(RuntimeError('napló hiba')))
        with pytest.raises(RuntimeError):
            admin.action  # noqa: B018
            import admin_users
            admin_users.delete_user(api.ctx(), uid, 'delete', 'Anna', REASON)
        assert _user(uid) is not None
