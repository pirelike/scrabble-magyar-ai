"""Admin panel: frissítés GitHubról (egy ág előretekerése) és az újraindítás.

Valódi git-tárakkal dolgozik: egy helyi „GitHub” (bare `origin`), a program mappája (klón) és egy harmadik klón, ahonnan
új commitokat „pusholunk”. Hálózat nincs. Az újraindítás (`os.execv`) és a `pip` hamisítva van.
"""
import json
import subprocess
from types import SimpleNamespace

import pytest

import admin
import admin_update
import auth
from admin import AdminError

CTX = admin.AdminContext(1, '127.0.0.1', 'teszt')
REASON = 'Frissítés a tesztből'


def git(cwd, *args):
    done = subprocess.run(['git', *args], cwd=str(cwd), capture_output=True, text=True, check=True)
    return done.stdout.strip()


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding='utf-8')


@pytest.fixture
def repos(tmp_path, monkeypatch):
    """origin (bare) · seed (innen megy fel új tartalom) · app (a program mappája)."""
    for key, value in (('GIT_AUTHOR_NAME', 'Teszt'), ('GIT_AUTHOR_EMAIL', 'teszt@example.com'),
                       ('GIT_COMMITTER_NAME', 'Teszt'), ('GIT_COMMITTER_EMAIL', 'teszt@example.com')):
        monkeypatch.setenv(key, value)
    origin, seed, app = tmp_path / 'origin.git', tmp_path / 'seed', tmp_path / 'app'
    git(tmp_path, 'init', '--bare', str(origin))
    git(origin, 'symbolic-ref', 'HEAD', 'refs/heads/main')
    git(tmp_path, 'init', str(seed))
    git(seed, 'symbolic-ref', 'HEAD', 'refs/heads/main')
    write(seed / 'server.py', 'VERSION = 1\n')
    write(seed / 'requirements.txt', 'flask\n')
    git(seed, 'add', '-A')
    git(seed, 'commit', '-m', 'Első verzió')
    git(seed, 'remote', 'add', 'origin', str(origin))
    git(seed, 'push', 'origin', 'main')
    git(tmp_path, 'clone', str(origin), str(app))

    monkeypatch.setattr(admin_update, 'ROOT', str(app))
    monkeypatch.setattr(admin_update, '_RUNNING_COMMIT', git(app, 'rev-parse', 'HEAD'))

    def push(filename, text, message, branch='main'):
        """Új commit a „GitHubra” (a seed klónból)."""
        if git(seed, 'rev-parse', '--abbrev-ref', 'HEAD') != branch:
            if subprocess.run(['git', 'rev-parse', '--verify', '--quiet', f'refs/heads/{branch}'], cwd=str(seed)).returncode:
                git(seed, 'checkout', '-b', branch)
            else:
                git(seed, 'checkout', branch)
        write(seed / filename, text)
        git(seed, 'add', '-A')
        git(seed, 'commit', '-m', message)
        git(seed, 'push', 'origin', branch)
        return git(seed, 'rev-parse', 'HEAD')

    return SimpleNamespace(origin=origin, seed=seed, app=app, push=push)


def audit_actions():
    with auth.transaction() as conn:
        return [(r['action'], json.loads(r['details_json'] or '{}'))
                for r in conn.execute("SELECT action, details_json FROM admin_audit WHERE action LIKE 'system.%' ORDER BY id")]


# ---------------------------------------------------------------- állapot

class TestStatus:
    def test_reports_branch_commit_and_remote(self, repos):
        status = admin_update.status()
        assert status['repo'] is True and status['branch'] == 'main'
        assert status['commit']['subject'] == 'Első verzió' and len(status['commit']['short']) >= 7
        assert status['remote'] == str(repos.origin)
        assert status['dirty'] == [] and status['restart_needed'] is False

    def test_not_a_git_repository(self, tmp_path, monkeypatch):
        monkeypatch.setattr(admin_update, 'ROOT', str(tmp_path))
        assert admin_update.status() == {'repo': False}
        with pytest.raises(AdminError) as error:
            admin_update.check()
        assert error.value.status == 409

    def test_local_changes_are_listed_but_untracked_files_are_not(self, repos):
        (repos.app / 'server.py').write_text('VERSION = 99\n', encoding='utf-8')
        (repos.app / 'scrabble.db').write_text('x', encoding='utf-8')
        status = admin_update.status()
        assert status['dirty'] == ['server.py'] and status['dirty_count'] == 1

    def test_credentials_in_the_remote_url_are_masked(self, repos):
        git(repos.app, 'remote', 'set-url', 'origin', 'https://huba:ghp_secrettoken@github.com/pirelike/scrabble.git')
        remote = admin_update.status()['remote']
        assert 'ghp_secrettoken' not in remote and 'huba' not in remote
        assert remote == 'https://***@github.com/pirelike/scrabble.git'

    def test_restart_needed_when_the_disk_is_newer_than_the_process(self, repos, monkeypatch):
        monkeypatch.setattr(admin_update, '_RUNNING_COMMIT', 'a' * 40)
        status = admin_update.status()
        assert status['restart_needed'] is True and status['running_commit'] == 'aaaaaaa'


