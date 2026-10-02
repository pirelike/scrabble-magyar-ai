import json
import os
import re
import time

from flask import (
    Blueprint, render_template, request, jsonify, make_response, current_app, send_from_directory,
)

import analysis
import async_games
import daily
import dictionary
import practice
import push_service
from config import SMTP_CONFIGURED
from tiles import tokenize_word, word_base_score, TILE_VALUES
from auth import (
    get_leaderboard, LEADERBOARD_METRICS, LEADERBOARD_MIN_GAMES,
    get_user_by_email, create_user, verify_password,
    create_verification_code, verify_code as auth_verify_code,
    create_session, validate_session, delete_session,
    is_email_verified, clear_email_verification,
    get_game_moves, get_user_game_history, get_game_by_id,
    get_or_create_share_token, get_game_by_share_token, get_game_results,
    get_user_achievements, get_game_analysis, save_game_analysis,
    get_daily_puzzle, get_daily_entry, get_daily_leaderboard,
    save_push_subscription, delete_push_subscription, count_push_subscriptions,
    get_user_async_games,
    get_user_active_games, abandon_game_by_id, is_user_in_game,
    get_friends, get_pending_requests, get_sent_requests, search_users,
)
from email_service import send_verification_email
from socket_auth import create_socket_token

# Inicializáláskor beállítandó (server.py-ból init_routes() hívással)
_rate_limiter = None
_state = None
_socketio = None

