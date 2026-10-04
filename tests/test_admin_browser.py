"""Az admin felület valódi böngészőben: minden menüpont betöltődik, a legfontosabb műveletek és az élő események működnek.

Valódi szervert indít (`admin_browser_server.py`, ideiglenes adatbázissal) és Playwrightot (node) futtat rajta
(`admin_browser_smoke.js`). Ha nincs node, Playwright vagy Chromium, a teszt kimarad. Útvonalak felülírása:
`PLAYWRIGHT_MODULE` (a playwright node-modul mappája), `PLAYWRIGHT_CHROMIUM` (a böngésző futtatható fájlja).
"""
import glob
import json
import os
import shutil
import socket
import subprocess
import sys
import time

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
HERE = os.path.dirname(os.path.abspath(__file__))


def _playwright_module():
    candidates = [os.environ.get('PLAYWRIGHT_MODULE'), '/opt/node-tools/node_modules/playwright']
    return next((c for c in candidates if c and os.path.isdir(c)), None)


def _chromium():
    explicit = os.environ.get('PLAYWRIGHT_CHROMIUM')
    if explicit and os.path.isfile(explicit):
        return explicit
    found = sorted(glob.glob('/opt/pw-browsers/chromium-*/chrome-linux/chrome'))
    return found[-1] if found else None


def _free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


pytestmark = pytest.mark.skipif(
    shutil.which('node') is None or _playwright_module() is None or _chromium() is None,
    reason='node, Playwright vagy Chromium nem elérhető')


@pytest.fixture(scope='module')
def running_server(tmp_path_factory):
    work = tmp_path_factory.mktemp('admin-browser')
    port = _free_port()
    env = dict(os.environ, ADMIN_SMOKE_DIR=str(work), ADMIN_SMOKE_PORT=str(port), PYTHONUNBUFFERED='1')
    for name in ('ADMIN_IP_ALLOWLIST', 'SMTP_HOST', 'SMTP_USER', 'SMTP_PASSWORD', 'SECRET_KEY'):
        env.pop(name, None)
    log = open(work / 'server.log', 'w', encoding='utf-8')
    process = subprocess.Popen([sys.executable, os.path.join(HERE, 'admin_browser_server.py')], env=env, cwd=ROOT,
                               stdout=log, stderr=subprocess.STDOUT)
    deadline = time.time() + 120
    ready = False
    while time.time() < deadline and process.poll() is None:
        log.flush()
        if 'READY' in (work / 'server.log').read_text(encoding='utf-8'):
            ready = True
            break
        time.sleep(0.5)
    if not ready:
        process.kill()
        pytest.fail('a szerver nem indult el:\n' + (work / 'server.log').read_text(encoding='utf-8')[-2000:])
    yield f'http://127.0.0.1:{port}'
    process.terminate()
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
    log.close()


def test_admin_client_in_a_real_browser(running_server):
    env = dict(os.environ, ADMIN_SMOKE_BASE=running_server, PLAYWRIGHT_MODULE=_playwright_module(),
               PLAYWRIGHT_CHROMIUM=_chromium())
    result = subprocess.run(['node', os.path.join(HERE, 'admin_browser_smoke.js')], env=env, capture_output=True, text=True,
                            timeout=400)
    assert result.returncode == 0, result.stderr
    line = next(l for l in result.stdout.splitlines() if l.startswith('RESULT '))
    data = json.loads(line[len('RESULT '):])
    assert not data['problems'], data['problems']
    checks = data['checks']
    assert checks['login'] == 200
    assert checks['nav_items'] == 15
    assert checks['muted_badge'] is True and checks['banned_badge'] is True
    assert checks['search_hash'].startswith('#users/')
    assert checks['subscribed'] and checks['live_overview'] and checks['watch_room'] and checks['live_room']
    assert checks['report_toast'] is True
    assert checks['mobile_nav'] is True
    assert checks['english_nav'].startswith('Overview')
