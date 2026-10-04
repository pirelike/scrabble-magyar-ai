"""Admin panel: a futó szerver élő állapota — áttekintés, riasztások, szobák és beavatkozások.

A modul a `server.py` függvényein keresztül avatkozik be (`_emit_all_states`, `_schedule_bot_turn`,
`room.invalidate_*`, `_save_game_to_db`...), sosem írja át közvetlenül a játékállapotot (kivétel az átnevezés).
A szerver modulját a `bind()` adja át (a `server.py` végén), így nincs körkörös import, és `python server.py`
indításnál sem töltődik be kétszer.
"""
import time
from datetime import timedelta

import admin
import admin_system
import auth
import mail_config
import dictionary
import settings
from admin import AdminError

_server = None

STUCK_IDLE_SECONDS = 1800        # ennyi mozdulatlanság után jelzünk időkorlát nélküli játékot
STUCK_BOT_SECONDS = 90           # a soron lévő robot ennyi ideje nem lépett, és nincs ütemezett lépése
STUCK_OVERDUE_SECONDS = 60       # a lejárt időzítő / szavazás ennyi késés után jelez
ROOM_ACTIONS = ('message', 'kick', 'transfer', 'skip', 'extend', 'pause', 'resume', 'resolve_vote',
                'reschedule_bot', 'save', 'end', 'void', 'disband')
EXTEND_CHOICES = (30, 60, 120, 300)
MAX_SYSTEM_MESSAGE = 200
ADMIN_ALERT_WINDOW = 15 * 60


def bind(module):
    """A futó szerver modulja (a `server.py` hívja)."""
    global _server
    _server = module


def srv():
    if _server is None:
        raise AdminError('A szerver állapota nem érhető el.', 503)
    return _server


def _state():
    return srv().state


# ===== Online felhasználók =====

def online_user_ids():
    return frozenset(_state().get_online_user_ids()) if _server else frozenset()


def user_live(user_id):
    """Hol van most a felhasználó: online-e, hány kapcsolata van, melyik szobákban."""
    if _server is None:
        return {'online': False, 'connections': 0, 'rooms': []}
    state = _state()
    sids = set(state.get_user_sids(user_id))
    rooms = []
    for room_id, room in list(state.rooms.items()):
        role = None
        for sid in sids:
            if state.player_rooms.get(sid) == room_id:
                role = 'player'
            elif state.spectator_rooms.get(sid) == room_id:
                role = 'spectator'
        if role is None and room.is_async and user_id in room.known_user_ids.values():
            role = 'async_player'
        if role:
            rooms.append({'room_id': room_id, 'code': room.join_code, 'name': room.name, 'role': role,
                          'since': room.created_at})
    return {'online': bool(sids), 'connections': len(sids), 'rooms': rooms}


def kick_user(user_id, event='account_banned', payload=None):
    """A felhasználó összes élő kapcsolatának bontása (kitiltás, kijelentkeztetés): előbb értesítés, aztán
    bontás. A játékban lévő játékos a szokásos módon `disconnected` lesz (a disconnect esemény kezeli).
    A türelmi időben lévő (már lecsatlakozott) kapcsolatainak újracsatlakozó tokenjét is érvényteleníti."""
    if _server is None:
        return 0
    server = srv()
    state = server.state
    for token, info in list(state._reconnect_tokens.items()):
        auth_info = info.get('auth_info') or {}
        if auth_info.get('user_id') == user_id and not auth_info.get('is_guest') \
                and token in state._disconnected_players:
            state._disconnected_players.pop(token, None)
            state._reconnect_tokens.pop(token, None)
    sids = list(state.get_user_sids(user_id))
    for sid in sids:
        if payload is not None:
            server.socketio.emit(event, payload, room=sid)
        try:
            server.socketio.server.disconnect(sid, namespace='/')
        except Exception:     # a kapcsolat közben megszűnhetett
            pass
    return len(sids)


def kick_all(except_user_ids=frozenset()):
    """Minden online felhasználó kapcsolatának bontása (tömeges kijelentkeztetés), a kivételek nélkül."""
    if _server is None:
        return 0
    total = 0
    for user_id in list(_state().get_online_user_ids()):
        if user_id not in except_user_ids:
            total += kick_user(user_id)
    return total


def _players_of_user(room, user_id):
    server = srv()
    return [p for p in room.game.players if not p.is_bot and server._user_id_for_player(room, p) == user_id]


