import json
import sqlite3
import secrets
import time
from contextlib import contextmanager
from datetime import datetime, timedelta, timezone
from werkzeug.security import generate_password_hash, check_password_hash

import elo
from config import (
    DB_PATH, SESSION_MAX_AGE_DAYS, VERIFICATION_CODE_EXPIRY_MINUTES,
    VERIFICATION_MAX_ATTEMPTS, EMAIL_VERIFIED_WINDOW_MINUTES,
)


def get_db():
    """Új SQLite kapcsolat létrehozása (backward compat)."""
    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row
    conn.execute('PRAGMA journal_mode=WAL')
    conn.execute('PRAGMA foreign_keys=ON')
    return conn


@contextmanager
def _db():
    """DB connection context manager: auto-commit, rollback on error, auto-close."""
    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row
    conn.execute('PRAGMA journal_mode=WAL')
    conn.execute('PRAGMA foreign_keys=ON')
    try:
        yield conn
        conn.commit()
    except Exception:
        conn.rollback()
        raise
    finally:
        conn.close()


def init_db():
    """Adatbázis séma létrehozása, ha nem létezik."""
    conn = get_db()
    conn.executescript('''
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            email TEXT NOT NULL,
            email_lower TEXT NOT NULL UNIQUE,
            display_name TEXT NOT NULL,
            password_hash TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            games_played INTEGER NOT NULL DEFAULT 0,
            games_won INTEGER NOT NULL DEFAULT 0,
            total_score INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS verification_codes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            email TEXT NOT NULL,
            code TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            expires_at TEXT NOT NULL,
            attempts INTEGER NOT NULL DEFAULT 0,
            used INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER NOT NULL,
            token TEXT NOT NULL UNIQUE,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            expires_at TEXT NOT NULL,
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS verified_emails (
            email TEXT PRIMARY KEY,
            expires_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_sessions_token ON sessions(token);
        CREATE INDEX IF NOT EXISTS idx_users_email_lower ON users(email_lower);
        CREATE INDEX IF NOT EXISTS idx_verification_codes_email ON verification_codes(email);

        CREATE TABLE IF NOT EXISTS saved_games (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            room_id TEXT NOT NULL,
            room_name TEXT NOT NULL DEFAULT '',
            state_json TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            challenge_mode INTEGER NOT NULL DEFAULT 0,
            owner_name TEXT NOT NULL DEFAULT '',
            owner_token TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS game_players (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            game_id INTEGER NOT NULL,
            user_id INTEGER,
            player_name TEXT NOT NULL,
            final_score INTEGER NOT NULL DEFAULT 0,
            is_winner INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (game_id) REFERENCES saved_games(id) ON DELETE CASCADE,
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL
        );

        CREATE TABLE IF NOT EXISTS game_moves (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            game_id INTEGER NOT NULL,
            move_number INTEGER NOT NULL,
            player_name TEXT NOT NULL,
            action_type TEXT NOT NULL,
            details_json TEXT,
            board_snapshot_json TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (game_id) REFERENCES saved_games(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_saved_games_room_id ON saved_games(room_id);
        CREATE INDEX IF NOT EXISTS idx_saved_games_status ON saved_games(status);
        CREATE INDEX IF NOT EXISTS idx_game_players_game_id ON game_players(game_id);
        CREATE INDEX IF NOT EXISTS idx_game_players_user_id ON game_players(user_id);
        CREATE INDEX IF NOT EXISTS idx_game_moves_game_id ON game_moves(game_id);

        CREATE TABLE IF NOT EXISTS app_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS push_subscriptions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER NOT NULL,
            endpoint TEXT NOT NULL UNIQUE,
            p256dh TEXT NOT NULL,
            auth TEXT NOT NULL,
            lang TEXT NOT NULL DEFAULT 'hu',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_push_subscriptions_user ON push_subscriptions(user_id);

        CREATE TABLE IF NOT EXISTS daily_puzzles (
            puzzle_date TEXT PRIMARY KEY,
            board_json TEXT NOT NULL,
            rack_json TEXT NOT NULL,
            best_score INTEGER NOT NULL,
            best_json TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS daily_scores (
            puzzle_date TEXT NOT NULL,
            user_id INTEGER NOT NULL,
            best_score INTEGER NOT NULL DEFAULT 0,
            attempts INTEGER NOT NULL DEFAULT 0,
            first_best_at TEXT NOT NULL DEFAULT (datetime('now')),
            revealed INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (puzzle_date, user_id),
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_daily_scores_ranking
            ON daily_scores(puzzle_date, best_score DESC);

        CREATE TABLE IF NOT EXISTS game_analysis (
            game_id INTEGER PRIMARY KEY,
            version INTEGER NOT NULL,
            result_json TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (game_id) REFERENCES saved_games(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS achievements (
            user_id INTEGER NOT NULL,
            badge TEXT NOT NULL,
            game_id INTEGER,
            earned_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (user_id, badge),
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS friendships (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER NOT NULL,
            friend_id INTEGER NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
            FOREIGN KEY (friend_id) REFERENCES users(id) ON DELETE CASCADE,
            UNIQUE(user_id, friend_id)
        );
        CREATE INDEX IF NOT EXISTS idx_friendships_user_id ON friendships(user_id);
        CREATE INDEX IF NOT EXISTS idx_friendships_friend_id ON friendships(friend_id);
    ''')
    # Migráció: owner_name oszlop hozzáadása ha nem létezik
    try:
        conn.execute('SELECT owner_name FROM saved_games LIMIT 1')
    except sqlite3.OperationalError:
        conn.execute("ALTER TABLE saved_games ADD COLUMN owner_name TEXT NOT NULL DEFAULT ''")
    try:
        conn.execute('SELECT owner_token FROM saved_games LIMIT 1')
    except sqlite3.OperationalError:
        conn.execute("ALTER TABLE saved_games ADD COLUMN owner_token TEXT")
    try:
        conn.execute('SELECT reconnect_token FROM users LIMIT 1')
    except sqlite3.OperationalError:
        conn.execute("ALTER TABLE users ADD COLUMN reconnect_token TEXT")
    try:
        conn.execute('SELECT has_bots FROM saved_games LIMIT 1')
    except sqlite3.OperationalError:
        # Robotos játékok nem számítanak bele a ranglistába
        conn.execute("ALTER TABLE saved_games ADD COLUMN has_bots INTEGER NOT NULL DEFAULT 0")
    try:
        conn.execute('SELECT share_token FROM saved_games LIMIT 1')
    except sqlite3.OperationalError:
        # Megosztható visszajátszás-link (nyilvános, a játékosok kérésére jön létre)
        conn.execute("ALTER TABLE saved_games ADD COLUMN share_token TEXT")
    try:
        conn.execute('SELECT is_async FROM saved_games LIMIT 1')
    except sqlite3.OperationalError:
        # Levelezős játék: a játékosok órák / napok alatt lépnek, a mentés a játék tartós otthona
        conn.execute("ALTER TABLE saved_games ADD COLUMN is_async INTEGER NOT NULL DEFAULT 0")
    try:
        conn.execute('SELECT rating FROM users LIMIT 1')
    except sqlite3.OperationalError:
        # Élő-értékszám (ELO) és az értékelt játékok száma
        conn.execute(f"ALTER TABLE users ADD COLUMN rating INTEGER NOT NULL DEFAULT {elo.INITIAL_RATING}")
        conn.execute("ALTER TABLE users ADD COLUMN rated_games INTEGER NOT NULL DEFAULT 0")
    try:
        conn.execute('SELECT rating_before FROM game_players LIMIT 1')
    except sqlite3.OperationalError:
        conn.execute("ALTER TABLE game_players ADD COLUMN rating_before INTEGER")
        conn.execute("ALTER TABLE game_players ADD COLUMN rating_after INTEGER")
    conn.execute('CREATE UNIQUE INDEX IF NOT EXISTS idx_saved_games_share_token '
                 'ON saved_games(share_token) WHERE share_token IS NOT NULL')
    conn.commit()
    conn.close()