# ---------------------------------------------------------------- ellenőrzés (git fetch)

class TestCheck:
    def test_lists_the_branches_and_what_would_change(self, repos):
        repos.push('server.py', 'VERSION = 2\n', 'Második verzió')
        repos.push('feature.py', 'X = 1\n', 'Kísérlet', branch='feature')
        before = git(repos.app, 'rev-parse', 'HEAD')
        result = admin_update.check()

        assert {b['name'] for b in result['branches']} == {'main', 'feature'}
        assert result['default_branch'] == 'main'
        plan = result['plan']
        assert plan['branch'] == 'main' and plan['behind'] == 1 and plan['ahead'] == 0 and plan['up_to_date'] is False
        assert [c['subject'] for c in plan['incoming']] == ['Második verzió']
        assert plan['files'] == [{'status': 'M', 'path': 'server.py'}] and plan['requirements_changed'] is False
        assert git(repos.app, 'rev-parse', 'HEAD') == before         # a munkafa nem változott

    def test_without_fetch_it_works_from_the_already_downloaded_data(self, repos):
        repos.push('feature.py', 'X = 1\n', 'Kísérlet', branch='feature')
        admin_update.check()
        git(repos.app, 'remote', 'set-url', 'origin', str(repos.origin) + '-missing')     # a hálózat nem elérhető
        assert admin_update.check('feature', fetch=False)['plan']['branch'] == 'feature'
        with pytest.raises(AdminError):
            admin_update.check('feature')

    def test_another_branch_is_a_switch(self, repos):
        repos.push('feature.py', 'X = 1\n', 'Kísérlet', branch='feature')
        plan = admin_update.check('feature')['plan']
        assert plan['switch'] is True and plan['branch'] == 'feature' and plan['files'][0]['path'] == 'feature.py'

    def test_unknown_branch_is_refused(self, repos):
        with pytest.raises(AdminError) as error:
            admin_update.check('nincs-ilyen')
        assert error.value.status == 400 and error.value.extra['field'] == 'branch'

    def test_requirements_change_is_flagged(self, repos):
        repos.push('requirements.txt', 'flask\ngevent\n', 'Új függőség')
        assert admin_update.check()['plan']['requirements_changed'] is True

    def test_unreachable_remote_is_a_bad_gateway(self, repos):
        git(repos.app, 'remote', 'set-url', 'origin', str(repos.origin) + '-missing')
        with pytest.raises(AdminError) as error:
            admin_update.check()
        assert error.value.status == 502 and error.value.extra['detail']


# ---------------------------------------------------------------- frissítés

