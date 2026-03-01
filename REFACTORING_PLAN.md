# Refactoring Terv — Magyar Scrabble

## Jelenlegi állapot összefoglalása

| Fájl | Sorok | Értékelés |
|------|-------|-----------|
| `server.py` | 1 543 | **Kritikus** — monolitikus, 9 globális dict, duplikált logika |
| `game.py` | 668 | Közepes — persistence tracking nem ide való |
| `auth.py` | 551 | Jó — N+1 query javítandó |
| `board.py` | 292 | Jó — `validate_placement()` túl komplex |
| `challenge.py` | 94 | Jó |
| `room.py` | 63 | Jó |
| `tiles.py` | 81 | Jó |
| `dictionary.py` | 95 | Jó |
| `config.py` | 24 | Jó |
| `email_service.py` | 52 | Jó |
| `static/app.js` | 2 207 | Közepes — monolitikus, de jól szervezett modulok |
| `templates/index.html` | 450 | Jó |

**Fő probléma:** A `server.py` (1 543 sor) a kódbázis teljes szerver oldali orchestrációját tartalmazza: Socket.IO handlerek, HTTP route-ok, szoba kezelés, reconnection logika, rate limiting, challenge timer, játék mentés, Cloudflare tunnel, globális állapot kezelés — mind egy fájlban.

---

## 1. fázis — `server.py` szétbontása (legkritikusabb)

### 1.1. `state.py` — Központi állapotkezelő osztály

**Probléma:** 9 globális szótár (`rooms`, `join_codes`, `player_rooms`, `player_names`, `player_auth`, `_reconnect_tokens`, `_sid_to_token`, `_disconnected_players`, `_rate_limits`) egymástól függetlenül van deklarálva. Egy játékos kilépésekor 5-6 dict-et kell kézzel szinkronban tartani, ami hibaforrás.

**Megoldás:** `ServerState` osztály, amely egységesen kezeli az összes dict-et.

```python
# state.py

class ServerState:
    """Szerver szintű állapot egyetlen objektumban."""

    def __init__(self):
        self.rooms = {}                    # room_id -> Room
        self.join_codes = {}               # join_code -> room_id
        self.player_rooms = {}             # sid -> room_id
        self.player_names = {}             # sid -> name
        self.player_auth = {}              # sid -> {user_id, is_guest}
        self._reconnect_tokens = {}        # token -> {room_id, player_name, sid}
        self._sid_to_token = {}            # sid -> token
        self._disconnected_players = {}    # token -> {room_id, sid, player_name}

    # --- Player lifecycle ---
    def register_player(self, sid, name, auth_info):
        """Játékos regisztráció (set_name)."""
        ...

    def unregister_player(self, sid):
        """Játékos összes adatának törlése (disconnect végén)."""
        self.player_names.pop(sid, None)
        self.player_auth.pop(sid, None)
        # token cleanup is itt

    # --- Room lifecycle ---
    def add_room(self, room, join_code):
        """Szoba hozzáadása a rooms és join_codes dict-hez."""
        ...

    def remove_room(self, room_id):
        """Szoba és összes kapcsolódó token/code törlése."""
        # _cleanup_room logikája ide kerül
        ...

    def get_room_for_player(self, sid):
        """Szoba lekérdezése SID alapján (room_id, Room, Game) tuple."""
        ...

    # --- Reconnection ---
    def generate_reconnect_token(self, sid, room_id, player_name):
        ...

    def mark_disconnected(self, token, sid, room_id, player_name):
        ...

    def complete_rejoin(self, token, new_sid):
        """Token alapú újracsatlakozás: dict-ek frissítése."""
        ...

    def finalize_disconnect(self, token):
        """Grace period lejárt: játékos adatok törlése."""
        ...

    def cleanup_room_tokens(self, room_id):
        """Szobához tartozó összes token/disconnected player törlése."""
        ...
```

**Előny:** Egy `state.remove_room(room_id)` hívás garantáltan kitakarít mindent, nem kell 3 helyen megismételni a cleanup logikát.

### 1.2. `rate_limiter.py` — Rate limiting kiszervezése