_VALID_NAME_RE = re.compile(r'^[\w\sáéíóöőúüűÁÉÍÓÖŐÚÜŰ._-]{1,20}$', re.UNICODE)
_VALID_EMAIL_RE = re.compile(r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$')

main_bp = Blueprint('main', __name__)
auth_bp = Blueprint('auth', __name__)
game_bp = Blueprint('game', __name__)
public_bp = Blueprint('public', __name__)

_BASE_DIR = os.path.dirname(os.path.abspath(__file__))
_STATIC_DIR = os.path.join(_BASE_DIR, 'static')
# Ezek módosítási ideje adja az "asset verziót": gyorsítótár-törés és service worker frissítés
_VERSIONED_FILES = (
    os.path.join('static', 'app.js'), os.path.join('static', 'style.css'),
    os.path.join('static', 'i18n-data.js'), os.path.join('static', 'i18n.js'),
    os.path.join('templates', 'index.html'),
)


def asset_version():
    """A kliens fájlok legutóbbi módosítási ideje (másodperc) — a gyorsítótár verziója."""
    latest = 0
    for rel in _VERSIONED_FILES:
        try:
            latest = max(latest, int(os.path.getmtime(os.path.join(_BASE_DIR, rel))))
        except OSError:
            pass
    return latest


def init_routes(rate_limiter, state, socketio):
    """Route-ok inicializálása a szükséges függőségekkel."""
    global _rate_limiter, _state, _socketio
    _rate_limiter = rate_limiter
    _state = state
    _socketio = socketio


_LOOPBACK_ADDRS = ('127.0.0.1', '::1')


def _get_client_ip():
    """Kliens IP cím lekérése.

    A proxy fejléceket (Cloudflare tunnel) csak akkor vesszük figyelembe, ha a kérés
    helyi (loopback) proxyról érkezett. Közvetlen eléréskor a fejléc hamisítható volna,
    ami kiütné az IP-alapú rate limitet.
    """
    remote = request.remote_addr or '127.0.0.1'
    if remote in _LOOPBACK_ADDRS:
        forwarded = (request.headers.get('CF-Connecting-IP')
                     or request.headers.get('X-Forwarded-For', ''))
        candidate = forwarded.split(',')[0].strip()
        if candidate:
            return candidate
    return remote


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


def _str_field(data, key):
    """Biztonságosan kiolvas egy szöveges mezőt a JSON törzsből (nem szöveg → '')."""
    value = data.get(key, '')
    return value.strip() if isinstance(value, str) else ''


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
    return render_template('index.html', asset_v=asset_version())


# ===== PWA (telepíthető alkalmazás) =====

@main_bp.route('/manifest.webmanifest')
def manifest():
    response = send_from_directory(_STATIC_DIR, 'manifest.webmanifest',
                                   mimetype='application/manifest+json')
    response.headers['Cache-Control'] = 'no-cache'
    return response


@main_bp.route('/sw.js')
def service_worker():
    """A service worker a gyökérről szolgálódik ki, hogy az egész oldalra érvényes legyen."""
    response = make_response(render_template('sw.js', version=asset_version()))
    response.headers['Content-Type'] = 'application/javascript; charset=utf-8'
    response.headers['Cache-Control'] = 'no-cache'
    response.headers['Service-Worker-Allowed'] = '/'
    return response


# ===== AUTH ROUTES =====

@auth_bp.route('/api/auth/request-code', methods=['POST'])
def request_code():
    ip = _get_client_ip()
    if not _rate_limiter.check_ip(ip, 'request_code'):
        return jsonify({'success': False, 'message': 'Túl sok kérés. Próbáld újra 5 perc múlva.'}), 429

    data = request.get_json(silent=True)
    if not data or not isinstance(data, dict) or not isinstance(data.get('email'), str):
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
    if not data or not isinstance(data, dict):
        return jsonify({'success': False, 'message': 'Érvénytelen kérés.'}), 400

    email = _str_field(data, 'email').lower()
    code = _str_field(data, 'code')

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
    if not data or not isinstance(data, dict):
        return jsonify({'success': False, 'message': 'Érvénytelen kérés.'}), 400

    email = _str_field(data, 'email').lower()
    password = data.get('password', '')
    if not isinstance(password, str):
        password = ''
    display_name = _str_field(data, 'display_name')

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

    if get_user_by_email(email):
        return jsonify({'success': False, 'message': 'Ez az email cím már regisztrálva van.'}), 409

    if not is_email_verified(email):
        return jsonify({
            'success': False,
            'message': 'Az email címet előbb meg kell erősíteni a kóddal.',
        }), 403

    success, result = create_user(email, name, password)
    if not success:
        return jsonify({'success': False, 'message': result}), 409
    clear_email_verification(email)

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
    if not data or not isinstance(data, dict):
        return jsonify({'success': False, 'message': 'Érvénytelen kérés.'}), 400

    email = _str_field(data, 'email').lower()
    password = data.get('password', '')
    if not isinstance(password, str):
        password = ''

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


@auth_bp.route('/api/auth/socket-token', methods=['GET'])
def socket_token():
    """Rövid életű token a Socket.IO `set_name` identitás igazolásához."""
    user = validate_session(request.cookies.get('session_token'))
    if not user:
        return jsonify({'success': False, 'message': 'Nincs érvényes session.'}), 401
    token = create_socket_token(current_app.config['SECRET_KEY'], user['id'])
    return jsonify({'success': True, 'token': token})


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
        'badges': get_user_achievements(user['id']),
        'stats': {
            'games_played': games_played,
            'games_won': games_won,
            'win_rate': win_rate,
            'avg_score': avg_score,
            'total_score': total_score,
            'rating': user['rating'],
            'rated_games': user['rated_games'],
        },
        'history': [
            {
                'game_id': h['game_id'],
                'room_name': h['room_name'],
                'created_at': h['created_at'],
                'final_score': h['final_score'],
                'is_winner': bool(h['is_winner']),
                'rating_change': (h['rating_after'] - h['rating_before']
                                  if h['rating_after'] is not None and h['rating_before'] is not None
                                  else None),
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

    games = get_user_active_games(user['id'], user.get('reconnect_token'))
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
                'is_owner': (g.get('owner_token') == user.get('reconnect_token')) if g.get('owner_token') and user.get('reconnect_token') else (g.get('owner_name', '') == user['display_name']),
            }
            for g in games
        ],
    })


@auth_bp.route('/api/auth/friends', methods=['GET'])
def friends():
    token = request.cookies.get('session_token')
    user = validate_session(token)
    if not user:
        return jsonify({'success': False, 'message': 'Nincs érvényes session.'}), 401

    user_id = user['id']
    friends_list = get_friends(user_id)
    
    # Online státusz hozzáadása
    online_user_ids = _state.get_online_user_ids() if _state else set()
    for f in friends_list:
        f['online'] = f['id'] in online_user_ids

    pending = get_pending_requests(user_id)
    sent = get_sent_requests(user_id)

    return jsonify({
        'success': True,
        'friends': friends_list,
        'pending_requests': pending,
        'sent_requests': sent,
    })


