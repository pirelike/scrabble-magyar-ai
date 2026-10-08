"""HTTP API differenciális próba: ugyanazok a kérések a Python (PY_PORT) és a Rust (RS_PORT) szerveren."""
import json, os, re, sys, time, requests
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import harness
from harness import leaf_diff

TS = re.compile(r'\d{4}-\d\d-\d\d[ T]\d\d:\d\d:\d\d(\.\d+)?(Z|[+-]\d\d:?\d\d)?')
VOLATILE = {'created_at', 'updated_at', 'ts', 'last_seen', 'last_login_at', 'expires_at', 'token', 'dev_code', 'generated_at',
            'uptime', 'rss', 'cpu_percent', 'greenlets', 'threads', 'db_size', 'size', 'since', 'stamp', 'started_at', 'fired_at',
            'idle_remaining', 'sudo_remaining', 'deadline', 'now', 'time', 'duration', 'elapsed', 'last_backup', 'mtime', 'at'}

def hnorm(value, key=None):
    if isinstance(value, dict):
        return {k: hnorm(v, k) for k, v in sorted(value.items())}
    if isinstance(value, list):
        return [hnorm(v, key) for v in value]
    if key in VOLATILE and value is not None and not isinstance(value, bool):
        return '<%s>' % key
    if isinstance(value, str):
        v = TS.sub('<ts>', value)
        v = re.sub(r'[A-Za-z0-9_-]{40,}', '<token>', v)
        return v
    if isinstance(value, float):
        return round(value, 2)
    return value

class Runner:
    def __init__(self, base):
        self.base = base
        self.sessions = {}
        self.out = []

    def session(self, name):
        return self.sessions.setdefault(name, requests.Session())

    def call(self, label, method, path, who='anon', json_body=None, admin=False, raw=False, **kw):
        s = self.session(who)
        headers = kw.pop('headers', {})
        if admin:
            headers.update({'X-Admin-Request': '1', 'Origin': self.base})
        try:
            r = s.request(method, self.base + path, json=json_body, headers=headers, timeout=60, allow_redirects=False, **kw)
        except Exception as e:
            self.out.append({'label': label, 'error': str(e)}); return None
        ctype = r.headers.get('content-type', '').split(';')[0]
        entry = {'label': label, 'status': r.status_code, 'type': ctype}
        if 'json' in ctype:
            try: entry['body'] = hnorm(r.json())
            except Exception: entry['body'] = 'érvénytelen JSON'
        elif raw or ctype.startswith('text/') or 'javascript' in ctype:
            entry['len_bucket'] = len(r.text) // 2000
            entry['head'] = hnorm(r.text[:60])
        else:
            entry['len_bucket'] = len(r.content) // 2000
        if r.headers.get('set-cookie'): entry['cookie_attrs'] = sorted(a.strip().split('=')[0].lower() for a in r.headers['set-cookie'].split(';')[1:])
        for h in ('cache-control', 'content-disposition', 'x-frame-options', 'service-worker-allowed', 'content-security-policy'):
            if h in r.headers: entry['h_' + h] = r.headers[h][:80]
        self.out.append(entry)
        return r


def run_all(scenario, name, limit=80):
    results = {}
    for tag, port in (('py', harness.PY_PORT), ('rs', harness.RS_PORT)):
        rn = Runner(f'http://localhost:{port}')
        scenario(rn)
        results[tag] = rn.out
        json.dump(rn.out, open(f'{harness.OUT}{name}.{tag}.json', 'w'), ensure_ascii=False, indent=1)
    lines = []
    for a, b in zip(results['py'], results['rs']):
        d = leaf_diff(a, b)
        if d:
            lines.append('## ' + a['label'])
            lines += ['   ' + x for x in d[:8]]
    if len(results['py']) != len(results['rs']):
        lines.append(f'lépésszám eltér: {len(results["py"])} vs {len(results["rs"])}')
    print(f'== {name}: {len(results["py"])} kérés, eltérés: {len(lines)} sor')
    print('\n'.join(l[:260] for l in lines[:limit]))
