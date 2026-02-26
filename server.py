# eventlet monkey-patch MUST be first, before any other imports
try:
    import eventlet
    eventlet.monkey_patch()
except ImportError:
    pass

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
from config import AUTH_RATE_LIMITS, SMTP_CONFIGURED
from auth import init_db, get_user_by_email, create_user, verify_password, \
    create_verification_code, verify_code, create_session, validate_session, \
    delete_session, cleanup_expired
from email_service import send_verification_email

app = Flask(__name__)
app.config['SECRET_KEY'] = os.environ.get('SECRET_KEY', os.urandom(32).hex())
socketio = SocketIO(app, cors_allowed_origins=[],
                    ping_timeout=120, ping_interval=25)

# Adatbázis inicializálása
init_db()

# Aktív szobák: {room_id: {game, owner, name, max_players, join_code}}
rooms = {}
# join_code -> room_id mapping (gyors keresés)
join_codes = {}
# Játékos -> szoba mapping: {sid: room_id}
player_rooms = {}
# Játékos nevek: {sid: name}
player_names = {}
# Játékos auth info: {sid: {user_id, is_guest}}
player_auth = {}
# Chat üzenetek szobánként: {room_id: [{name, message, timestamp}, ...]}
room_chat = {}
# Challenge timer azonosítók: {room_id: int}
_challenge_counter = {}
# Reconnect tokenek: {token: {room_id, player_name, sid}}
_reconnect_tokens = {}
# Sid -> reconnect token mapping: {sid: token}
_sid_to_token = {}
# Disconnected játékosok grace period: {token: {room_id, player_idx, timer_id, ...}}
_disconnected_players = {}
# Grace period időtartam (mp) — ennyi ideig várunk az újracsatlakozásra
_DISCONNECT_GRACE_PERIOD = 120

# --- Rate limiting ---
# {sid: {event_name: [timestamp, ...]}}
_rate_limits = defaultdict(lambda: defaultdict(list))
# Események limitjei: (max_kérés, időablak_mp)
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
}

# IP-alapú rate limiting az auth endpointokra
# {ip: {action: [timestamp, ...]}}
_ip_rate_limits = defaultdict(lambda: defaultdict(list))


def _check_rate_limit(sid, event):
    """Ellenőrzi, hogy a játékos túllépte-e a rate limitet. True = engedélyezve."""
    if event not in _RATE_LIMITS:
        return True
    max_requests, window = _RATE_LIMITS[event]
    now = time.time()
    timestamps = _rate_limits[sid][event]
    # Régi bejegyzések törlése
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
    # Üres IP bejegyzések törlése a memória-szivárgás megelőzésére
    if not _ip_rate_limits[ip][action] and not any(_ip_rate_limits[ip].values()):
        del _ip_rate_limits[ip]
        return True
    if len(_ip_rate_limits[ip][action]) >= max_requests:
        return False
    _ip_rate_limits[ip][action].append(now)
    return True