# --- User CRUD ---

def get_user_by_email(email):
    """Felhasználó keresése email alapján."""
    with _db() as conn:
        return conn.execute(
            'SELECT * FROM users WHERE email_lower = ?', (email.lower().strip(),)
        ).fetchone()


def get_user_by_id(user_id):
    """Felhasználó keresése ID alapján."""
    with _db() as conn:
        return conn.execute('SELECT * FROM users WHERE id = ?', (user_id,)).fetchone()


def create_user(email, display_name, password):
    """Új felhasználó létrehozása. Visszaad (success, user_id_or_error)."""
    email_lower = email.lower().strip()
    if get_user_by_email(email_lower):
        return False, 'Ez az email cím már regisztrálva van.'

    password_hash = generate_password_hash(password, method='pbkdf2:sha256:260000')
    try:
        with _db() as conn:
            cursor = conn.execute(
                'INSERT INTO users (email, email_lower, display_name, password_hash) VALUES (?, ?, ?, ?)',
                (email.strip(), email_lower, display_name.strip(), password_hash)
            )
            user_id = cursor.lastrowid
        return True, user_id
    except sqlite3.IntegrityError:
        return False, 'Ez az email cím már regisztrálva van.'


def verify_password(email, password):
    """Jelszó ellenőrzés. Visszaad (success, user_or_error)."""
    user = get_user_by_email(email)
    if not user:
        return False, 'Hibás email cím vagy jelszó.'
    if not check_password_hash(user['password_hash'], password):
        return False, 'Hibás email cím vagy jelszó.'
    return True, dict(user)


# --- Verification codes ---

def create_verification_code(email):
    """6 számjegyű verifikációs kód generálása. Visszaadja a kódot."""
    code = f'{secrets.randbelow(1000000):06d}'
    expires_at = (datetime.now(timezone.utc).replace(tzinfo=None) + timedelta(minutes=VERIFICATION_CODE_EXPIRY_MINUTES)).strftime('%Y-%m-%d %H:%M:%S')
    email_lower = email.lower().strip()

    with _db() as conn:
        conn.execute(
            'UPDATE verification_codes SET used = 1 WHERE email = ? AND used = 0',
            (email_lower,)
        )
        conn.execute(
            'INSERT INTO verification_codes (email, code, expires_at) VALUES (?, ?, ?)',
            (email_lower, code, expires_at)
        )
    return code


def verify_code(email, code):
    """Verifikációs kód ellenőrzés. Visszaad (success, message)."""
    email_lower = email.lower().strip()

    with _db() as conn:
        row = conn.execute(
            'SELECT * FROM verification_codes WHERE email = ? AND used = 0 ORDER BY created_at DESC LIMIT 1',
            (email_lower,)
        ).fetchone()

        if not row:
            return False, 'Nincs érvényes verifikációs kód. Kérj újat.'

        expires_at = datetime.strptime(row['expires_at'], '%Y-%m-%d %H:%M:%S')
        if datetime.now(timezone.utc).replace(tzinfo=None) > expires_at:
            conn.execute('UPDATE verification_codes SET used = 1 WHERE id = ?', (row['id'],))
            return False, 'A kód lejárt. Kérj újat.'

        if row['attempts'] >= VERIFICATION_MAX_ATTEMPTS:
            conn.execute('UPDATE verification_codes SET used = 1 WHERE id = ?', (row['id'],))
            return False, 'Túl sok próbálkozás. Kérj új kódot.'

        if row['code'] != code:
            # Atomi attempts növelés: UPDATE csak ha attempts < MAX
            updated = conn.execute(
                'UPDATE verification_codes SET attempts = attempts + 1 WHERE id = ? AND attempts < ?',
                (row['id'], VERIFICATION_MAX_ATTEMPTS)
            ).rowcount
            if not updated:
                conn.execute('UPDATE verification_codes SET used = 1 WHERE id = ?', (row['id'],))
                return False, 'Túl sok próbálkozás. Kérj új kódot.'
            remaining = VERIFICATION_MAX_ATTEMPTS - row['attempts'] - 1
            return False, f'Hibás kód. Még {remaining} próbálkozásod van.'

        conn.execute('UPDATE verification_codes SET used = 1 WHERE id = ?', (row['id'],))
        verified_until = (datetime.now(timezone.utc).replace(tzinfo=None)
                          + timedelta(minutes=EMAIL_VERIFIED_WINDOW_MINUTES)).strftime('%Y-%m-%d %H:%M:%S')
        conn.execute(
            'INSERT OR REPLACE INTO verified_emails (email, expires_at) VALUES (?, ?)',
            (email_lower, verified_until)
        )
        return True, 'Kód elfogadva.'