**Probléma:** Socket.IO és HTTP rate limiting logikája a `server.py`-ban van, holott önálló felelősség.

```python
# rate_limiter.py

class RateLimiter:
    """Generikus rate limiter SID-hez és IP-hez."""

    def __init__(self, socket_limits, ip_limits):
        self._socket_limits = socket_limits   # {event: (max, window)}
        self._ip_limits = ip_limits           # {action: (max, window)}
        self._socket_history = defaultdict(lambda: defaultdict(list))
        self._ip_history = defaultdict(lambda: defaultdict(list))

    def check_socket(self, sid, event):
        """Socket.IO event rate limit check. True = engedélyezve."""
        ...

    def check_ip(self, ip, action):
        """IP-alapú rate limit check. True = engedélyezve."""
        ...

    def clear_sid(self, sid):
        """SID törlése disconnect-kor."""
        self._socket_history.pop(sid, None)
```

### 1.3. `routes.py` — HTTP route-ok kiszervezése

**Probléma:** 12 HTTP route (`/api/auth/*`, `/api/game/*`, `/`) a `server.py`-ban él. Ezek Flask Blueprintként elkülöníthetők.

```python
# routes.py
from flask import Blueprint

auth_bp = Blueprint('auth', __name__)
game_bp = Blueprint('game', __name__)

@auth_bp.route('/api/auth/request-code', methods=['POST'])
def request_code():
    ...

@auth_bp.route('/api/auth/verify-code', methods=['POST'])
def verify_code():
    ...

# ... többi auth route ...

@game_bp.route('/api/game/<int:game_id>/moves', methods=['GET'])
def game_moves(game_id):
    ...

@game_bp.route('/api/game/<int:game_id>/abandon', methods=['POST'])
def abandon_game(game_id):
    ...
```

A `server.py`-ban:
```python
app.register_blueprint(auth_bp)
app.register_blueprint(game_bp)
```

**Mérés:** ~290 sor (370-656) kiszervezhető.

### 1.4. `tunnel.py` — Cloudflare tunnel kiszervezése

**Probléma:** Tunnel kezelés (1486-1529 sorok) a szerver fő fájljában van.

```python
# tunnel.py
def start_tunnel(port):
    ...

def stop_tunnel():
    ...
```

**Mérés:** ~45 sor kiszervezhető.

### 1.5. Duplikált room cleanup/disband logika deduplikálása

**Probléma:** A szoba feloszlatás logikája (owner disband + játékosok kitakarítása) 3 helyen ismétlődik, eltérő részletekkel:

1. `handle_leave_room()` (1061-1103 sorok) — owner aktív játékból kilép
2. `_finalize_player_disconnect()` (765-795 sorok) — owner grace period lejár
3. `_cleanup_room()` (231-245) — szoba törlésekor a tokenek

**Megoldás:** Egy `_disband_active_room(room_id, reason_message)` helper, amit mindhárom helyről hívunk:

```python
def _disband_active_room(room_id, message):
    """Aktív játék szoba feloszlatása: broadcast, játékosok kitakarítása, room törlés."""
    room = state.rooms.get(room_id)
    if not room:
        return
    game = room.game

    # Broadcast disbanded event
    socketio.emit('room_disbanded', {'message': message}, room=room_id)

    # Clean up all players (SID-ek, tokenek, disconnected-ek)
    for p in list(game.players):
        if p.id != room.owner:
            state.cleanup_player_from_room(p.id, room_id)
            socketio.emit('room_left', {}, room=p.id)

    state.cleanup_room_tokens(room_id)

    if game.started and not game.finished:
        abandon_game(room_id)
    state.remove_room(room_id)
```

Ez eliminál ~80 sor duplikációt.

### 1.6. Eredmény `server.py` szétbontás után

| Fájl | Becsült sorok | Tartalom |
|------|---------------|----------|
| `server.py` | ~650 | App init, Socket.IO handlerek, main |
| `state.py` | ~150 | ServerState osztály |
| `rate_limiter.py` | ~60 | RateLimiter osztály |
| `routes.py` | ~300 | HTTP route-ok (Blueprint) |
| `tunnel.py` | ~45 | Cloudflare tunnel |