def rename_conflicts(user_id, new_name):
    """Azok a futó szobák, ahol az új név ütközne egy másik játékos nevével."""
    if _server is None:
        return []
    clash = []
    for room in list(_state().rooms.values()):
        mine = _players_of_user(room, user_id)
        if mine and any(p.name.casefold() == new_name.casefold() and p not in mine for p in room.game.players):
            clash.append(room.name)
    return clash


def rename_live(user_id, old, new):
    """A felhasználó nevének frissítése a futó szobákban (játékos, lépésnapló, ismert nevek, tulajdonos).
    Visszatér: az érintett szobák azonosítói."""
    if _server is None:
        return []
    server = srv()
    state = server.state
    changed = []
    for room_id, room in list(state.rooms.items()):
        mine = _players_of_user(room, user_id)
        if not mine:
            continue
        for player in mine:
            if player.name != old:
                continue
            if player.id in state.player_names:
                state.player_names[player.id] = new
            player.name = new
        if old in room.known_user_ids:
            room.known_user_ids[new] = room.known_user_ids.pop(old)
        if room.owner_name == old:
            room.owner_name = new
        for move in room.game.move_log:
            if move['player_name'] == old:
                move['player_name'] = new
        room.game._history_cache = None
        if old in room.late_join_names:
            room.late_join_names[new] = room.late_join_names.pop(old)
        expected = getattr(room, 'expected_players', None)
        if expected:
            room.expected_players = [new if n == old else n for n in expected]
        changed.append(room_id)
    for info in list(state._reconnect_tokens.values()) + list(state._disconnected_players.values()):
        if info.get('player_name') == old and (info.get('auth_info') or {}).get('user_id') == user_id:
            info['player_name'] = new
    for room_id in changed:
        server._emit_all_states(state.rooms[room_id].game, room_id)
    return changed


# ===== Szobák: összegzés =====

def room_kind(room):
    if room.is_puzzle:
        return 'puzzle'
    if room.is_async:
        return 'async'
    if room.is_restored:
        return 'restore'
    return 'game'


def room_status(room):
    game = room.game
    if game.finished:
        return 'finished'
    if not game.started:
        return 'waiting'
    if game.pending_challenge:
        return 'voting'
    return 'playing'


def stuck_reason(room, now=None):
    """Miért tűnik elakadtnak a játék (vagy None): bot_idle / vote_overdue / timer_overdue / no_timer / idle."""
    now = now if now is not None else time.time()
    game = room.game
    if not game.started or game.finished or room.is_puzzle:
        return None
    pending = game.pending_challenge
    if pending:
        if now > pending.expires_at + STUCK_OVERDUE_SECONDS:
            return 'vote_overdue'
        return None
    current = game.current_player()
    if current is None:
        return None
    if current.is_bot:
        if srv()._bot_should_move(room):
            scheduled = room.bot_scheduled_at
            if scheduled is None or now - scheduled > STUCK_BOT_SECONDS:
                return 'bot_idle'
        return None
    if game.turn_time_limit and not room.is_async and room.timer_paused_left is None:
        expires = room.turn_timer_expires_at
        if expires is None:
            return 'no_timer' if now - room.last_activity > STUCK_OVERDUE_SECONDS else None
        if now > expires + STUCK_OVERDUE_SECONDS:
            return 'timer_overdue'
        return None
    if (not room.is_async and not game.turn_time_limit and now - room.last_activity > STUCK_IDLE_SECONDS
            and game.has_connected_human()):
        return 'idle'
    return None


def _player_row(room, player):
    server = srv()
    return {
        'name': player.name, 'is_bot': player.is_bot, 'difficulty': player.difficulty, 'score': player.score,
        'online': player.is_bot or not player.disconnected, 'hand_count': len(player.hand),
        'user_id': None if player.is_bot else server._user_id_for_player(room, player),
        'sid': None if player.is_bot else player.id, 'resigned': player.resigned,
        'owner': (not player.is_bot) and room.owner == player.id,
    }