# --- Input validáció ---
# Megengedett karakterek: betűk (magyar ékezetesek is), számok, szóköz, néhány írásjel
_VALID_NAME_RE = re.compile(r'^[\w\sáéíóöőúüűÁÉÍÓÖŐÚÜŰ._-]{1,20}$', re.UNICODE)
_VALID_ROOM_NAME_RE = re.compile(r'^[\w\sáéíóöőúüűÁÉÍÓÖŐÚÜŰ._!?-]{1,30}$', re.UNICODE)
_VALID_EMAIL_RE = re.compile(r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$')

# Érvényes magyar Scrabble betűk
_VALID_LETTERS = frozenset([
    'A', 'Á', 'B', 'C', 'CS', 'D', 'E', 'É', 'F', 'G', 'GY', 'H', 'I', 'Í',
    'J', 'K', 'L', 'LY', 'M', 'N', 'NY', 'O', 'Ó', 'Ö', 'Ő', 'P', 'R', 'S',
    'SZ', 'T', 'TY', 'U', 'Ú', 'Ü', 'Ű', 'V', 'Z', 'ZS',
])


def _sanitize_name(name, max_len=20):
    """Játékos név validálása és tisztítása."""
    if not isinstance(name, str):
        return None
    name = name.strip()
    if not name or len(name) > max_len:
        return None
    if not _VALID_NAME_RE.match(name):
        return None
    return name


def _sanitize_room_name(name, max_len=30):
    """Szoba név validálása és tisztítása."""
    if not isinstance(name, str):
        return None
    name = name.strip()
    if not name or len(name) > max_len:
        return None
    if not _VALID_ROOM_NAME_RE.match(name):
        return None
    return name


def _generate_join_code():
    """6 számjegyű egyedi csatlakozási kód generálása."""
    while True:
        code = f'{secrets.randbelow(1000000):06d}'
        if code not in join_codes:
            return code


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
        max_age=30 * 24 * 3600,  # 30 nap
    )
    return response


# --- Challenge timer ---

def _cleanup_room(room_id):
    """Szoba erőforrásainak felszabadítása (join_code, chat, challenge timer, tokenek)."""
    room = rooms.get(room_id)
    if room and room.get('join_code') in join_codes:
        del join_codes[room['join_code']]
    if room_id in room_chat:
        del room_chat[room_id]
    if room_id in _challenge_counter:
        del _challenge_counter[room_id]
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
    game = room['game']
    room['owner'] = game.players[0].id
    room['owner_name'] = game.players[0].name
    emit('room_code', {'code': room['join_code']}, room=game.players[0].id)


def _emit_all_states(game):
    """Minden játékosnak elküldi a saját állapotát (közös részt egyszer számítja)."""
    all_states = game.get_all_states()
    for player_id, state in all_states.items():
        emit('game_state', state, room=player_id)


def _start_challenge_timer(room_id):
    """Challenge timeout visszaszámlálás."""
    if room_id not in _challenge_counter:
        _challenge_counter[room_id] = 0
    _challenge_counter[room_id] += 1
    current_id = _challenge_counter[room_id]

    def timeout_callback():
        eventlet.sleep(CHALLENGE_TIMEOUT)
        if room_id in rooms and _challenge_counter.get(room_id) == current_id:
            game = rooms[room_id]['game']
            if game.pending_challenge:
                success, result, msg = game.accept_pending()
                all_states = game.get_all_states()
                for player_id, state in all_states.items():
                    socketio.emit('game_state', state, room=player_id)
                # Szavazás eredmény közlése ha volt szavazási fázis
                if result in ('vote_accepted', 'vote_rejected'):
                    socketio.emit('challenge_result', {
                        'challenge_won': result == 'vote_rejected',
                        'message': msg,
                    }, room=room_id)

    socketio.start_background_task(timeout_callback)


# ===== AUTH HTTP ROUTES =====

@app.route('/api/auth/request-code', methods=['POST'])
def auth_request_code():
    """Email validálás, verifikációs kód küldés."""
    ip = _get_client_ip()
    if not _check_ip_rate_limit(ip, 'request_code'):
        return jsonify({'success': False, 'message': 'Túl sok kérés. Próbáld újra 5 perc múlva.'}), 429

    data = request.get_json(silent=True)
    if not data or not isinstance(data.get('email'), str):
        return jsonify({'success': False, 'message': 'Email cím megadása kötelező.'}), 400

    email = data['email'].strip().lower()
    if not _VALID_EMAIL_RE.match(email) or len(email) > 254:
        return jsonify({'success': False, 'message': 'Érvénytelen email cím.'}), 400

    # Ellenőrizzük, hogy az email már regisztrálva van-e
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
    """6 számjegyű verifikációs kód ellenőrzés."""
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
    """Fiók létrehozása: jelszó + megjelenítési név."""
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

    # Auto-login: session létrehozás
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
    """Bejelentkezés email + jelszóval."""
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
    """Kijelentkezés: session törlés."""
    token = request.cookies.get('session_token')
    if token:
        delete_session(token)

    resp = make_response(jsonify({'success': True, 'message': 'Kijelentkezve.'}))
    resp.delete_cookie('session_token')
    return resp