A `server.py` 1 543 → ~650 sorra csökken (58%-os csökkenés).

---

## 2. fázis — `game.py` tisztítása

### 2.1. Persistence tracking eltávolítása a Game osztályból

**Probléma:** A `Game` osztály tartalmaz DB-specifikus attribútumokat:
- `self._db_game_id` (55. sor)
- `self._last_saved_move_count` (56. sor)

Ezek szerver szintű persistence metaadatok, nem a játék logikájának részei.

**Megoldás:** Áthelyezés a `Room` osztályba (vagy a `ServerState`-be), mivel a szoba szintjén van értelme a mentés nyilvántartásnak.

```python
# room.py módosítás
class Room:
    def __init__(self, ...):
        ...
        self.db_game_id = None
        self.last_saved_move_count = 0
```

A `_save_game_to_db()` a `room.db_game_id` és `room.last_saved_move_count`-ot használja `game._db_game_id` helyett.

### 2.2. Player osztály kiszervezése

**Probléma:** A `Player` osztály a `game.py`-ban van definiálva, de független koncepció.

**Megoldás:** `player.py` fájl létrehozása:

```python
# player.py
class Player:
    """Egy játékos állapota."""
    def __init__(self, player_id, name):
        ...
    def to_dict(self, reveal_hand=False):
        ...
```

Ez kicsi változás, de javítja a moduláris szervezettséget és csökkenti a `game.py` méretét.

---

## 3. fázis — `board.py` refaktorálása

### 3.1. `validate_placement()` szétbontása

**Probléma:** A `validate_placement()` metódus (~60 sor) 5 validációs fázist tartalmaz egyetlen metódusban, mély beágyazással és try-finally blokkal.

**Megoldás:** Fázisok szétbontása önálló metódusokba:

```python
class Board:
    def validate_placement(self, tiles_placed, skip_dictionary=False):
        """Fő validációs entry point."""
        err = self._validate_positions(tiles_placed)
        if err:
            return False, [], err

        direction = self._get_direction(tiles_placed)
        if direction is None:
            return False, [], "A betűknek egy sorban vagy oszlopban kell lenniük."

        err = self._validate_first_move(tiles_placed)
        if err:
            return False, [], err

        err = self._validate_continuity(tiles_placed, direction)
        if err:
            return False, [], err

        err = self._validate_adjacency(tiles_placed)
        if err:
            return False, [], err

        return self._extract_and_validate_words(tiles_placed, direction, skip_dictionary)

    def _validate_positions(self, tiles_placed):
        """Pozíció és foglaltság ellenőrzés."""
        ...

    def _get_direction(self, tiles_placed):
        """H/V irány meghatározása. None ha nem egyenesvonalú."""
        ...

    def _validate_first_move(self, tiles_placed):
        """Első lerakás: legalább 2 betű, középső mező."""
        ...

    def _validate_continuity(self, tiles_placed, direction):
        """Folytonosság: nincs lyuk a sorban."""
        ...

    def _validate_adjacency(self, tiles_placed):
        """Csatlakozás meglévő betűkhöz (nem első lépésnél)."""
        ...
```

### 3.2. Try-finally eliminálása

**Probléma:** A jelenlegi kód ideiglenesen elhelyezi a betűket a táblán validálás céljából, majd finally-ben eltávolítja:

```python
try:
    for r, c, letter, is_blank in tiles_placed:
        self.cells[r][c] = (letter, is_blank)
    # ... validáció ...
finally:
    for r, c, letter, is_blank in tiles_placed:
        self.cells[r][c] = None
```

**Megoldás:** Temporális cella-kezelő context manager:

```python
@contextmanager
def _temporary_placement(self, tiles_placed):
    """Ideiglenes betű elhelyezés validáláshoz."""
    placed = []
    try:
        for r, c, letter, is_blank in tiles_placed:
            self.cells[r][c] = (letter, is_blank)
            placed.append((r, c))
        yield
    finally:
        for r, c in placed:
            self.cells[r][c] = None
```