def room_summary(room, now=None):
    """A szoba egy sora a listához (és a `admin_room_update` eseményhez)."""
    now = now if now is not None else time.time()
    game = room.game
    current = game.current_player() if game.started and not game.finished else None
    return {
        'id': room.id, 'code': room.join_code, 'name': room.name, 'kind': room_kind(room),
        'status': room_status(room), 'private': room.is_private, 'owner': room.owner_name,
        'players': [_player_row(room, p) for p in game.players], 'spectators': len(room.spectators),
        'turn_time_limit': game.turn_time_limit, 'challenge_mode': game.challenge_mode,
        'has_bots': any(p.is_bot for p in game.players), 'created_at': room.created_at,
        'idle_seconds': int(now - room.last_activity), 'stuck': stuck_reason(room, now),
        'turn_number': game.turn_number, 'current_player': current.name if current else None,
        'db_game_id': room.db_game_id, 'paused': room.timer_paused_left is not None,
    }


def list_rooms(args):
    """Az összes szoba a memóriából, szűrhetően: q (kód / név / id / játékos), kind, status, stuck, bots."""
    now = time.time()
    q = (args.get('q') or '').strip().lower()
    rows = []
    for room in list(_state().rooms.values()):
        summary = room_summary(room, now)
        if args.get('kind') and summary['kind'] != args['kind']:
            continue
        if args.get('status') and summary['status'] != args['status']:
            continue
        if args.get('stuck') in ('1', 'true') and not summary['stuck']:
            continue
        if args.get('bots') in ('1', 'true') and not summary['has_bots']:
            continue
        if q and not (q in room.join_code or q in room.id.lower() or q in room.name.lower()
                      or any(q in p['name'].lower() for p in summary['players'])):
            continue
        rows.append(summary)
    rows.sort(key=lambda r: r['created_at'], reverse=True)
    return {'items': rows, 'total': len(rows)}


def find_room(key):
    """Szoba azonosító vagy 6 jegyű kód alapján (404, ha nincs)."""
    state = _state()
    room = state.rooms.get(key)
    if room is None and isinstance(key, str):
        room_id = state.join_codes.get(key)
        room = state.rooms.get(room_id) if room_id else None
    if room is None:
        raise AdminError('A szoba nem található.', 404)
    return room


def _chat_rows(room):
    rows = []
    for msg, meta in zip(room.chat_messages, room.chat_meta):
        rows.append({'name': msg['name'], 'message': msg['message'], 'system': bool(msg.get('system')),
                     'ts': meta['ts'], 'user_id': meta['user_id']})
    # A régebbi (meta nélküli) üzenetek időbélyeg nélkül
    for msg in room.chat_messages[:max(0, len(room.chat_messages) - len(room.chat_meta))]:
        rows.insert(0, {'name': msg['name'], 'message': msg['message'], 'system': bool(msg.get('system')),
                        'ts': None, 'user_id': None})
    return rows


def live_chat(ctx, args):
    """Az összes futó szoba chatje egy helyen (időrendben), keresővel; a tiltott szavas üzenetek jelölve.
    Naplózva: `view.chat`."""
    import admin_mod
    q = (args.get('q') or '').strip().lower()
    rows = []
    for room in list(_state().rooms.values()):
        for item in _chat_rows(room):
            if q and q not in item['message'].lower() and q not in item['name'].lower() \
                    and q not in room.name.lower():
                continue
            rows.append({**item, 'room_id': room.id, 'room_name': room.name, 'code': room.join_code,
                         'flagged': admin_mod.contains_banned(item['message'])})
    rows.sort(key=lambda r: r['ts'] or 0)
    limit = admin._int_arg(args.get('limit'), 200, 1, 500)
    with auth.transaction() as conn:
        admin.record(conn, ctx, 'view.chat', 'chat', None, {'source': 'live', 'q': q or None})
    return {'items': rows[-limit:], 'total': len(rows)}


def _grace_info(room):
    """A türelmi időben lévő játékosok hátralévő ideje (a lecsatlakozás idejéből és a beállított időkből)."""
    server = srv()
    state = server.state
    now = time.time()
    rows = []
    for token, info in list(state._disconnected_players.items()):
        if info['room_id'] != room.id:
            continue
        is_owner = room.owner_token == token
        grace = (server._disconnect_grace() if (room.game.started or not is_owner)
                 else server._waiting_owner_grace())
        rows.append({'name': info['player_name'], 'sid': info['sid'],
                     'remaining': max(0, int(grace - (now - info.get('at', now)))),
                     'token_prefix': token[:6]})
    return rows


