from gevent import monkey
monkey.patch_all()

import json
import random
import time

import uuid
import os
import re
import sys
from flask import Flask, request
from flask_socketio import SocketIO, emit, join_room, leave_room

import achievements
import ai_player
import async_games
import daily
import dictionary
import practice
import word_review
import push_service
from game import Game, CHALLENGE_TIMEOUT, ALLOWED_HINT_LIMITS, DEFAULT_HINT_LIMIT
from room import Room
from config import AUTH_RATE_LIMITS
from auth import (
    init_db, save_game, finish_game, add_game_move,
    load_active_games, abandon_game, abandon_game_by_id,
    is_user_in_game, get_game_by_id, get_game_moves, get_game_players,
    get_daily_puzzle, get_daily_entry, mark_daily_revealed, get_active_async_games,
    get_user_by_id, grant_achievements, get_game_rating_changes,
    send_friend_request as auth_send_friend_request,
    accept_friend_request as auth_accept_friend_request,
    decline_friend_request as auth_decline_friend_request,
    remove_friend as auth_remove_friend,
    get_friends as auth_get_friends
)
from state import ServerState
from rate_limiter import RateLimiter
from routes import main_bp, auth_bp, game_bp, public_bp, init_routes
from tunnel import start_tunnel
from socket_auth import verify_socket_token

app = Flask(__name__)
app.config['SECRET_KEY'] = os.environ.get('SECRET_KEY', os.urandom(32).hex())
socketio = SocketIO(app, cors_allowed_origins='*', async_mode='gevent',
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
    'withdraw_words': (5, 10),
    'send_chat': (10, 10),
    'rejoin_room': (5, 10),
    'save_game': (3, 30),
    'restore_game': (3, 30),
    'send_friend_request': (5, 30),
    'accept_friend_request': (10, 10),
    'decline_friend_request': (10, 10),
    'remove_friend': (5, 30),
    'invite_to_room': (10, 30),
    'respond_invite': (10, 10),
    'preview_move': (30, 10),
    'request_hint': (3, 30),
    'set_visibility': (20, 10),
    'create_async_game': (3, 30),
    'open_async_game': (10, 10),
    'resign_game': (3, 30),
    'start_daily': (3, 30),
    'retry_daily': (10, 30),
    'reveal_daily': (5, 30),
    'spectate_room': (5, 10),
    'leave_spectate': (5, 10),
}

rate_limiter = RateLimiter(_SOCKET_RATE_LIMITS, AUTH_RATE_LIMITS)
push_service.init(socketio.start_background_task)

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
app.register_blueprint(public_bp)

# --- Grace period ---
_DISCONNECT_GRACE_PERIOD = 120
# Várakozó szoba (a játék indítása előtt): a tulajdonos ennyi ideig lehet távol, mielőtt a szoba
# megszűnik. Telefonon az üzenetküldő appra váltás (a kód / meghívó link elküldése) megszakítja a
# kapcsolatot, ezért ez hosszabb, mint a játék közbeni türelmi idő.
_WAITING_OWNER_GRACE_PERIOD = 600
ALLOWED_TURN_TIME_LIMITS = {0, 60, 90, 120, 180, 300}
# A visszavont lerakás után legalább ennyi idő marad a körből (mp)
_MIN_TIME_AFTER_WITHDRAW = 10
MAX_BOTS = 3
# Robot "gondolkodási" ideje másodpercben (min, max): a lépés ennyi várakozás után jelenik meg
_BOT_THINK_DELAY = {'easy': (1.5, 3.0), 'medium': (1.2, 2.6), 'hard': (1.0, 2.2)}
_BOT_NAMES = {
    'easy': ['Robi', 'Rozi', 'Rudi'],
    'medium': ['Rita', 'Ricsi', 'Réka'],
    'hard': ['Rezső', 'Róbert', 'Rómeó'],
}


def _bot_tier(level):
    """A robot fokozatának sávja (név és gondolkodási idő): 1–3 könnyű, 4–7 közepes, 8–10 nehéz."""
    level = ai_player.normalize_level(level)
    if level <= 3:
        return 'easy'
    return 'medium' if level <= 7 else 'hard'

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

def _sio_leave_room(sid, room_id):
    """leave_room kérés-/alkalmazáskontextus nélkül is (háttérszálból is hívható).

    A `flask_socketio.leave_room` a `flask.current_app`-ot és a `request`-et használja,
    ami a grace period / időzítő háttérszálakban nem elérhető.
    """
    socketio.server.leave_room(sid, room_id, namespace='/')


def _transfer_ownership(room):
    """Szoba tulajdonjogának átadása az első online játékosnak."""
    game = room.game
    if not game.players:
        return
    humans = game.human_players() or game.players
    new_owner = next((p for p in humans if not p.disconnected), humans[0])
    token = state.get_reconnect_token_for_sid(new_owner.id)
    room.transfer_ownership(new_owner.id, new_owner.name, token)
    socketio.emit('room_code', {'code': room.join_code}, room=new_owner.id)


def _advance_turn_if_needed(room_id, room, game):
    """Ha a soron lévő játékos lecsatlakozott, továbbadja a kört és újraindítja az időzítőt."""
    if game.skip_disconnected_current():
        room.invalidate_turn_timer()
        _start_turn_timer(room_id)
        _schedule_bot_turn(room_id)


def _find_room_of(game, room_id=None):
    """A játékhoz tartozó szoba (azonosító alapján, ennek híján a játék objektum alapján)."""
    if room_id and room_id in state.rooms:
        return state.rooms[room_id]
    for r in state.rooms.values():
        if r.game is game:
            return r
    return None


def _emit_all_states(game, room_id=None):
    """Minden játékosnak (és megfigyelőnek) elküldi a saját állapotát.

    A `socketio.emit` kérés-kontextus nélkül is működik, így háttérszálból (időzítők, robotok)
    is hívható.
    """
    room = _find_room_of(game, room_id)
    expires_at = room.turn_timer_expires_at if room else None
    spectator_count = len(room.spectators) if room else 0
    # A lecsatlakozott / kilépett játékos SID-je még élhet (a lobbyban van, pl. levelezős játékból
    # kilépve): neki nem küldünk állapotot, különben a kliense visszaugrana a játékképernyőre
    away = {p.id for p in game.players if p.disconnected}
    for player_id, gs in game.get_all_states().items():
        if player_id in away:
            continue
        gs['turn_timer_expires_at'] = expires_at
        gs['spectator_count'] = spectator_count
        socketio.emit('game_state', gs, room=player_id)
    if room and room.spectators:
        spectator_state = game.get_spectator_state()
        spectator_state['turn_timer_expires_at'] = expires_at
        spectator_state['spectator_count'] = spectator_count
        for sid in list(room.spectators):
            socketio.emit('game_state', spectator_state, room=sid)
    _persist_async(room)
    _maybe_push_turn(room, game)


def _persist_async(room):
    """Levelezős játék: minden változás után menti az állapotot (a játék tartós otthona az adatbázis)."""
    if not room or not room.is_async or not room.game.started:
        return
    game = room.game
    signature = (len(game.move_log), game.finished, game.turn_number)
    if room.persisted_sig != signature:
        room.persisted_sig = signature
        _save_game_to_db(room.id)


def _maybe_push_turn(room, game):
    """Web Push „Te jössz!”, ha a soron lévő regisztrált játékos nincs jelen (lecsatlakozott vagy a
    böngészőlapja háttérbe került). Körönként legfeljebb egyszer."""
    if not room or room.is_puzzle or not game.started or game.finished or game.pending_challenge:
        return
    player = game.current_player()
    if not player or player.is_bot or room.pushed_turn == game.turn_number:
        return
    if not (player.disconnected or player.id in state.hidden_sids):
        return
    user_id = _user_id_for_player(room, player)
    if not user_id:
        return
    if room.is_async and room.notified_turn != game.turn_number:
        # A lobbyban (más képernyőn) lévő játékos az alkalmazáson belül is értesül
        room.notified_turn = game.turn_number
        for user_sid in list(state.get_user_sids(user_id)):
            socketio.emit('async_your_turn', {'game_id': room.db_game_id, 'room_name': room.name},
                          room=user_sid)
    if push_service.notify_turn(user_id, room.name, room.id):
        room.pushed_turn = game.turn_number


def _broadcast_rooms():
    """A nyilvános szobák és az élő játékok listájának frissítése minden kliensnek."""
    socketio.emit('rooms_list', state.get_rooms_list())
    socketio.emit('live_games', state.get_live_games())


def _cleanup_room(room_id):
    """Szoba törlése; a megfigyelőket értesíti és leválasztja."""
    room = state.rooms.get(room_id)
    if room and room.spectators:
        socketio.emit('room_disbanded',
                      {'message': 'A játék véget ért, a szoba megszűnt.'}, room=room_id)
        for sid in list(room.spectators):
            _sio_leave_room(sid, room_id)
    if room:
        room.invalidate_bot_turn()
    state.cleanup_room(room_id)