@auth_bp.route('/api/auth/search-users', methods=['GET'])
def search_users_route():
    ip = _get_client_ip()
    if not _rate_limiter.check_ip(ip, 'search_users'):
        return jsonify({'success': False, 'message': 'Túl sok keresés. Próbáld újra később.'}), 429

    token = request.cookies.get('session_token')
    user = validate_session(token)
    if not user:
        return jsonify({'success': False, 'message': 'Nincs érvényes session.'}), 401

    query = request.args.get('q', '').strip()
    if len(query) < 2:
        return jsonify({'success': True, 'users': []})

    results = search_users(query, user['id'])
    return jsonify({
        'success': True,
        'users': results
    })


# ===== PUBLIKUS API: RANGLISTA, SZÓTÁR =====

@public_bp.route('/api/leaderboard', methods=['GET'])
def leaderboard():
    ip = _get_client_ip()
    if not _rate_limiter.check_ip(ip, 'leaderboard'):
        return jsonify({'success': False, 'message': 'Túl sok kérés. Próbáld újra később.'}), 429

    metric = request.args.get('metric', 'wins')
    if metric not in LEADERBOARD_METRICS:
        return jsonify({'success': False, 'message': 'Ismeretlen rangsor.'}), 400
    try:
        limit = int(request.args.get('limit', 50))
    except ValueError:
        limit = 50

    user = validate_session(request.cookies.get('session_token'))
    my_id = user['id'] if user else None
    entries, me = get_leaderboard(metric, limit, my_id)
    for entry in entries:
        entry['is_me'] = entry['user_id'] == my_id
    return jsonify({
        'success': True,
        'metric': metric,
        'min_games': LEADERBOARD_MIN_GAMES[metric],
        'entries': entries,
        'me': me,
    })


@public_bp.route('/api/daily', methods=['GET'])
def daily_info():
    """A mai napi feladvány adatai: a saját eredményed, a nap ranglistájának eleje és a tegnapi megoldás.

    A feladvány legjobb pontszáma csak azoknak látszik, akik már beküldtek egy lépést."""
    if not _rate_limiter.check_ip(_get_client_ip(), 'daily'):
        return jsonify({'success': False, 'message': 'Túl sok kérés. Próbáld újra később.'}), 429
    user = validate_session(request.cookies.get('session_token'))
    user_id = user['id'] if user else None
    today = daily.today_str()
    entries, me = get_daily_leaderboard(today, 10, user_id)
    mine = get_daily_entry(today, user_id) if user_id else None
    played = bool(mine and mine['attempts'] > 0)
    puzzle = get_daily_puzzle(today)

    yesterday = None
    yesterday_puzzle = get_daily_puzzle(daily.previous_date(today))
    if yesterday_puzzle:
        top, _ = get_daily_leaderboard(yesterday_puzzle['date'], 3)
        yesterday = {'date': yesterday_puzzle['date'], 'best_score': yesterday_puzzle['best_score'],
                     'best_words': yesterday_puzzle['best']['words'], 'top': top}
    for entry in entries:
        entry['is_me'] = entry['user_id'] == user_id
    return jsonify({
        'success': True,
        'date': today,
        'ready': puzzle is not None,
        'my': mine,
        'best_score': puzzle['best_score'] if puzzle and (played or (mine and mine['revealed'])) else None,
        'leaderboard': entries,
        'me': me,
        'yesterday': yesterday,
    })


@public_bp.route('/api/daily/leaderboard', methods=['GET'])
def daily_leaderboard():
    """Egy nap ranglistája (alapértelmezés: ma; csak a mai és a korábbi napok kérhetők)."""
    if not _rate_limiter.check_ip(_get_client_ip(), 'daily'):
        return jsonify({'success': False, 'message': 'Túl sok kérés. Próbáld újra később.'}), 429
    today = daily.today_str()
    date_str = request.args.get('date', today)
    if not daily.is_valid_date(date_str) or date_str > today:
        return jsonify({'success': False, 'message': 'Érvénytelen dátum.'}), 400
    user = validate_session(request.cookies.get('session_token'))
    user_id = user['id'] if user else None
    entries, me = get_daily_leaderboard(date_str, 50, user_id)
    for entry in entries:
        entry['is_me'] = entry['user_id'] == user_id
    return jsonify({'success': True, 'date': date_str, 'entries': entries, 'me': me})


