import eventlet
eventlet.monkey_patch()

import json
import time

import uuid
import os
import re
import sys
from flask import Flask, request
from flask_socketio import SocketIO, emit, join_room, leave_room

from game import Game, CHALLENGE_TIMEOUT
from room import Room
from config import AUTH_RATE_LIMITS
from auth import (
    init_db, save_game, finish_game, add_game_move,
    load_active_games, abandon_game, abandon_game_by_id,
    is_user_in_game, get_game_by_id,
)
from state import ServerState
from rate_limiter import RateLimiter
from routes import main_bp, auth_bp, game_bp, init_routes
from tunnel import start_tunnel

app = Flask(__name__)
app.config['SECRET_KEY'] = os.environ.get('SECRET_KEY', os.urandom(32).hex())
socketio = SocketIO(app, cors_allowed_origins=[],
                    ping_timeout=120, ping_interval=25)

# Adatbázis inicializálása
init_db()

# --- State & Rate Limiter ---

state = ServerState()

_SOCKET_RATE_LIMITS = {
    'set_name': (5, 10),
    'create_room': (3, 30),
    'join_room': (5, 10),
    'place_tiles': (10, 10),
    'exchange_tiles': (5, 10),
    'pass_turn': (5, 10),
    'get_rooms': (10, 5),
    'accept_words': (5, 10),
    'reject_words': (5, 10),
    'send_chat': (10, 10),
    'rejoin_room': (5, 10),
    'save_game': (3, 30),
    'restore_game': (3, 30),
}

rate_limiter = RateLimiter(_SOCKET_RATE_LIMITS, AUTH_RATE_LIMITS)

# --- Backward compatibility ---
# A tesztek közvetlenül elérik ezeket a dict-eket (server.rooms, server.player_names, stb.)
# ezért globális aliasokat tartunk fenn.
rooms = state.rooms
join_codes = state.join_codes
player_rooms = state.player_rooms
player_names = state.player_names
player_auth = state.player_auth
_reconnect_tokens = state._reconnect_tokens
_sid_to_token = state._sid_to_token
_disconnected_players = state._disconnected_players
_ip_rate_limits = rate_limiter._ip_history

# --- Flask Blueprint regisztráció ---
init_routes(rate_limiter, state, socketio)
app.register_blueprint(main_bp)
app.register_blueprint(auth_bp)
app.register_blueprint(game_bp)

# --- Grace period ---
_DISCONNECT_GRACE_PERIOD = 120

# --- Input validáció ---

_VALID_NAME_RE = re.compile(r'^[\w\sáéíóöőúüűÁÉÍÓÖŐÚÜŰ._-]{1,20}$', re.UNICODE)
_VALID_ROOM_NAME_RE = re.compile(r'^[\w\sáéíóöőúüűÁÉÍÓÖŐÚÜŰ._!?-]{1,30}$', re.UNICODE)

_VALID_LETTERS = frozenset([
    'A', 'Á', 'B', 'C', 'CS', 'D', 'E', 'É', 'F', 'G', 'GY', 'H', 'I', 'Í',
    'J', 'K', 'L', 'LY', 'M', 'N', 'NY', 'O', 'Ó', 'Ö', 'Ő', 'P', 'R', 'S',
    'SZ', 'T', 'TY', 'U', 'Ú', 'Ü', 'Ű', 'V', 'Z', 'ZS',
])


def _sanitize_text(text, pattern, max_len):
    """Szöveg validálása és tisztítása adott regex és max hossz szerint."""
    if not isinstance(text, str):
        return None
    text = text.strip()
    if not text or len(text) > max_len:
        return None
    if not pattern.match(text):
        return None
    return text


def _sanitize_name(name, max_len=20):
    """Játékos név validálása és tisztítása."""
    return _sanitize_text(name, _VALID_NAME_RE, max_len)


def _sanitize_room_name(name, max_len=30):
    """Szoba név validálása és tisztítása."""
    return _sanitize_text(name, _VALID_ROOM_NAME_RE, max_len)