def _disband_active_room(room_id, message, owner_sid=None):
    """Aktív játék szoba feloszlatása: broadcast, játékosok kitakarítása, room törlés.

    owner_sid: ha megadva, ezt a SID-t nem küldi room_left-nek (az owner maga kezeli).
    """
    room = state.rooms.get(room_id)
    if not room:
        return
    game = room.game
    room.invalidate_turn_timer()

    state.remove_invites_for_room(room_id)
    socketio.emit('room_disbanded', {'message': message}, room=room_id)

    for spectator_sid in list(room.spectators):
        _sio_leave_room(spectator_sid, room_id)
    room.invalidate_bot_turn()

    for p in list(game.players):
        p_sid = p.id
        if p.is_bot:
            continue
        if p_sid != owner_sid:
            _sio_leave_room(p_sid, room_id)
            state.player_rooms.pop(p_sid, None)
            state.cleanup_player_token(p_sid)
            socketio.emit('room_left', {}, room=p_sid)

    state.cleanup_room_tokens(room_id)

    if game.started and not game.finished and not room.manually_saved:
        if room.db_game_id:
            abandon_game_by_id(room.db_game_id)
        else:
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
            _broadcast_challenge_result(room_id, result, msg)
            if game.finished:
                _emit_all_states(game, room_id)
                _save_game_to_db(room_id)
            else:
                _start_turn_timer(room_id)
                _emit_all_states(game, room_id)
                _schedule_bot_turn(room_id)

    socketio.start_background_task(timeout_callback)


def _start_turn_timer(room_id, seconds=None):
    """Kör visszaszámlálás indítása. Ha lejár, auto-passz. `seconds`: a teljes körnél rövidebb
    hátralévő idő (pl. a visszavont lerakás után)."""
    room = state.rooms.get(room_id)
    if not room or not room.game.turn_time_limit:
        return
    if room.game.pending_challenge:
        return  # challenge fut, nem indítunk kör timert

    timer_id = room.invalidate_turn_timer()
    limit = seconds if seconds is not None else room.game.turn_time_limit
    room.turn_timer_expires_at = time.time() + limit

    def timeout_callback():
        time.sleep(limit)
        if room_id not in state.rooms:
            return
        r = state.rooms[room_id]
        if r.turn_timer_id != timer_id:
            return  # már érvénytelen (új kör, vagy játék vége)
        game = r.game
        if game.finished or not game.started:
            return
        current = game.current_player()
        if not current:
            return
        # Auto-passz a jelenlegi játékos nevében
        success, msg = game.pass_turn(current.id)
        if success:
            socketio.emit('action_result',
                          {'success': True, 'message': f'Időtúllépés: {current.name} passzolt.'},
                          room=room_id)
            if game.finished:
                r.invalidate_turn_timer()
                _emit_all_states(game, room_id)
                _save_game_to_db(room_id)
            else:
                _start_turn_timer(room_id)  # következő kör timere, sets new expires_at
                _emit_all_states(game, room_id)
                _schedule_bot_turn(room_id)

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
    _broadcast_challenge_result(room_id, result, msg)
    if game.finished:
        _save_game_to_db(room_id)
    else:
        _start_turn_timer(room_id)
    _emit_all_states(game, room_id)
    _schedule_bot_turn(room_id)


# --- Robot ellenfelek ---

def _bot_name(game, difficulty):
    """Szabad robotnév az adott nehézséghez (nem ütközik a szobában lévő nevekkel)."""
    taken = {p.name.casefold() for p in game.players}
    pool = _BOT_NAMES[_bot_tier(difficulty)]
    for name in pool:
        if name.casefold() not in taken:
            return name
    n = 2
    while f'{pool[0]} {n}'.casefold() in taken:
        n += 1
    return f'{pool[0]} {n}'


def _bot_should_move(room):
    """Lépnie kell-e most a soron lévő robotnak?"""
    game = room.game
    if not game.started or game.finished or game.pending_challenge:
        return False
    current = game.current_player()
    if not current or not current.is_bot:
        return False
    # Emberi néző nélkül a robotok nem játszanak egymás ellen
    return game.has_connected_human() or bool(room.spectators)


def _schedule_bot_turn(room_id):
    """Ha a soron lévő játékos robot, háttérfeladatban elindítja a lépését."""
    room = state.rooms.get(room_id)
    if not room or not _bot_should_move(room):
        return
    game = room.game
    bot = game.current_player()
    turn_id = room.invalidate_bot_turn()
    turn_number = game.turn_number
    delay = random.uniform(*_BOT_THINK_DELAY[_bot_tier(bot.difficulty)])

    def run():
        socketio.sleep(delay)
        _play_bot_turn(room_id, turn_id, turn_number)

    socketio.start_background_task(run)


def _after_bot_move(room_id, room, game):
    """Egy robotlépés után: időzítők, állapot küldése, esetleg a következő robot."""
    room.invalidate_turn_timer()
    if game.pending_challenge:
        _start_challenge_timer(room_id)
    elif game.finished:
        _save_game_to_db(room_id)
    else:
        _start_turn_timer(room_id)
    _emit_all_states(game, room_id)
    if game.finished:
        _broadcast_rooms()
    _schedule_bot_turn(room_id)


def _play_bot_turn(room_id, turn_id=None, turn_number=None):
    """A soron lévő robot meglép egy lépést. Visszatér: True, ha lépett.

    turn_id / turn_number: ha megadva, csak akkor lép, ha közben nem érvénytelenedett a lépés
    (kilépés, új kör, mentés-visszaállítás).
    """
    room = state.rooms.get(room_id)
    if not room or (turn_id is not None and room.bot_turn_id != turn_id):
        return False
    game = room.game
    if not _bot_should_move(room):
        return False
    bot = game.current_player()
    started_turn = game.turn_number
    if turn_number is not None and started_turn != turn_number:
        return False

    try:
        action = ai_player.choose_action(
            game.board, list(bot.hand), game.bot_level(bot) or ai_player.DEFAULT_DIFFICULTY,
            game.bag.remaining(), yield_fn=lambda: socketio.sleep(0),
            avoid=game.rejected_placements)
    except Exception as e:  # a robot hibája ne akassza meg a játékot
        print(f"[bot] Hiba a lépés keresésében ({room_id}): {e}")
        action = {'action': 'pass'}

    # A keresés közben (kooperatív váltásnál) megváltozhatott az állapot
    room = state.rooms.get(room_id)
    if (not room or room.game is not game or game.finished or game.pending_challenge
            or game.current_player() is not bot or game.turn_number != started_turn
            or (turn_id is not None and room.bot_turn_id != turn_id)):
        return False

    success = False
    if action['action'] == 'place':
        success, _msg, _score = game.place_tiles(bot.id, action['tiles'])
    elif action['action'] == 'exchange':
        success, _msg = game.exchange_tiles(bot.id, action['indices'])
    if not success:
        success, _msg = game.pass_turn(bot.id)
    if not success:
        return False

    _after_bot_move(room_id, room, game)
    return True


# --- Game persistence ---

def _user_id_for_player(room, player):
    """A játékos regisztrált user_id-ja: élő auth info, különben a mentésből ismert (visszaállított
    játéknál a még meg nem érkezett játékosok is megtartják a fiókjukat)."""
    auth_info = state.player_auth.get(player.id) or {}
    user_id = auth_info.get('user_id')
    if user_id is None:
        user_id = room.known_user_ids.get(player.name)
    return user_id


def _save_game_to_db(room_id):
    """Játék mentése az adatbázisba (manuális mentés)."""
    room = state.rooms.get(room_id)
    if not room:
        return False, "A szoba nem létezik."
    game = room.game
    if game.puzzle is not None:
        return True, "Játék mentve."   # a napi feladvány nem mentődik (az eredmény külön rögzül)
    if not game.started:
        return False, "A játék még nem indult el."

    try:
        state_json = json.dumps(game.to_save_dict(), ensure_ascii=False)
        owner_name = state.player_names.get(room.owner, room.owner_name)
        has_bots = any(p.is_bot for p in game.players)

        if game.finished:
            if room.result_saved:
                # A végeredményt már rögzítettük (statisztika ne duplázódjon).
                return True, "Játék mentve."
            players_data = []
            for p in game.players:
                players_data.append({
                    'player_name': p.name,
                    'user_id': _user_id_for_player(room, p),
                    'final_score': p.score,
                    'is_winner': any(w.name == p.name for w in game.winners),
                    'resigned': p.resigned,
                })
            db_id = finish_game(room_id, state_json, players_data, room_name=room.name,
                                has_bots=has_bots)
            room.db_game_id = db_id
            room.result_saved = True
            _award_achievements(game, players_data, db_id)
            _announce_rating_changes(game, players_data, db_id)
            _announce_saved_game(game, players_data, db_id)
            socketio.emit('rooms_list', state.get_rooms_list())
            socketio.emit('live_games', state.get_live_games())
        else:
            players_data = []
            for p in game.players:
                players_data.append({
                    'player_name': p.name,
                    'user_id': _user_id_for_player(room, p),
                    'score': p.score,
                })
            db_id = save_game(room_id, room.name, state_json, game.challenge_mode,
                              players_data, owner_name=owner_name,
                              owner_token=getattr(room, 'owner_token', None),
                              has_bots=has_bots, is_async=room.is_async)
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


def _award_achievements(game, players_data, db_id):
    """A befejezett játék kitüntetéseinek rögzítése; az újakról a játékos értesítést kap."""
    try:
        name_to_user = {pd['player_name']: pd['user_id'] for pd in players_data if pd.get('user_id')}
        per_game = achievements.evaluate_game(game, name_to_user)
        for player in game.players:
            uid = name_to_user.get(player.name)
            if not uid or player.is_bot:
                continue
            user = get_user_by_id(uid)
            badges = set(per_game.get(uid, ()))
            if user:
                badges |= achievements.cumulative_badges(user['games_played'], user['games_won'])
            new = grant_achievements(uid, badges, db_id)
            if new:
                socketio.emit('achievements_earned', {'badges': new}, room=player.id)
    except Exception as e:  # a kitüntetés hibája ne akadályozza a mentést
        print(f"[achievements] Hiba a kitüntetések rögzítésénél: {e}")


