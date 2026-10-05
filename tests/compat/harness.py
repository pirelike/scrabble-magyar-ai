"""Differenciális próba: ugyanaz a forgatókönyv a Python és a Rust szerveren, a válaszok szerkezetének összevetése."""
import json, os, re, sys, time, threading, difflib, random
import requests, socketio

# A két szerver portja és a kimeneti mappa (lásd tests/compat/run.sh)
PY_PORT = int(os.environ.get('COMPAT_PY_PORT', 5056))
RS_PORT = int(os.environ.get('COMPAT_RS_PORT', 5055))
OUT = os.environ.get('COMPAT_OUT', os.path.join(os.path.dirname(os.path.abspath(__file__)), 'out')) + '/'
os.makedirs(OUT, exist_ok=True)

SETTLE = 0.35
NOISY = {'created_at', 'updated_at', 'ts', 'timestamp', 'deadline', 'turn_deadline', 'time_left', 'remaining', 'elapsed',
         'since', 'until', 'last_activity', 'turn_started_at', 'started_at', 'server_time', 'expires_at', 'id', 'room_id',
         'game_id', 'token', 'reconnect_token', 'join_code', 'code', 'owner_token', 'sid', 'player_id', 'user_id', 'owner_id',
         'current_player', 'turn_timer_expires_at', 'challenger', 'placer', 'player_ids'}
KEEP_INT = {'hand_count', 'tiles_remaining', 'turn_number', 'max_players', 'players', 'spectators', 'bots', 'turn_time_limit',
            'hint_limit', 'hints_left', 'count', 'rows', 'tiles', 'spectator_count', 'turn_hours', 'bag'}
TILE_RE = re.compile(r'^(?:[A-ZÁÉÍÓÖŐÚÜŰ]|CS|DZS|DZ|GY|LY|NY|SZ|TY|ZS|\?)$')
TS_RE = re.compile(r'^\d{4}-\d\d-\d\d[ T]\d\d:\d\d:\d\d')
WORD_RE = re.compile(r'[A-ZÁÉÍÓÖŐÚÜŰ]{2,}')
NAME_RE = re.compile(r'Jatekos\d')
SIDLIKE = re.compile(r'^(?=.*\d)(?=.*[A-Za-z])[A-Za-z0-9_-]{14,24}$')

def mask_text(value):
    value = NAME_RE.sub('<P>', value)
    value = WORD_RE.sub('<W>', value)
    value = re.sub(r'\d+', '<n>', value)
    return value

def norm(value, key=None):
    if key == 'board' and isinstance(value, list):
        return {'tiles': sum(1 for row in value for cell in row if cell), 'rows': len(value)}
    if key == 'hand' and isinstance(value, list):
        return ['<tile>'] * len(value)
    if isinstance(value, dict):
        if value and all(isinstance(k, str) and SIDLIKE.match(k) for k in value):
            return sorted((norm(v, key) for v in value.values()), key=json.dumps)
        return {k: norm(v, k) for k, v in sorted(value.items())}
    if isinstance(value, list):
        items = [norm(v, key) for v in value]
        if key in ('players', 'winners', 'voters', 'scores') and all(isinstance(i, dict) for i in items):
            items.sort(key=lambda d: json.dumps(d, sort_keys=True))
        return items
    if key in NOISY and not isinstance(value, bool) and value is not None:
        return '<%s>' % key
    if isinstance(value, bool) or value is None:
        return value
    if isinstance(value, (int, float)):
        return value if (isinstance(value, int) and key in KEEP_INT) else '<num>'
    if isinstance(value, str):
        if TS_RE.match(value): return '<ts>'
        if SIDLIKE.match(value): return '<sid>'
        if re.fullmatch(r'[0-9a-f-]{8,}', value) or re.fullmatch(r'\d{6}', value): return '<id>'
        if key in ('letter', 'tile') or TILE_RE.match(value): return '<tile>'
        if key in ('name', 'player', 'owner', 'current_player_name', 'player_name', 'winner', 'display_name', 'challenger_name', 'placer_name', 'by'):
            return mask_text(value) if NAME_RE.search(value) else value
        return mask_text(value)
    return str(type(value))