# --- Socket handler helpers ---

def _get_room_context(sid, event_name):
    """Rate limit + szoba keresés a socket handlerekben.
    Visszatér: (room_id, room, game) vagy (None, None, None) hiba esetén.
    """
    if not rate_limiter.check_socket(sid, event_name):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return None, None, None
    return state.get_room_for_player(sid)


def _validate_tiles_input(tiles):
    """Validálja a tiles listát a place_tiles handlerhez.
    Visszatér: (tiles_placed, error_msg) — tiles_placed=None hiba esetén.
    """
    if not isinstance(tiles, list) or len(tiles) == 0 or len(tiles) > 7:
        return None, 'Érvénytelen lerakás.'

    tiles_placed = []
    for t in tiles:
        if not isinstance(t, dict):
            return None, 'Érvénytelen adat.'
        try:
            row = int(t['row'])
            col = int(t['col'])
        except (KeyError, ValueError, TypeError):
            return None, 'Érvénytelen pozíció.'
        if not (0 <= row <= 14 and 0 <= col <= 14):
            return None, 'Pozíció a táblán kívül.'
        letter = t.get('letter', '')
        is_blank = bool(t.get('is_blank', False))
        if not isinstance(letter, str):
            return None, 'Érvénytelen betű.'
        if letter not in _VALID_LETTERS:
            return None, f'Érvénytelen betű: {letter}'
        tiles_placed.append((row, col, letter, is_blank))

    return tiles_placed, None


# --- Room lifecycle helpers ---

def _transfer_ownership(room):
    """Szoba tulajdonjogának átadása az első játékosnak."""
    game = room.game
    first = game.players[0]
    room.transfer_ownership(first.id, first.name)
    emit('room_code', {'code': room.join_code}, room=first.id)


def _emit_all_states(game):
    """Minden játékosnak elküldi a saját állapotát."""
    for player_id, gs in game.get_all_states().items():
        emit('game_state', gs, room=player_id)


def _disband_active_room(room_id, message, owner_sid=None):
    """Aktív játék szoba feloszlatása: broadcast, játékosok kitakarítása, room törlés.

    owner_sid: ha megadva, ezt a SID-t nem küldi room_left-nek (az owner maga kezeli).
    """
    room = state.rooms.get(room_id)
    if not room:
        return
    game = room.game

    socketio.emit('room_disbanded', {'message': message}, room=room_id)

    for p in list(game.players):
        p_sid = p.id
        if p_sid != owner_sid:
            leave_room(room_id, sid=p_sid)
            state.player_rooms.pop(p_sid, None)
            state.cleanup_player_token(p_sid)
            socketio.emit('room_left', {}, room=p_sid)

    state.cleanup_room_tokens(room_id)

    if game.started and not game.finished:
        abandon_game(room_id)
    state.cleanup_room(room_id)


# --- Challenge timer ---

def _start_challenge_timer(room_id):
    """Challenge timeout visszaszámlálás."""
    room = state.rooms.get(room_id)
    if not room:
        return
    timer_id = room.invalidate_challenge_timer()

    def timeout_callback():
        time.sleep(CHALLENGE_TIMEOUT)
        if room_id not in state.rooms:
            return
        r = state.rooms[room_id]
        if r.challenge_timer_id != timer_id:
            return
        game = r.game
        if game.pending_challenge:
            success, result, msg = game.accept_pending()
            for pid, gs in game.get_all_states().items():
                socketio.emit('game_state', gs, room=pid)
            _broadcast_challenge_result(room_id, result, msg)

    socketio.start_background_task(timeout_callback)


def _broadcast_challenge_result(room_id, result, msg):
    """Challenge/szavazás eredmény broadcast (ha vote)."""
    if result in ('vote_accepted', 'vote_rejected'):
        socketio.emit('challenge_result', {
            'challenge_won': result == 'vote_rejected',
            'message': msg,
        }, room=room_id)


