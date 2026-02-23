import sqlite3
import secrets
import time
from datetime import datetime, timedelta
from werkzeug.security import generate_password_hash, check_password_hash

from config import DB_PATH, SESSION_MAX_AGE_DAYS, VERIFICATION_CODE_EXPIRY_MINUTES, VERIFICATION_MAX_ATTEMPTS


def get_db():
    """Új SQLite kapcsolat létrehozása."""
    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row
    conn.execute('PRAGMA journal_mode=WAL')
    conn.execute('PRAGMA foreign_keys=ON')
    return conn


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
    ''')
    conn.commit()
    conn.close()


# --- User CRUD ---

def get_user_by_email(email):
    """Felhasználó keresése email alapján."""
    conn = get_db()
    user = conn.execute(
        'SELECT * FROM users WHERE email_lower = ?', (email.lower().strip(),)
    ).fetchone()
    conn.close()
    return user


def get_user_by_id(user_id):
    """Felhasználó keresése ID alapján."""
    conn = get_db()
    user = conn.execute('SELECT * FROM users WHERE id = ?', (user_id,)).fetchone()
    conn.close()
    return user


def create_user(email, display_name, password):
    """Új felhasználó létrehozása. Visszaad (success, user_id_or_error)."""
    email_lower = email.lower().strip()
    if get_user_by_email(email_lower):
        return False, 'Ez az email cím már regisztrálva van.'

    password_hash = generate_password_hash(password, method='pbkdf2:sha256:260000')
    conn = get_db()
    try:
        cursor = conn.execute(
            'INSERT INTO users (email, email_lower, display_name, password_hash) VALUES (?, ?, ?, ?)',
            (email.strip(), email_lower, display_name.strip(), password_hash)
        )
        conn.commit()
        user_id = cursor.lastrowid
        conn.close()
        return True, user_id
    except sqlite3.IntegrityError:
        conn.close()
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

    conn = get_db()
    # Régi, nem használt kódok érvénytelenítése ehhez az emailhez
    conn.execute(
        'UPDATE verification_codes SET used = 1 WHERE email = ? AND used = 0',
        (email.lower().strip(),)
    )
    conn.execute(
        'INSERT INTO verification_codes (email, code, expires_at) VALUES (?, ?, ?)',
        (email.lower().strip(), code, expires_at)
    )
    conn.commit()
    conn.close()
    return code


def verify_code(email, code):
    """Verifikációs kód ellenőrzés. Visszaad (success, message)."""
    email_lower = email.lower().strip()
    conn = get_db()

    row = conn.execute(
        'SELECT * FROM verification_codes WHERE email = ? AND used = 0 ORDER BY created_at DESC LIMIT 1',
        (email_lower,)
    ).fetchone()

    if not row:
        conn.close()
        return False, 'Nincs érvényes verifikációs kód. Kérj újat.'

    # Lejárat ellenőrzés
    expires_at = datetime.strptime(row['expires_at'], '%Y-%m-%d %H:%M:%S')
    if datetime.utcnow() > expires_at:
        conn.execute('UPDATE verification_codes SET used = 1 WHERE id = ?', (row['id'],))
        conn.commit()
        conn.close()
        return False, 'A kód lejárt. Kérj újat.'

    # Próbálkozások ellenőrzés
    if row['attempts'] >= VERIFICATION_MAX_ATTEMPTS:
        conn.execute('UPDATE verification_codes SET used = 1 WHERE id = ?', (row['id'],))
        conn.commit()
        conn.close()
        return False, 'Túl sok próbálkozás. Kérj új kódot.'

    # Kód egyezés
    if row['code'] != code:
        conn.execute(
            'UPDATE verification_codes SET attempts = attempts + 1 WHERE id = ?', (row['id'],)
        )
        conn.commit()
        remaining = VERIFICATION_MAX_ATTEMPTS - row['attempts'] - 1
        conn.close()
        return False, f'Hibás kód. Még {remaining} próbálkozásod van.'

    # Sikeres - kód felhasználva
    conn.execute('UPDATE verification_codes SET used = 1 WHERE id = ?', (row['id'],))
    conn.commit()
    conn.close()
    return True, 'Kód elfogadva.'


# --- Sessions ---

def create_session(user_id):
    """Új session token létrehozása. Visszaadja a tokent."""
    token = secrets.token_urlsafe(48)
    expires_at = (datetime.utcnow() + timedelta(days=SESSION_MAX_AGE_DAYS)).strftime('%Y-%m-%d %H:%M:%S')

    conn = get_db()
    conn.execute(
        'INSERT INTO sessions (user_id, token, expires_at) VALUES (?, ?, ?)',
        (user_id, token, expires_at)
    )
    conn.commit()
    conn.close()
    return token


def validate_session(token):
    """Session token ellenőrzés. Visszaad user dict-et vagy None-t."""
    if not token:
        return None

    conn = get_db()
    row = conn.execute(
        'SELECT s.*, u.id as uid, u.email, u.display_name, u.games_played, u.games_won, u.total_score '
        'FROM sessions s JOIN users u ON s.user_id = u.id '
        'WHERE s.token = ?',
        (token,)
    ).fetchone()

    if not row:
        conn.close()
        return None

    expires_at = datetime.strptime(row['expires_at'], '%Y-%m-%d %H:%M:%S')
    if datetime.utcnow() > expires_at:
        conn.execute('DELETE FROM sessions WHERE id = ?', (row['id'],))
        conn.commit()
        conn.close()
        return None

    conn.close()
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
    conn = get_db()
    conn.execute('DELETE FROM sessions WHERE token = ?', (token,))
    conn.commit()
    conn.close()


def cleanup_expired():
    """Lejárt sessionök és verifikációs kódok törlése."""
    now = datetime.utcnow().strftime('%Y-%m-%d %H:%M:%S')
    conn = get_db()
    conn.execute('DELETE FROM sessions WHERE expires_at < ?', (now,))
    conn.execute('DELETE FROM verification_codes WHERE expires_at < ?', (now,))
    conn.commit()
    conn.close()