@auth_bp.route('/api/push/public-key', methods=['GET'])
def push_public_key():
    """A Web Push nyilvános kulcsa; `available` hamis, ha a szerveren nincs push támogatás."""
    user = validate_session(request.cookies.get('session_token'))
    if not user:
        return jsonify({'success': False, 'message': 'Bejelentkezés szükséges.'}), 401
    if not push_service.is_available():
        return jsonify({'success': True, 'available': False})
    return jsonify({'success': True, 'available': True, 'public_key': push_service.public_key(),
                    'subscribed': count_push_subscriptions(user['id']) > 0})


_PUSH_ENDPOINT_RE = re.compile(r'^https://[^\s]{10,1000}$')


@auth_bp.route('/api/push/subscribe', methods=['POST'])
def push_subscribe():
    """Web Push feliratkozás (vagy a nyelvének frissítése) a bejelentkezett felhasználónak."""
    user = validate_session(request.cookies.get('session_token'))
    if not user:
        return jsonify({'success': False, 'message': 'Bejelentkezés szükséges.'}), 401
    if not _rate_limiter.check_ip(_get_client_ip(), 'push'):
        return jsonify({'success': False, 'message': 'Túl sok kérés. Próbáld újra később.'}), 429
    if not push_service.is_available():
        return jsonify({'success': False, 'message': 'Az értesítések ezen a szerveren nem érhetők el.'}), 503
    data = request.get_json(silent=True) or {}
    keys = data.get('keys') if isinstance(data.get('keys'), dict) else {}
    endpoint, p256dh, auth_key = data.get('endpoint'), keys.get('p256dh'), keys.get('auth')
    if not (isinstance(endpoint, str) and _PUSH_ENDPOINT_RE.match(endpoint)
            and isinstance(p256dh, str) and 20 <= len(p256dh) <= 200
            and isinstance(auth_key, str) and 8 <= len(auth_key) <= 100):
        return jsonify({'success': False, 'message': 'Érvénytelen feliratkozás.'}), 400
    lang = data.get('lang') if data.get('lang') in push_service.MESSAGES else push_service.DEFAULT_LANG
    save_push_subscription(user['id'], endpoint, p256dh, auth_key, lang)
    return jsonify({'success': True})


@auth_bp.route('/api/push/unsubscribe', methods=['POST'])
def push_unsubscribe():
    user = validate_session(request.cookies.get('session_token'))
    if not user:
        return jsonify({'success': False, 'message': 'Bejelentkezés szükséges.'}), 401
    data = request.get_json(silent=True) or {}
    endpoint = data.get('endpoint')
    if not isinstance(endpoint, str):
        return jsonify({'success': False, 'message': 'Érvénytelen feliratkozás.'}), 400
    delete_push_subscription(endpoint, user['id'])
    return jsonify({'success': True})


def _practice_guard():
    """Közös ellenőrzés a gyakorló végpontokhoz: rate limit + szótár. Hibaválasz, vagy None."""
    if not _rate_limiter.check_ip(_get_client_ip(), 'practice'):
        return jsonify({'success': False, 'message': 'Túl sok kérés. Próbáld újra később.'}), 429
    if not dictionary.is_available():
        return jsonify({'success': False, 'message': 'A szótár nem érhető el.'}), 503
    return None


@public_bp.route('/api/practice/quiz', methods=['GET'])
def practice_quiz():
    """„Melyik szó érvényes?” kvíz: a kérdések (szavak) és zsetonjaik, a válaszokat nem tartalmazza."""
    mode = request.args.get('mode', 'mixed')
    if mode not in practice.QUIZ_MODES:
        return jsonify({'success': False, 'message': 'Érvénytelen kvízmód.'}), 400
    blocked = _practice_guard()
    if blocked:
        return blocked
    try:
        count = int(request.args.get('n', practice.QUESTION_COUNT))
    except ValueError:
        count = practice.QUESTION_COUNT
    questions = practice.make_quiz(count, mode)
    return jsonify({'success': True, 'mode': mode, 'questions': questions,
                    'tiles': [tokenize_word(q) for q in questions]})