def _announce_saved_game(game, players_data, db_id):
    """A regisztrált játékosoknak elküldi a befejezett játék azonosítóját (visszajátszás, elemzés)."""
    name_to_user = {pd['player_name']: pd['user_id'] for pd in players_data if pd.get('user_id')}
    for player in game.players:
        if not player.is_bot and player.name in name_to_user:
            socketio.emit('game_saved', {'game_id': db_id}, room=player.id)


def _announce_rating_changes(game, players_data, db_id):
    """Az értékelt játékosoknak elküldi az új értékszámukat és a változást."""
    try:
        changes = get_game_rating_changes(db_id)
        name_to_user = {pd['player_name']: pd['user_id'] for pd in players_data if pd.get('user_id')}
        for player in game.players:
            if player.is_bot:
                continue
            uid = name_to_user.get(player.name)
            if uid in changes:
                before, after = changes[uid]
                socketio.emit('rating_update', {'rating': after, 'change': after - before},
                              room=player.id)
    except Exception as e:  # az értékszám kijelzése ne akadályozza a mentést
        print(f"[rating] Hiba az értékszám küldésénél: {e}")


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
    spectated_room = state.remove_spectator(sid)
    if spectated_room and spectated_room in state.rooms:
        _emit_all_states(state.rooms[spectated_room].game, spectated_room)
    room_id = state.player_rooms.get(sid)
    token = state.get_reconnect_token_for_sid(sid)
    auth_info = state.player_auth.get(sid, {})
    user_id = auth_info.get('user_id') if isinstance(auth_info, dict) else None
    is_registered_user = bool(user_id and not auth_info.get('is_guest')) if isinstance(auth_info, dict) else False
    was_online = state.is_user_online(user_id) if is_registered_user else False
    kept_for_grace = False

    if room_id and room_id in state.rooms:
        room = state.rooms[room_id]
        game = room.game

        if room.is_async and game.started and not game.finished:
            # Levelezős játék: a játék tovább él, a játékos bármikor visszatérhet (nincs türelmi idő)
            game.mark_disconnected(sid)
            leave_room(room_id)
            if token:
                state.mark_disconnected(token, sid, room_id, state.player_names.get(sid, '?'),
                                        state.player_auth.get(sid))
            _emit_all_states(game, room_id)
            emit('player_disconnected', {'name': state.player_names.get(sid, '?')}, room=room_id)
            del state.player_rooms[sid]
        elif token and not game.finished:
            # Aktív játék és várakozó szoba: türelmi idő, a játékos a tokenjével visszatérhet
            kept_for_grace = True
            was_owner = room.owner == sid or (room.owner_token is not None and room.owner_token == token)
            grace = (_DISCONNECT_GRACE_PERIOD if game.started or not was_owner
                     else _WAITING_OWNER_GRACE_PERIOD)

            game.mark_disconnected(sid)
            leave_room(room_id)

            disconnect_seq = state.mark_disconnected(token, sid, room_id,
                                                      state.player_names.get(sid, '?'),
                                                      state.player_auth.get(sid))

            def grace_timeout():
                time.sleep(grace)
                if state.disconnect_is_current(token, disconnect_seq):
                    _finalize_player_disconnect(token)

            socketio.start_background_task(grace_timeout)

            _emit_all_states(game, room_id)
            emit('player_disconnected',
                 {'name': state.player_names.get(sid, '?')}, room=room_id)
            del state.player_rooms[sid]
        else:
            # Befejezett játék (vagy token nélküli játékos): azonnali eltávolítás
            game.remove_player(sid)
            leave_room(room_id)

            if not game.human_players() or (room.is_async and not game.has_connected_human()):
                _cleanup_room(room_id)
            else:
                if room.owner == sid:
                    _transfer_ownership(room)
                _emit_all_states(game, room_id)
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
    state.hidden_sids.discard(sid)
    state.remove_online_user(sid)
    if is_registered_user and was_online and not state.is_user_online(user_id):
        _notify_friends_presence_change(user_id, False)
    # player_auth törlése: csak ha NEM grace period-ban van (aktív játék vagy várakozó szoba).
    # Grace period esetén az auth info a _disconnected_players-ben van mentve,
    # és a _finalize_player_disconnect fogja törölni.
    if not kept_for_grace:
        state.player_auth.pop(sid, None)
    rate_limiter.clear_sid(sid)

    emit('rooms_list', state.get_rooms_list(), broadcast=True)


def _notify_friends_presence_change(user_id, online):
    """Értesíti az online barátokat, ha egy felhasználó státusza változott."""
    for friend in auth_get_friends(user_id):
        friend_sids = state.get_user_sids(friend['id'])
        for fsid in friend_sids:
            emit('friend_presence_changed', {
                'friend_id': user_id,
                'online': online,
            }, room=fsid)


def _finalize_player_disconnect(token):
    """Grace period lejárt: játékos végleges eltávolítása."""
    info = state.finalize_disconnect(token)
    if not info:
        return
    room_id = info['room_id']
    old_sid = info['sid']

    # Grace period alatt megőrzött auth info törlése
    state.player_auth.pop(old_sid, None)

    if room_id not in state.rooms:
        return

    room = state.rooms[room_id]
    game = room.game
    is_owner = room.owner == old_sid
    is_active_game = game.started and not game.finished

    # Owner disconnect timeout during active game: disband room
    if is_owner and is_active_game and game.players:
        # Automatikus mentés, ha az owner timeoutol
        success, _ = _save_game_to_db(room_id)
        if success:
            room.manually_saved = True

        _disband_active_room(
            room_id,
            'A szoba tulajdonosa végleg lecsatlakozott. A játék automatikusan mentésre került, és a szoba feloszlott.',
            owner_sid=old_sid,
        )
        socketio.emit('rooms_list', state.get_rooms_list())
        return

    # Ha aktív a játék, NEM vesszük ki a Game objektumból, hogy megmaradjon a mentésben.
    # Csak disconnected=True marad (amit már a disconnect eventnél beállítottunk).
    if not is_active_game:
        game.remove_player(old_sid)

    if not game.has_connected_human():
        # Ha minden ember lecsatlakozott:
        if game.started and not game.finished and not room.manually_saved:
            if room.db_game_id:
                abandon_game_by_id(room.db_game_id)
            else:
                abandon_game(room_id)
        _cleanup_room(room_id)
    else:
        if room.owner == old_sid:
            _transfer_ownership(room)
        
        # Ha az ő köre volt, léptetjük
        if is_active_game:
            _advance_turn_if_needed(room_id, room, game)

        _emit_all_states(game, room_id)
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

    user_id = None
    is_guest = True
    if data.get('is_guest', True) is False:
        # Regisztrált felhasználónak csak aláírt tokennel adhatja ki magát valaki:
        # a kliens által küldött user_id önmagában nem bizonyít semmit.
        verified_id = verify_socket_token(app.config['SECRET_KEY'], data.get('auth_token'))
        user_row = get_user_by_id(verified_id) if verified_id else None
        if user_row:
            user_id = user_row['id']
            is_guest = False
            name = _sanitize_name(user_row['display_name']) or name
        else:
            emit('error', {'message': 'A munkamenet lejárt, jelentkezz be újra.'})

    previous_user_id = state.get_user_id_for_sid(sid)
    was_online = bool(user_id and state.is_user_online(user_id))

    state.register_player(sid, name, {
        'user_id': user_id,
        'is_guest': is_guest,
    })

    if previous_user_id and previous_user_id != user_id \
            and not state.is_user_online(previous_user_id):
        _notify_friends_presence_change(previous_user_id, False)
    if user_id and not was_online and state.is_user_online(user_id):
        _notify_friends_presence_change(user_id, True)