def room_detail(ctx, room_key):
    """A szoba teljes képe: tábla, pontok, zsák, szavazás, történet, chat, kapcsolatok. A kezek NEM szerepelnek
    (azokhoz külön, naplózott kérés kell: `room_racks`)."""
    room = find_room(room_key)
    game = room.game
    summary = room_summary(room)
    pending = None
    if game.pending_challenge:
        pc = game.pending_challenge
        names = {p.id: p.name for p in game.players}
        pending = {'player': game.players[pc.player_idx].name, 'words': list(pc.word_strs), 'score': pc.score,
                   'votes': {names.get(pid, pid): vote for pid, vote in pc.votes.items()},
                   'voters': [names[i] for i in game._get_voter_ids() if i in names], 'expires_at': pc.expires_at}
    bag = {}
    for tile in game.bag.tiles:
        bag[tile or '?'] = bag.get(tile or '?', 0) + 1
    detail = dict(summary)
    detail.update({
        'board': game.board.to_dict(), 'bag': bag, 'bag_count': game.bag.remaining(),
        'timer_expires_at': room.turn_timer_expires_at, 'timer_paused_left': room.timer_paused_left,
        'pending': pending, 'history': game.get_history(), 'chat': _chat_rows(room),
        'connections': _grace_info(room),
        'spectator_names': sorted(room.spectators.values()),
        'async': {'turn_hours': game.turn_hours, 'deadline': game.turn_deadline} if game.async_mode else None,
        'last_action': game.last_action, 'max_players': room.max_players, 'puzzle': game.puzzle,
        'saved': room.db_game_id, 'server_time': time.time(),
    })
    return detail


def room_racks(ctx, room_key):
    """A játékosok keze (csak kifejezett kérésre; naplózva `view.racks`)."""
    room = find_room(room_key)
    with auth.transaction() as conn:
        admin.record(conn, ctx, 'view.racks', 'room', room.id, {'code': room.join_code, 'name': room.name})
    return [{'name': p.name, 'hand': [t or '?' for t in p.hand]} for p in room.game.players]


# ===== Szobák: beavatkozások =====

def _find_human(room, name):
    for player in room.game.players:
        if player.name == name and not player.is_bot:
            return player
    raise AdminError('A játékos nem található.', 404, field='player')


def _after_change(room_id, room, restart_timer=True):
    """Beavatkozás után: időzítő, állapot küldése, szükség esetén robotlépés."""
    server = srv()
    game = room.game
    room.invalidate_turn_timer()
    if game.pending_challenge:
        server._start_challenge_timer(room_id)
    elif game.finished:
        room.invalidate_challenge_timer()
        server._save_game_to_db(room_id)
    elif restart_timer:
        server._start_turn_timer(room_id)
    server._emit_all_states(game, room_id)
    if game.finished:
        server._broadcast_rooms()
        if room.is_async:
            server._cleanup_finished_async(room_id)
    server._schedule_bot_turn(room_id)


def _remove_player(room_id, room, player):
    """Játékos eltávolítása a szobából: a token érvénytelen, aktív játékban `disconnected`, várakozóban törlés."""
    server = srv()
    state = server.state
    game = room.game
    sid = player.id
    started = game.started and not game.finished
    was_owner = room.owner == sid
    server._sio_leave_room(sid, room_id)
    state.player_rooms.pop(sid, None)
    state.cleanup_player_token(sid)
    server.socketio.emit('room_left', {}, room=sid)
    server._notify_removed(sid)
    if started:
        game.mark_disconnected(sid)
    else:
        game.remove_player(sid)
    if not game.human_players() or (not room.is_async and not game.has_connected_human()):
        if started and not room.manually_saved:
            if room.db_game_id:
                auth.abandon_game_by_id(room.db_game_id)
            else:
                auth.abandon_game(room_id)
        server._cleanup_room(room_id)
    else:
        if was_owner and not room.is_async:
            server._transfer_ownership(room)
        if started:
            server._advance_turn_if_needed(room_id, room, game)
        server._emit_all_states(game, room_id)
        server.socketio.emit('player_left', {'name': player.name}, room=room_id)
    server.socketio.emit('rooms_list', state.get_rooms_list())


def _timer_remaining(room):
    if room.timer_paused_left is not None:
        return room.timer_paused_left
    if room.turn_timer_expires_at:
        return max(0.0, room.turn_timer_expires_at - time.time())
    return None


