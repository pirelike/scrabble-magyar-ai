"""Admin panel: frissítés GitHubról (a legfrissebb vagy egy megadott ág), és a szerver újraindítása.

Szándékosan szűk, biztonságos művelet-készlet (a kód lecserélése a legkényesebb admin művelet):

* csak a rögzített `origin` távoli tárral dolgozik (az admin nem adhat meg tetszőleges URL-t);
* az ág neve szigorúan ellenőrzött, és szerepelnie kell a GitHubról éppen lekért ágak között;
* csak előretekerés (fast-forward) történik, tiszta munkafán (a helyi módosítást nem írja felül);
* a frissítés után a megváltozott Python fájlok szintaxisát ellenőrzi, hibánál visszaáll az előző állapotra;
* a git kimenetében szereplő hozzáférési adatok (`https://felhasznalo:token@...`) maszkolva vannak.

A futó folyamat a régi kódot használja, amíg újra nem indul: ezt a `restart_needed` jelzi (a folyamat induláskor
rögzíti a commitját). Az újraindítás a folyamat önmagára cserélése (`os.execv`), a tunnel előtte leáll.
"""
import os
import re
import subprocess
import sys
import threading
import time

import admin
import admin_live
import auth
from admin import AdminError

ROOT = os.path.dirname(os.path.abspath(__file__))
REMOTE = 'origin'
FETCH_TIMEOUT = 90
GIT_TIMEOUT = 30
PIP_TIMEOUT = 300
MAX_INCOMING = 30
MAX_FILES = 60
MAX_DIRTY = 20
RESTART_DELAY = 1.5
_BRANCH_RE = re.compile(r'^[A-Za-z0-9][A-Za-z0-9._/-]{0,99}$')
_CREDENTIALS_RE = re.compile(r'(?<=://)[^/@\s]+@')
_SEP = '\x1f'

_lock = threading.Lock()
_restart_scheduled = {'at': None}


class GitError(Exception):
    """Egy git parancs nem sikerült (a szöveg már maszkolt)."""


def _clean(text):
    """Hozzáférési adatok maszkolása és hossz-korlát a git kimenetében."""
    return _CREDENTIALS_RE.sub('***@', (text or '').strip())[:300]


def _git(*args, timeout=GIT_TIMEOUT, keep=False):
    """git parancs a program mappájában. Visszatér: a kimenet (levágva, `keep`: érintetlenül); hiba esetén GitError."""
    env = dict(os.environ, GIT_TERMINAL_PROMPT='0', GIT_ASKPASS='echo', LC_ALL='C')
    try:
        done = subprocess.run(['git', *args], cwd=ROOT, capture_output=True, text=True, timeout=timeout,
                              stdin=subprocess.DEVNULL, env=env)
    except FileNotFoundError:
        raise GitError('git nem található') from None
    except subprocess.TimeoutExpired:
        raise GitError('időtúllépés') from None
    if done.returncode != 0:
        raise GitError(_clean(done.stderr or done.stdout) or f'a git hibakódja: {done.returncode}')
    return done.stdout if keep else done.stdout.strip()


def _try(*args, **kwargs):
    try:
        return _git(*args, **kwargs)
    except GitError:
        return None


def _require_repo():
    if _try('rev-parse', '--is-inside-work-tree') != 'true':
        raise AdminError('A program nem git tárból fut: a frissítés nem érhető el.', 409)


def _commit_info(ref):
    raw = _try('log', '-1', f'--format=%H{_SEP}%h{_SEP}%s{_SEP}%cI{_SEP}%an', ref)
    if not raw:
        return None
    full, short, subject, date, author = (raw.split(_SEP) + [''] * 5)[:5]
    return {'hash': full, 'short': short, 'subject': subject[:200], 'date': date, 'author': author[:80]}


def _head():
    return _try('rev-parse', 'HEAD')