@socketio.on('logout')
def handle_logout():
    """Kijelentkezés: kilépés a szobából és az online azonosság törlése."""
    sid = request.sid
    if state.player_rooms.get(sid):
        handle_leave_room()
    if state.get_spectated_room(sid):
        handle_leave_spectate()
    previous_user_id = state.get_user_id_for_sid(sid)
    state.remove_online_user(sid)
    state.player_auth.pop(sid, None)
    state.player_names.pop(sid, None)
    if previous_user_id and not state.is_user_online(previous_user_id):
        _notify_friends_presence_change(previous_user_id, False)


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

    # Munkamenet-átvétel: a régi kapcsolatot a szerver még élőnek hiszi (pl. a telefon
    # háttérbe került, vagy újratöltődött az oldal), de a token már új kapcsolatról érkezik.
    takeover_old_sid = None
    if not dc_info and token_info:
        takeover_old_sid = _release_live_session(token, token_info)
        dc_info = state.get_disconnected_info(token)

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

    if room.owner == old_sid or (hasattr(room, 'owner_token') and room.owner_token == token):
        room.owner = sid
        room.owner_token = token

    player_name = dc_info['player_name']
    state.player_rooms[sid] = room_id
    state.player_names[sid] = player_name
    state.player_auth.pop(old_sid, None)
    # Visszaállítjuk az auth info-t is ha van
    if 'auth_info' in token_info:
        auth_info = token_info['auth_info'] or {'user_id': None, 'is_guest': True}
        state.player_auth[sid] = auth_info
        # Online tracking visszaállítása (register_player logikája)
        user_id = auth_info.get('user_id') if auth_info else None
        if user_id and not auth_info.get('is_guest'):
            state._sid_to_user_id[sid] = user_id
            state._online_users.setdefault(user_id, set()).add(sid)

    join_room(room_id)

    emit('room_joined', {
        'room_id': room_id,
        'room_name': room.name,
        'is_owner': (hasattr(room, 'owner_token') and room.owner_token == token) or room.owner == sid,
        'challenge_mode': game.challenge_mode,
        'is_private': room.is_private,
        'turn_time_limit': game.turn_time_limit,
        'hint_limit': game.hint_limit,
        'reconnect_token': token,
        'chat_messages': room.chat_messages
    })

    if (hasattr(room, 'owner_token') and room.owner_token == token) or room.owner == sid:
        emit('room_code', {'code': room.join_code})

    _emit_all_states(game, room_id)
    emit('player_reconnected', {'name': player_name}, room=room_id)
    emit('rooms_list', state.get_rooms_list(), broadcast=True)
    _schedule_bot_turn(room_id)

    if takeover_old_sid and takeover_old_sid != sid:
        # A régi (elavult) kapcsolat lezárása, miután az új már átvette a helyét.
        try:
            socketio.server.disconnect(takeover_old_sid, namespace='/')
        except Exception:
            pass


def _release_live_session(token, token_info):
    """Aktív játékban vagy várakozó szobában lévő, még "élő" kapcsolatot lecsatlakozottnak jelöl,
    hogy a token új kapcsolatról átvehető legyen. Visszaadja a régi SID-t, vagy None-t, ha nincs mit átvenni."""
    old_sid = token_info.get('sid')
    room_id = token_info.get('room_id')
    room = state.rooms.get(room_id)
    if not old_sid or not room:
        return None
    game = room.game
    if game.finished or state.player_rooms.get(old_sid) != room_id:
        return None

    game.mark_disconnected(old_sid)
    _sio_leave_room(old_sid, room_id)
    del state.player_rooms[old_sid]
    state.mark_disconnected(
        token, old_sid, room_id,
        state.player_names.get(old_sid) or token_info.get('player_name', '?'),
        state.player_auth.get(old_sid) or token_info.get('auth_info'),
    )
    return old_sid


@socketio.on('get_rooms')
def handle_get_rooms():
    if not rate_limiter.check_socket(request.sid, 'get_rooms'):
        return
    emit('rooms_list', state.get_rooms_list())
    emit('live_games', state.get_live_games())


@socketio.on('create_room')
def handle_create_room(data):
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'create_room'):
        emit('error', {'message': 'Túl sok szoba létrehozás, várj egy kicsit.'})
        return
    if not isinstance(data, dict):
        return
    if state.player_rooms.get(sid) in state.rooms:
        emit('error', {'message': 'Már egy szobában vagy. Előbb lépj ki belőle.'})
        return

    name = _sanitize_room_name(data.get('name', '')) or 'Szoba'
    try:
        max_players = min(max(int(data.get('max_players', 4)), 2), 4)
    except (ValueError, TypeError):
        max_players = 4
    challenge_mode = bool(data.get('challenge_mode', False))
    is_private = bool(data.get('is_private', False))
    try:
        turn_time_limit = int(data.get('turn_time_limit', 0))
    except (ValueError, TypeError):
        turn_time_limit = 0
    if turn_time_limit not in ALLOWED_TURN_TIME_LIMITS:
        turn_time_limit = 0
    try:
        hint_limit = int(data.get('hint_limit', DEFAULT_HINT_LIMIT))
    except (ValueError, TypeError):
        hint_limit = DEFAULT_HINT_LIMIT
    if hint_limit not in ALLOWED_HINT_LIMITS:
        hint_limit = DEFAULT_HINT_LIMIT
    player_name = state.player_names.get(sid, 'Névtelen')

    # Számítógépes ellenfelek: a nehézségek listája; a robotok is férőhelyet foglalnak
    ai_levels = data.get('ai_players', [])
    if not isinstance(ai_levels, list):
        ai_levels = []
    ai_levels = [ai_player.parse_difficulty(lv) for lv in ai_levels]
    ai_levels = [lv for lv in ai_levels if lv is not None][:MAX_BOTS]
    ai_levels = ai_levels[:max_players - 1]

    room_id = str(uuid.uuid4())[:8]
    join_code = state.generate_join_code()
    game = Game(room_id, challenge_mode=challenge_mode, turn_time_limit=turn_time_limit,
                hint_limit=hint_limit)
    game.add_player(sid, player_name)
    for level in ai_levels:
        game.add_bot(_bot_name(game, level), level)

    token = state.generate_reconnect_token(sid, room_id, player_name,
                                            state.player_auth.get(sid))

    room = Room(
        room_id=room_id, game=game, owner_sid=sid, owner_name=player_name,
        name=name, max_players=max_players, join_code=join_code,
        is_private=is_private, owner_token=token
    )
    state.add_room(room)

    state.player_rooms[sid] = room_id
    join_room(room_id)

    emit('room_joined', {
        'room_id': room_id,
        'room_name': name,
        'is_owner': True,
        'challenge_mode': game.challenge_mode,
        'is_private': is_private,
        'turn_time_limit': game.turn_time_limit,
        'hint_limit': game.hint_limit,
        'reconnect_token': token,
        'chat_messages': room.chat_messages
    })
    emit('room_code', {'code': join_code})
    _emit_all_states(game, room_id)
    emit('rooms_list', state.get_rooms_list(), broadcast=True)


_ANY_IDENTITY = object()  # a mentésből nem ismert a hely gazdája (régi mentés): név alapján enged


def _identity_matches(expected_user_id, auth_info):
    """A mentett játék helyét az foglalhatja el, akié volt: regisztrált játékos csak a saját
    fiókjával, vendég hely pedig csak vendéggel (név alapú azonosításnál nincs más igazolás)."""
    if expected_user_id is _ANY_IDENTITY:
        return True
    user_id = auth_info.get('user_id') if auth_info else None
    if expected_user_id is None:
        return not user_id
    return user_id == expected_user_id and not auth_info.get('is_guest')


def _try_late_join(sid, room_id, room, player_name, auth_info):
    """Visszaállított, már elindult játékba a hiányzó játékos utólag becsatlakozik.

    Visszatér: True, ha sikerült (a válaszokat is elküldte).
    """
    game = room.game
    if game.finished or player_name not in room.late_join_names:
        return False
    player = next((p for p in game.players
                   if p.name == player_name and p.disconnected), None)
    if not player or not _identity_matches(room.late_join_names[player_name], auth_info):
        return False

    game.replace_player_sid(player.id, sid)
    room.late_join_names.pop(player_name, None)
    state.player_rooms[sid] = room_id
    join_room(room_id)
    token = state.generate_reconnect_token(sid, room_id, player_name,
                                           state.player_auth.get(sid))

    emit('room_joined', {
        'room_id': room_id,
        'room_name': room.name,
        'is_owner': False,
        'challenge_mode': game.challenge_mode,
        'is_private': room.is_private,
        'turn_time_limit': game.turn_time_limit,
        'hint_limit': game.hint_limit,
        'reconnect_token': token,
        'chat_messages': room.chat_messages,
    })
    _emit_all_states(game, room_id)
    emit('game_started', {})
    emit('player_reconnected', {'name': player_name}, room=room_id)
    emit('rooms_list', state.get_rooms_list(), broadcast=True)
    _schedule_bot_turn(room_id)
    return True


@socketio.on('join_room')
def handle_join_room(data):
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'join_room'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    if not isinstance(data, dict):
        return

    if state.player_rooms.get(sid) in state.rooms:
        emit('error', {'message': 'Már egy szobában vagy. Előbb lépj ki belőle.'})
        return

    code = data.get('code', '')
    code = code.strip() if isinstance(code, str) else ''
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
    auth_info = state.player_auth.get(sid) or {}

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
        if not _identity_matches(room.known_user_ids.get(player_name, _ANY_IDENTITY), auth_info):
            emit('error', {'message': 'Ez a hely egy másik játékosé a mentett játékban.'})
            return

    if game.started:
        if _try_late_join(sid, room_id, room, player_name, auth_info):
            return
        emit('error', {'message': 'A játék már elkezdődött.'})
        return
    if len(game.players) >= room.max_players:
        emit('error', {'message': 'A szoba megtelt.'})
        return
    if any(p.name.casefold() == player_name.casefold() for p in game.players):
        emit('error', {'message': 'Ilyen nevű játékos már van a szobában.'})
        return

    success, msg = game.add_player(sid, player_name)
    if not success:
        emit('error', {'message': msg})
        return

    state.player_rooms[sid] = room_id
    join_room(room_id)

    token = state.generate_reconnect_token(sid, room_id, player_name,
                                            state.player_auth.get(sid))

    join_data = {
        'room_id': room_id,
        'room_name': room.name,
        'is_owner': False,
        'challenge_mode': game.challenge_mode,
        'is_private': room.is_private,
        'turn_time_limit': game.turn_time_limit,
        'hint_limit': game.hint_limit,
        'reconnect_token': token,
        'chat_messages': room.chat_messages
    }
    if room.is_restored and hasattr(room, 'expected_players'):
        join_data['is_restore_lobby'] = True
        join_data['expected_players'] = room.expected_players
    emit('room_joined', join_data)

    _emit_all_states(game, room_id)

    emit('player_joined', {'name': player_name}, room=room_id)
    emit('rooms_list', state.get_rooms_list(), broadcast=True)