def _handle_challenge_result(room_id, room, game, result, msg):
    """Challenge/szavazás eredmény feldolgozása: timer és broadcast."""
    if result in ('accepted', 'vote_accepted', 'vote_rejected'):
        room.invalidate_challenge_timer()
    _emit_all_states(game)
    _broadcast_challenge_result(room_id, result, msg)


# --- Game persistence ---

def _save_game_to_db(room_id):
    """Játék mentése az adatbázisba (manuális mentés)."""
    room = state.rooms.get(room_id)
    if not room:
        return False, "A szoba nem létezik."
    game = room.game
    if not game.started:
        return False, "A játék még nem indult el."

    try:
        state_json = json.dumps(game.to_save_dict(), ensure_ascii=False)
        owner_name = state.player_names.get(room.owner, room.owner_name)

        if game.finished:
            players_data = []
            for p in game.players:
                auth_info = state.player_auth.get(p.id, {})
                players_data.append({
                    'player_name': p.name,
                    'user_id': auth_info.get('user_id'),
                    'final_score': p.score,
                    'is_winner': game.winner and game.winner.name == p.name,
                })
            db_id = finish_game(room_id, state_json, players_data)
            room.db_game_id = db_id
            socketio.emit('rooms_list', state.get_rooms_list())
        else:
            players_data = []
            for p in game.players:
                auth_info = state.player_auth.get(p.id, {})
                players_data.append({
                    'player_name': p.name,
                    'user_id': auth_info.get('user_id'),
                    'score': p.score,
                })
            db_id = save_game(room_id, room.name, state_json, game.challenge_mode,
                              players_data, owner_name=owner_name)
            room.db_game_id = db_id

        # Új lépések mentése
        new_moves = game.move_log[room.last_saved_move_count:]
        for move in new_moves:
            add_game_move(
                db_id, move['move_number'], move['player_name'],
                move['action_type'], move['details_json'],
                move['board_snapshot_json']
            )
        room.last_saved_move_count = len(game.move_log)
        return True, "Játék mentve."
    except Exception as e:
        print(f"[save] Hiba a mentésnél ({room_id}): {e}")
        return False, "Mentési hiba."


def _cleanup_finished_saves():
    """Szerver induláskor befejezett állapotú aktív mentések lezárása."""
    active_games = load_active_games()
    for saved in active_games:
        try:
            s = json.loads(saved['state_json'])
            if s.get('finished', False):
                finish_game(saved['room_id'], saved['state_json'], [])
        except Exception as e:
            print(f"[cleanup] Hiba a mentés lezárásnál (game #{saved['id']}): {e}")


# Backward compat: a tesztek közvetlenül használják
def _check_rate_limit(sid, event):
    return rate_limiter.check_socket(sid, event)


def _check_ip_rate_limit(ip, action):
    return rate_limiter.check_ip(ip, action)


def get_rooms_list():
    return state.get_rooms_list()


# ===== SOCKET.IO EVENTS =====

@socketio.on('connect')
def handle_connect():
    pass


@socketio.on('disconnect')
def handle_disconnect():
    sid = request.sid
    room_id = state.player_rooms.get(sid)
    token = state.get_reconnect_token_for_sid(sid)

    if room_id and room_id in state.rooms:
        room = state.rooms[room_id]
        game = room.game

        if game.started and not game.finished and token:
            # Aktív játék: grace period
            game.mark_disconnected(sid)
            leave_room(room_id)

            state.mark_disconnected(token, sid, room_id,
                                    state.player_names.get(sid, '?'))

            def grace_timeout():
                time.sleep(_DISCONNECT_GRACE_PERIOD)
                if token in state._disconnected_players:
                    _finalize_player_disconnect(token)

            socketio.start_background_task(grace_timeout)

            _emit_all_states(game)
            emit('player_disconnected',
                 {'name': state.player_names.get(sid, '?')}, room=room_id)
            del state.player_rooms[sid]
        else:
            # Nem aktív játék (vagy befejezett): azonnali eltávolítás
            game.remove_player(sid)
            leave_room(room_id)

            if not game.players:
                state.cleanup_room(room_id)
            else:
                if room.owner == sid:
                    _transfer_ownership(room)
                _emit_all_states(game)
                emit('player_left', {'name': state.player_names.get(sid, '?')},
                     room=room_id)

            del state.player_rooms[sid]
            socketio.emit('rooms_list', state.get_rooms_list())

            state.cleanup_player_token(sid)
    else:
        # Nem volt szobában, de lehet tokenje
        if token:
            state.cleanup_player_token(sid)

    state.player_names.pop(sid, None)
    state.player_auth.pop(sid, None)
    rate_limiter.clear_sid(sid)

    emit('rooms_list', state.get_rooms_list(), broadcast=True)


