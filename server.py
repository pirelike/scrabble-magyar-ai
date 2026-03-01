import eventlet
eventlet.monkey_patch()

import json
import time

import uuid
import os
import subprocess
import shutil
import threading
import re
import secrets
import signal
import sys
import time
import atexit
from collections import defaultdict
from flask import Flask, render_template, request, jsonify, make_response
from flask_socketio import SocketIO, emit, join_room, leave_room

from game import Game, CHALLENGE_TIMEOUT
from room import Room, generate_join_code
from config import AUTH_RATE_LIMITS, SMTP_CONFIGURED
from auth import init_db, get_user_by_email, create_user, verify_password, \
    create_verification_code, verify_code, create_session, validate_session, \
    delete_session, cleanup_expired, save_game, finish_game, add_game_move, \
    load_active_games, get_game_moves, get_user_game_history, get_game_by_id, \
    abandon_game, get_user_active_games, abandon_game_by_id, is_user_in_game, \
    get_game_players
from email_service import send_verification_email

app = Flask(__name__)
app.config['SECRET_KEY'] = os.environ.get('SECRET_KEY', os.urandom(32).hex())
socketio = SocketIO(app, cors_allowed_origins=[],
                    ping_timeout=120, ping_interval=25)

# Adatbázis inicializálása
init_db()

# --- State ---

# Aktív szobák: {room_id: Room}
rooms = {}
# join_code -> room_id mapping (gyors keresés)
join_codes = {}
# Játékos -> szoba mapping: {sid: room_id}
player_rooms = {}
# Játékos nevek: {sid: name}
player_names = {}
# Játékos auth info: {sid: {user_id, is_guest}}
player_auth = {}
# Reconnect tokenek: {token: {room_id, player_name, sid}}
_reconnect_tokens = {}
# Sid -> reconnect token mapping: {sid: token}
_sid_to_token = {}
# Disconnected játékosok grace period: {token: {room_id, sid, player_name}}
_disconnected_players = {}
# Grace period időtartam (mp)
_DISCONNECT_GRACE_PERIOD = 120

# --- Rate limiting ---

# {sid: {event_name: [timestamp, ...]}}
_rate_limits = defaultdict(lambda: defaultdict(list))
_RATE_LIMITS = {
    'set_name': (5, 10),
    'create_room': (3, 30),
    'join_room': (5, 10),
    'place_tiles': (10, 10),
    'exchange_tiles': (5, 10),
    'pass_turn': (5, 10),
    'get_rooms': (10, 5),
    'challenge': (5, 10),
    'accept_words': (5, 10),
    'reject_words': (5, 10),
    'cast_vote': (5, 10),
    'send_chat': (10, 10),
    'rejoin_room': (5, 10),
    'save_game': (3, 30),
    'restore_game': (3, 30),
}

# IP-alapú rate limiting az auth endpointokra
_ip_rate_limits = defaultdict(lambda: defaultdict(list))


def _check_rate_limit(sid, event):
    """Ellenőrzi, hogy a játékos túllépte-e a rate limitet. True = engedélyezve."""
    if event not in _RATE_LIMITS:
        return True
    max_requests, window = _RATE_LIMITS[event]
    now = time.time()
    timestamps = _rate_limits[sid][event]
    _rate_limits[sid][event] = [t for t in timestamps if now - t < window]
    if len(_rate_limits[sid][event]) >= max_requests:
        return False
    _rate_limits[sid][event].append(now)
    return True


def _check_ip_rate_limit(ip, action):
    """IP-alapú rate limiting az auth endpointokra. True = engedélyezve."""
    if action not in AUTH_RATE_LIMITS:
        return True
    max_requests, window = AUTH_RATE_LIMITS[action]
    now = time.time()
    timestamps = _ip_rate_limits[ip][action]
    _ip_rate_limits[ip][action] = [t for t in timestamps if now - t < window]
    if not _ip_rate_limits[ip][action] and not any(_ip_rate_limits[ip].values()):
        del _ip_rate_limits[ip]
        return True
    if len(_ip_rate_limits[ip][action]) >= max_requests:
        return False
    _ip_rate_limits[ip][action].append(now)
    return True


# --- Input validáció ---