@socketio.on('leave_room')
def handle_leave_room(data=None):
    sid = request.sid
    room_id = state.player_rooms.get(sid)
    if not room_id or room_id not in state.rooms:
        return

    room = state.rooms[room_id]
    token = state.get_reconnect_token_for_sid(sid)
    game = room.game
    player_name = state.player_names.get(sid, '?')
    is_owner = room.owner == sid
    is_active_game = game.started and not game.finished

    # Levelezős játék: a kilépés csak a nézetet zárja be, a játék megmarad, és később folytatható
    if room.is_async and is_active_game:
        game.mark_disconnected(sid)
        if token:
            state.mark_disconnected(token, sid, room_id, player_name, state.player_auth.get(sid))
        leave_room(room_id)
        del state.player_rooms[sid]
        emit('room_left', {})
        _emit_all_states(game, room_id)
        emit('player_disconnected', {'name': player_name}, room=room_id)
        return

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
    if is_active_game and token:
        # Aktív játékban csak lecsatlakozottnak jelöljük, nem töröljük
        game.mark_disconnected(sid)
        state.mark_disconnected(token, sid, room_id, player_name,
                                state.player_auth.get(sid))

        # Ha az ő köre volt, léptetjük
        _advance_turn_if_needed(room_id, room, game)

        _emit_all_states(game, room_id)
        emit('player_left', {'name': player_name}, room=room_id)
    else:
        game.remove_player(sid)
        state.cleanup_player_token(sid)

    leave_room(room_id)
    del state.player_rooms[sid]

    if is_active_game and token:
        # Mar kezelve fentebb, semmit nem kell csinalni
        pass
    elif not game.human_players() or (room.is_async and not game.has_connected_human()):
        _cleanup_room(room_id)
    else:
        if room.owner == sid:
            _transfer_ownership(room)
        if not (is_active_game and token):
            _emit_all_states(game, room_id)
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
    token = state.get_reconnect_token_for_sid(sid)
    is_owner = (hasattr(room, 'owner_token') and room.owner_token == token) or room.owner == sid
    if not is_owner:
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
                if p.is_bot:
                    p.disconnected = False
                elif p.name in joined_names:
                    p.id = joined_names[p.name]
                    p.disconnected = False
                else:
                    # Aki nincs ott, azt lecsatlakozottnak jelöljük, de nem töröljük
                    p.disconnected = True
                    # A disconnected info-t is beállítjuk a state-be, ha van mentett tokenjük
                    # (Ez bonyolult, egyelőre elég ha a Game-ben benne maradnak)

            if not any(not p.disconnected for p in restored_game.human_players()):
                emit('error', {'message': 'Nincs csatlakozott játékos a mentett játékból.'})
                return

            # current_player_idx korrekció
            if restored_game.current_player_idx >= len(restored_game.players):
                restored_game.current_player_idx = 0
            # Ha a soron lévő játékos nincs jelen, ne akadjon el a játék rajta
            restored_game.skip_disconnected_current()

            # A korábbi lépések átvétele, hogy a visszajátszás a teljes játékot mutassa
            restored_game.move_log = [
                {
                    'move_number': m['move_number'],
                    'player_name': m['player_name'],
                    'action_type': m['action_type'],
                    'details_json': m['details_json'],
                    'board_snapshot_json': m['board_snapshot_json'],
                }
                for m in get_game_moves(save_data['id'])
            ]
            room.last_saved_move_count = 0

            room.game = restored_game
            room.restore_save_data = None
            room.is_restored = False
            # A hiányzók menet közben még becsatlakozhatnak (név + fiók alapján)
            room.late_join_names = {
                p.name: room.known_user_ids.get(p.name, _ANY_IDENTITY)
                for p in restored_game.players if p.disconnected
            }

            # Abandon the old DB save since we're now playing in a new room
            abandon_game_by_id(save_data['id'])
            room.db_game_id = None

            _start_turn_timer(room_id)
            _emit_all_states(restored_game, room_id)
            emit('game_started', {}, room=room_id)
            emit('rooms_list', state.get_rooms_list(), broadcast=True)
            emit('live_games', state.get_live_games(), broadcast=True)
            _schedule_bot_turn(room_id)

            # Új mentés az új szobához (játékoslista, lépések) — így nem vész el a játék
            _save_game_to_db(room_id)
        except Exception as e:
            print(f"[restore] Hiba a visszaállításnál: {e}")
            emit('error', {'message': 'Hiba a játék visszaállításánál.'})
        return

    game = room.game
    success, msg = game.start()
    if not success:
        emit('error', {'message': msg})
        return

    state.remove_invites_for_room(room_id)
    _start_turn_timer(room_id)
    _emit_all_states(game, room_id)
    emit('game_started', {}, room=room_id)
    emit('rooms_list', state.get_rooms_list(), broadcast=True)
    emit('live_games', state.get_live_games(), broadcast=True)
    _schedule_bot_turn(room_id)

    # Kezdeti mentés, hogy a játékos lista perzisztens legyen
    _save_game_to_db(room_id)


@socketio.on('save_game')
def handle_save_game():
    """Manuális mentés — csak a szoba tulajdonosa használhatja."""
    sid = request.sid
    room_id, room, game = _get_room_context(sid, 'save_game')
    if not room:
        return

    # Csak a szoba tulajdonosa menthet (token vagy SID alapján)
    token = state.get_reconnect_token_for_sid(sid)
    is_owner = (hasattr(room, 'owner_token') and room.owner_token == token) or room.owner == sid
    if not is_owner:
        emit('action_result', {'success': False, 'message': 'Csak a szoba tulajdonosa menthet.'})
        return

    if not game.started or game.finished:
        emit('action_result', {'success': False, 'message': 'Nincs aktív játék a mentéshez.'})
        return

    success, msg = _save_game_to_db(room_id)
    if success:
        room.manually_saved = True
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
    if not isinstance(game_id, int) or isinstance(game_id, bool):
        emit('error', {'message': 'Érvénytelen játék azonosító.'})
        return
    if state.player_rooms.get(sid) in state.rooms:
        emit('error', {'message': 'Már egy szobában vagy. Előbb lépj ki belőle.'})
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
    if game_row.get('is_async'):
        emit('error', {'message': 'A levelezős játékot a Levelezős fülről nyithatod meg.'})
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

    expected_players = [p['name'] for p in s.get('players', []) if not p.get('is_bot')]
    if not expected_players:
        emit('error', {'message': 'Nincs játékos a mentett játékban.'})
        return

    # Várakozó szoba létrehozása
    room_id = str(uuid.uuid4())[:8]
    join_code = state.generate_join_code()
    new_game = Game(room_id, challenge_mode=s.get('challenge_mode', False),
                    hint_limit=s.get('hint_limit', DEFAULT_HINT_LIMIT))
    new_game.add_player(sid, player_name)

    token = state.generate_reconnect_token(sid, room_id, player_name,
                                            state.player_auth.get(sid))

    room = Room(
        room_id=room_id, game=new_game, owner_sid=sid, owner_name=player_name,
        name=game_row.get('room_name', '') or 'Visszaállított szoba',
        max_players=max(len(s.get('players', [])), 2),
        join_code=join_code, is_private=True, owner_token=token
    )
    room.is_restored = True
    room.restore_save_data = game_row
    room.expected_players = expected_players
    room.known_user_ids = {
        gp['player_name']: gp['user_id'] for gp in get_game_players(game_id)
    }

    state.add_room(room)

    state.player_rooms[sid] = room_id
    join_room(room_id)

    emit('room_joined', {
        'room_id': room_id,
        'room_name': room.name,
        'is_owner': True,
        'challenge_mode': new_game.challenge_mode,
        'is_private': True,
        'reconnect_token': token,
        'is_restore_lobby': True,
        'expected_players': expected_players,
        'hint_limit': new_game.hint_limit,
        'chat_messages': room.chat_messages
    })
    emit('room_code', {'code': join_code})
    _emit_all_states(new_game, room_id)
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

    timer_expires_at = room.turn_timer_expires_at
    success, msg, score = game.place_tiles(sid, tiles_placed)

    if success:
        room.invalidate_turn_timer()
        emit('action_result', {'success': True, 'message': msg, 'score': score, 'own_turn': True})
        if game.pending_challenge:
            # Ha a lerakó visszavonja a lerakását, csak a maradék idejét kapja vissza
            room.withdraw_time_left = (max(0.0, timer_expires_at - time.time())
                                       if timer_expires_at else None)
            _start_challenge_timer(room_id)
        elif game.finished and game.puzzle is not None:
            _finish_puzzle(sid, game)
        elif game.finished:
            _save_game_to_db(room_id)
        else:
            _start_turn_timer(room_id)
        _emit_all_states(game, room_id)
        _schedule_bot_turn(room_id)
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
        room.invalidate_turn_timer()
        emit('action_result', {'success': True, 'message': msg, 'own_turn': True})
        if game.finished:
            _save_game_to_db(room_id)
        else:
            _start_turn_timer(room_id)
        _emit_all_states(game, room_id)
        _schedule_bot_turn(room_id)
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
        room.invalidate_turn_timer()
        emit('action_result', {'success': True, 'message': msg, 'own_turn': True})
        if game.finished:
            _save_game_to_db(room_id)
        else:
            _start_turn_timer(room_id)
        _emit_all_states(game, room_id)
        _schedule_bot_turn(room_id)
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