@public_bp.route('/api/practice/answer', methods=['POST'])
def practice_answer():
    """Egy kvíz-válasz kiértékelése: érvényes-e a szó, helyes volt-e a tipp, pontérték, javaslatok."""
    data = request.get_json(silent=True) or {}
    word = data.get('word')
    if not isinstance(word, str) or not isinstance(data.get('answer'), bool):
        return jsonify({'success': False, 'message': 'Érvénytelen kérés.'}), 400
    blocked = _practice_guard()
    if blocked:
        return blocked
    result = practice.check_answer(word, data['answer'])
    if result is None:
        return jsonify({'success': False, 'message': 'Érvénytelen szó.'}), 400
    return jsonify({'success': True, **result})


@public_bp.route('/api/practice/short-words', methods=['GET'])
def practice_short_words():
    """Az összes érvényes 2 vagy 3 zsetonos szó pontértékkel (a tanuláshoz)."""
    try:
        length = int(request.args.get('length', 2))
    except ValueError:
        length = 0
    if length not in practice.SHORT_LENGTHS:
        return jsonify({'success': False, 'message': 'Érvénytelen szóhossz.'}), 400
    blocked = _practice_guard()
    if blocked:
        return blocked
    return jsonify({'success': True, 'length': length, 'words': practice.short_words(length)})


@public_bp.route('/api/practice/rack', methods=['GET'])
def practice_rack():
    """Betűvadászat / bingó-edző: egy 7 zsetonos kéz és az összes kirakható szó pontértékkel."""
    kind = request.args.get('kind', 'hunt')
    if kind not in practice.RACK_KINDS:
        return jsonify({'success': False, 'message': 'Érvénytelen gyakorlás.'}), 400
    blocked = _practice_guard()
    if blocked:
        return blocked
    return jsonify({'success': True, **practice.make_rack(kind)})


@public_bp.route('/api/practice/rack-word', methods=['POST'])
def practice_rack_word():
    """Egy beírt szó bírálata a kézhez: kirakható-e és érvényes-e (a listán nem szereplő szavakhoz)."""
    data = request.get_json(silent=True) or {}
    rack, word = data.get('rack'), data.get('word')
    valid_rack = (isinstance(rack, list) and 1 <= len(rack) <= practice.RACK_SIZE
                  and all(isinstance(t, str) and t in practice.LETTERS for t in rack))
    if not valid_rack or not isinstance(word, str):
        return jsonify({'success': False, 'message': 'Érvénytelen kérés.'}), 400
    blocked = _practice_guard()
    if blocked:
        return blocked
    return jsonify({'success': True, **practice.check_rack_word(rack, word)})


_MAX_DICT_WORDS = 8
_MAX_SUGGESTION_WORDS = 3


@public_bp.route('/api/dictionary/check', methods=['GET', 'POST'])
def dictionary_check():
    """Szavak ellenőrzése a játék szótárával (érvényes-e, hány pontot ér, javaslatok)."""
    ip = _get_client_ip()
    if not _rate_limiter.check_ip(ip, 'dictionary'):
        return jsonify({'success': False, 'message': 'Túl sok kérés. Próbáld újra később.'}), 429

    if request.method == 'POST':
        data = request.get_json(silent=True)
        raw = data.get('words', data.get('q', '')) if isinstance(data, dict) else ''
    else:
        raw = request.args.get('q', '')
    if isinstance(raw, list):
        raw = ' '.join(str(w) for w in raw if isinstance(w, (str, int)))
    if not isinstance(raw, str):
        raw = ''
    raw = raw[:300]  # a szavak száma és hossza úgyis korlátozott: ne dolgozzunk fel óriás bemenetet

    words = []
    for token in re.split(r'[\s,;]+', raw):
        token = token.strip()
        if token and token.upper() not in [w.upper() for w in words]:
            words.append(token)
    if not words:
        return jsonify({'success': False, 'message': 'Adj meg legalább egy szót.'}), 400
    words = words[:_MAX_DICT_WORDS]

    if not dictionary.is_available():
        # Szótár híján a játék minden szót elfogadna: a Szótár-eszköz ne mondjon hamis "érvényes"-t
        return jsonify({'success': False, 'message': 'A szótár jelenleg nem elérhető.'}), 503

    results = []
    checkable = []
    for word in words:
        upper = word.upper()
        tokens = tokenize_word(upper) if len(upper) <= 15 else None
        entry = {'word': upper[:30], 'valid': False, 'tiles': [], 'score': None,
                 'reason': None, 'suggestions': []}
        if len(upper) > 15:
            entry['reason'] = 'too_long'
        elif tokens is None:
            entry['reason'] = 'invalid_chars'
        elif len(tokens) < 2:
            entry['reason'] = 'too_short'
            entry['tiles'] = tokens
            entry['score'] = word_base_score(upper)
        else:
            entry['tiles'] = tokens
            entry['score'] = sum(TILE_VALUES[t] for t in tokens)
            checkable.append(entry)
        results.append(entry)

    valid = dictionary.filter_valid([e['word'] for e in checkable])
    suggested = 0
    for entry in checkable:
        entry['valid'] = entry['word'] in valid
        if not entry['valid']:
            entry['reason'] = 'not_in_dictionary'
            if suggested < _MAX_SUGGESTION_WORDS:
                entry['suggestions'] = dictionary.suggest_words(entry['word'], limit=5)
                suggested += 1

    return jsonify({'success': True, 'results': results})