# A folyamat induláskori commitja: ehhez képest látszik, hogy a lemezen újabb kód van-e
_RUNNING_COMMIT = _head()


def remote_url():
    """A távoli tár címe hozzáférési adatok nélkül (vagy None)."""
    url = _try('config', '--get', f'remote.{REMOTE}.url')
    return _clean(url) if url else None


def _dirty_files():
    """A követett, módosított fájlok (a nem követettek — pl. az adatbázis — nem számítanak)."""
    # A sor elején álló szóköz jelentés (" M fájl"), ezért a kimenet nem vágható le
    out = _try('status', '--porcelain', '--untracked-files=no', keep=True) or ''
    return [line[3:].strip() for line in out.splitlines() if line.strip()]


def _current_branch():
    return _try('symbolic-ref', '--short', '-q', 'HEAD')      # None: leválasztott HEAD


def status():
    """A tár állapota hálózat nélkül: ág, commit, távoli cím, módosítások, újraindítás szükséges-e."""
    if _try('rev-parse', '--is-inside-work-tree') != 'true':
        return {'repo': False}
    head = _head()
    dirty = _dirty_files()
    return {
        'repo': True, 'branch': _current_branch(), 'commit': _commit_info('HEAD'), 'remote': remote_url(),
        'dirty': dirty[:MAX_DIRTY], 'dirty_count': len(dirty),
        'running_commit': _RUNNING_COMMIT[:7] if _RUNNING_COMMIT else None,
        'restart_needed': bool(_RUNNING_COMMIT and head and _RUNNING_COMMIT != head),
        'restart_scheduled': _restart_scheduled['at'] is not None,
    }


def _remote_branches():
    """A GitHubról lekért ágak: [{name, hash, short, subject, date}], a legfrissebb elöl."""
    out = _try('for-each-ref', '--sort=-committerdate',
               f'--format=%(refname:short){_SEP}%(objectname){_SEP}%(contents:subject){_SEP}%(committerdate:iso-strict)',
               f'refs/remotes/{REMOTE}') or ''
    branches = []
    for line in out.splitlines():
        name, full, subject, date = (line.split(_SEP) + [''] * 4)[:4]
        name = name[len(REMOTE) + 1:] if name.startswith(REMOTE + '/') else ''
        if name and name != 'HEAD':
            branches.append({'name': name, 'hash': full, 'short': full[:7], 'subject': subject[:200], 'date': date})
    return branches


def _default_branch(branches):
    head = _try('symbolic-ref', '--short', '-q', f'refs/remotes/{REMOTE}/HEAD')
    if head and head.startswith(REMOTE + '/'):
        return head[len(REMOTE) + 1:]
    names = {b['name'] for b in branches}
    return next((n for n in ('main', 'master') if n in names), branches[0]['name'] if branches else None)


def _plan(branch):
    """Mi változna, ha a `branch`-re frissítenénk: hány új / helyi-only commit, bejövő commitok, érintett fájlok."""
    target = f'{REMOTE}/{branch}'
    behind = int(_try('rev-list', '--count', f'HEAD..{target}') or 0)
    ahead = int(_try('rev-list', '--count', f'{target}..HEAD') or 0)
    incoming = []
    for line in (_try('log', f'--format=%h{_SEP}%s{_SEP}%cI{_SEP}%an', f'--max-count={MAX_INCOMING}', f'HEAD..{target}') or '').splitlines():
        short, subject, date, author = (line.split(_SEP) + [''] * 4)[:4]
        incoming.append({'short': short, 'subject': subject[:200], 'date': date, 'author': author[:80]})
    files = []
    for line in (_try('diff', '--name-status', 'HEAD', target) or '').splitlines():
        status_code, _tab, path = line.partition('\t')
        files.append({'status': status_code[:1], 'path': path[:200]})
    return {
        'branch': branch, 'behind': behind, 'ahead': ahead, 'incoming': incoming, 'files': files[:MAX_FILES],
        'files_total': len(files), 'requirements_changed': any(f['path'] == 'requirements.txt' for f in files),
        'switch': branch != _current_branch(), 'up_to_date': behind == 0 and ahead == 0 and branch == _current_branch(),
    }