@app.route('/api/auth/me', methods=['GET'])
def auth_me():
    """Aktuális session ellenőrzés."""
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


# ===== ROUTES =====

@app.route('/')
def index():
    return render_template('index.html')


def get_rooms_list():
    """Szobák listája a lobby számára (kód nélkül - az csak a tulajdonosnak)."""
    result = []
    for room_id, room in rooms.items():
        result.append({
            'id': room_id,
            'name': room['name'],
            'players': len(room['game'].players),
            'max_players': room['max_players'],
            'started': room['game'].started,
            'owner': room['owner_name'],
            'challenge_mode': room['game'].challenge_mode,
        })
    return result


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
        game = room['game']

        # Ha a játék folyamatban van és van reconnect token,
        # grace period-ot adunk az újracsatlakozásra
        if game.started and not game.finished and token:
            game.mark_disconnected(sid)
            leave_room(room_id)

            # Grace period timer indítása
            _disconnected_players[token] = {
                'room_id': room_id,
                'sid': sid,
                'player_name': player_names.get(sid, '?'),
            }

            def grace_timeout():
                eventlet.sleep(_DISCONNECT_GRACE_PERIOD)
                if token in _disconnected_players:
                    _finalize_player_disconnect(token)

            socketio.start_background_task(grace_timeout)

            _emit_all_states(game)
            emit('player_disconnected',
                 {'name': player_names.get(sid, '?')}, room=room_id)
            del player_rooms[sid]
        else:
            # Nem aktív játék: azonnali eltávolítás
            game.remove_player(sid)
            leave_room(room_id)

            if not game.players:
                _cleanup_room(room_id)
            else:
                if room['owner'] == sid:
                    _transfer_ownership(room)
                _emit_all_states(game)
                emit('player_left', {'name': player_names.get(sid, '?')},
                     room=room_id)

            del player_rooms[sid]

            # Token cleanup
            if token:
                _reconnect_tokens.pop(token, None)
                _sid_to_token.pop(sid, None)
    else:
        # Token cleanup ha nem volt szobában
        if token:
            _reconnect_tokens.pop(token, None)
            _sid_to_token.pop(sid, None)

    if sid in player_names:
        del player_names[sid]

    if sid in player_auth:
        del player_auth[sid]

    if sid in _rate_limits:
        del _rate_limits[sid]

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
    game = room['game']
    game.remove_player(old_sid)

    if not game.players:
        _cleanup_room(room_id)
    else:
        if room['owner'] == old_sid:
            _transfer_ownership(room)

        all_states = game.get_all_states()
        for player_id, state in all_states.items():
            socketio.emit('game_state', state, room=player_id)
        socketio.emit('player_left',
                      {'name': info['player_name']}, room=room_id)

    socketio.emit('rooms_list', get_rooms_list(), broadcast=True)


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
    is_guest = data.get('is_guest', True)
    user_id = data.get('user_id')

    player_names[sid] = name
    player_auth[sid] = {'user_id': user_id, 'is_guest': is_guest}


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

    # Ellenőrizzük, hogy van-e disconnected játékos ezzel a tokennel
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
    game = room['game']

    # Sid csere a játékban
    if not game.replace_player_sid(old_sid, sid):
        emit('rejoin_failed', {'message': 'Nem sikerült újracsatlakozni.'})
        return

    # Cleanup: grace period leállítása
    del _disconnected_players[token]

    # Token frissítése az új sid-del
    _reconnect_tokens[token]['sid'] = sid
    _sid_to_token.pop(old_sid, None)
    _sid_to_token[sid] = token

    # Owner frissítése ha szükséges
    if room['owner'] == old_sid:
        room['owner'] = sid

    # Szoba kezelés
    player_name = dc_info['player_name']
    player_rooms[sid] = room_id
    player_names[sid] = player_name
    join_room(room_id)

    emit('room_joined', {
        'room_id': room_id,
        'room_name': room['name'],
        'is_owner': room['owner'] == sid,
        'challenge_mode': game.challenge_mode,
        'reconnect_token': token,
    })

    if room['owner'] == sid:
        emit('room_code', {'code': room['join_code']})

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

    name = _sanitize_room_name(data.get('name', ''))
    if not name:
        name = 'Szoba'
    try:
        max_players = min(max(int(data.get('max_players', 4)), 2), 4)
    except (ValueError, TypeError):
        max_players = 4
    challenge_mode = bool(data.get('challenge_mode', False))
    player_name = player_names.get(sid, 'Névtelen')

    room_id = str(uuid.uuid4())[:8]
    join_code = _generate_join_code()
    game = Game(room_id, challenge_mode=challenge_mode)
    game.add_player(sid, player_name)

    rooms[room_id] = {
        'game': game,
        'owner': sid,
        'owner_name': player_name,
        'name': name,
        'max_players': max_players,
        'join_code': join_code,
    }
    join_codes[join_code] = room_id

    player_rooms[sid] = room_id
    join_room(room_id)

    token = _generate_reconnect_token(sid, room_id, player_name)

    emit('room_joined', {
        'room_id': room_id,
        'room_name': name,
        'is_owner': True,
        'challenge_mode': game.challenge_mode,
        'reconnect_token': token,
    })
    # Csak a tulajdonosnak küldjük el a csatlakozási kódot
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

    # Kóddal történő csatlakozás
    code = data.get('code', '').strip()
    room_id = data.get('room_id')

    if code:
        if not re.match(r'^\d{6}$', code):
            emit('error', {'message': 'Érvénytelen kód. 6 számjegyű kódot adj meg.'})
            return
        room_id = join_codes.get(code)
        if not room_id:
            emit('error', {'message': 'Nincs ilyen kódú szoba.'})
            return
    elif room_id:
        if not isinstance(room_id, str) or len(room_id) > 8:
            emit('error', {'message': 'Érvénytelen szoba azonosító.'})
            return
    else:
        emit('error', {'message': 'Szoba kód vagy azonosító szükséges.'})
        return

    player_name = player_names.get(sid, 'Névtelen')

    if room_id not in rooms:
        emit('error', {'message': 'A szoba nem létezik.'})
        return

    room = rooms[room_id]
    game = room['game']

    if game.started:
        emit('error', {'message': 'A játék már elkezdődött.'})
        return

    if len(game.players) >= room['max_players']:
        emit('error', {'message': 'A szoba megtelt.'})
        return

    success, msg = game.add_player(sid, player_name)
    if not success:
        emit('error', {'message': msg})
        return

    player_rooms[sid] = room_id
    join_room(room_id)

    token = _generate_reconnect_token(sid, room_id, player_name)

    emit('room_joined', {
        'room_id': room_id,
        'room_name': room['name'],
        'is_owner': False,
        'challenge_mode': game.challenge_mode,
        'reconnect_token': token,
    })

    _emit_all_states(game)

    emit('player_joined', {'name': player_name}, room=room_id)
    emit('rooms_list', get_rooms_list(), broadcast=True)