@socketio.on('withdraw_words')
def handle_withdraw_words():
    """A lerakó visszavonja a szavazásra váró lerakását (amíg senki sem szavazott)."""
    sid = request.sid
    room_id, room, game = _get_room_context(sid, 'withdraw_words')
    if not room:
        return

    success, msg = game.withdraw_pending(sid)
    if success:
        room.invalidate_challenge_timer()
        # A lerakás + visszavonás ne adjon új, teljes kört: a lerakáskor hátralévő idő folytatódik
        left = room.withdraw_time_left
        _start_turn_timer(room_id, None if left is None else max(_MIN_TIME_AFTER_WITHDRAW, left))
        emit('action_result', {'success': True, 'message': msg, 'own_turn': True})
        _emit_all_states(game, room_id)
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
    chat_msg = {'name': player_name, 'message': message, 'sid': sid}
    room.add_chat_message(player_name, message)

    emit('chat_message', chat_msg, room=room_id)


# --- Előnézet és tipp ---

@socketio.on('preview_move')
def handle_preview_move(data):
    """A jelenlegi lerakás kipróbálása véglegesítés nélkül: szavak és pontszám (élő előnézet)."""
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'preview_move'):
        return  # az előnézet nem kritikus, csendben eldobjuk
    room_id, room, game = state.get_room_for_player(sid)
    if not room or not isinstance(data, dict):
        return
    tiles_placed, err = _validate_tiles_input(data.get('tiles', []))
    if not tiles_placed:
        emit('move_preview', {'valid': False, 'score': 0, 'words': [], 'message': err or ''})
        return
    emit('move_preview', game.preview_placement(sid, tiles_placed))