---

## 4. fázis — `auth.py` optimalizálása

### 4.1. N+1 query javítása `get_user_game_history()`-ban

**Probléma:** Minden játékhoz külön lekérdezés fut az ellenfelek neveiért:

```python
for row in rows:
    opponents = _get_opponents(row['game_id'], user_id)
    result.append({..., 'opponents': opponents})
```

**Megoldás:** Egyetlen JOIN query:

```sql
SELECT sg.id as game_id, sg.room_name, sg.created_at,
       gp.final_score, gp.is_winner,
       GROUP_CONCAT(gp2.player_name, ', ') as opponents
FROM saved_games sg
JOIN game_players gp ON sg.id = gp.game_id AND gp.user_id = ?
LEFT JOIN game_players gp2 ON sg.id = gp2.game_id AND gp2.user_id != ?
WHERE sg.status = 'finished'
GROUP BY sg.id
ORDER BY sg.created_at DESC
LIMIT ?
```

### 4.2. DB connection context manager

**Probléma:** Minden függvény saját `conn = sqlite3.connect(...)` hívást tartalmaz.

**Megoldás:**

```python
@contextmanager
def _get_db():
    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA foreign_keys=ON")
    try:
        yield conn
        conn.commit()
    except Exception:
        conn.rollback()
        raise
    finally:
        conn.close()
```

---

## 5. fázis — Tesztek javítása

### 5.1. Közös fixture-ök kiszervezése `conftest.py`-ba

**Probléma:** A `temp_db` fixture a `test_auth.py`-ban, hasonló fixture-ök a `test_server_auth.py`-ban és `test_server_socket.py`-ban vannak, duplikálva.

```python
# tests/conftest.py
@pytest.fixture(autouse=True)
def temp_db(tmp_path, monkeypatch):
    db_path = str(tmp_path / "test.db")
    monkeypatch.setattr('auth.DB_PATH', db_path)
    init_db()
    yield db_path
```

### 5.2. Hiányzó tesztek hozzáadása

**Kritikus hiányok:**
1. **Reconnection grace period** — a 120 mp-es ablak egyáltalán nincs tesztelve integrációs szinten
2. **Challenge timer timeout** — nincs teszt a 30 mp-es timeout-ra
3. **Owner disband flow** — a teljes cleanup logika nincs tesztelve (token + disconnected player cleanup)
4. **Concurrent actions** — egyidejű lépések

### 5.3. Teszt helper kiszervezése

Ismétlődő minták:
```python
# tests/helpers.py
def create_room_with_players(socketio_app, app, n_players=2, challenge_mode=False):
    """N játékost tartalmazó szoba létrehozása és játék indítása."""
    ...

def place_word(client, tiles):
    """Betűk lerakása egyszerűsített interfészen."""
    ...
```

---

## 6. fázis — Frontend (`app.js`) szétbontása

### 6.1. ES modul rendszerre váltás

**Probléma:** 2 207 sor egyetlen fájlban, 13+ pseudo-modul globális objektumokként. Nincs valódi modul rendszer.

**Megoldás:** Szétbontás ES modulokra (natív böngésző `import/export`, bundler nélkül):

```
static/
├── app.js              (main entry, ~30 sor — importok + init)
├── modules/
│   ├── constants.js    (TILE_VALUES, ALL_LETTERS, PREMIUM_MAP)
│   ├── state.js        (AppState, BoardState exportálás)
│   ├── utils.js        (escapeHtml, showScreen, showMessage, showConfirm)
│   ├── theme.js        (téma init és toggle)
│   ├── auth.js         (Auth modul)
│   ├── lobby.js        (Lobby modul)
│   ├── waiting-room.js (WaitingRoom modul)
│   ├── game-board.js   (GameBoard modul — szétbontva)
│   ├── hand.js         (Kéz kezelés, drag & drop)
│   ├── touch-drag.js   (Mobil touch drag)
│   ├── board-zoom.js   (Pinch-to-zoom)
│   ├── challenge-ui.js (ChallengeUI modul)
│   ├── chat.js         (Chat modul)
│   ├── blank-dialog.js (BlankDialog modul)
│   ├── game-over.js    (GameOver modul)
│   ├── exit-game.js    (ExitGame modul)
│   ├── reconnection.js (Reconnection modul)
│   ├── profile.js      (Profile modul)
│   └── replay.js       (Replay modul)
```