def _finalize_player_disconnect(token):
    """Grace period lejárt: játékos végleges eltávolítása."""
    info = state.finalize_disconnect(token)
    if not info:
        return
    room_id = info['room_id']
    old_sid = info['sid']

    if room_id not in state.rooms:
        return

    room = state.rooms[room_id]
    game = room.game
    is_owner = room.owner == old_sid
    is_active_game = game.started and not game.finished

    # Owner disconnect timeout during active game: disband room
    if is_owner and is_active_game and game.players:
        _disband_active_room(
            room_id,
            'A szoba tulajdonosa végleg lecsatlakozott, a játék véget ért.',
            owner_sid=old_sid,
        )
        socketio.emit('rooms_list', state.get_rooms_list())
        return

    game.remove_player(old_sid)

    if not game.players:
        if game.started and not game.finished:
            abandon_game(room_id)
        state.cleanup_room(room_id)
    else:
        if room.owner == old_sid:
            _transfer_ownership(room)

        for pid, gs in game.get_all_states().items():
            socketio.emit('game_state', gs, room=pid)
        socketio.emit('player_left',
                      {'name': info['player_name']}, room=room_id)

    socketio.emit('rooms_list', state.get_rooms_list())


@socketio.on('set_name')
def handle_set_name(data):
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'set_name'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    if not isinstance(data, dict):
        return
    name = _sanitize_name(data.get('name', ''))
    if not name:
        name = 'Névtelen'

    state.register_player(sid, name, {
        'user_id': data.get('user_id'),
        'is_guest': data.get('is_guest', True),
    })


@socketio.on('rejoin_room')
def handle_rejoin_room(data):
    """Újracsatlakozás egy aktív játékba reconnect tokennel."""
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'rejoin_room'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    if not isinstance(data, dict):
        emit('rejoin_failed', {'message': 'Érvénytelen kérés.'})
        return

    token = data.get('token', '')
    if not isinstance(token, str) or not token:
        emit('rejoin_failed', {'message': 'Hiányzó token.'})
        return

    dc_info = state.get_disconnected_info(token)
    token_info = state.get_token_info(token)

    if not dc_info or not token_info:
        emit('rejoin_failed', {'message': 'Érvénytelen vagy lejárt token.'})
        return

    room_id = dc_info['room_id']
    old_sid = dc_info['sid']

    if room_id not in state.rooms:
        state._disconnected_players.pop(token, None)
        state._reconnect_tokens.pop(token, None)
        emit('rejoin_failed', {'message': 'A szoba már nem létezik.'})
        return

    room = state.rooms[room_id]
    game = room.game

    if not game.replace_player_sid(old_sid, sid):
        emit('rejoin_failed', {'message': 'Nem sikerült újracsatlakozni.'})
        return

    state.complete_rejoin(token, sid)

    if room.owner == old_sid:
        room.owner = sid

    player_name = dc_info['player_name']
    state.player_rooms[sid] = room_id
    state.player_names[sid] = player_name
    join_room(room_id)

    emit('room_joined', {
        'room_id': room_id,
        'room_name': room.name,
        'is_owner': room.owner == sid,
        'challenge_mode': game.challenge_mode,
        'reconnect_token': token,
    })

    if room.owner == sid:
        emit('room_code', {'code': room.join_code})

    _emit_all_states(game)
    emit('player_reconnected', {'name': player_name}, room=room_id)
    emit('rooms_list', state.get_rooms_list(), broadcast=True)