def room_action(ctx, room_key, body):
    """Beavatkozás egy szobába. A törzs: {action, reason, ...}. Minden művelet naplózva, indoklással
    (a rendszerüzenet kivétel: annak a szövege kerül a naplóba). A naplósor a művelet ELŐTT íródik, így ha a
    naplózás hibázik, a művelet nem történik meg. Visszatér: rövid eredmény."""
    action = body.get('action')
    if action not in ROOM_ACTIONS:
        raise AdminError('Ismeretlen művelet.', 400, field='action')
    room = find_room(room_key)
    room_id, game, server = room.id, room.game, srv()
    target = {'code': room.join_code, 'name': room.name}

    if action == 'message':
        text = admin.clean_text(body.get('message'), 'message', MAX_SYSTEM_MESSAGE)
        with auth.transaction() as conn:
            admin.record(conn, ctx, 'room.message', 'room', room_id, {**target, 'message': text})
        room.add_chat_message('Rendszer', text, system=True)
        server.socketio.emit('chat_message', {'name': 'Rendszer', 'message': text, 'system': True}, room=room_id)
        return {'message': text}

    reason = body.get('reason')
    admin.normalize_reason(reason)          # indoklás nélkül nincs beavatkozás (az állapot-ellenőrzések előtt)
    started = game.started and not game.finished
    name = f'room.{action}'
    if action == 'kick':
        player = _find_human(room, admin.clean_text(body.get('player'), 'player', 40))
        with admin.action(ctx, name, 'room', room_id, reason=reason, details={**target, 'player': player.name}):
            pass
        _remove_player(room_id, room, player)
        return {'player': player.name}

    if action == 'transfer':
        player = _find_human(room, admin.clean_text(body.get('player'), 'player', 40))
        if room.is_async or room.is_puzzle:
            raise AdminError('Ebben a szobában nincs tulajdonos.', 409)
        if player.disconnected:
            raise AdminError('A játékos nincs online.', 409, field='player')
        with admin.action(ctx, name, 'room', room_id, reason=reason,
                          details={**target, 'before': {'owner': room.owner_name},
                                   'after': {'owner': player.name}}):
            pass
        room.transfer_ownership(player.id, player.name, server.state.get_reconnect_token_for_sid(player.id))
        server.socketio.emit('room_code', {'code': room.join_code}, room=player.id)
        server._emit_all_states(game, room_id)
        return {'owner': player.name}

    if action == 'skip':
        if not started or game.puzzle is not None:
            raise AdminError('Nincs aktív játék.', 409)
        if game.pending_challenge:
            raise AdminError('Szavazás közben nem lehet átugrani a kört.', 409)
        current = game.current_player()
        with admin.action(ctx, name, 'room', room_id, reason=reason,
                          details={**target, 'player': current.name, 'turn': game.turn_number}):
            pass
        ok, _msg = game.pass_turn(current.id, timeout=room.is_async)
        if not ok:
            raise AdminError('A kör nem ugorható át.', 409)
        # A lépésnaplóban „admin” jelöléssel
        if game.move_log:
            import json
            last = game.move_log[-1]
            try:
                details = json.loads(last['details_json'] or '{}')
            except ValueError:
                details = {}
            details['admin'] = True
            last['details_json'] = json.dumps(details, ensure_ascii=False)
        _after_change(room_id, room)
        return {'player': current.name}

    if action in ('extend', 'pause', 'resume'):
        if not started or not game.turn_time_limit or room.is_async:
            raise AdminError('Ebben a játékban nincs körönkénti időzítő.', 409)
        if game.pending_challenge:
            raise AdminError('Szavazás közben nem módosítható az időzítő.', 409)
        remaining = _timer_remaining(room)
        if action == 'extend':
            seconds = body.get('seconds')
            if seconds not in EXTEND_CHOICES:
                raise AdminError('Érvénytelen időtartam.', 400, field='seconds')
            with admin.action(ctx, name, 'room', room_id, reason=reason, details={**target, 'seconds': seconds}):
                pass
            paused = room.timer_paused_left is not None
            total = (remaining or 0) + seconds
            if paused:
                room.timer_paused_left = total
                server._emit_all_states(game, room_id)
            else:
                server._start_turn_timer(room_id, seconds=total)
                server._emit_all_states(game, room_id)
            return {'seconds': int(total)}
        if action == 'pause':
            if room.timer_paused_left is not None:
                raise AdminError('Az időzítő már szünetel.', 409)
            if remaining is None:
                raise AdminError('Nincs futó időzítő.', 409)
            with admin.action(ctx, name, 'room', room_id, reason=reason, details={**target, 'remaining': int(remaining)}):
                pass
            room.invalidate_turn_timer()
            room.timer_paused_left = remaining
            server._emit_all_states(game, room_id)
            return {'remaining': int(remaining)}
        if room.timer_paused_left is None:
            raise AdminError('Az időzítő nem szünetel.', 409)
        with admin.action(ctx, name, 'room', room_id, reason=reason, details={**target}):
            pass
        server._start_turn_timer(room_id, seconds=max(1.0, room.timer_paused_left))
        server._emit_all_states(game, room_id)
        return {}

    if action == 'resolve_vote':
        if not game.pending_challenge:
            raise AdminError('Nincs függő lerakás.', 409)
        accept = body.get('accept')
        if not isinstance(accept, bool):
            raise AdminError('Érvénytelen kérés.', 400, field='accept')
        with admin.action(ctx, name, 'room', room_id, reason=reason,
                          details={**target, 'accept': accept, 'words': list(game.pending_challenge.word_strs)}):
            pass
        ok, result, msg = game.force_resolve_pending(accept)
        server._handle_challenge_result(room_id, room, game, result, msg)
        return {'result': result}

    if action == 'reschedule_bot':
        current = game.current_player() if started else None
        if not current or not current.is_bot:
            raise AdminError('Nem robot következik.', 409)
        if not server._bot_should_move(room):
            raise AdminError('A robot most nem léphet (nincs jelen ember és néző).', 409)
        with admin.action(ctx, name, 'room', room_id, reason=reason, details={**target, 'bot': current.name}):
            pass
        server._schedule_bot_turn(room_id)
        return {'bot': current.name}

    if action == 'save':
        if not game.started or game.puzzle is not None:
            raise AdminError('Nincs mit menteni.', 409)
        with admin.action(ctx, name, 'room', room_id, reason=reason, details=target):
            pass
        ok, _msg = server._save_game_to_db(room_id)
        if not ok:
            raise AdminError('Mentési hiba.', 500)
        room.manually_saved = True
        return {'game_id': room.db_game_id}

    if action == 'end':
        if not started or game.puzzle is not None:
            raise AdminError('Nincs aktív játék.', 409)
        with admin.action(ctx, name, 'room', room_id, reason=reason,
                          details={**target, 'scores': {p.name: p.score for p in game.players}}):
            pass
        game.force_end()
        room.invalidate_challenge_timer()
        room.invalidate_bot_turn()
        _after_change(room_id, room, restart_timer=False)
        return {'winners': [p.name for p in game.winners]}

    if action == 'void':
        if not started or game.puzzle is not None:
            raise AdminError('Nincs aktív játék.', 409)
        with admin.action(ctx, name, 'room', room_id, reason=reason, details=target):
            pass
        ok, _msg = server._save_game_to_db(room_id)
        if not ok or not room.db_game_id:
            raise AdminError('Mentési hiba.', 500)
        auth.set_game_status(room.db_game_id, 'voided')
        room.manually_saved = True     # a feloszlatás ne írja át „abandoned”-re
        server._disband_active_room(room_id, 'A játék érvénytelenítve lett.')
        server.socketio.emit('rooms_list', server.state.get_rooms_list())
        return {'game_id': None}

    # disband
    save = body.get('save')
    if not isinstance(save, bool):
        save = False
    message = admin.clean_text(body.get('message'), 'message', MAX_SYSTEM_MESSAGE, required=False) \
        or 'A szobát feloszlatták.'
    with admin.action(ctx, name, 'room', room_id, reason=reason,
                      details={**target, 'save': save, 'message': message}):
        pass
    if save and game.started and not game.finished and game.puzzle is None:
        ok, _msg = server._save_game_to_db(room_id)
        if ok:
            room.manually_saved = True
    server._disband_active_room(room_id, message)
    server.socketio.emit('rooms_list', server.state.get_rooms_list())
    return {}