def is_email_verified(email):
    """Igaz, ha az email címet a közelmúltban sikeresen megerősítették kóddal."""
    email_lower = email.lower().strip()
    with _db() as conn:
        row = conn.execute(
            'SELECT expires_at FROM verified_emails WHERE email = ?', (email_lower,)
        ).fetchone()
        if not row:
            return False
        expires_at = datetime.strptime(row['expires_at'], '%Y-%m-%d %H:%M:%S')
        if datetime.now(timezone.utc).replace(tzinfo=None) > expires_at:
            conn.execute('DELETE FROM verified_emails WHERE email = ?', (email_lower,))
            return False
        return True


def clear_email_verification(email):
    """A megerősítés felhasználása (regisztráció után egyszer használatos)."""
    with _db() as conn:
        conn.execute('DELETE FROM verified_emails WHERE email = ?', (email.lower().strip(),))


# --- Sessions ---

def create_session(user_id):
    """Új session token létrehozása. Visszaadja a tokent."""
    token = secrets.token_urlsafe(48)
    expires_at = (datetime.now(timezone.utc).replace(tzinfo=None) + timedelta(days=SESSION_MAX_AGE_DAYS)).strftime('%Y-%m-%d %H:%M:%S')

    with _db() as conn:
        conn.execute(
            'INSERT INTO sessions (user_id, token, expires_at) VALUES (?, ?, ?)',
            (user_id, token, expires_at)
        )
    return token


def validate_session(token):
    """Session token ellenőrzés. Visszaad user dict-et vagy None-t."""
    if not token:
        return None

    with _db() as conn:
        row = conn.execute(
            'SELECT s.*, u.id as uid, u.email, u.display_name, u.games_played, u.games_won, u.total_score, u.reconnect_token, '
            'u.rating, u.rated_games '
            'FROM sessions s JOIN users u ON s.user_id = u.id '
            'WHERE s.token = ?',
            (token,)
        ).fetchone()

        if not row:
            return None

        expires_at = datetime.strptime(row['expires_at'], '%Y-%m-%d %H:%M:%S')
        if datetime.now(timezone.utc).replace(tzinfo=None) > expires_at:
            conn.execute('DELETE FROM sessions WHERE id = ?', (row['id'],))
            return None

        return {
            'id': row['uid'],
            'email': row['email'],
            'display_name': row['display_name'],
            'games_played': row['games_played'],
            'games_won': row['games_won'],
            'total_score': row['total_score'],
            'reconnect_token': row['reconnect_token'],
            'rating': row['rating'],
            'rated_games': row['rated_games'],
        }


def delete_session(token):
    """Session törlése (logout)."""
    if not token:
        return
    with _db() as conn:
        conn.execute('DELETE FROM sessions WHERE token = ?', (token,))


def get_or_create_user_reconnect_token(user_id):
    """Visszaadja a felhasználó reconnect tokenjét, vagy generál egyet (kriptográfiailag erős)."""
    with _db() as conn:
        row = conn.execute('SELECT reconnect_token FROM users WHERE id = ?',
                           (user_id,)).fetchone()
        if row and row['reconnect_token']:
            return row['reconnect_token']
        while True:
            token = secrets.token_urlsafe(16)
            existing = conn.execute(
                'SELECT id FROM users WHERE reconnect_token = ?', (token,)
            ).fetchone()
            if not existing:
                conn.execute('UPDATE users SET reconnect_token = ? WHERE id = ?',
                             (token, user_id))
                return token


def cleanup_expired():
    """Lejárt sessionök és verifikációs kódok törlése."""
    now = datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M:%S')
    with _db() as conn:
        conn.execute('DELETE FROM sessions WHERE expires_at < ?', (now,))
        conn.execute('DELETE FROM verification_codes WHERE expires_at < ?', (now,))
        conn.execute('DELETE FROM verified_emails WHERE expires_at < ?', (now,))


# --- Game persistence ---

def save_game(room_id, room_name, state_json, challenge_mode, players_data=None, owner_name='',
              owner_token=None, has_bots=False, is_async=False):
    """Játék mentése (upsert: room_id + active alapján). Visszaadja a game_id-t.
    players_data: [{player_name, user_id (or None), score}, ...] — ha megadva, upsert a game_players-be.
    has_bots: robot ellenfél is van a játékban (a ranglista nem számolja).
    """
    with _db() as conn:
        now = datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M:%S')
        # Atomic upsert: UPDATE first, INSERT only if no row was updated
        updated = conn.execute(
            "UPDATE saved_games SET state_json = ?, updated_at = ?, owner_name = ?, owner_token = ?, "
            "has_bots = ? WHERE room_id = ? AND status = 'active'",
            (state_json, now, owner_name, owner_token, 1 if has_bots else 0, room_id)
        ).rowcount
        if updated:
            game_id = conn.execute(
                "SELECT id FROM saved_games WHERE room_id = ? AND status = 'active'",
                (room_id,)
            ).fetchone()['id']
        else:
            cursor = conn.execute(
                'INSERT INTO saved_games (room_id, room_name, state_json, status, challenge_mode, owner_name, '
                'owner_token, has_bots, is_async) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)',
                (room_id, room_name, state_json, 'active', 1 if challenge_mode else 0, owner_name,
                 owner_token, 1 if has_bots else 0, 1 if is_async else 0)
            )
            game_id = cursor.lastrowid

        if players_data:
            _upsert_game_players(conn, game_id, players_data)

    return game_id


def _upsert_game_players(conn, game_id, players_data):
    """Game players upsert: UPDATE first, INSERT only if no row was updated."""
    for pd in players_data:
        updated = conn.execute(
            'UPDATE game_players SET final_score = ?, user_id = COALESCE(?, user_id) '
            'WHERE game_id = ? AND player_name = ?',
            (pd.get('score', 0), pd.get('user_id'), game_id, pd['player_name'])
        ).rowcount
        if not updated:
            conn.execute(
                'INSERT INTO game_players (game_id, user_id, player_name, final_score) '
                'VALUES (?, ?, ?, ?)',
                (game_id, pd.get('user_id'), pd['player_name'], pd.get('score', 0))
            )