def check(branch=None, fetch=True):
    """GitHubról lekéri az ágakat (`git fetch`), és megmondja, mi változna a kiválasztott ágra váltva / frissítve.
    Az ág megadása nélkül a jelenlegi ág (ha megvan a GitHubon), különben az alapértelmezett ág az alapértelmezés.
    `fetch=False`: nem megy a hálózatra (egy másik ág terve a már lekért adatokból)."""
    _require_repo()
    if fetch:
        with _lock:
            try:
                _git('fetch', '--prune', REMOTE, timeout=FETCH_TIMEOUT)
            except GitError as exc:
                raise AdminError('A GitHubról való lekérés nem sikerült.', 502, detail=str(exc)) from None
    branches = _remote_branches()
    if not branches:
        raise AdminError('A GitHubon nincs elérhető ág.', 409)
    names = {b['name'] for b in branches}
    current = _current_branch()
    if branch is None:
        branch = current if current in names else _default_branch(branches)
    elif branch not in names:
        raise AdminError('Ez az ág nincs a GitHubon.', 400, field='branch')
    return {**status(), 'branches': [{**b, 'current': b['name'] == current} for b in branches],
            'default_branch': _default_branch(branches), 'plan': _plan(branch)}


def _valid_branch(branch):
    if (not isinstance(branch, str) or not _BRANCH_RE.match(branch) or '..' in branch or '//' in branch
            or branch.endswith(('/', '.lock', '.'))):
        raise AdminError('Érvénytelen ág.', 400, field='branch')
    return branch


def _compile_check(paths):
    """A megadott Python fájlok szintaxisa; az első hibás fájl neve, vagy None."""
    for path in paths:
        full = os.path.join(ROOT, path)
        if not path.endswith('.py') or not os.path.isfile(full):
            continue
        try:
            with open(full, encoding='utf-8') as fh:
                compile(fh.read(), full, 'exec')
        except (SyntaxError, ValueError, UnicodeDecodeError):
            return path
    return None


def _restore(old_head, old_branch):
    """Visszaállás az előző állapotra (ág + commit). Visszatér: sikerült-e."""
    try:
        if old_branch:
            _git('checkout', old_branch, '--')
        else:
            _git('checkout', '--detach', old_head)
        _git('reset', '--hard', old_head)
        return True
    except GitError:
        return False


