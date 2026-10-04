"""Admin panel: a rendszer állapota — szerver, adatbázis (mentés, takarítás), naplók, folyamatok, konfiguráció, verziók.

Csak olvasható / karbantartó funkciók; a futó játékokat nem érintik. A modul a Flask kéréstől független.
"""
import collections
import datetime as _dt
import gc
import logging
import os
import re
import sqlite3
import subprocess
import sys
import threading
import time
from importlib import metadata

import admin
import auth
import config
from admin import AdminError

START_TIME = time.time()
_ROOT = os.path.dirname(os.path.abspath(__file__))

# ===== Szerver naplója: gyűrűpuffer =====

LOG_BUFFER_SIZE = 2000
LOG_LEVELS = ('DEBUG', 'INFO', 'WARNING', 'ERROR', 'CRITICAL')
_LEVEL_ORDER = {name: i for i, name in enumerate(LOG_LEVELS)}
_LOCATION_RE = re.compile(r'File "([^"]+)", line (\d+), in (\S+)')
_print_level_re = re.compile(r'\b(hiba|error|traceback|exception)\b', re.IGNORECASE)
_print_warn_re = re.compile(r'\b(figyelem|warning|warn)\b', re.IGNORECASE)


class LogBuffer(logging.Handler):
    """A szerver naplósorainak gyűrűpuffere (az admin panel „Naplók” nézetéhez)."""

    def __init__(self, size=LOG_BUFFER_SIZE):
        super().__init__(level=logging.DEBUG)
        self.records = collections.deque(maxlen=size)
        self._seq = 0
        self._lock = threading.Lock()

    def add(self, level, logger, message, exc_type=None, location=None, ts=None):
        with self._lock:
            self._seq += 1
            self.records.append({'id': self._seq, 'ts': ts if ts is not None else time.time(), 'level': level,
                                 'logger': logger, 'message': message[:2000], 'exc_type': exc_type,
                                 'location': location})

    def emit(self, record):
        try:
            exc_type = location = None
            if record.exc_info and record.exc_info[0] is not None:
                exc_type = record.exc_info[0].__name__
                tb = record.exc_info[2]
                while tb is not None and tb.tb_next is not None:
                    tb = tb.tb_next
                if tb is not None:
                    location = f'{os.path.basename(tb.tb_frame.f_code.co_filename)}:{tb.tb_lineno} ({tb.tb_frame.f_code.co_name})'
            self.add(record.levelname if record.levelname in _LEVEL_ORDER else 'INFO', record.name,
                     record.getMessage(), exc_type, location, record.created)
        except Exception:    # a naplózás hibája sosem törheti meg a szervert
            pass

    def query(self, level=None, q=None, since_id=0, limit=200):
        """A legutóbbi sorok (régebbitől az újabbig), szint szerint (az adott szinttől felfelé) és szöveg szerint szűrve."""
        floor = _LEVEL_ORDER.get(level, 0)
        needle = (q or '').lower()
        with self._lock:
            records = list(self.records)
        rows = [r for r in records if r['id'] > since_id and _LEVEL_ORDER.get(r['level'], 1) >= floor
                and (not needle or needle in r['message'].lower() or needle in (r['logger'] or '').lower())]
        return rows[-limit:]

    def error_groups(self):
        """A hibák csoportosítva (kivétel típusa + hely), darabszámmal és az utolsó előfordulással."""
        groups = {}
        with self._lock:
            records = list(self.records)
        for r in records:
            if _LEVEL_ORDER.get(r['level'], 1) < _LEVEL_ORDER['ERROR']:
                continue
            key = (r['exc_type'] or '', r['location'] or '', '' if r['exc_type'] else (r['message'] or '')[:80])
            group = groups.setdefault(key, {'exc_type': r['exc_type'], 'location': r['location'],
                                            'sample': r['message'][:300], 'count': 0, 'last_ts': 0, 'logger': r['logger']})
            group['count'] += 1
            group['last_ts'] = max(group['last_ts'], r['ts'])
        return sorted(groups.values(), key=lambda g: g['last_ts'], reverse=True)