# ===== GAME ROUTES =====

def _moves_payload(game_id):
    return [
        {
            'move_number': m['move_number'],
            'player_name': m['player_name'],
            'action_type': m['action_type'],
            'details_json': m['details_json'],
            'board_snapshot_json': m['board_snapshot_json'],
        }
        for m in get_game_moves(game_id)
    ]


_SHARE_TOKEN_RE = re.compile(r'^[A-Za-z0-9_-]{6,40}$')


@game_bp.route('/api/game/<int:game_id>/share', methods=['POST'])
def share_game(game_id):
    """Megosztható linket készít egy befejezett játék visszajátszásához (csak a résztvevőknek)."""
    user = validate_session(request.cookies.get('session_token'))
    if not user:
        return jsonify({'success': False, 'message': 'Bejelentkezés szükséges.'}), 401
    if not get_game_by_id(game_id):
        return jsonify({'success': False, 'message': 'Játék nem található.'}), 404
    if not is_user_in_game(game_id, user['id']):
        return jsonify({'success': False, 'message': 'Nincs jogosultságod a játék megosztásához.'}), 403
    token = get_or_create_share_token(game_id)
    if not token:
        return jsonify({'success': False, 'message': 'Csak befejezett játék osztható meg.'}), 400
    return jsonify({'success': True, 'token': token})


@game_bp.route('/api/replay/<token>', methods=['GET'])
def shared_replay(token):
    """Megosztott visszajátszás: nyilvános, bejelentkezés nélkül is elérhető."""
    if not _rate_limiter.check_ip(_get_client_ip(), 'replay'):
        return jsonify({'success': False, 'message': 'Túl sok kérés. Próbáld újra később.'}), 429
    game_row = get_game_by_share_token(token) if _SHARE_TOKEN_RE.match(token) else None
    if not game_row:
        return jsonify({'success': False, 'message': 'A megosztott visszajátszás nem található.'}), 404
    return jsonify({
        'success': True,
        'room_name': game_row['room_name'],
        'created_at': game_row['created_at'],
        'players': get_game_results(game_row['id']),
        'moves': _moves_payload(game_row['id']),
    })


@game_bp.route('/api/game/<int:game_id>/moves', methods=['GET'])
def game_moves(game_id):
    token = request.cookies.get('session_token')
    user = validate_session(token)
    if not user:
        return jsonify({'success': False, 'message': 'Bejelentkezés szükséges.'}), 401

    game_row = get_game_by_id(game_id)
    if not game_row:
        return jsonify({'success': False, 'message': 'Játék nem található.'}), 404

    if not is_user_in_game(game_id, user['id']):
        return jsonify({'success': False, 'message': 'Nincs jogosultságod a játék megtekintéséhez.'}), 403

    return jsonify({
        'success': True,
        'moves': _moves_payload(game_id),
        'players': get_game_results(game_id),
        'finished': game_row['status'] == 'finished',
    })


# Futó elemzések: {game_id: {'done', 'total'}} vagy {'error': True, 'at': idő} hiba után
_analysis_jobs = {}
_ANALYSIS_RETRY_AFTER = 30  # mp: hibás elemzés újrapróbálásáig