# ===== Áttekintés =====

def _count_today(conn, sql, params=()):
    return conn.execute(sql, params).fetchone()[0]


def today_counters():
    """Mai számlálók (UTC nap): regisztrációk, befejezett játékok, napi feladvány próbálkozások, szótár-építő
    döntések, kizárt szavak."""
    import daily
    today = auth.utcnow().strftime('%Y-%m-%d')
    with auth.transaction() as conn:
        data = {
            'registrations': _count_today(conn, 'SELECT COUNT(*) FROM users WHERE date(created_at) = ?', (today,)),
            'finished_games': _count_today(
                conn, "SELECT COUNT(*) FROM saved_games WHERE status = 'finished' AND date(updated_at) = ?", (today,)),
            'daily_attempts': _count_today(
                conn, 'SELECT COALESCE(SUM(attempts), 0) FROM daily_scores WHERE puzzle_date = ?',
                (daily.today_str(),)),
            'review_decisions': _count_today(
                conn, 'SELECT COUNT(*) FROM word_reviews WHERE date(created_at) = ?', (today,)),
            'review_rejections': _count_today(
                conn, 'SELECT COUNT(*) FROM word_reviews WHERE verdict = 0 AND date(created_at) = ?', (today,)),
        }
    data['rejected_words'] = len(dictionary.listed_rejected()) + len(dictionary._rejected_voted)
    return data