def _apply_ratings(conn, game_id, players_data):
    """ELO: a regisztrált játékosok értékszámának frissítése a végeredmény alapján (legalább két
    regisztrált játékos kell; a vendégek nem vesznek részt). A változás a game_players sorban is
    rögzül (rating_before / rating_after)."""
    ranked = {}
    for pd in players_data:
        uid = pd.get('user_id')
        if uid and uid not in ranked:
            row = conn.execute('SELECT rating, rated_games FROM users WHERE id = ?', (uid,)).fetchone()
            if row:
                # Aki feladta, az pontszámától függetlenül mindenki mögé kerül
                score = -10 ** 9 if pd.get('resigned') else pd['final_score']
                ranked[uid] = (row['rating'], row['rated_games'], score)
    if len(ranked) < 2:
        return
    changes = elo.rating_changes([(uid, r, g, s) for uid, (r, g, s) in ranked.items()])
    for uid, (before, _games, _score) in ranked.items():
        after = before + changes[uid]
        conn.execute('UPDATE users SET rating = ?, rated_games = rated_games + 1 WHERE id = ?',
                     (after, uid))
        conn.execute('UPDATE game_players SET rating_before = ?, rating_after = ? '
                     'WHERE game_id = ? AND user_id = ?', (before, after, game_id, uid))


def get_game_rating_changes(game_id):
    """Egy játék értékszám-változásai: {user_id: (előtte, utána)} (csak az értékelt játékosok)."""
    with _db() as conn:
        rows = conn.execute(
            'SELECT user_id, rating_before, rating_after FROM game_players '
            'WHERE game_id = ? AND user_id IS NOT NULL AND rating_after IS NOT NULL', (game_id,)
        ).fetchall()
    return {r['user_id']: (r['rating_before'], r['rating_after']) for r in rows}


def finish_game(room_id, state_json, players_data, room_name='', has_bots=False):
    """Játék befejezése: status='finished', game_players INSERT, users stats UPDATE.
    players_data: [{player_name, user_id (or None), final_score, is_winner}, ...]
    Robotos játék nem számít az értékszámba (és a ranglistába sem).
    """
    with _db() as conn:
        now = datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M:%S')
        row = conn.execute(
            "SELECT id FROM saved_games WHERE room_id = ? AND status = 'active'",
            (room_id,)
        ).fetchone()
        if not row:
            cursor = conn.execute(
                'INSERT INTO saved_games (room_id, room_name, state_json, status, has_bots) '
                'VALUES (?, ?, ?, ?, ?)',
                (room_id, room_name or '', state_json, 'finished', 1 if has_bots else 0)
            )
            game_id = cursor.lastrowid
        else:
            game_id = row['id']
            conn.execute(
                'UPDATE saved_games SET state_json = ?, status = ?, updated_at = ?, has_bots = ? WHERE id = ?',
                (state_json, 'finished', now, 1 if has_bots else 0, game_id)
            )

        for pd in players_data:
            existing = conn.execute(
                'SELECT id FROM game_players WHERE game_id = ? AND player_name = ?',
                (game_id, pd['player_name'])
            ).fetchone()
            if existing:
                conn.execute(
                    'UPDATE game_players SET final_score = ?, is_winner = ?, '
                    'user_id = COALESCE(?, user_id) WHERE id = ?',
                    (pd['final_score'], 1 if pd.get('is_winner') else 0,
                     pd.get('user_id'), existing['id'])
                )
            else:
                conn.execute(
                    'INSERT INTO game_players (game_id, user_id, player_name, final_score, is_winner) '
                    'VALUES (?, ?, ?, ?, ?)',
                    (game_id, pd.get('user_id'), pd['player_name'], pd['final_score'],
                     1 if pd.get('is_winner') else 0)
                )
            if pd.get('user_id'):
                conn.execute(
                    'UPDATE users SET games_played = games_played + 1, '
                    'games_won = games_won + ?, total_score = total_score + ? '
                    'WHERE id = ?',
                    (1 if pd.get('is_winner') else 0, pd['final_score'], pd['user_id'])
                )

        if not has_bots:
            _apply_ratings(conn, game_id, players_data)

    return game_id


def add_game_move(game_id, move_number, player_name, action_type, details_json, board_snapshot_json):
    """Lépés hozzáadása a játékhoz."""
    with _db() as conn:
        conn.execute(
            'INSERT INTO game_moves (game_id, move_number, player_name, action_type, details_json, board_snapshot_json) '
            'VALUES (?, ?, ?, ?, ?, ?)',
            (game_id, move_number, player_name, action_type, details_json, board_snapshot_json)
        )


def load_active_games():
    """Visszaadja az aktív játékokat."""
    with _db() as conn:
        rows = conn.execute(
            "SELECT * FROM saved_games WHERE status = 'active' ORDER BY updated_at DESC"
        ).fetchall()
    return [dict(r) for r in rows]


def get_game_moves(game_id):
    """Lépések move_number sorrendben."""
    with _db() as conn:
        rows = conn.execute(
            'SELECT * FROM game_moves WHERE game_id = ? ORDER BY move_number',
            (game_id,)
        ).fetchall()
    return [dict(r) for r in rows]


def get_user_game_history(user_id, limit=20):
    """Befejezett játékok + ellenfelek a felhasználóhoz (egyetlen query, N+1 fix)."""
    with _db() as conn:
        rows = conn.execute(
            'SELECT sg.id as game_id, sg.room_name, sg.created_at, '
            'gp.final_score, gp.is_winner, gp.player_name, gp.rating_before, gp.rating_after '
            'FROM saved_games sg '
            'JOIN game_players gp ON sg.id = gp.game_id AND gp.user_id = ? '
            "WHERE sg.status = 'finished' "
            'ORDER BY sg.created_at DESC LIMIT ?',
            (user_id, limit)
        ).fetchall()

        game_ids = [r['game_id'] for r in rows]
        if not game_ids:
            return []

        # Ellenfelek lekérdezése egyetlen query-vel
        placeholders = ','.join('?' * len(game_ids))
        opponent_rows = conn.execute(
            'SELECT game_id, player_name, final_score, is_winner FROM game_players '
            f'WHERE game_id IN ({placeholders}) AND user_id IS NOT ?',
            (*game_ids, user_id)
        ).fetchall()

    # Ellenfelek csoportosítása game_id szerint
    opponents_by_game = {}
    for o in opponent_rows:
        gid = o['game_id']
        if gid not in opponents_by_game:
            opponents_by_game[gid] = []
        opponents_by_game[gid].append({
            'player_name': o['player_name'],
            'final_score': o['final_score'],
            'is_winner': o['is_winner'],
        })

    return [
        {
            'game_id': r['game_id'],
            'room_name': r['room_name'],
            'created_at': r['created_at'],
            'final_score': r['final_score'],
            'is_winner': r['is_winner'],
            'player_name': r['player_name'],
            'rating_before': r['rating_before'],
            'rating_after': r['rating_after'],
            'opponents': opponents_by_game.get(r['game_id'], []),
        }
        for r in rows
    ]