class TestApply:
    def test_fast_forwards_the_current_branch(self, repos):
        new = repos.push('server.py', 'VERSION = 2\n', 'Második verzió')
        result = admin_update.apply(CTX, 'main', REASON)

        assert git(repos.app, 'rev-parse', 'HEAD') == new
        assert (repos.app / 'server.py').read_text(encoding='utf-8') == 'VERSION = 2\n'
        assert result['changed_files'] == 1 and result['restart_needed'] is True and result['restarting'] is False
        actions = audit_actions()
        assert [a for a, _d in actions] == ['system.update', 'system.update_done']
        assert actions[0][1]['reason'] == REASON and actions[0][1]['incoming'] == 1
        assert actions[1][1]['to'] == new and actions[1][1]['from'] == result['from'] + actions[1][1]['from'][7:]

    def test_switches_to_a_specific_branch(self, repos):
        new = repos.push('feature.py', 'X = 1\n', 'Kísérlet', branch='feature')
        admin_update.apply(CTX, 'feature', REASON)
        assert git(repos.app, 'rev-parse', '--abbrev-ref', 'HEAD') == 'feature'
        assert git(repos.app, 'rev-parse', 'HEAD') == new and (repos.app / 'feature.py').exists()

    def test_switching_back_keeps_both_branches_in_sync(self, repos):
        repos.push('feature.py', 'X = 1\n', 'Kísérlet', branch='feature')
        admin_update.apply(CTX, 'feature', REASON)
        main_new = repos.push('server.py', 'VERSION = 2\n', 'Második verzió', branch='main')
        admin_update.apply(CTX, 'main', REASON)
        assert git(repos.app, 'rev-parse', '--abbrev-ref', 'HEAD') == 'main'
        assert git(repos.app, 'rev-parse', 'HEAD') == main_new and not (repos.app / 'feature.py').exists()

    def test_already_up_to_date(self, repos):
        with pytest.raises(AdminError) as error:
            admin_update.apply(CTX, 'main', REASON)
        assert error.value.status == 409 and audit_actions() == []

    def test_a_reason_is_required_before_anything_runs(self, repos):
        repos.push('server.py', 'VERSION = 2\n', 'Második verzió')
        before = git(repos.app, 'rev-parse', 'HEAD')
        with pytest.raises(AdminError) as error:
            admin_update.apply(CTX, 'main', '')
        assert error.value.extra['field'] == 'reason'
        assert git(repos.app, 'rev-parse', 'HEAD') == before and audit_actions() == []

    def test_local_modifications_block_the_update(self, repos):
        repos.push('server.py', 'VERSION = 2\n', 'Második verzió')
        (repos.app / 'server.py').write_text('VERSION = 99  # helyi\n', encoding='utf-8')
        with pytest.raises(AdminError) as error:
            admin_update.apply(CTX, 'main', REASON)
        assert error.value.status == 409 and error.value.extra['files'] == ['server.py']
        assert (repos.app / 'server.py').read_text(encoding='utf-8') == 'VERSION = 99  # helyi\n'
        assert audit_actions() == []

    def test_diverged_history_is_not_overwritten(self, repos):
        repos.push('server.py', 'VERSION = 2\n', 'Távoli commit')
        write(repos.app / 'local.py', 'LOCAL = 1\n')
        git(repos.app, 'add', '-A')
        git(repos.app, 'commit', '-m', 'Helyi commit')
        local = git(repos.app, 'rev-parse', 'HEAD')
        with pytest.raises(AdminError) as error:
            admin_update.apply(CTX, 'main', REASON)
        assert error.value.status == 409
        assert git(repos.app, 'rev-parse', 'HEAD') == local and (repos.app / 'local.py').exists()

    def test_a_syntax_error_in_the_new_code_rolls_the_update_back(self, repos):
        before = git(repos.app, 'rev-parse', 'HEAD')
        repos.push('broken.py', 'def (:\n', 'Hibás kód')
        with pytest.raises(AdminError) as error:
            admin_update.apply(CTX, 'main', REASON)
        assert error.value.status == 409 and error.value.extra == {'file': 'broken.py', 'restored': True}
        assert git(repos.app, 'rev-parse', 'HEAD') == before and not (repos.app / 'broken.py').exists()
        assert [a for a, _d in audit_actions()] == ['system.update', 'system.update_failed']

    def test_a_failed_switch_returns_to_the_previous_branch(self, repos):
        repos.push('feature.py', 'def (:\n', 'Hibás ág', branch='feature')
        before = git(repos.app, 'rev-parse', 'HEAD')
        with pytest.raises(AdminError):
            admin_update.apply(CTX, 'feature', REASON)
        assert git(repos.app, 'rev-parse', '--abbrev-ref', 'HEAD') == 'main'
        assert git(repos.app, 'rev-parse', 'HEAD') == before and not (repos.app / 'feature.py').exists()

    @pytest.mark.parametrize('branch', [None, '', 123, '--upload-pack=x', '-x', '../main', 'a..b', 'main;ls', 'main branch',
                                        'x' * 101, 'feature/', 'a.lock', 'főág'])
    def test_malformed_branch_names_are_refused(self, repos, branch):
        with pytest.raises(AdminError) as error:
            admin_update.apply(CTX, branch, REASON)
        assert error.value.status == 400 and error.value.extra['field'] == 'branch'

    def test_a_branch_that_is_not_on_github_is_refused(self, repos):
        git(repos.app, 'branch', 'csak-helyi')
        with pytest.raises(AdminError) as error:
            admin_update.apply(CTX, 'csak-helyi', REASON)
        assert error.value.status == 400

    def test_dependencies_are_installed_only_when_asked_and_changed(self, repos, monkeypatch):
        installs = []
        monkeypatch.setattr(admin_update, '_pip_install', lambda: installs.append(1) or True)
        repos.push('server.py', 'VERSION = 2\n', 'Második verzió')
        assert admin_update.apply(CTX, 'main', REASON, install=True)['installed'] is None      # nem változott
        repos.push('requirements.txt', 'flask\ngevent\n', 'Új függőség')
        assert admin_update.apply(CTX, 'main', REASON, install=False)['installed'] is None     # nem kérték
        repos.push('requirements.txt', 'flask\ngevent\npywebpush\n', 'Még egy')
        assert admin_update.apply(CTX, 'main', REASON, install=True)['installed'] is True
        assert installs == [1]

    def test_a_concurrent_update_is_refused(self, repos):
        repos.push('server.py', 'VERSION = 2\n', 'Második verzió')
        assert admin_update._lock.acquire(blocking=False)
        try:
            with pytest.raises(AdminError) as error:
                admin_update.apply(CTX, 'main', REASON)
            assert error.value.status == 409
        finally:
            admin_update._lock.release()