def live_counters():
    """Az élő számlálók (a szerver memóriájából)."""
    server = srv()
    state = server.state
    rooms = list(state.rooms.values())
    guests = sum(1 for sid in state.player_names if (state.player_auth.get(sid) or {}).get('is_guest', True))
    try:
        sockets = len(server.socketio.server.eio.sockets)
    except Exception:
        sockets = 0
    sockets = max(sockets, len(state.player_names))     # a még be nem mutatkozott kapcsolatokat csak az első adja
    waiting = sum(1 for r in rooms if not r.game.started and not r.game.finished)
    running = [r for r in rooms if r.game.started and not r.game.finished]
    return {
        'online_users': len(state.get_online_user_ids()), 'online_guests': guests, 'sockets': sockets,
        'rooms_total': len(rooms), 'rooms_waiting': waiting,
        'rooms_active': sum(1 for r in running if not r.is_async and not r.is_puzzle),
        'rooms_async': sum(1 for r in rooms if r.is_async), 'rooms_puzzle': sum(1 for r in rooms if r.is_puzzle),
        'rooms_public': sum(1 for r in rooms if not r.is_private), 'rooms_private': sum(1 for r in rooms if r.is_private),
        'games_running': len(running),
        'games_with_bots': sum(1 for r in running if any(p.is_bot for p in r.game.players)),
        'spectators': sum(len(r.spectators) for r in rooms),
        'pending_votes': sum(1 for r in running if r.game.pending_challenge),
        'turn_timers': sum(1 for r in running if r.turn_timer_expires_at),
        'grace_players': len(state._disconnected_players),
    }


def alerts():
    """Figyelmeztető kártyák: [{code, level: 'red'|'yellow', count?, detail?}]."""
    import push_service
    server = srv()
    found = []

    def add(code, level, **extra):
        found.append({'code': code, 'level': level, **extra})

    if not dictionary.is_available():
        add('dictionary_down', 'red')
    if not mail_config.is_configured():
        add('smtp_missing', 'yellow')
    if not push_service.is_available():
        add('push_missing', 'yellow')
    now = time.time()
    stuck = [(r.name, stuck_reason(r, now)) for r in list(server.state.rooms.values())]
    stuck = [(name, reason) for name, reason in stuck if reason]
    if stuck:
        add('stuck_games', 'red', count=len(stuck), detail=', '.join(f'{n}: {r}' for n, r in stuck[:5]))
    overdue = [r for r in list(server.state.rooms.values()) if r.is_async and r.game.started
               and not r.game.finished and r.game.turn_deadline and now > r.game.turn_deadline + 300]
    if overdue:
        add('async_overdue', 'yellow', count=len(overdue))
    window = auth.format_ts(auth.utcnow() - timedelta(seconds=ADMIN_ALERT_WINDOW))
    with auth.transaction() as conn:
        by_ip = conn.execute(
            'SELECT ip, COUNT(*) AS n FROM login_events WHERE success = 0 AND created_at >= ? AND ip IS NOT NULL '
            'GROUP BY ip HAVING n >= 10 ORDER BY n DESC LIMIT 3', (window,)).fetchall()
        by_account = conn.execute(
            "SELECT email, COUNT(*) AS n FROM login_events WHERE success = 0 AND created_at >= ? AND email != '' "
            'GROUP BY email HAVING n >= 5 ORDER BY n DESC LIMIT 3', (window,)).fetchall()
        spike = conn.execute(
            'SELECT user_id, COUNT(*) AS n FROM word_reviews WHERE verdict = 0 AND created_at >= ? '
            'GROUP BY user_id HAVING n >= 25 ORDER BY n DESC LIMIT 3',
            (auth.format_ts(auth.utcnow() - timedelta(minutes=10)),)).fetchall()
    if by_ip or by_account:
        add('failed_logins', 'yellow', count=sum(r['n'] for r in by_ip) + sum(r['n'] for r in by_account),
            detail=', '.join([f"{r['ip']}: {r['n']}" for r in by_ip] + [f"{r['email']}: {r['n']}" for r in by_account]))
    if spike:
        add('review_spike', 'yellow', count=len(spike), detail=', '.join(f"#{r['user_id']}: {r['n']}" for r in spike))
    last_backup = admin_system.last_backup_time()
    if last_backup is None or now - last_backup > 24 * 3600:
        add('backup_old', 'yellow')
    return found


