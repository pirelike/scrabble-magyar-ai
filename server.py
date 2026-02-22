import uuid
import os
import subprocess
import shutil
import threading
import re
import signal
import sys
import time
import atexit
from collections import defaultdict
from flask import Flask, render_template, request
from flask_socketio import SocketIO, emit, join_room, leave_room

from game import Game

app = Flask(__name__)
app.config['SECRET_KEY'] = os.environ.get('SECRET_KEY', os.urandom(32).hex())
socketio = SocketIO(app, cors_allowed_origins=[])

# Aktív szobák: {room_id: {game, owner, name, max_players}}
rooms = {}
# Játékos -> szoba mapping: {sid: room_id}
player_rooms = {}
# Játékos nevek: {sid: name}
player_names = {}

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
}


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


# --- Input validáció ---
# Megengedett karakterek: betűk (magyar ékezetesek is), számok, szóköz, néhány írásjel
_VALID_NAME_RE = re.compile(r'^[\w\sáéíóöőúüűÁÉÍÓÖŐÚÜŰ._-]{1,20}$', re.UNICODE)
_VALID_ROOM_NAME_RE = re.compile(r'^[\w\sáéíóöőúüűÁÉÍÓÖŐÚÜŰ._!?-]{1,30}$', re.UNICODE)

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


@app.route('/')
def index():
    return render_template('index.html')


def get_rooms_list():
    """Szobák listája a lobby számára."""
    result = []
    for room_id, room in rooms.items():
        result.append({
            'id': room_id,
            'name': room['name'],
            'players': len(room['game'].players),
            'max_players': room['max_players'],
            'started': room['game'].started,
            'owner': room['owner_name'],
        })
    return result


@socketio.on('connect')
def handle_connect():
    pass


@socketio.on('disconnect')
def handle_disconnect():
    sid = request.sid
    room_id = player_rooms.get(sid)
    if room_id and room_id in rooms:
        room = rooms[room_id]
        game = room['game']
        game.remove_player(sid)
        leave_room(room_id)

        if not game.players:
            del rooms[room_id]
        else:
            # Ha a tulajdonos ment el, új tulajdonos
            if room['owner'] == sid:
                room['owner'] = game.players[0].id
                room['owner_name'] = game.players[0].name
            emit('game_state', game.get_state(), room=room_id)
            emit('player_left', {'name': player_names.get(sid, '?')}, room=room_id)

        del player_rooms[sid]

    if sid in player_names:
        del player_names[sid]

    # Rate limit bejegyzések törlése
    if sid in _rate_limits:
        del _rate_limits[sid]

    # Lobby frissítés
    emit('rooms_list', get_rooms_list(), broadcast=True)


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
    player_name = player_names.get(sid, 'Névtelen')

    room_id = str(uuid.uuid4())[:8]
    game = Game(room_id)
    game.add_player(sid, player_name)

    rooms[room_id] = {
        'game': game,
        'owner': sid,
        'owner_name': player_name,
        'name': name,
        'max_players': max_players,
    }

    player_rooms[sid] = room_id
    join_room(room_id)

    emit('room_joined', {
        'room_id': room_id,
        'room_name': name,
        'is_owner': True,
    })
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
    room_id = data.get('room_id')
    if not isinstance(room_id, str) or len(room_id) > 8:
        emit('error', {'message': 'Érvénytelen szoba azonosító.'})
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

    emit('room_joined', {
        'room_id': room_id,
        'room_name': room['name'],
        'is_owner': False,
    })

    # Frissítjük mindenkit a szobában
    for player in game.players:
        emit('game_state', game.get_state(for_player_id=player.id), room=player.id)

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
    game.remove_player(sid)
    leave_room(room_id)
    del player_rooms[sid]

    if not game.players:
        del rooms[room_id]
    else:
        if room['owner'] == sid:
            room['owner'] = game.players[0].id
            room['owner_name'] = game.players[0].name
        emit('game_state', game.get_state(), room=room_id)

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

    # Minden játékosnak elküldjük a saját állapotát
    for player in game.players:
        emit('game_state', game.get_state(for_player_id=player.id), room=player.id)

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
        for player in game.players:
            emit('game_state', game.get_state(for_player_id=player.id), room=player.id)
        emit('action_result', {'success': True, 'message': msg, 'score': score}, room=room_id)
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
        for player in game.players:
            emit('game_state', game.get_state(for_player_id=player.id), room=player.id)
        emit('action_result', {'success': True, 'message': msg}, room=room_id)
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
        for player in game.players:
            emit('game_state', game.get_state(for_player_id=player.id), room=player.id)
        emit('action_result', {'success': True, 'message': msg}, room=room_id)
    else:
        emit('action_result', {'success': False, 'message': msg})


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
    port = 5000
    use_tunnel = '--no-tunnel' not in sys.argv

    if use_tunnel:
        start_tunnel(port)

    socketio.run(app, host='0.0.0.0', port=port, debug=False,
                 use_reloader=False)
