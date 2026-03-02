import re

from flask import Blueprint, render_template, request, jsonify, make_response

from config import SMTP_CONFIGURED
from auth import (
    get_user_by_email, create_user, verify_password,
    create_verification_code, verify_code as auth_verify_code,
    create_session, validate_session, delete_session,
    get_game_moves, get_user_game_history, get_game_by_id,
    get_user_active_games, abandon_game_by_id, is_user_in_game,
)
from email_service import send_verification_email

# Inicializáláskor beállítandó (server.py-ból init_routes() hívással)
_rate_limiter = None
_state = None
_socketio = None

_VALID_NAME_RE = re.compile(r'^[\w\sáéíóöőúüűÁÉÍÓÖŐÚÜŰ._-]{1,20}$', re.UNICODE)
_VALID_EMAIL_RE = re.compile(r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$')

main_bp = Blueprint('main', __name__)
auth_bp = Blueprint('auth', __name__)
game_bp = Blueprint('game', __name__)


def init_routes(rate_limiter, state, socketio):
    """Route-ok inicializálása a szükséges függőségekkel."""
    global _rate_limiter, _state, _socketio
    _rate_limiter = rate_limiter
    _state = state
    _socketio = socketio


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


# ===== MAIN ROUTES =====

@main_bp.route('/')
def index():
    return render_template('index.html')


# ===== AUTH ROUTES =====

@auth_bp.route('/api/auth/request-code', methods=['POST'])
def request_code():
    ip = _get_client_ip()
    if not _rate_limiter.check_ip(ip, 'request_code'):
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


@auth_bp.route('/api/auth/verify-code', methods=['POST'])
def verify_code():
    data = request.get_json(silent=True)
    if not data:
        return jsonify({'success': False, 'message': 'Érvénytelen kérés.'}), 400

    email = data.get('email', '').strip().lower()
    code = data.get('code', '').strip()

    if not email or not code:
        return jsonify({'success': False, 'message': 'Email és kód megadása kötelező.'}), 400

    if not re.match(r'^\d{6}$', code):
        return jsonify({'success': False, 'message': 'A kód 6 számjegyből áll.'}), 400

    success, message = auth_verify_code(email, code)
    return jsonify({'success': success, 'message': message}), (200 if success else 400)


@auth_bp.route('/api/auth/register', methods=['POST'])
def register():
    ip = _get_client_ip()
    if not _rate_limiter.check_ip(ip, 'register'):
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


@auth_bp.route('/api/auth/login', methods=['POST'])
def login():
    ip = _get_client_ip()
    if not _rate_limiter.check_ip(ip, 'login'):
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


@auth_bp.route('/api/auth/logout', methods=['POST'])
def logout():
    token = request.cookies.get('session_token')
    if token:
        delete_session(token)
    resp = make_response(jsonify({'success': True, 'message': 'Kijelentkezve.'}))
    resp.delete_cookie('session_token')
    return resp


@auth_bp.route('/api/auth/me', methods=['GET'])
def me():
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


@auth_bp.route('/api/auth/profile', methods=['GET'])
def profile():
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


@auth_bp.route('/api/auth/saved-games', methods=['GET'])
def saved_games():
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


# ===== GAME ROUTES =====

@game_bp.route('/api/game/<int:game_id>/moves', methods=['GET'])
def game_moves(game_id):
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


@game_bp.route('/api/game/<int:game_id>/abandon', methods=['POST'])
def abandon_game(game_id):
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
    if room_id in _state.rooms:
        _state.cleanup_room(room_id)
        if _socketio:
            _socketio.emit('rooms_list', _state.get_rooms_list())

    abandon_game_by_id(game_id)
    return jsonify({'success': True})