_VALID_NAME_RE = re.compile(r'^[\w\sáéíóöőúüűÁÉÍÓÖŐÚÜŰ._-]{1,20}$', re.UNICODE)
_VALID_ROOM_NAME_RE = re.compile(r'^[\w\sáéíóöőúüűÁÉÍÓÖŐÚÜŰ._!?-]{1,30}$', re.UNICODE)
_VALID_EMAIL_RE = re.compile(r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$')

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


def _generate_reconnect_token(sid, room_id, player_name):
    """Reconnect token generálása és mentése."""
    token = secrets.token_urlsafe(24)
    _reconnect_tokens[token] = {
        'room_id': room_id,
        'player_name': player_name,
        'sid': sid,
    }
    _sid_to_token[sid] = token
    return token


def _get_client_ip():
    """Kliens IP cím lekérése (proxy mögötti is)."""
    return request.headers.get('X-Forwarded-For', request.remote_addr or '127.0.0.1').split(',')[0].strip()


def _set_session_cookie(response, token):
    """Session cookie beállítása a response-on."""
    response.set_cookie(
        'session_token',
        token,
        httponly=True,
        samesite='Lax',
        secure=request.is_secure,
        max_age=30 * 24 * 3600,
    )
    return response


# --- Socket handler helpers ---

def _get_room_context(sid, event_name):
    """Rate limit + szoba keresés a socket handlerekben.
    Visszatér: (room_id, room, game) vagy (None, None, None) hiba esetén.
    """
    if not _check_rate_limit(sid, event_name):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return None, None, None
    room_id = player_rooms.get(sid)
    if not room_id or room_id not in rooms:
        return None, None, None
    room = rooms[room_id]
    return room_id, room, room.game


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


# --- Room lifecycle ---

def _cleanup_room(room_id):
    """Szoba erőforrásainak felszabadítása."""
    room = rooms.get(room_id)
    if room and room.join_code in join_codes:
        del join_codes[room.join_code]
    # Disconnected játékosok és tokenek törlése a szobához
    tokens_to_remove = [
        t for t, info in _disconnected_players.items()
        if info['room_id'] == room_id
    ]
    for t in tokens_to_remove:
        dc = _disconnected_players.pop(t)
        _sid_to_token.pop(dc['sid'], None)
        _reconnect_tokens.pop(t, None)
    del rooms[room_id]


def _transfer_ownership(room):
    """Szoba tulajdonjogának átadása az első játékosnak."""
    game = room.game
    first = game.players[0]
    room.transfer_ownership(first.id, first.name)
    emit('room_code', {'code': room.join_code}, room=first.id)


def _emit_all_states(game):
    """Minden játékosnak elküldi a saját állapotát."""
    for player_id, state in game.get_all_states().items():
        emit('game_state', state, room=player_id)


# --- Challenge timer ---

def _start_challenge_timer(room_id):
    """Challenge timeout visszaszámlálás."""
    room = rooms.get(room_id)
    if not room:
        return
    timer_id = room.invalidate_challenge_timer()

    def timeout_callback():
        time.sleep(CHALLENGE_TIMEOUT)
        if room_id not in rooms:
            return
        r = rooms[room_id]
        if r.challenge_timer_id != timer_id:
            return
        game = r.game
        if game.pending_challenge:
            success, result, msg = game.accept_pending()
            for pid, state in game.get_all_states().items():
                socketio.emit('game_state', state, room=pid)
            if result in ('vote_accepted', 'vote_rejected'):
                socketio.emit('challenge_result', {
                    'challenge_won': result == 'vote_rejected',
                    'message': msg,
                }, room=room_id)

    socketio.start_background_task(timeout_callback)


def _save_game_to_db(room_id):
    """Játék mentése az adatbázisba (manuális mentés)."""
    room = rooms.get(room_id)
    if not room:
        return False, "A szoba nem létezik."
    game = room.game
    if not game.started:
        return False, "A játék még nem indult el."

    try:
        state_json = json.dumps(game.to_save_dict(), ensure_ascii=False)
        owner_name = player_names.get(room.owner, room.owner_name)

        if game.finished:
            players_data = []
            for p in game.players:
                auth_info = player_auth.get(p.id, {})
                players_data.append({
                    'player_name': p.name,
                    'user_id': auth_info.get('user_id'),
                    'final_score': p.score,
                    'is_winner': game.winner and game.winner.name == p.name,
                })
            db_id = finish_game(room_id, state_json, players_data)
            game._db_game_id = db_id
            socketio.emit('rooms_list', get_rooms_list())
        else:
            players_data = []
            for p in game.players:
                auth_info = player_auth.get(p.id, {})
                players_data.append({
                    'player_name': p.name,
                    'user_id': auth_info.get('user_id'),
                    'score': p.score,
                })
            db_id = save_game(room_id, room.name, state_json, game.challenge_mode,
                              players_data, owner_name=owner_name)
            game._db_game_id = db_id

        # Új lépések mentése
        new_moves = game.move_log[game._last_saved_move_count:]
        for move in new_moves:
            add_game_move(
                db_id, move['move_number'], move['player_name'],
                move['action_type'], move['details_json'],
                move['board_snapshot_json']
            )
        game._last_saved_move_count = len(game.move_log)
        return True, "Játék mentve."
    except Exception as e:
        print(f"[save] Hiba a mentésnél ({room_id}): {e}")
        return False, "Mentési hiba."


def _cleanup_finished_saves():
    """Szerver induláskor befejezett állapotú aktív mentések lezárása."""
    active_games = load_active_games()
    for saved in active_games:
        try:
            state = json.loads(saved['state_json'])
            if state.get('finished', False):
                finish_game(saved['room_id'], saved['state_json'], [])
        except Exception as e:
            print(f"[cleanup] Hiba a mentés lezárásnál (game #{saved['id']}): {e}")


def _handle_challenge_result(room_id, room, game, result, msg):
    """Challenge/szavazás eredmény feldolgozása: timer és broadcast."""
    if result in ('accepted', 'vote_accepted', 'vote_rejected'):
        room.invalidate_challenge_timer()
    _emit_all_states(game)
    if result in ('vote_accepted', 'vote_rejected'):
        emit('challenge_result', {
            'challenge_won': result == 'vote_rejected',
            'message': msg,
        }, room=room_id)


# ===== AUTH HTTP ROUTES =====

@app.route('/api/auth/request-code', methods=['POST'])
def auth_request_code():
    ip = _get_client_ip()
    if not _check_ip_rate_limit(ip, 'request_code'):
        return jsonify({'success': False, 'message': 'Túl sok kérés. Próbáld újra 5 perc múlva.'}), 429

    data = request.get_json(silent=True)
    if not data or not isinstance(data.get('email'), str):
        return jsonify({'success': False, 'message': 'Email cím megadása kötelező.'}), 400

    email = data['email'].strip().lower()
    if not _VALID_EMAIL_RE.match(email) or len(email) > 254:
        return jsonify({'success': False, 'message': 'Érvénytelen email cím.'}), 400

    if get_user_by_email(email):
        return jsonify({'success': False, 'message': 'Ez az email cím már regisztrálva van.'}), 409

    code = create_verification_code(email)
    send_verification_email(email, code)

    response = {'success': True, 'message': 'Verifikációs kód elküldve.'}
    if not SMTP_CONFIGURED:
        response['dev_code'] = code
        response['message'] = 'Fejlesztői mód: SMTP nincs konfigurálva.'
    return jsonify(response)


@app.route('/api/auth/verify-code', methods=['POST'])
def auth_verify_code():
    data = request.get_json(silent=True)
    if not data:
        return jsonify({'success': False, 'message': 'Érvénytelen kérés.'}), 400

    email = data.get('email', '').strip().lower()
    code = data.get('code', '').strip()

    if not email or not code:
        return jsonify({'success': False, 'message': 'Email és kód megadása kötelező.'}), 400

    if not re.match(r'^\d{6}$', code):
        return jsonify({'success': False, 'message': 'A kód 6 számjegyből áll.'}), 400

    success, message = verify_code(email, code)
    return jsonify({'success': success, 'message': message}), (200 if success else 400)


@app.route('/api/auth/register', methods=['POST'])
def auth_register():
    ip = _get_client_ip()
    if not _check_ip_rate_limit(ip, 'register'):
        return jsonify({'success': False, 'message': 'Túl sok regisztráció. Próbáld újra később.'}), 429

    data = request.get_json(silent=True)
    if not data:
        return jsonify({'success': False, 'message': 'Érvénytelen kérés.'}), 400

    email = data.get('email', '').strip().lower()
    password = data.get('password', '')
    display_name = data.get('display_name', '').strip()

    if not email or not password or not display_name:
        return jsonify({'success': False, 'message': 'Minden mező kitöltése kötelező.'}), 400

    if not _VALID_EMAIL_RE.match(email) or len(email) > 254:
        return jsonify({'success': False, 'message': 'Érvénytelen email cím.'}), 400

    if len(password) < 6:
        return jsonify({'success': False, 'message': 'A jelszó legalább 6 karakter legyen.'}), 400

    if len(password) > 128:
        return jsonify({'success': False, 'message': 'A jelszó maximum 128 karakter lehet.'}), 400

    name = _sanitize_name(display_name)
    if not name:
        return jsonify({'success': False, 'message': 'Érvénytelen megjelenítési név (1-20 karakter, betűk és számok).'}), 400

    success, result = create_user(email, name, password)
    if not success:
        return jsonify({'success': False, 'message': result}), 409

    user_id = result
    token = create_session(user_id)

    resp = make_response(jsonify({
        'success': True,
        'message': 'Fiók létrehozva!',
        'user': {
            'id': user_id,
            'email': email,
            'display_name': name,
        }
    }))
    return _set_session_cookie(resp, token)


@app.route('/api/auth/login', methods=['POST'])
def auth_login():
    ip = _get_client_ip()
    if not _check_ip_rate_limit(ip, 'login'):
        return jsonify({'success': False, 'message': 'Túl sok bejelentkezési kísérlet. Próbáld újra 5 perc múlva.'}), 429

    data = request.get_json(silent=True)
    if not data:
        return jsonify({'success': False, 'message': 'Érvénytelen kérés.'}), 400

    email = data.get('email', '').strip().lower()
    password = data.get('password', '')

    if not email or not password:
        return jsonify({'success': False, 'message': 'Email és jelszó megadása kötelező.'}), 400

    success, result = verify_password(email, password)
    if not success:
        return jsonify({'success': False, 'message': result}), 401

    user = result
    token = create_session(user['id'])

    resp = make_response(jsonify({
        'success': True,
        'message': 'Sikeres bejelentkezés!',
        'user': {
            'id': user['id'],
            'email': user['email'],
            'display_name': user['display_name'],
        }
    }))
    return _set_session_cookie(resp, token)


@app.route('/api/auth/logout', methods=['POST'])
def auth_logout():
    token = request.cookies.get('session_token')
    if token:
        delete_session(token)
    resp = make_response(jsonify({'success': True, 'message': 'Kijelentkezve.'}))
    resp.delete_cookie('session_token')
    return resp


@app.route('/api/auth/me', methods=['GET'])
def auth_me():
    token = request.cookies.get('session_token')
    user = validate_session(token)
    if not user:
        return jsonify({'success': False, 'message': 'Nincs érvényes session.'}), 401

    return jsonify({
        'success': True,
        'user': {
            'id': user['id'],
            'email': user['email'],
            'display_name': user['display_name'],
            'games_played': user['games_played'],
            'games_won': user['games_won'],
            'total_score': user['total_score'],
        }
    })


@app.route('/api/auth/profile', methods=['GET'])
def auth_profile():
    token = request.cookies.get('session_token')
    user = validate_session(token)
    if not user:
        return jsonify({'success': False, 'message': 'Nincs érvényes session.'}), 401

    games_played = user['games_played']
    games_won = user['games_won']
    total_score = user['total_score']
    win_rate = round(games_won / games_played * 100, 1) if games_played > 0 else 0
    avg_score = round(total_score / games_played, 1) if games_played > 0 else 0

    history = get_user_game_history(user['id'])

    return jsonify({
        'success': True,
        'stats': {
            'games_played': games_played,
            'games_won': games_won,
            'win_rate': win_rate,
            'avg_score': avg_score,
            'total_score': total_score,
        },
        'history': [
            {
                'game_id': h['game_id'],
                'room_name': h['room_name'],
                'created_at': h['created_at'],
                'final_score': h['final_score'],
                'is_winner': bool(h['is_winner']),
                'opponents': h['opponents'],
            }
            for h in history
        ],
    })


@app.route('/api/game/<int:game_id>/moves', methods=['GET'])
def game_moves_api(game_id):
    game_row = get_game_by_id(game_id)
    if not game_row:
        return jsonify({'success': False, 'message': 'Játék nem található.'}), 404

    moves = get_game_moves(game_id)
    return jsonify({
        'success': True,
        'moves': [
            {
                'move_number': m['move_number'],
                'player_name': m['player_name'],
                'action_type': m['action_type'],
                'details_json': m['details_json'],
                'board_snapshot_json': m['board_snapshot_json'],
            }
            for m in moves
        ],
    })


@app.route('/api/auth/saved-games', methods=['GET'])
def saved_games_api():
    token = request.cookies.get('session_token')
    if not token:
        return jsonify({'success': False, 'message': 'Nincs session.'}), 401
    user = validate_session(token)
    if not user:
        return jsonify({'success': False, 'message': 'Nincs érvényes session.'}), 401

    games = get_user_active_games(user['id'])
    return jsonify({
        'success': True,
        'games': [
            {
                'game_id': g['game_id'],
                'room_name': g['room_name'],
                'room_id': g['room_id'],
                'created_at': g['created_at'],
                'updated_at': g['updated_at'],
                'challenge_mode': bool(g['challenge_mode']),
                'player_name': g['player_name'],
                'score': g['final_score'],
                'opponents': g['opponents'],
                'owner_name': g.get('owner_name', ''),
                'is_owner': g.get('owner_name', '') == user['display_name'],
            }
            for g in games
        ],
    })


@app.route('/api/game/<int:game_id>/abandon', methods=['POST'])
def abandon_game_api(game_id):
    token = request.cookies.get('session_token')
    if not token:
        return jsonify({'success': False, 'message': 'Nincs session.'}), 401
    user = validate_session(token)
    if not user:
        return jsonify({'success': False, 'message': 'Nincs érvényes session.'}), 401

    game_row = get_game_by_id(game_id)
    if not game_row:
        return jsonify({'success': False, 'message': 'Játék nem található.'}), 404
    if game_row['status'] != 'active':
        return jsonify({'success': False, 'message': 'A játék nem aktív.'}), 400

    if not is_user_in_game(game_id, user['id']):
        return jsonify({'success': False, 'message': 'Nincs jogosultságod ehhez a játékhoz.'}), 403

    # Clean up in-memory room if it exists
    room_id = game_row['room_id']
    if room_id in rooms:
        room = rooms[room_id]
        code = room.join_code
        if code in join_codes:
            del join_codes[code]
        del rooms[room_id]
        # Broadcast updated room list
        emit_func = getattr(socketio, 'emit', None)
        if emit_func:
            emit_func('rooms_list', get_rooms_list())

    abandon_game_by_id(game_id)
    return jsonify({'success': True})


# ===== ROUTES =====

@app.route('/')
def index():
    return render_template('index.html')


def get_rooms_list():
    """Nyilvános szobák listája a lobby számára (restored szobák kiszűrve)."""
    return [
        room.to_lobby_dict()
        for room in rooms.values()
        if not room.is_private and not room.is_restored and not room.game.finished
    ]


# ===== SOCKET.IO EVENTS =====

@socketio.on('connect')
def handle_connect():
    pass


@socketio.on('disconnect')
def handle_disconnect():
    sid = request.sid
    room_id = player_rooms.get(sid)
    token = _sid_to_token.get(sid)

    if room_id and room_id in rooms:
        room = rooms[room_id]
        game = room.game

        if game.started and not game.finished and token:
            # Aktív játék: grace period
            game.mark_disconnected(sid)
            leave_room(room_id)

            _disconnected_players[token] = {
                'room_id': room_id,
                'sid': sid,
                'player_name': player_names.get(sid, '?'),
            }

            def grace_timeout():
                time.sleep(_DISCONNECT_GRACE_PERIOD)
                if token in _disconnected_players:
                    _finalize_player_disconnect(token)

            socketio.start_background_task(grace_timeout)

            _emit_all_states(game)
            emit('player_disconnected',
                 {'name': player_names.get(sid, '?')}, room=room_id)
            del player_rooms[sid]
        else:
            # Nem aktív játék (vagy befejezett): azonnali eltávolítás
            game.remove_player(sid)
            leave_room(room_id)

            if not game.players:
                _cleanup_room(room_id)
            else:
                if room.owner == sid:
                    _transfer_ownership(room)
                _emit_all_states(game)
                emit('player_left', {'name': player_names.get(sid, '?')},
                     room=room_id)

            del player_rooms[sid]
            socketio.emit('rooms_list', get_rooms_list())

            if token:
                _reconnect_tokens.pop(token, None)
                _sid_to_token.pop(sid, None)
    else:
        if token:
            _reconnect_tokens.pop(token, None)
            _sid_to_token.pop(sid, None)

    player_names.pop(sid, None)
    player_auth.pop(sid, None)
    _rate_limits.pop(sid, None)

    emit('rooms_list', get_rooms_list(), broadcast=True)


def _finalize_player_disconnect(token):
    """Grace period lejárt: játékos végleges eltávolítása."""
    info = _disconnected_players.pop(token, None)
    if not info:
        return
    room_id = info['room_id']
    old_sid = info['sid']

    _reconnect_tokens.pop(token, None)
    _sid_to_token.pop(old_sid, None)

    if room_id not in rooms:
        return

    room = rooms[room_id]
    game = room.game
    is_owner = room.owner == old_sid
    is_active_game = game.started and not game.finished

    # Owner disconnect timeout during active game: disband room
    if is_owner and is_active_game and game.players:
        socketio.emit('room_disbanded', {
            'message': 'A szoba tulajdonosa végleg lecsatlakozott, a játék véget ért.',
        }, room=room_id)

        # Clean up all remaining players
        for p in list(game.players):
            p_sid = p.id
            if p_sid != old_sid:
                player_rooms.pop(p_sid, None)
                p_token = _sid_to_token.pop(p_sid, None)
                if p_token:
                    _reconnect_tokens.pop(p_token, None)
                    _disconnected_players.pop(p_token, None)
                socketio.emit('room_left', {}, room=p_sid)

        # Also clean up other disconnected players in this room
        tokens_to_remove = [
            t for t, dc_info in _disconnected_players.items()
            if dc_info['room_id'] == room_id
        ]
        for t in tokens_to_remove:
            dc = _disconnected_players.pop(t)
            _sid_to_token.pop(dc['sid'], None)
            _reconnect_tokens.pop(t, None)

        if is_active_game:
            abandon_game(room_id)
        _cleanup_room(room_id)
        socketio.emit('rooms_list', get_rooms_list())
        return

    game.remove_player(old_sid)

    if not game.players:
        if game.started and not game.finished:
            abandon_game(room_id)
        _cleanup_room(room_id)
    else:
        if room.owner == old_sid:
            _transfer_ownership(room)

        for pid, state in game.get_all_states().items():
            socketio.emit('game_state', state, room=pid)
        socketio.emit('player_left',
                      {'name': info['player_name']}, room=room_id)

    socketio.emit('rooms_list', get_rooms_list())


@socketio.on('set_name')
def handle_set_name(data):
    sid = request.sid
    if not _check_rate_limit(sid, 'set_name'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    if not isinstance(data, dict):
        return
    name = _sanitize_name(data.get('name', ''))
    if not name:
        name = 'Névtelen'

    player_names[sid] = name
    player_auth[sid] = {
        'user_id': data.get('user_id'),
        'is_guest': data.get('is_guest', True),
    }


@socketio.on('rejoin_room')
def handle_rejoin_room(data):
    """Újracsatlakozás egy aktív játékba reconnect tokennel."""
    sid = request.sid
    if not _check_rate_limit(sid, 'rejoin_room'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    if not isinstance(data, dict):
        emit('rejoin_failed', {'message': 'Érvénytelen kérés.'})
        return

    token = data.get('token', '')
    if not isinstance(token, str) or not token:
        emit('rejoin_failed', {'message': 'Hiányzó token.'})
        return

    dc_info = _disconnected_players.get(token)
    token_info = _reconnect_tokens.get(token)

    if not dc_info or not token_info:
        emit('rejoin_failed', {'message': 'Érvénytelen vagy lejárt token.'})
        return

    room_id = dc_info['room_id']
    old_sid = dc_info['sid']

    if room_id not in rooms:
        _disconnected_players.pop(token, None)
        _reconnect_tokens.pop(token, None)
        emit('rejoin_failed', {'message': 'A szoba már nem létezik.'})
        return

    room = rooms[room_id]
    game = room.game

    if not game.replace_player_sid(old_sid, sid):
        emit('rejoin_failed', {'message': 'Nem sikerült újracsatlakozni.'})
        return

    del _disconnected_players[token]
    _reconnect_tokens[token]['sid'] = sid
    _sid_to_token.pop(old_sid, None)
    _sid_to_token[sid] = token

    if room.owner == old_sid:
        room.owner = sid

    player_name = dc_info['player_name']
    player_rooms[sid] = room_id
    player_names[sid] = player_name
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
    emit('rooms_list', get_rooms_list(), broadcast=True)


@socketio.on('get_rooms')
def handle_get_rooms():
    if not _check_rate_limit(request.sid, 'get_rooms'):
        return
    emit('rooms_list', get_rooms_list())


@socketio.on('create_room')
def handle_create_room(data):
    sid = request.sid
    if not _check_rate_limit(sid, 'create_room'):
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
    player_name = player_names.get(sid, 'Névtelen')

    room_id = str(uuid.uuid4())[:8]
    join_code = generate_join_code(join_codes)
    game = Game(room_id, challenge_mode=challenge_mode)
    game.add_player(sid, player_name)

    room = Room(
        room_id=room_id, game=game, owner_sid=sid, owner_name=player_name,
        name=name, max_players=max_players, join_code=join_code,
        is_private=is_private,
    )
    rooms[room_id] = room
    join_codes[join_code] = room_id

    player_rooms[sid] = room_id
    join_room(room_id)

    token = _generate_reconnect_token(sid, room_id, player_name)

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
    emit('rooms_list', get_rooms_list(), broadcast=True)


@socketio.on('join_room')
def handle_join_room(data):
    sid = request.sid
    if not _check_rate_limit(sid, 'join_room'):
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
        room_id = join_codes.get(code)
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

    if room_id not in rooms:
        emit('error', {'message': 'A szoba nem létezik.'})
        return

    room = rooms[room_id]
    if room.is_private and not joined_by_code:
        emit('error', {'message': 'Ez egy privát szoba. Csatlakozáshoz kód szükséges.'})
        return

    game = room.game
    player_name = player_names.get(sid, 'Névtelen')

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

    player_rooms[sid] = room_id
    join_room(room_id)

    token = _generate_reconnect_token(sid, room_id, player_name)

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
    emit('rooms_list', get_rooms_list(), broadcast=True)


@socketio.on('leave_room')
def handle_leave_room(data=None):
    sid = request.sid
    room_id = player_rooms.get(sid)
    if not room_id or room_id not in rooms:
        return

    room = rooms[room_id]
    game = room.game
    player_name = player_names.get(sid, '?')
    is_owner = room.owner == sid
    is_active_game = game.started and not game.finished

    # Owner leaving an active game: kick all players and disband room
    if is_owner and is_active_game:
        # Notify all other players that the room is disbanded
        emit('room_disbanded', {
            'message': 'A szoba tulajdonosa kilépett, a játék véget ért.',
        }, room=room_id)

        # Remove all players from the room
        for p in list(game.players):
            p_sid = p.id
            if p_sid != sid:
                leave_room(room_id, sid=p_sid)
                player_rooms.pop(p_sid, None)
                p_token = _sid_to_token.pop(p_sid, None)
                if p_token:
                    _reconnect_tokens.pop(p_token, None)
                    _disconnected_players.pop(p_token, None)
                # Send room_left to each player individually
                emit('room_left', {}, room=p_sid)

        # Also clean up disconnected players in this room
        tokens_to_remove = [
            t for t, info in _disconnected_players.items()
            if info['room_id'] == room_id
        ]
        for t in tokens_to_remove:
            dc = _disconnected_players.pop(t)
            _sid_to_token.pop(dc['sid'], None)
            _reconnect_tokens.pop(t, None)

        # Remove the owner
        game.remove_player(sid)
        leave_room(room_id)
        del player_rooms[sid]

        token = _sid_to_token.pop(sid, None)
        if token:
            _reconnect_tokens.pop(token, None)
            _disconnected_players.pop(token, None)

        _cleanup_room(room_id)
        emit('room_left', {})
        emit('rooms_list', get_rooms_list(), broadcast=True)
        return

    # Normal leave (not owner of active game)
    game.remove_player(sid)
    leave_room(room_id)
    del player_rooms[sid]

    token = _sid_to_token.pop(sid, None)
    if token:
        _reconnect_tokens.pop(token, None)
        _disconnected_players.pop(token, None)

    if not game.players:
        _cleanup_room(room_id)
    else:
        if room.owner == sid:
            _transfer_ownership(room)
        _emit_all_states(game)
        emit('player_left', {'name': player_name}, room=room_id)

    emit('room_left', {})
    emit('rooms_list', get_rooms_list(), broadcast=True)


@socketio.on('start_game')
def handle_start_game():
    sid = request.sid
    room_id = player_rooms.get(sid)
    if not room_id or room_id not in rooms:
        return

    room = rooms[room_id]
    if room.owner != sid:
        emit('error', {'message': 'Csak a szoba tulajdonosa indíthatja a játékot.'})
        return

    # Restore lobby: visszaállítás mentett állapotból
    if hasattr(room, 'restore_save_data') and room.restore_save_data:
        save_data = room.restore_save_data
        try:
            state = json.loads(save_data['state_json'])
            restored_game = Game.from_save_dict(state)
            restored_game._db_game_id = save_data['id']

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
            emit('rooms_list', get_rooms_list(), broadcast=True)
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
    emit('rooms_list', get_rooms_list(), broadcast=True)


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
    if not _check_rate_limit(sid, 'restore_game'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    if not isinstance(data, dict):
        return

    game_id = data.get('game_id')
    if not isinstance(game_id, int):
        emit('error', {'message': 'Érvénytelen játék azonosító.'})
        return

    # Auth ellenőrzés
    auth_info = player_auth.get(sid, {})
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
    player_name = player_names.get(sid, 'Névtelen')
    saved_owner = game_row.get('owner_name', '')
    if saved_owner and saved_owner != player_name:
        emit('error', {'message': 'Csak a mentés tulajdonosa állíthatja vissza a játékot.'})
        return

    # Parse state to get expected players
    try:
        state = json.loads(game_row['state_json'])
    except Exception:
        emit('error', {'message': 'Hibás mentett állapot.'})
        return

    expected_players = [p['name'] for p in state.get('players', [])]
    if not expected_players:
        emit('error', {'message': 'Nincs játékos a mentett játékban.'})
        return

    # Várakozó szoba létrehozása
    room_id = str(uuid.uuid4())[:8]
    join_code = generate_join_code(join_codes)
    new_game = Game(room_id, challenge_mode=state.get('challenge_mode', False))
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

    rooms[room_id] = room
    join_codes[join_code] = room_id

    player_rooms[sid] = room_id
    join_room(room_id)

    token = _generate_reconnect_token(sid, room_id, player_name)

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
    emit('rooms_list', get_rooms_list(), broadcast=True)


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


@socketio.on('challenge')
def handle_challenge():
    sid = request.sid
    room_id, room, game = _get_room_context(sid, 'challenge')
    if not room:
        return

    success, result, msg = game.challenge(sid)

    if success:
        room.invalidate_challenge_timer()
        if result == 'voting':
            _start_challenge_timer(room_id)
        _emit_all_states(game)
        if result in ('vote_accepted', 'vote_rejected'):
            emit('challenge_result', {
                'challenge_won': result == 'vote_rejected',
                'message': msg,
            }, room=room_id)
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
        room.invalidate_challenge_timer()
        _emit_all_states(game)
        emit('challenge_result', {
            'challenge_won': True,
            'message': msg,
        }, room=room_id)
    else:
        emit('action_result', {'success': False, 'message': msg})


@socketio.on('cast_vote')
def handle_cast_vote(data):
    sid = request.sid
    room_id, room, game = _get_room_context(sid, 'cast_vote')
    if not room:
        return
    if not isinstance(data, dict):
        return

    vote = data.get('vote')
    if vote not in ('accept', 'reject'):
        emit('action_result', {'success': False, 'message': 'Érvénytelen szavazat.'})
        return

    success, result, msg = game.cast_vote(sid, vote)
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

    player_name = player_names.get(sid, '?')
    chat_msg = {'name': player_name, 'message': message}
    room.add_chat_message(player_name, message)

    emit('chat_message', chat_msg, room=room_id)


# ===== Tunnel =====

tunnel_process = None


def start_tunnel(port):
    """Cloudflare tunnel indítása háttérben."""
    global tunnel_process
    cloudflared = shutil.which('cloudflared')
    if not cloudflared:
        print("\n  [!] cloudflared nincs telepítve - tunnel nem elérhető")
        print("      Telepítés: sudo pacman -S cloudflared\n")
        return

    print("\n  [*] Cloudflare tunnel indítása...")
    tunnel_process = subprocess.Popen(
        [cloudflared, 'tunnel', '--url', f'http://localhost:{port}'],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )

    def read_output():
        for line in tunnel_process.stdout:
            match = re.search(r'(https://[a-z0-9-]+\.trycloudflare\.com)', line)
            if match:
                url = match.group(1)
                print(f"\n{'='*50}")
                print(f"  PUBLIKUS URL: {url}")
                print(f"  Oszd meg ezt a linket a barátaiddal!")
                print(f"{'='*50}\n")

    thread = threading.Thread(target=read_output, daemon=True)
    thread.start()


def stop_tunnel():
    global tunnel_process
    if tunnel_process:
        tunnel_process.terminate()
        tunnel_process = None


atexit.register(stop_tunnel)


if __name__ == '__main__':
    port = int(os.environ.get('PORT', 5000))
    use_tunnel = '--no-tunnel' not in sys.argv

    cleanup_expired()
    _cleanup_finished_saves()

    if use_tunnel:
        start_tunnel(port)

    socketio.run(app, host='0.0.0.0', port=port, debug=False,
                 use_reloader=False)