def overview():
    """Az áttekintő oldal teljes tartalma."""
    data = {
        'live': live_counters(), 'today': today_counters(), 'server': admin_system.process_info(),
        'services': admin_system.services_info(), 'alerts': alerts(),
        'maintenance': None, 'generated_at': time.time(),
    }
    data['maintenance'] = settings.maintenance()
    data['versions'] = {'git': admin_system.git_commit(), 'asset_version': _asset_version()}
    return data


def _asset_version():
    import routes
    return routes.asset_version()


def overview_light():
    """A 5 másodpercenként küldött élő számlálók (Socket.IO `admin_overview`)."""
    return {'live': live_counters(), 'server': admin_system.process_info(), 'generated_at': time.time()}


# ===== Grafikonok (utolsó 30 nap) =====

def _day_range(days):
    today = auth.utcnow().date()
    return [(today - timedelta(days=i)).isoformat() for i in range(days - 1, -1, -1)]


def _fill(days, rows, key='day', value='n'):
    mapping = {r[key]: r[value] for r in rows}
    return [{'day': d, 'value': mapping.get(d, 0)} for d in days]


ACTIVITY_SQL = (
    "SELECT date(created_at) AS day, user_id FROM login_events WHERE success = 1 AND user_id IS NOT NULL "
    "UNION SELECT date(sg.updated_at), gp.user_id FROM game_players gp JOIN saved_games sg ON sg.id = gp.game_id "
    "WHERE gp.user_id IS NOT NULL AND sg.status = 'finished' "
    "UNION SELECT puzzle_date, user_id FROM daily_scores WHERE attempts > 0 "
    "UNION SELECT date(created_at), user_id FROM word_reviews"
)


def charts(days=30):
    """Napi bontású idősorok: regisztrációk, aktív felhasználók (DAU), befejezett játékok, napi feladvány."""
    days = admin.int_field(days, 'days', 7, 365)
    span = _day_range(days)
    since = span[0]
    with auth.transaction() as conn:
        registrations = conn.execute(
            'SELECT date(created_at) AS day, COUNT(*) AS n FROM users WHERE date(created_at) >= ? GROUP BY day',
            (since,)).fetchall()
        active = conn.execute(
            f'SELECT day, COUNT(*) AS n FROM ({ACTIVITY_SQL}) WHERE day >= ? GROUP BY day', (since,)).fetchall()
        human = conn.execute(
            "SELECT date(updated_at) AS day, COUNT(*) AS n FROM saved_games WHERE status = 'finished' "
            'AND has_bots = 0 AND date(updated_at) >= ? GROUP BY day', (since,)).fetchall()
        robot = conn.execute(
            "SELECT date(updated_at) AS day, COUNT(*) AS n FROM saved_games WHERE status = 'finished' "
            'AND has_bots = 1 AND date(updated_at) >= ? GROUP BY day', (since,)).fetchall()
        daily = conn.execute(
            'SELECT puzzle_date AS day, COUNT(*) AS n FROM daily_scores WHERE attempts > 0 AND puzzle_date >= ? '
            'GROUP BY day', (since,)).fetchall()
    return {
        'days': span, 'registrations': _fill(span, registrations), 'active_users': _fill(span, active),
        'games_human': _fill(span, human), 'games_bots': _fill(span, robot), 'daily_players': _fill(span, daily),
    }