@socketio.on('leave_room')
def handle_leave_room():
    sid = request.sid
    room_id = player_rooms.get(sid)
    if not room_id or room_id not in rooms:
        return

    room = rooms[room_id]
    game = room['game']
    player_name = player_names.get(sid, '?')
    game.remove_player(sid)
    leave_room(room_id)
    del player_rooms[sid]

    # Token cleanup (explicit leave = nem kell grace period)
    token = _sid_to_token.pop(sid, None)
    if token:
        _reconnect_tokens.pop(token, None)
        _disconnected_players.pop(token, None)

    if not game.players:
        _cleanup_room(room_id)
    else:
        if room['owner'] == sid:
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
    if room['owner'] != sid:
        emit('error', {'message': 'Csak a szoba tulajdonosa indíthatja a játékot.'})
        return

    game = room['game']
    success, msg = game.start()
    if not success:
        emit('error', {'message': msg})
        return

    _emit_all_states(game)

    emit('game_started', {}, room=room_id)
    emit('rooms_list', get_rooms_list(), broadcast=True)


@socketio.on('place_tiles')
def handle_place_tiles(data):
    sid = request.sid
    if not _check_rate_limit(sid, 'place_tiles'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    room_id = player_rooms.get(sid)
    if not room_id or room_id not in rooms:
        return
    if not isinstance(data, dict):
        return

    game = rooms[room_id]['game']
    tiles = data.get('tiles', [])

    if not isinstance(tiles, list) or len(tiles) == 0 or len(tiles) > 7:
        emit('action_result', {'success': False, 'message': 'Érvénytelen lerakás.'})
        return

    # Konvertáljuk és validáljuk a tiles listát
    tiles_placed = []
    for t in tiles:
        if not isinstance(t, dict):
            emit('action_result', {'success': False, 'message': 'Érvénytelen adat.'})
            return
        try:
            row = int(t['row'])
            col = int(t['col'])
        except (KeyError, ValueError, TypeError):
            emit('action_result', {'success': False, 'message': 'Érvénytelen pozíció.'})
            return
        if not (0 <= row <= 14 and 0 <= col <= 14):
            emit('action_result', {'success': False, 'message': 'Pozíció a táblán kívül.'})
            return
        letter = t.get('letter', '')
        is_blank = bool(t.get('is_blank', False))
        if not isinstance(letter, str):
            emit('action_result', {'success': False, 'message': 'Érvénytelen betű.'})
            return
        if is_blank:
            if letter not in _VALID_LETTERS:
                emit('action_result', {'success': False, 'message': f'Érvénytelen joker betű: {letter}'})
                return
        else:
            if letter not in _VALID_LETTERS:
                emit('action_result', {'success': False, 'message': f'Érvénytelen betű: {letter}'})
                return
        tiles_placed.append((row, col, letter, is_blank))

    success, msg, score = game.place_tiles(sid, tiles_placed)

    if success:
        _emit_all_states(game)
        emit('action_result', {'success': True, 'message': msg, 'score': score})
        # Challenge timer indítása ha szükséges
        if game.pending_challenge:
            _start_challenge_timer(room_id)
    else:
        emit('action_result', {'success': False, 'message': msg})


@socketio.on('exchange_tiles')
def handle_exchange_tiles(data):
    sid = request.sid
    if not _check_rate_limit(sid, 'exchange_tiles'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    room_id = player_rooms.get(sid)
    if not room_id or room_id not in rooms:
        return
    if not isinstance(data, dict):
        return

    game = rooms[room_id]['game']
    indices = data.get('indices', [])
    if not isinstance(indices, list) or len(indices) > 7:
        emit('action_result', {'success': False, 'message': 'Érvénytelen csere.'})
        return
    # Validáljuk az indexeket
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
    if not _check_rate_limit(sid, 'pass_turn'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    room_id = player_rooms.get(sid)
    if not room_id or room_id not in rooms:
        return

    game = rooms[room_id]['game']
    success, msg = game.pass_turn(sid)

    if success:
        _emit_all_states(game)
        emit('action_result', {'success': True, 'message': msg})
    else:
        emit('action_result', {'success': False, 'message': msg})


@socketio.on('challenge')
def handle_challenge():
    sid = request.sid
    if not _check_rate_limit(sid, 'challenge'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    room_id = player_rooms.get(sid)
    if not room_id or room_id not in rooms:
        return

    game = rooms[room_id]['game']
    success, result, msg = game.challenge(sid)

    if success:
        # Timer érvénytelenítése
        if room_id not in _challenge_counter:
            _challenge_counter[room_id] = 0
        _challenge_counter[room_id] += 1

        if result == 'voting':
            # Szavazás indult: új timer
            _start_challenge_timer(room_id)

        _emit_all_states(game)

        # Ha azonnal eldőlt (minden szavazó már szavazott)
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
    if not _check_rate_limit(sid, 'accept_words'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    room_id = player_rooms.get(sid)
    if not room_id or room_id not in rooms:
        return

    game = rooms[room_id]['game']
    success, result, msg = game.accept_pending_by_player(sid)

    if success:
        if result in ('accepted', 'vote_accepted', 'vote_rejected'):
            # Teljesen elfogadva/eldöntve: timer törlése
            if room_id not in _challenge_counter:
                _challenge_counter[room_id] = 0
            _challenge_counter[room_id] += 1

        _emit_all_states(game)

        if result in ('vote_accepted', 'vote_rejected'):
            emit('challenge_result', {
                'challenge_won': result == 'vote_rejected',
                'message': msg,
            }, room=room_id)
    else:
        emit('action_result', {'success': False, 'message': msg})


@socketio.on('reject_words')
def handle_reject_words():
    sid = request.sid
    if not _check_rate_limit(sid, 'reject_words'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    room_id = player_rooms.get(sid)
    if not room_id or room_id not in rooms:
        return

    game = rooms[room_id]['game']
    success, result, msg = game.reject_pending_by_player(sid)

    if success:
        # Timer törlése
        if room_id not in _challenge_counter:
            _challenge_counter[room_id] = 0
        _challenge_counter[room_id] += 1

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
    if not _check_rate_limit(sid, 'cast_vote'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    room_id = player_rooms.get(sid)
    if not room_id or room_id not in rooms:
        return
    if not isinstance(data, dict):
        return

    vote = data.get('vote')
    if vote not in ('accept', 'reject'):
        emit('action_result', {'success': False, 'message': 'Érvénytelen szavazat.'})
        return

    game = rooms[room_id]['game']
    success, result, msg = game.cast_vote(sid, vote)

    if success:
        if result in ('vote_accepted', 'vote_rejected'):
            # Szavazás eldőlt: timer törlése
            if room_id not in _challenge_counter:
                _challenge_counter[room_id] = 0
            _challenge_counter[room_id] += 1

        _emit_all_states(game)

        if result in ('vote_accepted', 'vote_rejected'):
            emit('challenge_result', {
                'challenge_won': result == 'vote_rejected',
                'message': msg,
            }, room=room_id)
    else:
        emit('action_result', {'success': False, 'message': msg})


@socketio.on('send_chat')
def handle_send_chat(data):
    sid = request.sid
    if not _check_rate_limit(sid, 'send_chat'):
        emit('error', {'message': 'Túl sok üzenet, várj egy kicsit.'})
        return
    room_id = player_rooms.get(sid)
    if not room_id or room_id not in rooms:
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

    chat_msg = {
        'name': player_name,
        'message': message,
    }

    if room_id not in room_chat:
        room_chat[room_id] = []
    room_chat[room_id].append(chat_msg)
    # Max 100 üzenet
    if len(room_chat[room_id]) > 100:
        room_chat[room_id] = room_chat[room_id][-100:]

    emit('chat_message', chat_msg, room=room_id)


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

    # Lejárt sessionök és kódok tisztítása induláskor
    cleanup_expired()

    if use_tunnel:
        start_tunnel(port)

    socketio.run(app, host='0.0.0.0', port=port, debug=False,
                 use_reloader=False)