@socketio.on('get_rooms')
def handle_get_rooms():
    if not rate_limiter.check_socket(request.sid, 'get_rooms'):
        return
    emit('rooms_list', state.get_rooms_list())


@socketio.on('create_room')
def handle_create_room(data):
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'create_room'):
        emit('error', {'message': 'Túl sok szoba létrehozás, várj egy kicsit.'})
        return
    if not isinstance(data, dict):
        return

    name = _sanitize_room_name(data.get('name', '')) or 'Szoba'
    try:
        max_players = min(max(int(data.get('max_players', 4)), 2), 4)
    except (ValueError, TypeError):
        max_players = 4
    challenge_mode = bool(data.get('challenge_mode', False))
    is_private = bool(data.get('is_private', False))
    player_name = state.player_names.get(sid, 'Névtelen')

    room_id = str(uuid.uuid4())[:8]
    join_code = state.generate_join_code()
    game = Game(room_id, challenge_mode=challenge_mode)
    game.add_player(sid, player_name)

    room = Room(
        room_id=room_id, game=game, owner_sid=sid, owner_name=player_name,
        name=name, max_players=max_players, join_code=join_code,
        is_private=is_private,
    )
    state.add_room(room)

    state.player_rooms[sid] = room_id
    join_room(room_id)

    token = state.generate_reconnect_token(sid, room_id, player_name)

    emit('room_joined', {
        'room_id': room_id,
        'room_name': name,
        'is_owner': True,
        'challenge_mode': game.challenge_mode,
        'is_private': is_private,
        'reconnect_token': token,
    })
    emit('room_code', {'code': join_code})
    emit('game_state', game.get_state(for_player_id=sid))
    emit('rooms_list', state.get_rooms_list(), broadcast=True)


@socketio.on('join_room')
def handle_join_room(data):
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'join_room'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    if not isinstance(data, dict):
        return

    code = data.get('code', '').strip()
    room_id = data.get('room_id')

    joined_by_code = False
    if code:
        if not re.match(r'^\d{6}$', code):
            emit('error', {'message': 'Érvénytelen kód. 6 számjegyű kódot adj meg.'})
            return
        room_id = state.join_codes.get(code)
        if not room_id:
            emit('error', {'message': 'Nincs ilyen kódú szoba.'})
            return
        joined_by_code = True
    elif room_id:
        if not isinstance(room_id, str) or len(room_id) > 8:
            emit('error', {'message': 'Érvénytelen szoba azonosító.'})
            return
    else:
        emit('error', {'message': 'Szoba kód vagy azonosító szükséges.'})
        return

    if room_id not in state.rooms:
        emit('error', {'message': 'A szoba nem létezik.'})
        return

    room = state.rooms[room_id]
    if room.is_private and not joined_by_code:
        emit('error', {'message': 'Ez egy privát szoba. Csatlakozáshoz kód szükséges.'})
        return

    game = room.game
    player_name = state.player_names.get(sid, 'Névtelen')

    # Restore lobby: csak az elvárt játékosok csatlakozhatnak
    if room.is_restored and hasattr(room, 'expected_players') and room.expected_players:
        if player_name not in room.expected_players:
            emit('error', {'message': 'Csak a mentett játék eredeti játékosai csatlakozhatnak.'})
            return
        # Ellenőrzés: már csatlakozott-e ilyen nevű játékos
        for p in game.players:
            if p.name == player_name:
                emit('error', {'message': 'Ezzel a névvel már csatlakozott valaki.'})
                return

    if game.started:
        emit('error', {'message': 'A játék már elkezdődött.'})
        return
    if len(game.players) >= room.max_players:
        emit('error', {'message': 'A szoba megtelt.'})
        return

    success, msg = game.add_player(sid, player_name)
    if not success:
        emit('error', {'message': msg})
        return

    state.player_rooms[sid] = room_id
    join_room(room_id)

    token = state.generate_reconnect_token(sid, room_id, player_name)

    join_data = {
        'room_id': room_id,
        'room_name': room.name,
        'is_owner': False,
        'challenge_mode': game.challenge_mode,
        'is_private': room.is_private,
        'reconnect_token': token,
    }
    if room.is_restored and hasattr(room, 'expected_players'):
        join_data['is_restore_lobby'] = True
        join_data['expected_players'] = room.expected_players
    emit('room_joined', join_data)

    _emit_all_states(game)

    emit('player_joined', {'name': player_name}, room=room_id)
    emit('rooms_list', state.get_rooms_list(), broadcast=True)