# ---------------------------------------------------------------- újraindítás

@pytest.fixture
def fake_server(monkeypatch):
    """A háttérfeladatot nem futtatja le azonnal (a teszt hívja), az `os.execv` helyett számol."""
    tasks, restarts = [], []
    socketio = SimpleNamespace(start_background_task=tasks.append, sleep=lambda seconds: None)
    monkeypatch.setattr(admin_update.admin_live, 'srv', lambda: SimpleNamespace(socketio=socketio))
    monkeypatch.setattr(admin_update, '_restart_process', lambda: restarts.append(1))
    monkeypatch.setitem(admin_update._restart_scheduled, 'at', None)
    return SimpleNamespace(tasks=tasks, restarts=restarts)


class TestRestart:
    def test_restart_is_logged_first_and_runs_in_the_background(self, fake_server):
        assert admin_update.restart(CTX, REASON) == {'restarting': True}
        assert [a for a, _d in audit_actions()] == ['system.restart'] and fake_server.restarts == []
        fake_server.tasks[0]()
        assert fake_server.restarts == [1]

    def test_a_second_request_is_refused_while_it_is_pending(self, fake_server):
        admin_update.restart(CTX, REASON)
        with pytest.raises(AdminError) as error:
            admin_update.restart(CTX, REASON)
        assert error.value.status == 409 and len(fake_server.tasks) == 1

    def test_reason_is_required(self, fake_server):
        with pytest.raises(AdminError):
            admin_update.restart(CTX, 'x')
        assert fake_server.tasks == [] and audit_actions() == []

    def test_update_can_restart_afterwards(self, repos, fake_server):
        repos.push('server.py', 'VERSION = 2\n', 'Második verzió')
        assert admin_update.apply(CTX, 'main', REASON, restart=True)['restarting'] is True
        assert len(fake_server.tasks) == 1


# ---------------------------------------------------------------- HTTP

class TestEndpoints:
    def test_status_and_check(self, api, repos):
        data = api.get('/api/admin/system/update').get_json()['update']
        assert data['repo'] is True and data['branch'] == 'main'
        repos.push('server.py', 'VERSION = 2\n', 'Második verzió')
        checked = api.post('/api/admin/system/update/check', {}).get_json()['update']
        assert checked['plan']['behind'] == 1 and checked['branches'][0]['name'] == 'main'
        wrong = api.post('/api/admin/system/update/check', {'branch': 'nincs'})
        assert wrong.status_code == 400

    def test_apply_needs_sudo_and_a_reason(self, api, repos):
        new = repos.push('server.py', 'VERSION = 2\n', 'Második verzió')
        before = git(repos.app, 'rev-parse', 'HEAD')
        denied = api.post('/api/admin/system/update/apply', {'branch': 'main', 'reason': REASON})
        assert denied.status_code == 401 and denied.get_json()['sudo_required'] is True
        api.sudo()
        assert api.post('/api/admin/system/update/apply', {'branch': 'main'}).status_code == 400
        assert git(repos.app, 'rev-parse', 'HEAD') == before
        done = api.post('/api/admin/system/update/apply', {'branch': 'main', 'reason': REASON})
        assert done.status_code == 200 and done.get_json()['update']['restart_needed'] is True
        assert git(repos.app, 'rev-parse', 'HEAD') == new

    def test_restart_needs_sudo(self, api, fake_server):
        assert api.post('/api/admin/system/restart', {'reason': REASON}).status_code == 401
        api.sudo()
        assert api.post('/api/admin/system/restart', {'reason': REASON}).get_json() == {'success': True, 'restarting': True}
        assert len(fake_server.tasks) == 1