log_buffer = LogBuffer()
_installed = {'logging': False, 'output': False}


def install_log_capture():
    """A gyűrűpuffer rákötése a naplózás gyökerére (egyszer)."""
    if not _installed['logging']:
        logging.getLogger().addHandler(log_buffer)
        if logging.getLogger().level in (logging.NOTSET, logging.WARNING):
            logging.getLogger().setLevel(logging.INFO)
        _installed['logging'] = True


class _Tee:
    """A `print` kimenet másolata a pufferbe (a szerver sok üzenete nem a `logging`-on megy)."""

    def __init__(self, stream, stream_name):
        self._stream = stream
        self._name = stream_name
        self._partial = ''

    def write(self, text):
        written = self._stream.write(text)
        self._partial += text
        while '\n' in self._partial:
            line, self._partial = self._partial.split('\n', 1)
            line = line.strip()
            if line:
                level = 'ERROR' if _print_level_re.search(line) else (
                    'WARNING' if _print_warn_re.search(line) else 'INFO')
                log_buffer.add(level, self._name, line)
        return written

    def __getattr__(self, name):
        return getattr(self._stream, name)


def capture_output():
    """A `print` kimenetek (stdout / stderr) másolása a naplópufferbe (szerverindításkor, egyszer)."""
    if not _installed['output']:
        sys.stdout = _Tee(sys.stdout, 'stdout')
        sys.stderr = _Tee(sys.stderr, 'stderr')
        _installed['output'] = True


# ===== Folyamatok (háttérfeladatok) =====

_jobs = {}
KNOWN_JOBS = ('async_sweeper', 'admin_broadcaster', 'maintenance_tasks')


def job_ran(name, ok=True, info=None):
    """Egy háttérfeladat futásának rögzítése (utolsó futás, sikeresség, szám / hiba)."""
    job = _jobs.setdefault(name, {'runs': 0, 'errors': 0})
    job['runs'] += 1
    job['last_run'] = time.time()
    job['ok'] = bool(ok)
    job['info'] = info
    if not ok:
        job['errors'] += 1


def jobs_status():
    jobs = []
    for name in KNOWN_JOBS:
        job = _jobs.get(name)
        jobs.append({'name': name, 'runs': job['runs'] if job else 0, 'errors': job['errors'] if job else 0,
                     'last_run': job['last_run'] if job else None, 'ok': job['ok'] if job else None,
                     'info': job['info'] if job else None})
    return jobs


def analysis_queue():
    """A futó (és hibás) játékelemzések: [{game_id, done, total, error}]."""
    import routes
    return [{'game_id': gid, 'done': job.get('done', 0), 'total': job.get('total', 0),
             'error': bool(job.get('error'))} for gid, job in list(routes._analysis_jobs.items())]


# ===== Szerver =====

_greenlet_cache = {'at': 0.0, 'count': None}
_cpu_state = {'wall': None, 'cpu': None, 'percent': None}


def _rss_bytes():
    try:
        with open('/proc/self/status', encoding='utf-8') as fh:
            for line in fh:
                if line.startswith('VmRSS:'):
                    return int(line.split()[1]) * 1024
    except (OSError, ValueError):
        pass
    try:
        import resource
        peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        return peak if sys.platform == 'darwin' else peak * 1024
    except (ImportError, ValueError):
        return None


def _greenlets():
    """A gevent greenletek száma (drága, ezért 30 másodpercre gyorsítótárazva)."""
    now = time.time()
    if now - _greenlet_cache['at'] > 30:
        try:
            import greenlet
            _greenlet_cache['count'] = sum(1 for obj in gc.get_objects() if isinstance(obj, greenlet.greenlet))
        except Exception:
            _greenlet_cache['count'] = None
        _greenlet_cache['at'] = now
    return _greenlet_cache['count']