@socketio.on('leave_room')
def handle_leave_room(data=None):
    sid = request.sid
    room_id = state.player_rooms.get(sid)
    if not room_id or room_id not in state.rooms:
        return

    room = state.rooms[room_id]
    game = room.game
    player_name = state.player_names.get(sid, '?')
    is_owner = room.owner == sid
    is_active_game = game.started and not game.finished

    # Owner leaving an active game: kick all players and disband room
    if is_owner and is_active_game:
        _disband_active_room(
            room_id,
            'A szoba tulajdonosa kilépett, a játék véget ért.',
            owner_sid=sid,
        )

        # Remove the owner
        game.remove_player(sid)
        leave_room(room_id)
        del state.player_rooms[sid]
        state.cleanup_player_token(sid)

        emit('room_left', {})
        emit('rooms_list', state.get_rooms_list(), broadcast=True)
        return

    # Normal leave (not owner of active game)
    game.remove_player(sid)
    leave_room(room_id)
    del state.player_rooms[sid]
    state.cleanup_player_token(sid)

    if not game.players:
        state.cleanup_room(room_id)
    else:
        if room.owner == sid:
            _transfer_ownership(room)
        _emit_all_states(game)
        emit('player_left', {'name': player_name}, room=room_id)

    emit('room_left', {})
    emit('rooms_list', state.get_rooms_list(), broadcast=True)


@socketio.on('start_game')
def handle_start_game():
    sid = request.sid
    room_id = state.player_rooms.get(sid)
    if not room_id or room_id not in state.rooms:
        return

    room = state.rooms[room_id]
    if room.owner != sid:
        emit('error', {'message': 'Csak a szoba tulajdonosa indíthatja a játékot.'})
        return

    # Restore lobby: visszaállítás mentett állapotból
    if hasattr(room, 'restore_save_data') and room.restore_save_data:
        save_data = room.restore_save_data
        try:
            s = json.loads(save_data['state_json'])
            restored_game = Game.from_save_dict(s)
            room.db_game_id = save_data['id']

            # Összepárosítás: a várakozó szobában lévő játékosok + mentett játékosok
            joined_names = {}
            for p in room.game.players:
                joined_names[p.name] = p.id  # name -> current SID

            # Mentett játékosok, akik csatlakoztak: SID csere
            for p in restored_game.players:
                if p.name in joined_names:
                    p.id = joined_names[p.name]
                    p.disconnected = False

            # Játékosok akik nem csatlakoztak: eltávolítás, zsetonjaik vissza a zsákba
            missing = [p for p in restored_game.players if p.name not in joined_names]
            for p in missing:
                for tile in p.hand:
                    restored_game.bag.tiles.append(tile)
                restored_game.players.remove(p)

            if not restored_game.players:
                emit('error', {'message': 'Nincs csatlakozott játékos a mentett játékból.'})
                return

            # current_player_idx korrekció
            if restored_game.current_player_idx >= len(restored_game.players):
                restored_game.current_player_idx = 0

            room.game = restored_game
            room.restore_save_data = None
            room.is_restored = False

            # Abandon the old DB save since we're now playing in a new room
            abandon_game_by_id(save_data['id'])

            _emit_all_states(restored_game)
            emit('game_started', {}, room=room_id)
            emit('rooms_list', state.get_rooms_list(), broadcast=True)
        except Exception as e:
            print(f"[restore] Hiba a visszaállításnál: {e}")
            emit('error', {'message': 'Hiba a játék visszaállításánál.'})
        return

    game = room.game
    success, msg = game.start()
    if not success:
        emit('error', {'message': msg})
        return

    _emit_all_states(game)
    emit('game_started', {}, room=room_id)
    emit('rooms_list', state.get_rooms_list(), broadcast=True)