@socketio.on('request_hint')
def handle_request_hint():
    """Tipp kérése: a legjobb lépések. Csak egyedül (robotok ellen) játszva elérhető."""
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'request_hint'):
        emit('hint_result', {'success': False, 'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    room_id, room, game = state.get_room_for_player(sid)
    if not room:
        return
    if game.hint_limit == 0:
        emit('hint_result', {'success': False,
                             'message': 'A tippek ki vannak kapcsolva ebben a szobában.'})
        return
    if not game.started or game.finished or game.pending_challenge:
        emit('hint_result', {'success': False, 'message': 'Most nem kérhetsz tippet.'})
        return
    if game.hints_left() <= 0:
        emit('hint_result', {'success': False, 'message': 'Elfogytak a tippjeid.', 'hints_left': 0})
        return
    if len(game.human_players()) != 1:
        emit('hint_result', {'success': False,
                             'message': 'Tippet csak egyedül, robotok ellen játszva kérhetsz.'})
        return
    player = game.current_player()
    if not player or player.id != sid:
        emit('hint_result', {'success': False, 'message': 'Nem te következel.'})
        return

    try:
        moves = ai_player.best_moves(game.board, list(player.hand), count=3,
                                     yield_fn=lambda: socketio.sleep(0))
    except Exception as e:  # pl. hiányzó szótárfájl: a játék ettől még folytatható
        print(f"[hint] Hiba a tipp keresésében ({room_id}): {e}")
        emit('hint_result', {'success': False, 'message': 'Nem sikerült tippet adni.'})
        return

    # A keresés közben (kooperatív váltásnál) elfogyhatott a tipp vagy véget érhetett a kör
    if game.finished or game.current_player() is not player:
        emit('hint_result', {'success': False, 'message': 'Most nem kérhetsz tippet.'})
        return
    if moves and not game.use_hint():
        emit('hint_result', {'success': False, 'message': 'Elfogytak a tippjeid.', 'hints_left': 0})
        return
    if moves:
        _emit_all_states(game, room_id)  # a hátralévő tippek száma frissül
    emit('hint_result', {
        'success': True,
        'message': '',
        'hints_left': game.hints_left(),
        'moves': [
            {
                'tiles': [{'row': r, 'col': c, 'letter': l, 'is_blank': b} for r, c, l, b in m.tiles],
                'words': m.words,
                'score': m.score,
            }
            for m in moves
        ],
    })


@socketio.on('set_visibility')
def handle_set_visibility(data):
    """A kliens jelzi, ha a lapja háttérbe került: ilyenkor a saját körről push értesítést kap."""
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'set_visibility') or not isinstance(data, dict):
        return
    if data.get('hidden') is True:
        state.hidden_sids.add(sid)
        room_id, room, game = state.get_room_for_player(sid)
        if room:
            _maybe_push_turn(room, game)   # már az ő köre van: azonnal értesítjük
    else:
        state.hidden_sids.discard(sid)


# --- Napi feladvány ---

def _registered_user_id(sid):
    auth_info = state.player_auth.get(sid) or {}
    return None if auth_info.get('is_guest') else auth_info.get('user_id')


def _daily_started_payload(sid, date_str):
    user_id = _registered_user_id(sid)
    return {'date': date_str, 'registered': bool(user_id),
            'my': get_daily_entry(date_str, user_id) if user_id else None}


@socketio.on('start_daily')
def handle_start_daily():
    """Napi feladvány indítása: egyjátékos, nem listázott szoba a nap közös állásával."""
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'start_daily'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    if state.player_rooms.get(sid) in state.rooms:
        emit('error', {'message': 'Már egy szobában vagy. Előbb lépj ki belőle.'})
        return
    try:
        puzzle = daily.ensure_puzzle(yield_fn=lambda: socketio.sleep(0))
    except Exception as e:
        print(f"[daily] Nem sikerült a feladvány: {e}")
        emit('error', {'message': 'A napi feladvány most nem érhető el.'})
        return
    # A feladvány előállítása alatt (kooperatív váltás) egy második kérés már szobát nyithatott
    if state.player_rooms.get(sid) in state.rooms:
        return

    player_name = state.player_names.get(sid, 'Névtelen')
    room_id = str(uuid.uuid4())[:8]
    join_code = state.generate_join_code()
    game = daily.build_game(room_id, sid, player_name, puzzle)
    token = state.generate_reconnect_token(sid, room_id, player_name, state.player_auth.get(sid))
    room = Room(room_id=room_id, game=game, owner_sid=sid, owner_name=player_name,
                name='Napi feladvány', max_players=1, join_code=join_code,
                is_private=True, owner_token=token)
    room.is_puzzle = True
    state.add_room(room)
    state.player_rooms[sid] = room_id
    join_room(room_id)

    emit('room_joined', {
        'room_id': room_id, 'room_name': room.name, 'is_owner': True, 'challenge_mode': False,
        'is_private': True, 'turn_time_limit': 0, 'hint_limit': 0,
        'reconnect_token': token, 'chat_messages': [],
    })
    _emit_all_states(game, room_id)
    emit('game_started', {})
    emit('daily_started', _daily_started_payload(sid, puzzle['date']))


def _finish_puzzle(sid, game):
    """A beküldött lépés rögzítése (ranglista, kitüntetés) és az eredmény elküldése."""
    puzzle = get_daily_puzzle(game.puzzle['date'])
    if not puzzle:
        return
    details = json.loads(game.move_log[-1]['details_json'])
    user_id = _registered_user_id(sid)
    result = daily.record_result(puzzle, user_id, game.puzzle['score'],
                                 details.get('tiles', []), details.get('words', []))
    if user_id and result['recorded'] and result['is_best']:
        new = grant_achievements(user_id, {'daily_best'})
        if new:
            socketio.emit('achievements_earned', {'badges': new}, room=sid)
    socketio.emit('daily_result', result, room=sid)


@socketio.on('retry_daily')
def handle_retry_daily():
    """Új próbálkozás ugyanarra a feladványra (a legjobb eredmény számít)."""
    sid = request.sid
    room_id, room, game = _get_room_context(sid, 'retry_daily')
    if not room or game.puzzle is None or not game.finished:
        return
    puzzle = get_daily_puzzle(game.puzzle['date'])
    if not puzzle:
        return
    daily.reset_game(game, puzzle)
    _emit_all_states(game, room_id)
    emit('daily_started', _daily_started_payload(sid, puzzle['date']))


@socketio.on('reveal_daily')
def handle_reveal_daily():
    """A megoldás megmutatása: innentől a további próbálkozások nem kerülnek a ranglistára."""
    sid = request.sid
    room_id, room, game = _get_room_context(sid, 'reveal_daily')
    if not room or game.puzzle is None:
        return
    puzzle = get_daily_puzzle(game.puzzle['date'])
    if not puzzle:
        return
    user_id = _registered_user_id(sid)
    if user_id:
        mark_daily_revealed(puzzle['date'], user_id)
    emit('daily_solution', {'tiles': puzzle['best']['tiles'], 'words': puzzle['best']['words'],
                            'score': puzzle['best_score'], 'ranked': False})


# --- Levelezős (aszinkron) játék ---

def _load_async_room(row):
    """A mentett levelezős játékból szobát épít (minden játékos lecsatlakozottként). None, ha hibás."""
    try:
        game = Game.from_save_dict(json.loads(row['state_json']))
    except Exception as e:
        print(f"[async] A mentett játék nem tölthető be (game #{row['id']}): {e}")
        return None
    game.async_mode = True
    room_id = row['room_id']
    known = {gp['player_name']: gp['user_id'] for gp in get_game_players(row['id'])}
    for player in game.players:
        if not player.is_bot:
            player.disconnected = True
            if known.get(player.name):
                player.id = async_games.placeholder_id(room_id, known[player.name])
    game.move_log = [
        {
            'move_number': m['move_number'], 'player_name': m['player_name'],
            'action_type': m['action_type'], 'details_json': m['details_json'],
            'board_snapshot_json': m['board_snapshot_json'],
        }
        for m in get_game_moves(row['id'])
    ]
    room = Room(room_id=room_id, game=game, owner_sid=None, owner_name=row.get('owner_name') or '',
                name=row.get('room_name') or 'Levelezős játék', max_players=len(game.players),
                join_code=state.generate_join_code(), is_private=True)
    room.is_async = True
    room.db_game_id = row['id']
    room.known_user_ids = known
    room.last_saved_move_count = len(game.move_log)
    room.persisted_sig = (len(game.move_log), game.finished, game.turn_number)
    state.add_room(room)
    return room


def _async_room_for_db_game(row):
    """A levelezős játék szobája: a memóriában lévő, ennek híján az adatbázisból visszaépített."""
    for room in state.rooms.values():
        if room.is_async and room.db_game_id == row['id']:
            return room
    return _load_async_room(row)


def restore_async_games():
    """Szerverindításkor: a folyamatban lévő levelezős játékok szobáinak visszaépítése."""
    restored = 0
    for row in get_active_async_games():
        if _async_room_for_db_game(row):
            restored += 1
    return restored


def _cleanup_finished_async(room_id):
    """A befejezett levelezős játék szobája megszűnik, ha senki sem néz éppen."""
    room = state.rooms.get(room_id)
    if room and room.is_async and room.game.finished and not room.game.has_connected_human():
        _cleanup_room(room_id)


def _expire_async_turns(now=None):
    """Lejárt határidejű körök: automatikus passz (három egymás utáni után a játékos feladja).
    Visszatér: a léptetett játékok száma."""
    now = now if now is not None else time.time()
    expired = 0
    for room_id, room in list(state.rooms.items()):
        game = room.game
        if not room.is_async or not game.started or game.finished:
            continue
        if game.turn_deadline and now > game.turn_deadline and game.expire_turn():
            expired += 1
            _emit_all_states(game, room_id)
            _cleanup_finished_async(room_id)
    return expired


def _async_sweeper():
    """Háttérfeladat: percenként ellenőrzi a levelezős játékok határidejét."""
    while True:
        socketio.sleep(60)
        try:
            _expire_async_turns()
        except Exception as e:
            print(f"[async] Hiba a határidők ellenőrzésénél: {e}")


@socketio.on('create_async_game')
def handle_create_async_game(data):
    """Levelezős játék indítása barátokkal: a barátok a Levelezős fülön látják, és értesítést kapnak."""
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'create_async_game'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    if not isinstance(data, dict):
        return
    user_id = _registered_user_id(sid)
    if not user_id:
        emit('error', {'message': 'Csak regisztrált felhasználók játszhatnak levelezős játékot.'})
        return
    if state.player_rooms.get(sid) in state.rooms:
        emit('error', {'message': 'Már egy szobában vagy. Előbb lépj ki belőle.'})
        return

    ids = data.get('friend_ids')
    if (not isinstance(ids, list) or not 1 <= len(ids) <= async_games.MAX_FRIENDS
            or any(not isinstance(i, int) or isinstance(i, bool) for i in ids) or len(set(ids)) != len(ids)):
        emit('error', {'message': 'Válassz egy-három barátot a játékhoz.'})
        return
    friends_by_id = {f['id']: f for f in auth_get_friends(user_id)}
    if any(i not in friends_by_id for i in ids):
        emit('error', {'message': 'Csak barátaidat hívhatod meg.'})
        return
    try:
        turn_hours = int(data.get('turn_hours', async_games.DEFAULT_TURN_HOURS))
    except (ValueError, TypeError):
        turn_hours = async_games.DEFAULT_TURN_HOURS
    if turn_hours not in async_games.ALLOWED_TURN_HOURS:
        turn_hours = async_games.DEFAULT_TURN_HOURS

    name = _sanitize_room_name(data.get('name', '')) or 'Levelezős játék'
    player_name = state.player_names.get(sid, 'Névtelen')
    room_id = str(uuid.uuid4())[:8]
    friends = [{'id': i, 'name': _sanitize_name(friends_by_id[i]['display_name']) or 'Játékos'} for i in ids]
    game, known = async_games.build_game(room_id, sid, player_name, user_id, friends, turn_hours)

    token = state.generate_reconnect_token(sid, room_id, player_name, state.player_auth.get(sid))
    room = Room(room_id=room_id, game=game, owner_sid=sid, owner_name=player_name, name=name,
                max_players=len(game.players), join_code=state.generate_join_code(),
                is_private=True, owner_token=token)
    room.is_async = True
    room.known_user_ids = known
    state.add_room(room)
    state.player_rooms[sid] = room_id
    join_room(room_id)

    emit('room_joined', {
        'room_id': room_id, 'room_name': name, 'is_owner': False, 'challenge_mode': False,
        'is_private': True, 'turn_time_limit': 0, 'hint_limit': 0, 'is_async': True,
        'reconnect_token': token, 'chat_messages': [],
    })
    _emit_all_states(game, room_id)   # elmenti a játékot, és értesíti a kezdő játékost, ha nem te vagy az
    emit('game_started', {})

    starter = game.current_player().name
    for friend in friends:
        for friend_sid in list(state.get_user_sids(friend['id'])):
            socketio.emit('async_invited', {'game_id': room.db_game_id, 'room_name': name,
                                            'from_name': player_name}, room=friend_sid)
        if known.get(starter) != friend['id']:   # a kezdő játékos a „Te jössz!” értesítést kapja
            push_service.notify_turn(friend['id'], name, room_id, kind='invite')


@socketio.on('open_async_game')
def handle_open_async_game(data):
    """Egy folyamatban lévő levelezős játék megnyitása (a lobby listájából)."""
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'open_async_game'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    if not isinstance(data, dict):
        return
    game_id = data.get('game_id')
    if not isinstance(game_id, int) or isinstance(game_id, bool):
        emit('error', {'message': 'Érvénytelen játék azonosító.'})
        return
    user_id = _registered_user_id(sid)
    if not user_id:
        emit('error', {'message': 'Csak regisztrált felhasználók játszhatnak levelezős játékot.'})
        return
    if state.player_rooms.get(sid) in state.rooms:
        emit('error', {'message': 'Már egy szobában vagy. Előbb lépj ki belőle.'})
        return
    row = get_game_by_id(game_id)
    if not row or not row.get('is_async') or row['status'] != 'active':
        emit('error', {'message': 'Ez a levelezős játék nem található.'})
        return
    if not is_user_in_game(game_id, user_id):
        emit('error', {'message': 'Nem vagy részese ennek a játéknak.'})
        return
    room = _async_room_for_db_game(row)
    if room is None:
        emit('error', {'message': 'Ez a levelezős játék nem tölthető be.'})
        return

    game = room.game
    my_name = next((n for n, uid in room.known_user_ids.items() if uid == user_id), None)
    player = next((p for p in game.players if p.name == my_name), None)
    if player is None:
        emit('error', {'message': 'Nem vagy részese ennek a játéknak.'})
        return
    old_sid = player.id
    if not player.disconnected and state.player_rooms.get(old_sid) == room.id:
        # Másik eszközön már meg van nyitva: onnan átvesszük
        _sio_leave_room(old_sid, room.id)
        state.player_rooms.pop(old_sid, None)
        state.cleanup_player_token(old_sid)
        socketio.emit('room_left', {}, room=old_sid)

    game.replace_player_sid(old_sid, sid)
    state.player_rooms[sid] = room.id
    join_room(room.id)
    token = state.generate_reconnect_token(sid, room.id, player.name, state.player_auth.get(sid))
    emit('room_joined', {
        'room_id': room.id, 'room_name': room.name, 'is_owner': False, 'challenge_mode': False,
        'is_private': True, 'turn_time_limit': 0, 'hint_limit': 0, 'is_async': True,
        'reconnect_token': token, 'chat_messages': room.chat_messages,
    })
    _emit_all_states(game, room.id)
    emit('game_started', {})
    emit('player_reconnected', {'name': player.name}, room=room.id)


@socketio.on('resign_game')
def handle_resign_game():
    """Levelezős játék feladása: a játék véget ér, a feladó nem lehet győztes."""
    sid = request.sid
    room_id, room, game = _get_room_context(sid, 'resign_game')
    if not room or not room.is_async:
        return
    success, msg = game.resign(sid)
    emit('action_result', {'success': success, 'message': msg, **({'own_turn': True} if success else {})})
    if success:
        _emit_all_states(game, room_id)   # elmenti a végeredményt


# --- Megfigyelő mód ---

@socketio.on('spectate_room')
def handle_spectate_room(data):
    """Folyamatban lévő játék megfigyelése játékosként való részvétel nélkül."""
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'spectate_room'):
        emit('error', {'message': 'Túl sok kérés, várj egy kicsit.'})
        return
    if not isinstance(data, dict):
        return
    if state.player_rooms.get(sid) in state.rooms or state.get_spectated_room(sid) in state.rooms:
        emit('error', {'message': 'Már egy szobában vagy. Előbb lépj ki belőle.'})
        return

    code = data.get('code', '')
    code = code.strip() if isinstance(code, str) else ''
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
    elif not isinstance(room_id, str) or not room_id or len(room_id) > 8:
        emit('error', {'message': 'Szoba kód vagy azonosító szükséges.'})
        return

    room = state.rooms.get(room_id)
    if not room:
        emit('error', {'message': 'A szoba nem létezik.'})
        return
    if room.is_private and not joined_by_code:
        emit('error', {'message': 'Ez egy privát szoba. A megfigyeléshez kód szükséges.'})
        return
    game = room.game
    if not game.started or game.finished:
        emit('error', {'message': 'Csak folyamatban lévő játékot lehet megfigyelni.'})
        return
    if len(room.spectators) >= Room.MAX_SPECTATORS:
        emit('error', {'message': 'A szobának nem fér több megfigyelője.'})
        return

    join_room(room_id)
    room.spectators[sid] = state.player_names.get(sid, 'Névtelen')
    state.add_spectator(sid, room_id)

    emit('spectate_joined', {
        'room_id': room_id,
        'room_name': room.name,
        'challenge_mode': game.challenge_mode,
        'turn_time_limit': game.turn_time_limit,
        'hint_limit': game.hint_limit,
        'chat_messages': room.chat_messages,
    })
    _emit_all_states(game, room_id)
    # Ha csak robotok vannak soron, a néző jelenléte újra elindítja a robotlépéseket
    _schedule_bot_turn(room_id)