def apply(ctx, branch, reason, install=False, restart=False):
    """Frissítés a GitHub `branch` ágára (fast-forward). A naplósor a művelet ELŐTT íródik (indoklás kötelező).
    Hibánál (eltérő előzmény, szintaxishiba) a tár az előző állapotban marad."""
    _require_repo()
    branch = _valid_branch(branch)
    admin.normalize_reason(reason)          # indoklás nélkül a hálózati lekérés sem indul el
    if not _lock.acquire(blocking=False):
        raise AdminError('Már fut egy frissítés.', 409)
    try:
        try:
            _git('fetch', '--prune', REMOTE, timeout=FETCH_TIMEOUT)
        except GitError as exc:
            raise AdminError('A GitHubról való lekérés nem sikerült.', 502, detail=str(exc)) from None
        if branch not in {b['name'] for b in _remote_branches()}:
            raise AdminError('Ez az ág nincs a GitHubon.', 400, field='branch')
        dirty = _dirty_files()
        if dirty:
            raise AdminError('A programmappában helyi módosítások vannak: a frissítés nem írja felül őket.', 409,
                             files=dirty[:MAX_DIRTY])
        old_head, old_branch = _head(), _current_branch()
        plan = _plan(branch)
        if plan['up_to_date']:
            raise AdminError('Már a legfrissebb állapoton vagy.', 409)

        with admin.action(ctx, 'system.update', 'system', 'git', reason=reason,
                          details={'branch': branch, 'from_branch': old_branch, 'from': old_head,
                                   'incoming': plan['behind'], 'install': bool(install), 'restart': bool(restart)}):
            pass     # a naplósor előbb íródik, mint a kód cseréje

        target = f'{REMOTE}/{branch}'
        try:
            if plan['switch']:
                if _try('rev-parse', '--verify', '--quiet', f'refs/heads/{branch}'):
                    _git('checkout', branch, '--')
                else:
                    _git('checkout', '-b', branch, '--track', target)
            _git('merge', '--ff-only', target)
        except GitError as exc:
            _restore(old_head, old_branch)
            raise AdminError('A frissítés nem végezhető el: a helyi ág eltér a GitHubon lévőtől.', 409,
                             detail=str(exc)) from None

        new_head = _head()
        changed = (_try('diff', '--name-only', old_head, new_head) or '').splitlines()
        broken = _compile_check(changed)
        if broken:
            restored = _restore(old_head, old_branch)
            _record(ctx, 'system.update_failed', {'branch': branch, 'file': broken, 'restored': restored})
            raise AdminError('Az új kód szintaktikai hibát tartalmaz: a frissítés visszavonva.', 409,
                             file=broken, restored=restored)

        installed = None
        if install and 'requirements.txt' in changed:
            installed = _pip_install()
        _record(ctx, 'system.update_done', {'branch': branch, 'from': old_head, 'to': new_head,
                                            'files': len(changed), 'installed': installed})
    finally:
        _lock.release()
    scheduled = False
    if restart:
        scheduled = schedule_restart()
    return {**status(), 'from': old_head[:7] if old_head else None, 'changed_files': len(changed),
            'installed': installed, 'restarting': scheduled}


def _pip_install():
    """A `requirements.txt` telepítése a futó Pythonnal. Visszatér: sikerült-e."""
    try:
        done = subprocess.run([sys.executable, '-m', 'pip', 'install', '--disable-pip-version-check', '--quiet', '-r',
                               'requirements.txt'], cwd=ROOT, capture_output=True, text=True, timeout=PIP_TIMEOUT,
                              stdin=subprocess.DEVNULL)
    except (OSError, subprocess.SubprocessError):
        return False
    return done.returncode == 0


def _record(ctx, name, details):
    with auth.transaction() as conn:
        admin.record(conn, ctx, name, 'system', 'git', details)


# ===== Újraindítás =====

def _restart_process():
    """A folyamat cseréje önmagára (azonos parancssorral). Az `os.execv` nem tér vissza; a tesztek ezt cserélik."""
    import tunnel
    tunnel.stop_tunnel()
    os.execv(sys.executable, [sys.executable, *sys.argv])


def schedule_restart():
    """Az újraindítás ütemezése röviddel később, hogy a HTTP válasz még kimehessen. Visszatér: ütemezve lett-e."""
    if _restart_scheduled['at'] is not None:
        return False
    _restart_scheduled['at'] = time.time()

    def run():
        admin_live.srv().socketio.sleep(RESTART_DELAY)
        try:
            _restart_process()
        finally:
            _restart_scheduled['at'] = None      # csak akkor érünk ide, ha az újraindítás nem sikerült

    admin_live.srv().socketio.start_background_task(run)
    return True


def restart(ctx, reason):
    """Újraindítás (kötelező indoklással). A naplósor az újraindítás ELŐTT íródik."""
    if _restart_scheduled['at'] is not None:
        raise AdminError('Az újraindítás már folyamatban van.', 409)
    with admin.action(ctx, 'system.restart', 'system', 'server', reason=reason,
                      details={'commit': _RUNNING_COMMIT, 'head': _head()}):
        pass
    schedule_restart()
    return {'restarting': True}