def get_game_by_id(game_id):
    """Egyetlen játék sor."""
    with _db() as conn:
        row = conn.execute('SELECT * FROM saved_games WHERE id = ?', (game_id,)).fetchone()
    return dict(row) if row else None


def get_active_async_games():
    """Az összes folyamatban lévő levelezős játék (szerverindításkor a szobák visszaépítéséhez)."""
    with _db() as conn:
        rows = conn.execute(
            "SELECT * FROM saved_games WHERE status = 'active' AND is_async = 1 ORDER BY id"
        ).fetchall()
    return [dict(r) for r in rows]


def get_user_async_games(user_id):
    """A felhasználó folyamatban lévő levelezős játékai: a mentett állapot + a saját játékosneve."""
    with _db() as conn:
        rows = conn.execute(
            'SELECT sg.id AS game_id, sg.room_id, sg.room_name, sg.state_json, sg.updated_at, '
            'sg.created_at, gp.player_name AS my_name '
            'FROM game_players gp JOIN saved_games sg ON sg.id = gp.game_id '
            "WHERE gp.user_id = ? AND sg.status = 'active' AND sg.is_async = 1 "
            'ORDER BY sg.updated_at DESC', (user_id,)
        ).fetchall()
    return [dict(r) for r in rows]


def get_user_active_games(user_id, reconnect_token=None):
    """Felhasználó aktív (mentett) játékai — csak ahol ő az owner."""
    with _db() as conn:
        if reconnect_token:
            rows = conn.execute(
                'SELECT gp.game_id, gp.player_name, gp.final_score, '
                'sg.room_name, sg.room_id, sg.created_at, sg.updated_at, sg.challenge_mode, sg.owner_name, sg.owner_token '
                'FROM game_players gp '
                'JOIN saved_games sg ON gp.game_id = sg.id '
                "WHERE gp.user_id = ? AND sg.status = 'active' AND sg.owner_token = ? AND sg.is_async = 0 "
                'ORDER BY sg.updated_at DESC',
                (user_id, reconnect_token)
            ).fetchall()
        else:
            rows = conn.execute(
                'SELECT gp.game_id, gp.player_name, gp.final_score, '
                'sg.room_name, sg.room_id, sg.created_at, sg.updated_at, sg.challenge_mode, sg.owner_name, sg.owner_token '
                'FROM game_players gp '
                'JOIN saved_games sg ON gp.game_id = sg.id '
                'JOIN game_players gp_owner ON gp_owner.game_id = sg.id AND gp_owner.user_id = ? AND gp_owner.player_name = sg.owner_name '
                "WHERE gp.user_id = ? AND sg.status = 'active' AND sg.is_async = 0 "
                'ORDER BY sg.updated_at DESC',
                (user_id, user_id)
            ).fetchall()

        game_ids = [r['game_id'] for r in rows]
        if not game_ids:
            return []

        player_names = {r['player_name'] for r in rows}
        placeholders = ','.join('?' * len(game_ids))
        opponent_rows = conn.execute(
            'SELECT game_id, player_name, final_score FROM game_players '
            f'WHERE game_id IN ({placeholders}) AND user_id IS NOT ?',
            (*game_ids, user_id)
        ).fetchall()

    opponents_by_game = {}
    for o in opponent_rows:
        gid = o['game_id']
        if gid not in opponents_by_game:
            opponents_by_game[gid] = []
        opponents_by_game[gid].append({
            'name': o['player_name'],
            'score': o['final_score'],
        })

    result = []
    for r in rows:
        r = dict(r)
        r['opponents'] = opponents_by_game.get(r['game_id'], [])
        result.append(r)
    return result


def is_user_in_game(game_id, user_id):
    """Ellenőrzi, hogy a felhasználó részese-e a játéknak."""
    with _db() as conn:
        row = conn.execute(
            'SELECT id FROM game_players WHERE game_id = ? AND user_id = ?',
            (game_id, user_id)
        ).fetchone()
    return row is not None


def get_or_create_share_token(game_id):
    """A befejezett játék visszajátszásának megosztási tokenje (ha még nincs, létrehozza).
    Visszatér: token, vagy None, ha a játék nem létezik / még nem fejeződött be."""
    with _db() as conn:
        row = conn.execute('SELECT status, share_token FROM saved_games WHERE id = ?',
                           (game_id,)).fetchone()
        if not row or row['status'] != 'finished':
            return None
        if row['share_token']:
            return row['share_token']
        token = secrets.token_urlsafe(9)
        conn.execute('UPDATE saved_games SET share_token = ? WHERE id = ?', (token, game_id))
        return token


def get_game_by_share_token(token):
    """A megosztási tokenhez tartozó befejezett játék (vagy None)."""
    if not token:
        return None
    with _db() as conn:
        row = conn.execute(
            "SELECT * FROM saved_games WHERE share_token = ? AND status = 'finished'", (token,)
        ).fetchone()
    return dict(row) if row else None


def get_game_results(game_id):
    """A játék végeredménye: [{player_name, final_score, is_winner}], pontszám szerint csökkenően."""
    with _db() as conn:
        rows = conn.execute(
            'SELECT player_name, final_score, is_winner FROM game_players WHERE game_id = ? '
            'ORDER BY final_score DESC, id', (game_id,)
        ).fetchall()
    return [{'player_name': r['player_name'], 'final_score': r['final_score'],
             'is_winner': bool(r['is_winner'])} for r in rows]


# --- Beállítások és push feliratkozások ---

def get_setting(key):
    with _db() as conn:
        row = conn.execute('SELECT value FROM app_settings WHERE key = ?', (key,)).fetchone()
    return row['value'] if row else None