@socketio.on('leave_spectate')
def handle_leave_spectate(data=None):
    """Kilépés a megfigyelésből."""
    sid = request.sid
    room_id = state.remove_spectator(sid)
    if not room_id:
        return
    if room_id in state.rooms:
        leave_room(room_id)
        _emit_all_states(state.rooms[room_id].game, room_id)
    emit('spectate_left', {})


# --- Friendship and Invites ---

@socketio.on('send_friend_request')
def handle_send_friend_request(data):
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'send_friend_request'):
        return
    if not isinstance(data, dict):
        return
        
    auth_info = state.player_auth.get(sid, {})
    user_id = auth_info.get('user_id')
    if not user_id or auth_info.get('is_guest'):
        emit('friend_request_result', {'success': False, 'message': 'Csak regisztrált felhasználók küldhetnek barátkérést.'})
        return
        
    friend_id = data.get('friend_id')
    if not isinstance(friend_id, int):
        return
        
    success, msg = auth_send_friend_request(user_id, friend_id)
    emit('friend_request_result', {'success': success, 'message': msg})
    
    if success:
        # Értesítés a fogadónak ha online
        friend_sids = state.get_user_sids(friend_id)
        if friend_sids:
            sender_name = state.player_names.get(sid, 'Ismeretlen')
            for fsid in friend_sids:
                emit('friend_request_received', {
                    'from_user_id': user_id, 
                    'from_name': sender_name
                }, room=fsid)


@socketio.on('accept_friend_request')
def handle_accept_friend_request(data):
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'accept_friend_request'):
        return
    if not isinstance(data, dict):
        return
        
    auth_info = state.player_auth.get(sid, {})
    user_id = auth_info.get('user_id')
    if not user_id or auth_info.get('is_guest'):
        return
        
    requester_id = data.get('requester_id')
    if not isinstance(requester_id, int):
        return
        
    success, msg = auth_accept_friend_request(user_id, requester_id)
    emit('friend_request_result', {'success': success, 'message': msg})
    
    if success:
        user_name = state.player_names.get(sid, 'Ismeretlen')
        requester_sids = state.get_user_sids(requester_id)
        for rsid in requester_sids:
            emit('friend_request_accepted', {
                'user_id': user_id,
                'display_name': user_name
            }, room=rsid)


@socketio.on('decline_friend_request')
def handle_decline_friend_request(data):
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'decline_friend_request'):
        return
    if not isinstance(data, dict):
        return
        
    auth_info = state.player_auth.get(sid, {})
    user_id = auth_info.get('user_id')
    if not user_id or auth_info.get('is_guest'):
        return
        
    requester_id = data.get('requester_id')
    if not isinstance(requester_id, int):
        return
        
    success, msg = auth_decline_friend_request(user_id, requester_id)
    emit('friend_request_result', {'success': success, 'message': msg})


@socketio.on('remove_friend')
def handle_remove_friend(data):
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'remove_friend'):
        return
    if not isinstance(data, dict):
        return
        
    auth_info = state.player_auth.get(sid, {})
    user_id = auth_info.get('user_id')
    if not user_id or auth_info.get('is_guest'):
        return
        
    friend_id = data.get('friend_id')
    if not isinstance(friend_id, int):
        return
        
    success, msg = auth_remove_friend(user_id, friend_id)
    emit('friend_request_result', {'success': success, 'message': msg})


@socketio.on('invite_to_room')
def handle_invite_to_room(data):
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'invite_to_room'):
        return
    if not isinstance(data, dict):
        return
        
    auth_info = state.player_auth.get(sid, {})
    user_id = auth_info.get('user_id')
    if not user_id or auth_info.get('is_guest'):
        emit('invite_sent', {'success': False, 'message': 'Csak regisztrált felhasználók hívhatnak meg barátokat.'})
        return
        
    friend_id = data.get('friend_id')
    if not isinstance(friend_id, int):
        return
        
    # Barátság ellenőrzése
    friends = auth_get_friends(user_id)
    if not any(f['id'] == friend_id for f in friends):
        emit('invite_sent', {'success': False, 'message': 'Csak barátaidat hívhatod meg.'})
        return
        
    room_id = state.player_rooms.get(sid)
    if not room_id or room_id not in state.rooms:
        emit('invite_sent', {'success': False, 'message': 'Nem vagy szobában.'})
        return
        
    room = state.rooms[room_id]
    game = room.game
    if game.started:
        emit('invite_sent', {'success': False, 'message': 'A játék már elkezdődött.'})
        return
    if len(game.players) >= room.max_players:
        emit('invite_sent', {'success': False, 'message': 'A szoba megtelt.'})
        return
        
    if not state.is_user_online(friend_id):
        emit('invite_sent', {'success': False, 'message': 'A barátod jelenleg nincs online.'})
        return
        
    # Check if friend is already in a room
    friend_sids = state.get_user_sids(friend_id)
    friend_in_room = False
    for fsid in friend_sids:
        if state.player_rooms.get(fsid):
            friend_in_room = True
            break
            
    if friend_in_room:
        emit('invite_sent', {'success': False, 'message': 'A barátod már egy másik szobában van.'})
        return
        
    invite_id = state.create_invite(user_id, friend_id, room_id, sid)
    sender_name = state.player_names.get(sid, 'Ismeretlen')
    
    for fsid in friend_sids:
        emit('game_invite', {
            'invite_id': invite_id,
            'from_name': sender_name,
            'room_name': room.name,
            'room_id': room_id,
            'join_code': room.join_code
        }, room=fsid)
        
    emit('invite_sent', {'success': True, 'message': 'Meghívó elküldve!'})


@socketio.on('respond_invite')
def handle_respond_invite(data):
    sid = request.sid
    if not rate_limiter.check_socket(sid, 'respond_invite'):
        return
    if not isinstance(data, dict):
        return
        
    auth_info = state.player_auth.get(sid, {})
    user_id = auth_info.get('user_id')
    if not user_id:
        return
        
    invite_id = data.get('invite_id')
    accept = data.get('accept', False)
    
    if not isinstance(invite_id, int):
        return
        
    invite = state.get_invite(invite_id)
    if not invite or invite['to_user_id'] != user_id:
        emit('error', {'message': 'Meghívó nem található vagy lejárt.'})
        return
        
    state.remove_invite(invite_id)
    
    if accept:
        room_id = invite['room_id']
        if room_id not in state.rooms:
            emit('error', {'message': 'A szoba már nem létezik.'})
            return
            
        room = state.rooms[room_id]
        emit('invite_accepted', {
            'room_id': room_id,
            'join_code': room.join_code
        })
    else:
        emit('invite_declined', {'success': True})


# ===== Main =====

if __name__ == '__main__':
    from auth import cleanup_expired

    port = int(os.environ.get('PORT', 5000))
    use_tunnel = '--no-tunnel' not in sys.argv

    cleanup_expired()
    _cleanup_finished_saves()
    dictionary.warm_up()  # a szótár betöltése indításkor (az első lerakásnál ne kelljen várni)
    word_review.refresh()  # a szótár-építőn elutasított szavak kizárása (az adatbázisból)
    ai_player.get_vocabulary()  # a robot szókincse (a ragozott alakokkal ~2 mp) is előre épüljön fel
    practice.warm_up()  # gyakorló módok: tőszavak, a 2–3 zsetonos szavak listái
    try:
        daily.ensure_puzzle()  # a mai feladvány is készen álljon
    except Exception as e:
        print(f"[daily] A mai feladvány előállítása nem sikerült: {e}")
    print(f"[async] {restore_async_games()} levelezős játék visszaállítva")
    socketio.start_background_task(_async_sweeper)

    if use_tunnel:
        start_tunnel(port)

    socketio.run(app, host='0.0.0.0', port=port, debug=False,
                 use_reloader=False)
