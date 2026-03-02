import sqlite3
import secrets
import time
from contextlib import contextmanager
from datetime import datetime, timedelta
from werkzeug.security import generate_password_hash, check_password_hash

from config import DB_PATH, SESSION_MAX_AGE_DAYS, VERIFICATION_CODE_EXPIRY_MINUTES, VERIFICATION_MAX_ATTEMPTS


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
    ''')
    # Migráció: owner_name oszlop hozzáadása ha nem létezik
    try:
        conn.execute('SELECT owner_name FROM saved_games LIMIT 1')
    except sqlite3.OperationalError:
        conn.execute("ALTER TABLE saved_games ADD COLUMN owner_name TEXT NOT NULL DEFAULT ''")
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
    expires_at = (datetime.utcnow() + timedelta(minutes=VERIFICATION_CODE_EXPIRY_MINUTES)).strftime('%Y-%m-%d %H:%M:%S')
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
        if datetime.utcnow() > expires_at:
            conn.execute('UPDATE verification_codes SET used = 1 WHERE id = ?', (row['id'],))
            return False, 'A kód lejárt. Kérj újat.'

        if row['attempts'] >= VERIFICATION_MAX_ATTEMPTS:
            conn.execute('UPDATE verification_codes SET used = 1 WHERE id = ?', (row['id'],))
            return False, 'Túl sok próbálkozás. Kérj új kódot.'

        if row['code'] != code:
            conn.execute(
                'UPDATE verification_codes SET attempts = attempts + 1 WHERE id = ?', (row['id'],)
            )
            remaining = VERIFICATION_MAX_ATTEMPTS - row['attempts'] - 1
            return False, f'Hibás kód. Még {remaining} próbálkozásod van.'

        conn.execute('UPDATE verification_codes SET used = 1 WHERE id = ?', (row['id'],))
        return True, 'Kód elfogadva.'


# --- Sessions ---

def create_session(user_id):
    """Új session token létrehozása. Visszaadja a tokent."""
    token = secrets.token_urlsafe(48)
    expires_at = (datetime.utcnow() + timedelta(days=SESSION_MAX_AGE_DAYS)).strftime('%Y-%m-%d %H:%M:%S')

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
            'SELECT s.*, u.id as uid, u.email, u.display_name, u.games_played, u.games_won, u.total_score '
            'FROM sessions s JOIN users u ON s.user_id = u.id '
            'WHERE s.token = ?',
            (token,)
        ).fetchone()

        if not row:
            return None

        expires_at = datetime.strptime(row['expires_at'], '%Y-%m-%d %H:%M:%S')
        if datetime.utcnow() > expires_at:
            conn.execute('DELETE FROM sessions WHERE id = ?', (row['id'],))
            return None

        return {
            'id': row['uid'],
            'email': row['email'],
            'display_name': row['display_name'],
            'games_played': row['games_played'],
            'games_won': row['games_won'],
            'total_score': row['total_score'],
        }


def delete_session(token):
    """Session törlése (logout)."""
    if not token:
        return
    with _db() as conn:
        conn.execute('DELETE FROM sessions WHERE token = ?', (token,))


def cleanup_expired():
    """Lejárt sessionök és verifikációs kódok törlése."""
    now = datetime.utcnow().strftime('%Y-%m-%d %H:%M:%S')
    with _db() as conn:
        conn.execute('DELETE FROM sessions WHERE expires_at < ?', (now,))
        conn.execute('DELETE FROM verification_codes WHERE expires_at < ?', (now,))


# --- Game persistence ---

def save_game(room_id, room_name, state_json, challenge_mode, players_data=None, owner_name=''):
    """Játék mentése (upsert: room_id + active alapján). Visszaadja a game_id-t.
    players_data: [{player_name, user_id (or None), score}, ...] — ha megadva, upsert a game_players-be.
    """
    with _db() as conn:
        now = datetime.utcnow().strftime('%Y-%m-%d %H:%M:%S')
        row = conn.execute(
            "SELECT id FROM saved_games WHERE room_id = ? AND status = 'active'",
            (room_id,)
        ).fetchone()
        if row:
            conn.execute(
                'UPDATE saved_games SET state_json = ?, updated_at = ?, owner_name = ? WHERE id = ?',
                (state_json, now, owner_name, row['id'])
            )
            game_id = row['id']
        else:
            cursor = conn.execute(
                'INSERT INTO saved_games (room_id, room_name, state_json, status, challenge_mode, owner_name) '
                'VALUES (?, ?, ?, ?, ?, ?)',
                (room_id, room_name, state_json, 'active', 1 if challenge_mode else 0, owner_name)
            )
            game_id = cursor.lastrowid

        if players_data:
            _upsert_game_players(conn, game_id, players_data)

    return game_id


def _upsert_game_players(conn, game_id, players_data):
    """Game players upsert: INSERT ha nem létezik, UPDATE score ha igen."""
    for pd in players_data:
        existing = conn.execute(
            'SELECT id FROM game_players WHERE game_id = ? AND player_name = ?',
            (game_id, pd['player_name'])
        ).fetchone()
        if existing:
            conn.execute(
                'UPDATE game_players SET final_score = ?, user_id = COALESCE(?, user_id) WHERE id = ?',
                (pd.get('score', 0), pd.get('user_id'), existing['id'])
            )
        else:
            conn.execute(
                'INSERT INTO game_players (game_id, user_id, player_name, final_score) '
                'VALUES (?, ?, ?, ?)',
                (game_id, pd.get('user_id'), pd['player_name'], pd.get('score', 0))
            )


def finish_game(room_id, state_json, players_data):
    """Játék befejezése: status='finished', game_players INSERT, users stats UPDATE.
    players_data: [{player_name, user_id (or None), final_score, is_winner}, ...]
    """
    with _db() as conn:
        now = datetime.utcnow().strftime('%Y-%m-%d %H:%M:%S')
        row = conn.execute(
            "SELECT id FROM saved_games WHERE room_id = ? AND status = 'active'",
            (room_id,)
        ).fetchone()
        if not row:
            cursor = conn.execute(
                'INSERT INTO saved_games (room_id, state_json, status) VALUES (?, ?, ?)',
                (room_id, state_json, 'finished')
            )
            game_id = cursor.lastrowid
        else:
            game_id = row['id']
            conn.execute(
                'UPDATE saved_games SET state_json = ?, status = ?, updated_at = ? WHERE id = ?',
                (state_json, 'finished', now, game_id)
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
            'gp.final_score, gp.is_winner, gp.player_name '
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
            'opponents': opponents_by_game.get(r['game_id'], []),
        }
        for r in rows
    ]


def get_game_by_id(game_id):
    """Egyetlen játék sor."""
    with _db() as conn:
        row = conn.execute('SELECT * FROM saved_games WHERE id = ?', (game_id,)).fetchone()
    return dict(row) if row else None


def get_user_active_games(user_id):
    """Felhasználó aktív (mentett) játékai (N+1 fix)."""
    with _db() as conn:
        rows = conn.execute(
            'SELECT gp.game_id, gp.player_name, gp.final_score, '
            'sg.room_name, sg.room_id, sg.created_at, sg.updated_at, sg.challenge_mode, sg.owner_name '
            'FROM game_players gp '
            'JOIN saved_games sg ON gp.game_id = sg.id '
            "WHERE gp.user_id = ? AND sg.status = 'active' "
            'ORDER BY sg.updated_at DESC',
            (user_id,)
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