def set_setting(key, value):
    with _db() as conn:
        conn.execute('INSERT INTO app_settings (key, value) VALUES (?, ?) '
                     'ON CONFLICT(key) DO UPDATE SET value = excluded.value', (key, value))


def save_push_subscription(user_id, endpoint, p256dh, auth_key, lang='hu'):
    """Web Push feliratkozás mentése (egy végpont egyszer szerepelhet; másik felhasználóhoz is átkerülhet,
    ha ugyanazon az eszközön másik fiókkal léptek be)."""
    with _db() as conn:
        conn.execute(
            'INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth, lang) VALUES (?, ?, ?, ?, ?) '
            'ON CONFLICT(endpoint) DO UPDATE SET user_id = excluded.user_id, p256dh = excluded.p256dh, '
            'auth = excluded.auth, lang = excluded.lang',
            (user_id, endpoint, p256dh, auth_key, lang))


def delete_push_subscription(endpoint, user_id=None):
    """Feliratkozás törlése (a `user_id` megadásakor csak a sajátja). Visszatér: törölt sorok száma."""
    with _db() as conn:
        if user_id is None:
            return conn.execute('DELETE FROM push_subscriptions WHERE endpoint = ?', (endpoint,)).rowcount
        return conn.execute('DELETE FROM push_subscriptions WHERE endpoint = ? AND user_id = ?',
                            (endpoint, user_id)).rowcount


def get_push_subscriptions(user_id):
    with _db() as conn:
        rows = conn.execute('SELECT endpoint, p256dh, auth, lang FROM push_subscriptions WHERE user_id = ?',
                            (user_id,)).fetchall()
    return [dict(r) for r in rows]


def count_push_subscriptions(user_id):
    with _db() as conn:
        return conn.execute('SELECT COUNT(*) FROM push_subscriptions WHERE user_id = ?',
                            (user_id,)).fetchone()[0]


# --- Napi feladvány ---

def get_daily_puzzle(puzzle_date):
    """A nap feladványa: {'board', 'rack', 'best_score', 'best'} vagy None."""
    with _db() as conn:
        row = conn.execute('SELECT * FROM daily_puzzles WHERE puzzle_date = ?', (puzzle_date,)).fetchone()
    if not row:
        return None
    return {
        'date': row['puzzle_date'],
        'board': json.loads(row['board_json']),
        'rack': json.loads(row['rack_json']),
        'best_score': row['best_score'],
        'best': json.loads(row['best_json']),
    }


def save_daily_puzzle(puzzle_date, board, rack, best_score, best):
    """A feladvány mentése (ha az adott napra már van, az marad: mindenkinek ugyanaz)."""
    with _db() as conn:
        conn.execute(
            'INSERT OR IGNORE INTO daily_puzzles (puzzle_date, board_json, rack_json, best_score, best_json) '
            'VALUES (?, ?, ?, ?, ?)',
            (puzzle_date, json.dumps(board), json.dumps(rack), best_score, json.dumps(best, ensure_ascii=False))
        )


def raise_daily_best(puzzle_date, score, best):
    """Ha valaki a feladvány eddig ismert legjobb lépésénél többet ért el, az lesz az új legjobb."""
    with _db() as conn:
        conn.execute(
            'UPDATE daily_puzzles SET best_score = ?, best_json = ? WHERE puzzle_date = ? AND best_score < ?',
            (score, json.dumps(best, ensure_ascii=False), puzzle_date, score)
        )


def get_daily_entry(puzzle_date, user_id):
    with _db() as conn:
        row = conn.execute(
            'SELECT best_score, attempts, revealed FROM daily_scores WHERE puzzle_date = ? AND user_id = ?',
            (puzzle_date, user_id)
        ).fetchone()
    return {'best_score': row['best_score'], 'attempts': row['attempts'],
            'revealed': bool(row['revealed'])} if row else None


def record_daily_score(puzzle_date, user_id, score):
    """Egy beküldött lépés rögzítése. A legjobb pontszám számít; ha valaki a megoldást megnézte,
    további próbálkozása már nem kerül a ranglistára.

    Visszatér: {'recorded', 'best', 'attempts', 'improved'}"""
    now = datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M:%S')
    with _db() as conn:
        row = conn.execute(
            'SELECT best_score, attempts, revealed FROM daily_scores WHERE puzzle_date = ? AND user_id = ?',
            (puzzle_date, user_id)
        ).fetchone()
        if row and row['revealed']:
            return {'recorded': False, 'best': row['best_score'], 'attempts': row['attempts'],
                    'improved': False}
        if not row:
            conn.execute(
                'INSERT INTO daily_scores (puzzle_date, user_id, best_score, attempts, first_best_at) '
                'VALUES (?, ?, ?, 1, ?)', (puzzle_date, user_id, score, now))
            return {'recorded': True, 'best': score, 'attempts': 1, 'improved': True}
        improved = score > row['best_score']
        if improved:
            conn.execute(
                'UPDATE daily_scores SET best_score = ?, attempts = attempts + 1, first_best_at = ? '
                'WHERE puzzle_date = ? AND user_id = ?', (score, now, puzzle_date, user_id))
        else:
            conn.execute('UPDATE daily_scores SET attempts = attempts + 1 '
                         'WHERE puzzle_date = ? AND user_id = ?', (puzzle_date, user_id))
        return {'recorded': True, 'best': max(score, row['best_score']),
                'attempts': row['attempts'] + 1, 'improved': improved}


def mark_daily_revealed(puzzle_date, user_id):
    """Jelzi, hogy a felhasználó megnézte a megoldást (innentől nem javíthat a ranglistán)."""
    with _db() as conn:
        conn.execute(
            'INSERT INTO daily_scores (puzzle_date, user_id, best_score, attempts, revealed) '
            'VALUES (?, ?, 0, 0, 1) '
            'ON CONFLICT(puzzle_date, user_id) DO UPDATE SET revealed = 1', (puzzle_date, user_id))


_DAILY_ORDER = 'ds.best_score DESC, ds.attempts ASC, ds.first_best_at ASC, ds.user_id ASC'