@socketio.on('save_game')
def handle_save_game():
    """Manuális mentés — csak a szoba tulajdonosa használhatja."""
    sid = request.sid
    room_id, room, game = _get_room_context(sid, 'save_game')
    if not room:
        return

    if room.owner != sid:
        emit('action_result', {'success': False, 'message': 'Csak a szoba tulajdonosa menthet.'})
        return

    if not game.started or game.finished:
        emit('action_result', {'success': False, 'message': 'Nincs aktív játék a mentéshez.'})
        return

    success, msg = _save_game_to_db(room_id)
    emit('action_result', {'success': success, 'message': msg})


@socketio.on('restore_game')
def handle_restore_game(data):
    """Mentett játék visszaállítása: várakozó szoba létrehozása."""
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'restore_game'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    if not isinstance(data, dict):
        return

    game_id = data.get('game_id')
    if not isinstance(game_id, int):
        emit('error', {'message': 'Érvénytelen játék azonosító.'})
        return

    # Auth ellenőrzés
    auth_info = state.player_auth.get(sid, {})
    user_id = auth_info.get('user_id')
    if not user_id:
        emit('error', {'message': 'Csak regisztrált felhasználók állíthatnak vissza játékot.'})
        return

    # Játék lekérdezés
    game_row = get_game_by_id(game_id)
    if not game_row:
        emit('error', {'message': 'Mentett játék nem található.'})
        return
    if game_row['status'] != 'active':
        emit('error', {'message': 'A mentett játék nem aktív.'})
        return

    # Ellenőrzés: a hívó játékos részese-e a játéknak
    if not is_user_in_game(game_id, user_id):
        emit('error', {'message': 'Nem vagy részese ennek a mentett játéknak.'})
        return

    # Owner ellenőrzés: csak az eredeti owner állíthat vissza
    player_name = state.player_names.get(sid, 'Névtelen')
    saved_owner = game_row.get('owner_name', '')
    if saved_owner and saved_owner != player_name:
        emit('error', {'message': 'Csak a mentés tulajdonosa állíthatja vissza a játékot.'})
        return

    # Parse state to get expected players
    try:
        s = json.loads(game_row['state_json'])
    except Exception:
        emit('error', {'message': 'Hibás mentett állapot.'})
        return

    expected_players = [p['name'] for p in s.get('players', [])]
    if not expected_players:
        emit('error', {'message': 'Nincs játékos a mentett játékban.'})
        return

    # Várakozó szoba létrehozása
    room_id = str(uuid.uuid4())[:8]
    join_code = state.generate_join_code()
    new_game = Game(room_id, challenge_mode=s.get('challenge_mode', False))
    new_game.add_player(sid, player_name)

    room = Room(
        room_id=room_id, game=new_game, owner_sid=sid, owner_name=player_name,
        name=game_row.get('room_name', '') or 'Visszaállított szoba',
        max_players=max(len(expected_players), 2),
        join_code=join_code, is_private=True,
    )
    room.is_restored = True
    room.restore_save_data = game_row
    room.expected_players = expected_players

    state.add_room(room)

    state.player_rooms[sid] = room_id
    join_room(room_id)

    token = state.generate_reconnect_token(sid, room_id, player_name)

    emit('room_joined', {
        'room_id': room_id,
        'room_name': room.name,
        'is_owner': True,
        'challenge_mode': new_game.challenge_mode,
        'is_private': True,
        'reconnect_token': token,
        'is_restore_lobby': True,
        'expected_players': expected_players,
    })
    emit('room_code', {'code': join_code})
    emit('game_state', new_game.get_state(for_player_id=sid))
    emit('rooms_list', state.get_rooms_list(), broadcast=True)