**HTML módosítás:**
```html
<script type="module" src="/static/app.js"></script>
```

**Fontos:** A Socket.IO instance-t egyetlen helyen hozzuk létre és exportáljuk:

```javascript
// modules/socket.js
export const socket = io({ ... });
```

### 6.2. GameBoard szétbontása

**Probléma:** A `GameBoard` modul (420 sor) kezel: tábla renderelést, kéz renderelést, scoreboard-ot, game info-t, desktop drag & drop-ot, mobil touch-ot, tile placement logikát.

**Megoldás:** 3 modulra bontás:
- `game-board.js` — Tábla építés + renderelés + desktop drag & drop
- `hand.js` — Kéz renderelés + tile kiválasztás + exchange mód
- `game-info.js` — Scoreboard + game info renderelés

### 6.3. Magic number-ök konstansokba

```javascript
// modules/constants.js
export const TOUCH_LONG_PRESS_MS = 200;
export const CHALLENGE_TIMEOUT_SEC = 30;
export const SAVE_TIMEOUT_MS = 3000;
export const MAX_CHAT_MESSAGES = 100;
export const MAX_CHAT_LENGTH = 200;
export const BOARD_SIZE = 15;
```

---

## 7. fázis — Kisebb javítások

### 7.1. Input validáció konszolidálása

**Probléma:** A tile pozíció validálás kétszer fut: `_validate_tiles_input()` a `server.py`-ban és `validate_placement()` a `board.py`-ban.

**Megoldás:** A `server.py`-ban csak a formátum-validáció marad (típusellenőrzés), a pozíció érvényesség a `board.py`-ra bízható.

### 7.2. Challenge timer emit duplikáció

**Probléma:** A `challenge_result` broadcast 3 helyen történik különböző formában:
- `_handle_challenge_result()` (358-367)
- `_start_challenge_timer()` callback-ben (283-287)
- `handle_challenge()` (1401-1404)

**Megoldás:** Egyetlen `_broadcast_challenge_result(room_id, result, msg)` helper.

### 7.3. `get_rooms_list()` — szűrés egységesítése

**Probléma:** A `get_rooms_list()` szűrési logikája inline van.

**Megoldás:** A `Room` osztályon `is_lobby_visible` property:

```python
class Room:
    @property
    def is_lobby_visible(self):
        return not self.is_private and not self.is_restored and not self.game.finished
```

---

## Sorrend és prioritás

| Fázis | Prioritás | Kockázat | Becsült munka |
|-------|-----------|----------|---------------|
| 1. server.py szétbontása | **Kritikus** | Közepes | Nagy |
| 2. game.py tisztítása | Közepes | Alacsony | Kicsi |
| 3. board.py refaktorálás | Közepes | Alacsony | Kicsi |
| 4. auth.py optimalizálás | Alacsony | Alacsony | Kicsi |
| 5. Tesztek javítása | Közepes | Alacsony | Közepes |
| 6. Frontend szétbontás | Alacsony | Közepes | Nagy |
| 7. Kisebb javítások | Alacsony | Alacsony | Kicsi |

**Javasolt sorrend:** 1 → 5 → 2 → 3 → 7 → 4 → 6

Az 1-es fázist (server.py szétbontás) érdemes a tesztek javításával együtt végezni, mivel a szétbontás után a teszteket is módosítani kell.

---

## Ami NEM szerepel a tervben

- **Framework váltás** (Vue/React): Túl nagy változás, a vanilla JS megoldás működik
- **ORM bevezetése** (SQLAlchemy): Túl sok overhead SQLite-hoz
- **TypeScript migráció**: A frontend mérete nem indokolja
- **CSS preprocesszor** (SASS): A CSS változós rendszer jól működik
- **Bundler** (Webpack/Vite): Az ES modulok natívan működnek, bundler nem szükséges