def get_daily_leaderboard(puzzle_date, limit=20, user_id=None):
    """A nap ranglistája (csak azok, akik legalább egy lépést beküldtek).
    Visszatér: (entries, me) — `me` a megadott felhasználó helyezése (a top listán kívül is)."""
    limit = max(1, min(int(limit), LEADERBOARD_MAX_LIMIT))
    with _db() as conn:
        rows = conn.execute(
            'SELECT ds.user_id AS user_id, u.display_name AS display_name, ds.best_score AS best_score, '
            'ds.attempts AS attempts FROM daily_scores ds JOIN users u ON u.id = ds.user_id '
            f'WHERE ds.puzzle_date = ? AND ds.attempts > 0 ORDER BY {_DAILY_ORDER}', (puzzle_date,)
        ).fetchall()
    entries, me = [], None
    for rank, row in enumerate(rows, start=1):
        entry = {'rank': rank, 'user_id': row['user_id'], 'display_name': row['display_name'],
                 'best_score': row['best_score'], 'attempts': row['attempts']}
        if rank <= limit:
            entries.append(entry)
        if user_id is not None and row['user_id'] == user_id:
            me = entry
    return entries, me


def get_game_analysis(game_id):
    """A gyorsítótárazott játékelemzés: {'version', 'result_json'} vagy None."""
    with _db() as conn:
        row = conn.execute('SELECT version, result_json FROM game_analysis WHERE game_id = ?',
                           (game_id,)).fetchone()
    return dict(row) if row else None


def save_game_analysis(game_id, version, result_json):
    with _db() as conn:
        conn.execute(
            'INSERT INTO game_analysis (game_id, version, result_json) VALUES (?, ?, ?) '
            'ON CONFLICT(game_id) DO UPDATE SET version = excluded.version, '
            "result_json = excluded.result_json, created_at = datetime('now')",
            (game_id, version, result_json)
        )


def grant_achievements(user_id, badges, game_id=None):
    """Kitüntetések rögzítése (kulcsonként egyszer). Visszatér: az újonnan megszerzettek listája."""
    new = []
    with _db() as conn:
        for badge in sorted(badges):
            inserted = conn.execute(
                'INSERT OR IGNORE INTO achievements (user_id, badge, game_id) VALUES (?, ?, ?)',
                (user_id, badge, game_id)
            ).rowcount
            if inserted:
                new.append(badge)
    return new


def get_user_achievements(user_id):
    """A felhasználó kitüntetései: [{badge, game_id, earned_at}], megszerzés sorrendjében."""
    with _db() as conn:
        rows = conn.execute(
            'SELECT badge, game_id, earned_at FROM achievements WHERE user_id = ? '
            'ORDER BY earned_at, rowid', (user_id,)
        ).fetchall()
    return [dict(r) for r in rows]


def get_game_players(game_id):
    """Visszaadja a játék játékosait."""
    with _db() as conn:
        rows = conn.execute(
            'SELECT player_name, user_id, final_score FROM game_players WHERE game_id = ?',
            (game_id,)
        ).fetchall()
    return [dict(r) for r in rows]


def abandon_game(room_id):
    """Játék elhagyása: status='abandoned'."""
    with _db() as conn:
        conn.execute(
            "UPDATE saved_games SET status = 'abandoned', updated_at = datetime('now') "
            "WHERE room_id = ? AND status = 'active'",
            (room_id,)
        )


def abandon_game_by_id(game_id):
    """Játék elhagyása ID alapján: status='abandoned'."""
    with _db() as conn:
        conn.execute(
            "UPDATE saved_games SET status = 'abandoned', updated_at = datetime('now') "
            "WHERE id = ? AND status = 'active'",
            (game_id,)
        )


# --- Friendship System ---

def send_friend_request(user_id, friend_id):
    """Barátkérés küldése."""
    if user_id == friend_id:
        return False, "Nem küldhetsz magadnak barátkérést."
    
    with _db() as conn:
        # Check if friend exists
        friend = conn.execute("SELECT id FROM users WHERE id = ?", (friend_id,)).fetchone()
        if not friend:
            return False, "Felhasználó nem található."
            
        # Check existing friendship or request
        existing = conn.execute(
            "SELECT status, user_id, friend_id FROM friendships WHERE "
            "(user_id = ? AND friend_id = ?) OR (user_id = ? AND friend_id = ?)",
            (user_id, friend_id, friend_id, user_id)
        ).fetchone()
        
        if existing:
            if existing['status'] == 'accepted':
                return False, "Már barátok vagytok."
            elif existing['user_id'] == user_id:
                return False, "Már küldtél barátkérést ennek a felhasználónak."
            else:
                return False, "Ez a felhasználó már küldött neked barátkérést. Fogadd el!"

        try:
            conn.execute(
                "INSERT INTO friendships (user_id, friend_id, status) VALUES (?, ?, 'pending')",
                (user_id, friend_id)
            )
            return True, "Barátkérés elküldve."
        except sqlite3.IntegrityError:
            return False, "Hiba történt a barátkérés során."


def accept_friend_request(user_id, requester_id):
    """Barátkérés elfogadása."""
    with _db() as conn:
        updated = conn.execute(
            "UPDATE friendships SET status = 'accepted' "
            "WHERE user_id = ? AND friend_id = ? AND status = 'pending'",
            (requester_id, user_id)
        ).rowcount
        
        if updated:
            return True, "Barátkérés elfogadva."
        return False, "Barátkérés nem található vagy már elfogadtad."


def decline_friend_request(user_id, requester_id):
    """Barátkérés elutasítása."""
    with _db() as conn:
        deleted = conn.execute(
            "DELETE FROM friendships "
            "WHERE user_id = ? AND friend_id = ? AND status = 'pending'",
            (requester_id, user_id)
        ).rowcount
        
        if deleted:
            return True, "Barátkérés elutasítva."
        return False, "Barátkérés nem található."


def remove_friend(user_id, friend_id):
    """Barát törlése."""
    with _db() as conn:
        deleted = conn.execute(
            "DELETE FROM friendships "
            "WHERE ((user_id = ? AND friend_id = ?) OR (user_id = ? AND friend_id = ?)) "
            "AND status = 'accepted'",
            (user_id, friend_id, friend_id, user_id)
        ).rowcount
        
        if deleted:
            return True, "Barát törölve."
        return False, "Nem vagytok barátok."