def _cpu_percent():
    """A folyamat átlagos CPU-terhelése az előző hívás óta (%)."""
    wall, cpu = time.time(), time.process_time()
    last_wall, last_cpu = _cpu_state['wall'], _cpu_state['cpu']
    _cpu_state.update(wall=wall, cpu=cpu)
    if last_wall is None or wall - last_wall < 0.5:
        return _cpu_state['percent']
    _cpu_state['percent'] = round(100.0 * (cpu - last_cpu) / (wall - last_wall), 1)
    return _cpu_state['percent']


def db_file_size():
    path = auth.DB_PATH
    total = 0
    for suffix in ('', '-wal', '-shm'):
        try:
            total += os.path.getsize(path + suffix)
        except OSError:
            pass
    return total


def process_info():
    return {'uptime': int(time.time() - START_TIME), 'rss': _rss_bytes(), 'cpu_percent': _cpu_percent(),
            'greenlets': _greenlets(), 'threads': threading.active_count(), 'db_size': db_file_size(),
            'python': sys.version.split()[0]}


def services_info():
    """A szolgáltatások elérhetősége: szótár, robot szókincse, push, SMTP, tunnel."""
    import ai_player
    import dictionary
    import push_service
    import tunnel
    return {
        'dictionary': bool(dictionary.is_available()),
        'vocabulary': ai_player._vocabulary is not None,
        'push': bool(push_service.is_available()),
        'smtp': bool(config.SMTP_CONFIGURED),
        'tunnel': tunnel.status(),
    }


# ===== Verziók =====

_PACKAGES = ('Flask', 'Flask-SocketIO', 'python-socketio', 'python-engineio', 'gevent', 'gevent-websocket',
             'pywebpush', 'Werkzeug')
_git_cache = {}


def git_commit():
    """A futó kód git commitja (rövid azonosító + az első sor), vagy None. Egyszer olvasódik."""
    if 'value' not in _git_cache:
        value = None
        try:
            out = subprocess.run(['git', '-C', _ROOT, 'log', '-1', '--format=%h %s'], capture_output=True, text=True,
                                 timeout=5)
            if out.returncode == 0 and out.stdout.strip():
                value = out.stdout.strip()[:200]
        except (OSError, subprocess.SubprocessError):
            pass
        _git_cache['value'] = value
    return _git_cache['value']


def test_count():
    """A tesztek száma a CLAUDE.md-ből („Összesen: N teszt”), vagy None."""
    try:
        with open(os.path.join(_ROOT, 'CLAUDE.md'), encoding='utf-8') as fh:
            match = re.search(r'Összesen:\s*(\d+)\s*teszt', fh.read())
        return int(match.group(1)) if match else None
    except OSError:
        return None


def versions():
    import routes
    import admin_routes
    packages = {}
    for name in _PACKAGES:
        try:
            packages[name] = metadata.version(name)
        except metadata.PackageNotFoundError:
            packages[name] = None
    return {'git': git_commit(), 'asset_version': routes.asset_version(),
            'admin_asset_version': admin_routes.admin_asset_version(), 'python': sys.version.split()[0],
            'platform': sys.platform, 'packages': packages, 'tests': test_count()}


# ===== Konfiguráció (csak olvasható, a titkok maszkolva) =====

MASK = '••••'