def _run_analysis(game_id, moves):
    """Háttérfeladat: elemzi a játékot, és elmenti az eredményt."""
    job = _analysis_jobs[game_id]

    def progress(done, total):
        job['done'], job['total'] = done, total

    try:
        result = analysis.analyze_game(
            moves, yield_fn=(lambda: _socketio.sleep(0)) if _socketio else None, progress=progress)
        save_game_analysis(game_id, analysis.ANALYSIS_VERSION, json.dumps(result, ensure_ascii=False))
        _analysis_jobs.pop(game_id, None)
    except Exception as e:  # az elemzés hibája ne dobjon ki a háttérből
        print(f"[analysis] Hiba az elemzésnél (game #{game_id}): {e}")
        _analysis_jobs[game_id] = {'error': True, 'at': time.time()}


@game_bp.route('/api/game/<int:game_id>/analysis', methods=['GET'])
def game_analysis(game_id):
    """A befejezett játék elemzése: lépésenként a legjobb lehetséges lépés és a kint maradt pont.

    A számítás háttérben fut; amíg tart, `status: 'running'` (a kliens időnként újrakérdezi)."""
    user = validate_session(request.cookies.get('session_token'))
    if not user:
        return jsonify({'success': False, 'message': 'Bejelentkezés szükséges.'}), 401
    if not _rate_limiter.check_ip(_get_client_ip(), 'analysis'):
        return jsonify({'success': False, 'message': 'Túl sok kérés. Próbáld újra később.'}), 429
    game_row = get_game_by_id(game_id)
    if not game_row:
        return jsonify({'success': False, 'message': 'Játék nem található.'}), 404
    if not is_user_in_game(game_id, user['id']):
        return jsonify({'success': False, 'message': 'Nincs jogosultságod a játék megtekintéséhez.'}), 403
    if game_row['status'] != 'finished':
        return jsonify({'success': False, 'message': 'Csak befejezett játék elemezhető.'}), 400

    cached = get_game_analysis(game_id)
    if cached and cached['version'] == analysis.ANALYSIS_VERSION:
        return jsonify({'success': True, 'status': 'ready', **json.loads(cached['result_json'])})

    job = _analysis_jobs.get(game_id)
    if job and not job.get('error'):
        return jsonify({'success': True, 'status': 'running',
                        'done': job.get('done', 0), 'total': job.get('total', 0)})
    if job and time.time() - job['at'] < _ANALYSIS_RETRY_AFTER:
        return jsonify({'success': True, 'status': 'error'})

    moves = get_game_moves(game_id)
    turns = analysis.analyzable_turns(moves)
    if not turns:
        # Régi játék: a lépésnapló nem őrizte meg a kezeket
        return jsonify({'success': True, 'status': 'unavailable'})
    _analysis_jobs[game_id] = {'done': 0, 'total': len(turns)}
    _socketio.start_background_task(_run_analysis, game_id, moves)
    return jsonify({'success': True, 'status': 'running', 'done': 0, 'total': len(turns)})


@game_bp.route('/api/async/games', methods=['GET'])
def async_games_list():
    """A bejelentkezett felhasználó folyamatban lévő levelezős játékai (akinél a sor, az elöl)."""
    user = validate_session(request.cookies.get('session_token'))
    if not user:
        return jsonify({'success': False, 'message': 'Bejelentkezés szükséges.'}), 401
    games = [g for g in (async_games.summarize(r) for r in get_user_async_games(user['id'])) if g]
    games.sort(key=lambda g: not g['my_turn'])   # a rendezés stabil: a többi az utolsó lépés szerinti
    return jsonify({'success': True, 'games': games,
                    'my_turn_count': sum(1 for g in games if g['my_turn'])})


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
    if game_row.get('is_async'):
        return jsonify({'success': False,
                        'message': 'A levelezős játékot a játékban, a Feladom gombbal fejezheted be.'}), 400

    is_owner = False
    if game_row.get('owner_token') and user.get('reconnect_token'):
        is_owner = game_row['owner_token'] == user['reconnect_token']
    else:
        is_owner = game_row.get('owner_name', '') == user['display_name']
    if not is_owner:
        return jsonify({'success': False, 'message': 'Csak a mentés tulajdonosa törölheti a játékot.'}), 403

    # Clean up in-memory room if it exists
    room_id = game_row['room_id']
    if room_id in _state.rooms:
        _state.cleanup_room(room_id)
        if _socketio:
            _socketio.emit('rooms_list', _state.get_rooms_list())

    abandon_game_by_id(game_id)
    return jsonify({'success': True})