@socketio.on('place_tiles')
def handle_place_tiles(data):
    sid = request.sid
    room_id, room, game = _get_room_context(sid, 'place_tiles')
    if not room:
        return
    if not isinstance(data, dict):
        return

    tiles_placed, err = _validate_tiles_input(data.get('tiles', []))
    if not tiles_placed:
        emit('action_result', {'success': False, 'message': err})
        return

    success, msg, score = game.place_tiles(sid, tiles_placed)

    if success:
        _emit_all_states(game)
        emit('action_result', {'success': True, 'message': msg, 'score': score})
        if game.pending_challenge:
            _start_challenge_timer(room_id)
        if game.finished:
            _save_game_to_db(room_id)
    else:
        emit('action_result', {'success': False, 'message': msg})


@socketio.on('exchange_tiles')
def handle_exchange_tiles(data):
    sid = request.sid
    room_id, room, game = _get_room_context(sid, 'exchange_tiles')
    if not room:
        return
    if not isinstance(data, dict):
        return

    indices = data.get('indices', [])
    if not isinstance(indices, list) or len(indices) > 7:
        emit('action_result', {'success': False, 'message': 'Érvénytelen csere.'})
        return
    try:
        indices = [int(i) for i in indices]
    except (ValueError, TypeError):
        emit('action_result', {'success': False, 'message': 'Érvénytelen index.'})
        return

    success, msg = game.exchange_tiles(sid, indices)

    if success:
        _emit_all_states(game)
        emit('action_result', {'success': True, 'message': msg})
    else:
        emit('action_result', {'success': False, 'message': msg})


@socketio.on('pass_turn')
def handle_pass_turn():
    sid = request.sid
    room_id, room, game = _get_room_context(sid, 'pass_turn')
    if not room:
        return

    success, msg = game.pass_turn(sid)

    if success:
        _emit_all_states(game)
        emit('action_result', {'success': True, 'message': msg})
        if game.finished:
            _save_game_to_db(room_id)
    else:
        emit('action_result', {'success': False, 'message': msg})


@socketio.on('accept_words')
def handle_accept_words():
    sid = request.sid
    room_id, room, game = _get_room_context(sid, 'accept_words')
    if not room:
        return

    success, result, msg = game.accept_pending_by_player(sid)
    if success:
        _handle_challenge_result(room_id, room, game, result, msg)
    else:
        emit('action_result', {'success': False, 'message': msg})


@socketio.on('reject_words')
def handle_reject_words():
    sid = request.sid
    room_id, room, game = _get_room_context(sid, 'reject_words')
    if not room:
        return

    success, result, msg = game.reject_pending_by_player(sid)
    if success:
        _handle_challenge_result(room_id, room, game, result, msg)
    else:
        emit('action_result', {'success': False, 'message': msg})


@socketio.on('send_chat')
def handle_send_chat(data):
    sid = request.sid
    room_id, room, game = _get_room_context(sid, 'send_chat')
    if not room:
        return
    if not isinstance(data, dict):
        return

    message = data.get('message', '')
    if not isinstance(message, str):
        return
    message = message.strip()
    if not message or len(message) > 200:
        return

    player_name = state.player_names.get(sid, '?')
    chat_msg = {'name': player_name, 'message': message}
    room.add_chat_message(player_name, message)

    emit('chat_message', chat_msg, room=room_id)


# ===== Main =====

if __name__ == '__main__':
    from auth import cleanup_expired

    port = int(os.environ.get('PORT', 5000))
    use_tunnel = '--no-tunnel' not in sys.argv

    cleanup_expired()
    _cleanup_finished_saves()

    if use_tunnel:
        start_tunnel(port)

    socketio.run(app, host='0.0.0.0', port=port, debug=False,
                 use_reloader=False)