class Client:
    def __init__(self, base, label, user=None, guest_name=None):
        self.base, self.label = base, label
        self.events = []
        self.lock = threading.Lock()
        self.http = requests.Session()
        self.user = user
        self.sio = socketio.Client(reconnection=False)
        self.sio.on('*', self._on)
        self.sio.connect(base, transports=['websocket'])
        self.state = None            # utolsó game_state
        self.room_code = None
        self.room_id = None
        self.reconnect_token = None
        if user:
            r = self.http.post(f'{base}/api/auth/login', json={'email': f'{user}@example.com', 'password': 'secret12'})
            assert r.status_code == 200, r.text
            self.name = r.json()['user']['display_name']
            tok = self.http.get(f'{base}/api/auth/socket-token').json()['token']
            self.emit('set_name', {'name': self.name, 'is_guest': False, 'auth_token': tok})
        elif guest_name:
            self.name = guest_name
            self.emit('set_name', {'name': guest_name, 'is_guest': True})

    def _on(self, event, data=None):
        with self.lock:
            self.events.append((event, data))
        if event == 'game_state' and isinstance(data, dict):
            self.state = data
        if event == 'room_code':
            self.room_code = data.get('code') if isinstance(data, dict) else data
        if event == 'room_joined' and isinstance(data, dict):
            self.room_id = data.get('room_id') or data.get('id') or self.room_id
            self.reconnect_token = data.get('reconnect_token') or self.reconnect_token
            if data.get('code') or data.get('join_code'):
                self.room_code = data.get('code') or data.get('join_code')

    NOARG = {'get_rooms', 'start_game', 'save_game', 'pass_turn', 'accept_words', 'reject_words', 'withdraw_words',
             'request_hint', 'start_daily', 'retry_daily', 'reveal_daily', 'resign_game', 'logout', 'leave_room', 'leave_spectate'}

    def emit(self, event, data=None):
        if event in self.NOARG and not data:
            self.sio.emit(event)
        else:
            self.sio.emit(event, data if data is not None else {})

    def take(self):
        with self.lock:
            ev, self.events = self.events, []
        return ev

    def close(self):
        try: self.sio.disconnect()
        except Exception: pass


class Scenario:
    def __init__(self, base):
        self.base = base
        self.transcript = []
        self.clients = {}

    def client(self, label, **kw):
        c = Client(self.base, label, **kw)
        self.clients[label] = c
        return c

    def step(self, title, *actions, wait=SETTLE, coarse=False):
        """actions: (client, event, data) vagy hívható."""
        for a in actions:
            if callable(a): a()
            else:
                c, ev, data = a
                c.emit(ev, data)
        time.sleep(wait)
        self.record(title, coarse=coarse)

    def record(self, title, coarse=False):
        entry = {'step': title, 'clients': {}}
        for label, c in self.clients.items():
            ev = c.take()
            if coarse:
                seen = {}
                for e, d in ev:
                    seen.setdefault(e, d)
                ev = list(seen.items())
                ev.sort(key=lambda x: x[0])
            if ev:
                entry['clients'][label] = [[e, norm(d)] for e, d in ev]
        self.transcript.append(entry)

    def wait_until(self, cond, timeout=25, title=None, coarse=True):
        end = time.time() + timeout
        while time.time() < end:
            try:
                if cond(): break
            except Exception:
                pass
            time.sleep(0.15)
        time.sleep(0.3)
        self.record(title or 'várakozás', coarse=coarse)

    def relabel(self, mapping):
        """mapping: {kliens: új címke}; a későbbi lépések az új címkével kerülnek a naplóba."""
        self.clients = {mapping.get(c, label): c for label, c in self.clients.items() for c in [c]} if False else {
            mapping[c] if c in mapping else label: c for label, c in self.clients.items()}

    def note(self, title, value):
        self.transcript.append({'step': title, 'note': norm(value)})

    def close(self):
        for c in self.clients.values(): c.close()