def config_view():
    """A `config.py` értékei és a releváns környezeti változók; a titkoknál csak a „beállítva / nincs” látszik."""
    def plain(key, value):
        return {'key': key, 'value': value, 'secret': False, 'set': value not in (None, '', [])}

    def secret(key, is_set):
        return {'key': key, 'value': MASK if is_set else '', 'secret': True, 'set': bool(is_set)}

    import push_service
    items = [
        plain('SMTP_HOST', config.SMTP_HOST), plain('SMTP_PORT', config.SMTP_PORT),
        plain('SMTP_USER', config.SMTP_USER), plain('SMTP_FROM', config.SMTP_FROM),
        secret('SMTP_PASSWORD', bool(config.SMTP_PASSWORD)),
        plain('SMTP_CONFIGURED', config.SMTP_CONFIGURED),
        secret('SECRET_KEY', bool(os.environ.get('SECRET_KEY'))),
        secret('VAPID_PRIVATE_KEY', bool(os.environ.get('VAPID_PRIVATE_KEY'))),
        plain('VAPID_SUBJECT', push_service.subject()),
        plain('DB_PATH', config.DB_PATH), plain('BACKUP_DIR', config.BACKUP_DIR),
        plain('PORT', os.environ.get('PORT', '5000')),
        plain('SESSION_MAX_AGE_DAYS', config.SESSION_MAX_AGE_DAYS),
        plain('VERIFICATION_CODE_EXPIRY_MINUTES', config.VERIFICATION_CODE_EXPIRY_MINUTES),
        plain('VERIFICATION_MAX_ATTEMPTS', config.VERIFICATION_MAX_ATTEMPTS),
        plain('EMAIL_VERIFIED_WINDOW_MINUTES', config.EMAIL_VERIFIED_WINDOW_MINUTES),
        plain('WORD_REJECT_THRESHOLD', config.WORD_REJECT_THRESHOLD),
        plain('ADMIN_EMAILS', sorted(config.ADMIN_EMAILS)),
        plain('ADMIN_SESSION_IDLE_MINUTES', config.ADMIN_SESSION_IDLE_MINUTES),
        plain('ADMIN_SUDO_MINUTES', config.ADMIN_SUDO_MINUTES),
        plain('ADMIN_IP_ALLOWLIST', sorted(str(n) for n in config.ADMIN_IP_ALLOWLIST)),
        plain('AUTH_RATE_LIMITS', {k: list(v) for k, v in config.AUTH_RATE_LIMITS.items()}),
    ]
    return items


# ===== Adatbázis =====

# Az `sqlite_master`-ből jövő táblanevek idézőjelezve kerülnek a lekérdezésbe
_TABLE_NAME_RE = re.compile(r'^[A-Za-z_][A-Za-z0-9_]*$')