def get_friends(user_id):
    """Barátlista lekérdezése."""
    with _db() as conn:
        rows = conn.execute('''
            SELECT u.id, u.display_name
            FROM friendships f
            JOIN users u ON (f.user_id = u.id OR f.friend_id = u.id)
            WHERE (f.user_id = ? OR f.friend_id = ?) 
              AND f.status = 'accepted'
              AND u.id != ?
            ORDER BY u.display_name
        ''', (user_id, user_id, user_id)).fetchall()
        return [dict(r) for r in rows]


def get_pending_requests(user_id):
    """Bejövő barátkérések."""
    with _db() as conn:
        rows = conn.execute('''
            SELECT u.id, u.display_name, f.created_at
            FROM friendships f
            JOIN users u ON f.user_id = u.id
            WHERE f.friend_id = ? AND f.status = 'pending'
            ORDER BY f.created_at DESC
        ''', (user_id,)).fetchall()
        return [dict(r) for r in rows]


def get_sent_requests(user_id):
    """Kimenő barátkérések."""
    with _db() as conn:
        rows = conn.execute('''
            SELECT u.id, u.display_name, f.created_at
            FROM friendships f
            JOIN users u ON f.friend_id = u.id
            WHERE f.user_id = ? AND f.status = 'pending'
            ORDER BY f.created_at DESC
        ''', (user_id,)).fetchall()
        return [dict(r) for r in rows]


def search_users(query, exclude_user_id, limit=10):
    """Felhasználók keresése megjelenítési név részlet (kis/nagybetű- és
    ékezet-egyező kisbetűsítéssel) vagy pontos email cím alapján.

    Az email cím nem részletre illeszkedik: így a keresés nem szivárogtatja ki,
    hogy kinek milyen email címe van.
    """
    query = (query or '').strip()
    if len(query) < 2:
        return []

    escaped = query.lower().replace('\\', '\\\\').replace('%', '\\%').replace('_', '\\_')
    with _db() as conn:
        # SQLite lower()/LIKE csak ASCII-ra kis/nagybetű-független: Python lower() kell az ékezetekhez
        conn.create_function('py_lower', 1, lambda v: v.lower() if isinstance(v, str) else v)
        rows = conn.execute(
            "SELECT id, display_name "
            "FROM users "
            "WHERE (py_lower(display_name) LIKE ? ESCAPE '\\' OR email_lower = ?) "
            "  AND id != ? "
            "ORDER BY display_name "
            "LIMIT ?",
            (f"%{escaped}%", query.lower(), exclude_user_id, limit)
        ).fetchall()
        return [dict(r) for r in rows]


# --- Ranglista ---

LEADERBOARD_METRICS = ('rating', 'wins', 'win_rate', 'avg_score', 'best_game')
# A százalékos / átlag alapú listákra csak elég sok játékkal lehet kerülni (különben egy
# nyert játék után 100%-kal vezetne valaki)
LEADERBOARD_MIN_GAMES = {'rating': elo.MIN_RATED_GAMES_FOR_RANKING, 'wins': 1, 'win_rate': 3,
                         'avg_score': 3, 'best_game': 1}
LEADERBOARD_MAX_LIMIT = 100

# A rendezés (a metrikához tartozó, rögzített SQL-részlet; felhasználói adat nem kerül bele)
_LEADERBOARD_ORDER = {
    'rating': 'u.rating DESC, games_won DESC, total_score DESC',
    'wins': 'games_won DESC, (games_won * 1.0 / games_played) DESC, total_score DESC',
    'win_rate': '(games_won * 1.0 / games_played) DESC, games_played DESC, games_won DESC',
    'avg_score': '(total_score * 1.0 / games_played) DESC, games_played DESC',
    'best_game': 'best_score DESC, games_won DESC',
}


def _leaderboard_entry(row, rank):
    played = row['games_played']
    return {
        'rank': rank,
        'user_id': row['user_id'],
        'display_name': row['display_name'],
        'games_played': played,
        'games_won': row['games_won'],
        'win_rate': round(row['games_won'] / played * 100, 1) if played else 0,
        'avg_score': round(row['total_score'] / played, 1) if played else 0,
        'total_score': row['total_score'],
        'best_score': row['best_score'],
        'rating': row['rating'],
        'rated_games': row['rated_games'],
    }


def get_leaderboard(metric='wins', limit=50, user_id=None):
    """Regisztrált játékosok ranglistája (csak befejezett, robot nélküli játékokból).

    metric: 'rating' | 'wins' | 'win_rate' | 'avg_score' | 'best_game'
    Visszatér: (entries, me) — `entries` a legjobb `limit` játékos (rank-kal), `me` a megadott
    felhasználó helyezése (akkor is, ha nincs a top listában), vagy None.
    """
    if metric not in LEADERBOARD_METRICS:
        metric = 'wins'
    limit = max(1, min(int(limit), LEADERBOARD_MAX_LIMIT))
    min_games = LEADERBOARD_MIN_GAMES[metric]
    # Az értékszám-ranglistára csak elég sok értékelt (regisztráltak közti) játék után lehet kerülni
    rated_only = 'AND u.rated_games >= ? ' if metric == 'rating' else ''
    params = (min_games, min_games) if metric == 'rating' else (min_games,)

    with _db() as conn:
        rows = conn.execute(
            'SELECT gp.user_id AS user_id, u.display_name AS display_name, '
            'COUNT(*) AS games_played, COALESCE(SUM(gp.is_winner), 0) AS games_won, '
            'COALESCE(SUM(gp.final_score), 0) AS total_score, MAX(gp.final_score) AS best_score, '
            'u.rating AS rating, u.rated_games AS rated_games '
            'FROM game_players gp '
            "JOIN saved_games sg ON sg.id = gp.game_id AND sg.status = 'finished' AND sg.has_bots = 0 "
            'JOIN users u ON u.id = gp.user_id '
            'WHERE gp.user_id IS NOT NULL '
            f'{rated_only}'
            'GROUP BY gp.user_id '
            'HAVING COUNT(*) >= ? '
            f'ORDER BY {_LEADERBOARD_ORDER[metric]}, gp.user_id ASC',
            params,
        ).fetchall()

    entries = []
    me = None
    for rank, row in enumerate(rows, start=1):
        if rank <= limit:
            entries.append(_leaderboard_entry(row, rank))
        if user_id is not None and row['user_id'] == user_id:
            me = _leaderboard_entry(row, rank)
    return entries, me