def leaf_diff(a, b, path=''):
    out = []
    if isinstance(a, dict) and isinstance(b, dict):
        for k in sorted(set(a) | set(b)):
            if k not in a: out.append(f'{path}.{k}: csak rust = {json.dumps(b[k], ensure_ascii=False)[:90]}')
            elif k not in b: out.append(f'{path}.{k}: csak python = {json.dumps(a[k], ensure_ascii=False)[:90]}')
            else: out += leaf_diff(a[k], b[k], f'{path}.{k}')
    elif isinstance(a, list) and isinstance(b, list) and len(a) == len(b):
        for i, (x, y) in enumerate(zip(a, b)): out += leaf_diff(x, y, f'{path}[{i}]')
    elif a != b:
        out.append(f'{path}: py={json.dumps(a, ensure_ascii=False)[:90]} rs={json.dumps(b, ensure_ascii=False)[:90]}')
    return out


def compare(py, rs, limit=60):
    lines = []
    for ep, er in zip(py, rs):
        title = ep['step']
        if ep.get('note') != er.get('note'):
            lines.append(f'## {title}: note py={ep.get("note")} rs={er.get("note")}')
        labels = sorted(set(ep.get('clients', {})) | set(er.get('clients', {})))
        step_lines = []
        for label in labels:
            a = ep.get('clients', {}).get(label, [])
            b = er.get('clients', {}).get(label, [])
            ea, eb = [x[0] for x in a], [x[0] for x in b]
            if ea != eb:
                step_lines.append(f'   [{label}] eseménysor eltér: py={ea} rs={eb}')
            # azonos nevű események hasonlítása sorrendben (név szerint párosítva)
            used = set()
            for i, (name, payload) in enumerate(a):
                j = next((j for j, (n2, p2) in enumerate(b) if n2 == name and j not in used), None)
                if j is None: continue
                used.add(j)
                for d in leaf_diff(payload, b[j][1])[:6]:
                    step_lines.append(f'   [{label}] {name}{d}')
        if step_lines:
            lines.append('## ' + title); lines += step_lines
    return lines[:limit] if limit else lines


def render(transcript):
    return [json.dumps(t, ensure_ascii=False, sort_keys=True) for t in transcript]


def run_both(scenario_fn, name, limit=80):
    results = {}
    base = OUT
    for tag, port in (('py', PY_PORT), ('rs', RS_PORT)):
        random.seed(1234)
        sc = Scenario(f'http://localhost:{port}')
        try:
            scenario_fn(sc)
        finally:
            sc.close()
        results[tag] = sc.transcript
        json.dump(sc.transcript, open(base + f'{name}.{tag}.json', 'w'), ensure_ascii=False, indent=1)
    lines = compare(results['py'], results['rs'], limit)
    print(f'== {name}: {len(results["py"])} lépés, eltérés: {len(lines)} sor')
    print('\n'.join(l[:300] for l in lines))
    return lines


# ---- játékmenet-segédek ----
DIGRAPHS = {'CS', 'GY', 'LY', 'NY', 'SZ', 'TY', 'ZS'}
_VOCAB = None

def vocab():
    global _VOCAB
    if _VOCAB is None:
        words = open(os.environ.get('COMPAT_PY_VOCAB', OUT + 'py_vocab.txt')).read().split('\n')
        _VOCAB = [w.upper() for w in words if 2 <= len(w) <= 7]
        _VOCAB.sort(key=lambda w: (-len(w), w))
    return _VOCAB