def table_counts():
    with auth.transaction() as conn:
        names = [r[0] for r in conn.execute(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")]
        return [{'name': n, 'rows': conn.execute(f'SELECT COUNT(*) FROM "{n}"').fetchone()[0]}
                for n in names if _TABLE_NAME_RE.match(n)]


def backup_dir():
    path = config.BACKUP_DIR
    return path if os.path.isabs(path) else os.path.join(_ROOT, path)


def list_backups():
    """A mentések: [{name, size, mtime}], a legújabbal kezdve."""
    folder = backup_dir()
    items = []
    try:
        for name in os.listdir(folder):
            if re.match(r'^scrabble-\d{8}-\d{6}\.db$', name):
                stat = os.stat(os.path.join(folder, name))
                items.append({'name': name, 'size': stat.st_size, 'mtime': stat.st_mtime})
    except OSError:
        pass
    return sorted(items, key=lambda i: i['name'], reverse=True)


def last_backup_time():
    backups = list_backups()
    return backups[0]['mtime'] if backups else None


def make_backup(directory=None):
    """Konzisztens pillanatkép az SQLite backup API-val. Visszatér: a fájl útja."""
    folder = directory or backup_dir()
    os.makedirs(folder, exist_ok=True)
    stamp = _dt.datetime.now(_dt.timezone.utc).strftime('%Y%m%d-%H%M%S')
    path = os.path.join(folder, f'scrabble-{stamp}.db')
    counter = 1
    while os.path.exists(path):   # ugyanabban a másodpercben készült második mentés
        counter += 1
        path = os.path.join(folder, f'scrabble-{stamp}-{counter}.db')
    source = sqlite3.connect(auth.DB_PATH)
    try:
        dest = sqlite3.connect(path)
        try:
            source.backup(dest)
        finally:
            dest.close()
    finally:
        source.close()
    return path


def prune_backups(keep):
    """A legutóbbi `keep` mentés megtartása. Visszatér: a törölt fájlok száma."""
    removed = 0
    for item in list_backups()[max(1, keep):]:
        try:
            os.remove(os.path.join(backup_dir(), item['name']))
            removed += 1
        except OSError:
            pass
    return removed


def maintenance_vacuum():
    """VACUUM és ANALYZE (autocommit kapcsolaton: tranzakción belül nem futhat). Visszatér: (előtte, utána) bájt."""
    before = db_file_size()
    conn = sqlite3.connect(auth.DB_PATH, isolation_level=None)
    try:
        conn.execute('VACUUM')
        conn.execute('ANALYZE')
    finally:
        conn.close()
    return before, db_file_size()


# ===== Takarítás =====

ABANDONED_DEFAULT_DAYS = 30
_CLEANUP_ITEMS = ('sessions', 'codes', 'verified_emails', 'abandoned_games', 'chat_log', 'ip_bans', 'login_events')
LOGIN_EVENT_KEEP_DAYS = 180


def _cleanup_queries(days, now):
    stamp = auth.format_ts(now)
    abandoned_cutoff = auth.format_ts(now - _dt.timedelta(days=days))
    chat_cutoff = auth.format_ts(now - _dt.timedelta(days=int(_setting('chat_log_days'))))
    login_cutoff = auth.format_ts(now - _dt.timedelta(days=LOGIN_EVENT_KEEP_DAYS))
    return {
        'sessions': ('FROM sessions WHERE expires_at < ?', [stamp]),
        'codes': ('FROM verification_codes WHERE expires_at < ? OR used = 1', [stamp]),
        'verified_emails': ('FROM verified_emails WHERE expires_at < ?', [stamp]),
        'abandoned_games': ("FROM saved_games WHERE status = 'abandoned' AND updated_at < ?", [abandoned_cutoff]),
        'chat_log': ('FROM chat_log WHERE created_at < ?', [chat_cutoff]),
        'ip_bans': ('FROM ip_bans WHERE expires_at IS NOT NULL AND expires_at < ?', [stamp]),
        'login_events': ('FROM login_events WHERE created_at < ?', [login_cutoff]),
    }


def _setting(key):
    import settings
    return settings.get(key)


def cleanup_preview(days=ABANDONED_DEFAULT_DAYS):
    """Mit törölne a takarítás: {tétel: darabszám}."""
    days = admin.int_field(days, 'days', 1, 3650)
    queries = _cleanup_queries(days, auth.utcnow())
    with auth.transaction() as conn:
        return {name: conn.execute('SELECT COUNT(*) ' + sql, params).fetchone()[0]
                for name, (sql, params) in queries.items()}


def cleanup_run(ctx, items, days, reason):
    """A kijelölt tételek takarítása (naplózva, egy tranzakcióban). Visszatér: {tétel: törölt sorok}."""
    days = admin.int_field(days, 'days', 1, 3650)
    if not isinstance(items, list) or not items or any(i not in _CLEANUP_ITEMS for i in items):
        raise AdminError('Érvénytelen kérés.', 400, field='items')
    queries = _cleanup_queries(days, auth.utcnow())
    removed = {}
    with admin.action(ctx, 'system.cleanup', 'system', None, reason=reason, details={'days': days}) as act:
        for name in items:
            sql, params = queries[name]
            removed[name] = act.conn.execute('DELETE ' + sql, params).rowcount
        act.details['removed'] = removed
    return removed


def run_scheduled_maintenance(now=None):
    """Napi feladatok: a napi mentés (ha be van kapcsolva) és a régi chat napló törlése. Visszatér: a végzett feladatok."""
    import settings
    now = now or time.time()
    done = []
    if settings.get('backup_daily'):
        last = last_backup_time()
        if last is None or now - last >= 23 * 3600:
            make_backup()
            prune_backups(int(settings.get('backup_keep')))
            done.append('backup')
    if settings.get('chat_log_enabled'):
        cutoff = auth.format_ts(auth.utcnow() - _dt.timedelta(days=int(settings.get('chat_log_days'))))
        with auth.transaction() as conn:
            if conn.execute('DELETE FROM chat_log WHERE created_at < ?', (cutoff,)).rowcount:
                done.append('chat_log')
    return done