def assign(word, hand, fixed=None):
    """A szó kirakása a kézből: [(tile, letter, is_blank)] vagy None. `fixed`: {index: betű} a táblán már álló betűk."""
    fixed = fixed or {}
    hand = list(hand)
    out = []

    def rec(i, hand):
        if i == len(word):
            return True
        if i in fixed:
            ch = fixed[i]
            if word[i:i+len(ch)] != ch:
                return False
            if len(ch) == 2:
                out.append(None)
                # a következő pozíciót a fix kétjegyű betű lefedi
                return rec(i + 2, hand)
            out.append(None)
            return rec(i + 1, hand)
        for size in (2, 1):
            piece = word[i:i+size]
            if len(piece) < size:
                continue
            if size == 1 and word[i:i+2] in DIGRAPHS:
                continue  # a kétjegyű betű csak a saját zsetonjával
            if piece in hand:
                h2 = list(hand); h2.remove(piece)
                out.append((piece, piece, False))
                if rec(i + size, h2): return True
                out.pop()
        if size == 1 or True:
            piece = word[i:i+1]
            if '' in hand and word[i:i+2] not in DIGRAPHS:
                h2 = list(hand); h2.remove('')
                out.append(('', piece, True))
                if rec(i + 1, h2): return True
                out.pop()
        return False

    return out if rec(0, hand) else None


def first_move(hand, skip=0, size=None):
    """Szó a közepén át (7,7), vízszintesen. Visszatér a place_tiles tiles listájával."""
    hits = 0
    for word in vocab():
        if len(word) < 2: continue
        a = assign(word, hand)
        if a is None: continue
        if any(x[2] for x in a) and len(word) > 3 and sum(1 for x in a if x[2]) > 1: continue
        if hits < skip:
            hits += 1; continue
        # a betűszám (a kétjegyű betű egy zseton)
        n = len(a)
        if size and n != size: continue
        c0 = 7 - n // 2
        return [{'row': 7, 'col': c0 + i, 'letter': a[i][1], 'is_blank': a[i][2]} for i in range(n)]
    return None


def cross_move(board, hand, skip=0, size=None):
    """Függőleges szó egy már a táblán lévő betűn át, keresztszó nélkül."""
    def cell(r, c):
        if 0 <= r < 15 and 0 <= c < 15:
            x = board[r][c]
            return x['letter'] if x else None
        return None
    hits = 0
    for r in range(15):
        for c in range(15):
            ch = cell(r, c)
            if not ch: continue
            # a betű vízszintes szomszédos betűje nélküli (különben keresztszó) — csak az egyszerű eset
            if cell(r, c - 1) is None or cell(r, c + 1) is None:
                pass
            for word in vocab():
                if len(word) < 3 or len(word) > 6: continue
                idxs = [i for i in range(len(word)) if word[i:i+len(ch)] == ch]
                for k in idxs:
                    fixed = {k: ch}
                    a = assign(word, hand, fixed)
                    if a is None: continue
                    # a függőleges vonal: a szó betűi soronként (kétjegyű betű egy mező)
                    tiles_letters = []
                    j = 0; pos = 0
                    cells = []
                    while pos < len(word):
                        if pos == k:
                            cells.append(None); pos += len(ch)
                        else:
                            t = a[len(cells)]
                            cells.append(t); pos += len(t[1]) if t else 1
                    n = len(cells)
                    # a már álló betű helye a cellák között
                    kidx = cells.index(None)
                    top = r - kidx
                    if top < 0 or top + n > 15: continue
                    ok = True
                    placed = []
                    for i, t in enumerate(cells):
                        rr = top + i
                        if t is None:
                            continue
                        if cell(rr, c) is not None or cell(rr, c - 1) is not None or cell(rr, c + 1) is not None:
                            ok = False; break
                        placed.append({'row': rr, 'col': c, 'letter': t[1], 'is_blank': t[2]})
                    if not ok or not placed: continue
                    if size and len(placed) != size: continue
                    if cell(top - 1, c) is not None or cell(top + n, c) is not None: continue
                    if hits < skip:
                        hits += 1; continue
                    return placed
    return None


def my_hand(client):
    st = client.state
    for p in st['players']:
        if 'hand' in p:
            return p['hand']
    return []


def my_id(client):
    for p in client.state['players']:
        if 'hand' in p:
            return p['id']


def current_client(clients):
    cid = clients[0].state['current_player']
    return next(c for c in clients if my_id(c) == cid)
