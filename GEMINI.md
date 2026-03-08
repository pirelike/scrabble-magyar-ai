# Gemini CLI Kontextus

Ez a fájl a Gemini CLI számára tartalmaz projekt kontextust. Ha Claude Code delegál feladatot, ez a fájl adja a háttérinformációt.

# Magyar Scrabble Klón

## Áttekintés
Webes magyar Scrabble játék online multiplayer támogatással. Flask + Socket.IO backend, vanilla JS frontend.

## Futtatás
```bash
cd ~/Documents/Scripts/scrabble
.venv/bin/python3 server.py           # Cloudflare tunnel-lel (publikus URL)
.venv/bin/python3 server.py --no-tunnel  # Csak helyi hálózat
```
Böngészőben: http://localhost:5000

## Fájlstruktúra
- `server.py` — Flask + SocketIO szerver, lobby/szoba kezelés, Cloudflare tunnel integráció, Socket.IO event handlerek, reconnection grace period
- `game.py` — Játéklogika (Game osztály), körök, pontozás, játék vége, challenge rendszer, kör időlimit
- `player.py` — Player osztály (id, név, kéz, pontszám, disconnected állapot)
- `board.py` — 15×15 tábla, premium mezők, szó elhelyezés validáció és pontozás
- `dictionary.py` — Magyar szótár-ellenőrzés (pyenchant / hunspell CLI fallback)
- `tiles.py` — Magyar betűkészlet (100 zseton), TileBag osztály
- `challenge.py` — Challenge (megtámadás) logika, szavazási állapotgép, vote resolution
- `room.py` — Room osztály (szoba állapot, owner, beállítások, chat, timer invalidálás)
- `state.py` — ServerState singleton (szobák, játékosok, tokenek, reconnect tracking — 18 metódus)
- `routes.py` — Flask blueprint-ek: auth (8 route), game (3 route), main (index)
- `config.py` — SMTP, auth, DB, rate limit konfigurációs konstansok (`os.environ`-ból)
- `auth.py` — SQLite DB kezelés, regisztráció, login, session, jelszó hash (PBKDF2), játék mentés/visszatöltés/lépésnaplózás
- `email_service.py` — 6 számjegyű kód generálás, SMTP küldés (háttérszálon)
- `rate_limiter.py` — Generikus rate limiter Socket.IO (SID) és HTTP (IP) endpointokhoz
- `tunnel.py` — Cloudflare tunnel subprocess kezelés (indítás/leállítás)
- `dict/` — Beágyazott hu_HU hunspell szótár fájlok (hu_HU.dic, hu_HU.aff)
- `templates/index.html` — Egyoldalas UI: auth (3 tab), lobby, várakozó szoba, játék
- `static/app.js` — Kliens logika, drag & drop, pinch-to-zoom, Socket.IO kommunikáció, auth flow, téma váltás, hang rendszer (SoundManager, SoundSettings)
- `static/style.css` — Stílusok, sötét/világos téma (Slate+Gold paletta), reszponzív layout
- `tests/` — Tesztek (pytest, 363 teszt)
- `requirements.txt` — Python függőségek (flask, flask-socketio, pyenchant, eventlet)
- `.venv/` — Virtual environment

## Funkciók
- 1-4 játékos (egyedül is játszható)
- Online multiplayer: lobby, szobák, Cloudflare tunnel automatikus publikus URL
- **Nyilvános és privát szobák**: privát szoba csak 6-jegyű kóddal csatlakozható, nyilvános szobák a lobbyban listázva
- Felhasználói fiók rendszer: regisztráció (email verifikáció), bejelentkezés, vendég mód
- Teljes magyar betűkészlet (SZ, CS, GY, LY, NY, ZS, TY többkarakteres betűk)
- Standard Scrabble pontozás: DL, TL, DW, TW premium mezők
- 50 pont bónusz mind a 7 zseton kirakásakor
- Drag & drop és kattintásos betű elhelyezés
- Joker (üres zseton) bármely betűként használható
- Betűcsere és passz
- **Körönkénti időlimit**: opcionális (0/60/90/120/180/300 mp), lejáratkor automatikus passz
- Challenge (megtámadás) mód: 2 játékosnál kötelező elfogadás, 3+ játékosnál szavazásos rendszer (nincs szótár)
- Játék közbeni chat: szöveges üzenetküldés a szobában
- **Sötét / világos téma**: automatikus detektálás (`prefers-color-scheme`), manuális váltás, `localStorage`-ban mentve
- **Hang effektek**: Web Audio API (nincs külső fájl), szintetizált hangok — betű lerakás, szavazás, challenge eredmény, kör értesítő, chat, játék kezdés/vége; hangerő-csúszka + kategóriánkénti kapcsolók, `localStorage`-ban mentve
- **Újracsatlakozás (grace period)**: 120 másodperc a visszacsatlakozásra ha a kapcsolat megszakad játék közben (token alapú)
- **Pinch-to-zoom**: mobilon a tábla nagyítható/kicsinyíthető csípő mozdulattal
- **Szótár-böngésző (Challenge fázis)**: A megtámadás során a lerakott szavak kattintható linkként jelennek meg, amelyek egy új lapon indítanak Google keresést az adott szóra ("A magyar nyelv értelmező szótára" fókusszal), segítve a játékosok döntését a szavazásnál.
- Szótár-ellenőrzés: pyenchant + beágyazott hu_HU szótár (cross-platform, Windows-kompatibilis)
- **Játék mentés / visszatöltés**: manuális mentés (owner-only) a kilépés menüből, lobby-first restore flow
- **Visszajátszás**: befejezett játékok lépésről lépésre visszanézhetők (board snapshot-okkal)
- **Kilépés menü**: owner: mentés+kilépés / kilépés mentés nélkül / mégsem; nem-owner: kilépés / mégsem
- **Profil oldal**: statisztikák (játszott, győzelem, nyerési arány, átl. pontszám) + játékelőzmények

## Biztonság
- `SECRET_KEY`: környezeti változóból (`SECRET_KEY`) vagy futásidőben generált véletlenszerű kulcs
- CORS: `cors_allowed_origins='*'` — minden origin engedélyezett (Cloudflare tunnel kompatibilitáshoz szükséges)
- Rate limiting: minden Socket.IO event-re (SID-alapú, `rate_limiter.py`) + IP-alapú HTTP auth endpointokra
- Input validáció: játékos nevek, szoba nevek, tile placement, email, jelszó szerver oldali validálás
- Board bounds check: a `board.py` és `server.py` is ellenőrzi a pozíciók érvényességét
- Dictionary sanitizálás: szavak regex-szel validálva hunspell hívás előtt
- Production szerver: eventlet WSGI (nem Werkzeug dev server)
- XSS védelem: frontend innerHTML helyett DOM API (textContent, createElement, addEventListener)
- Jelszó: `werkzeug.security` PBKDF2-SHA256, 260k iteráció, random salt
- Session: `secrets.token_urlsafe(48)`, HttpOnly cookie, 30 nap lejárat

## Szoba rendszer

### Nyilvános és privát szobák
- Szoba létrehozásakor a játékos megadhatja: szoba név, max játékosszám (2-4), challenge mód, privát/nyilvános, körönkénti időlimit
- Minden szobához egyedi 6-jegyű csatlakozási kód generálódik
- **Nyilvános szobák**: megjelennek a lobby listájában, csatlakozhatók kóddal vagy room ID-val
- **Privát szobák**: NEM jelennek meg a listában, kizárólag 6-jegyű kóddal csatlakozhatók
- A szoba tulajdonosa (owner) az első csatlakozó játékos; stabil token (`owner_token`) azonosítja
- Ha a tulajdonos kilép, az ownership átadódik az első online játékosnak
- Csak regisztrált felhasználók hozhatnak létre szobát

## Újracsatlakozás és Roster megőrzés

### disconnected állapot
- Aktív játék közben a kapcsolat megszakadásakor **vagy manuális kilépéskor** a játékost nem távolítja el a rendszer
- `disconnected=True` állapot a `Game` objektumban (nem törlődik a listából)
- Automatikus kör-átugrás a lecsatlakozott játékosok felett (`_next_turn`)
- Token alapú újracsatlakozás: `rejoin_room` event
- A játékoslista (roster) a kezdés pillanatában rögzül az adatbázisban is

### Adatstruktúrák (`state.py` — ServerState singleton)
```python
_reconnect_tokens = {token: {room_id, player_name, sid, auth_info}}
_sid_to_token = {sid: token}
_disconnected_players = {token: {room_id, sid, player_name}}
```

## Felhasználói fiók rendszer

### Adatbázis: SQLite (`scrabble.db`)
- **`users`**: id, email, email_lower, display_name, password_hash, created_at, games_played, games_won, total_score, reconnect_token
- **`verification_codes`**: email, code, created_at, expires_at (10 perc), attempts (max 5), used
- **`sessions`**: user_id, token (64 char, `secrets.token_urlsafe`), created_at, expires_at (30 nap)

### Auth HTTP route-ok (`routes.py` — Flask blueprint)
- `POST /api/auth/request-code` — email validálás, kód küldés
- `POST /api/auth/verify-code` — 6 számjegyű kód ellenőrzés
- `POST /api/auth/register` — jelszó + név, fiók létrehozás, auto-login
- `POST /api/auth/login` — email + jelszó
- `POST /api/auth/logout` — session törlés
- `GET /api/auth/me` — session cookie ellenőrzés
- `GET /api/auth/profile` — statisztikák és játékelőzmények
- `GET /api/game/<int:game_id>/moves` — lépések listája (replay-hez)

## Tesztek

```bash
.venv/bin/python -m pytest tests/ -v
```

| Fájl | Tesztek | Lefedettség |
|---|---|---|
| `tests/test_auth.py` | 63 | DB, user CRUD, jelszó hash, verifikációs kódok, session kezelés |
| `tests/test_game_logic.py` | 138 | TileBag, Board, Player, Game, Challenge szavazásos rendszer, kör időlimit |
| `tests/test_server_auth.py` | 48 | HTTP auth route-ok, cookie flow |
| `tests/test_server_socket.py` | 62 | Socket.IO eventek, lobby, szobák, privát szobák, challenge szavazás, chat, owner kilépés, kör időlimit |
| `tests/test_challenge.py` | 17 | Challenge szavazásos rendszer |
| `tests/test_dictionary.py` | 19 | Szótár-ellenőrzés |
| `tests/test_email_service.py` | 4 | Email küldés |
| `tests/test_room.py` | 12 | Room osztály |

**Összesen: 363 teszt**

## Challenge (megtámadás) rendszer — szavazásos

Szoba létrehozásakor bekapcsolható. Játékosszám-függő:
- **2 játékos**: Elfogad/Elutasít gombok, 30 mp timeout, nincs szótár
- **3+ játékos**: 30 mp elfogadási ablak → ha megtámadják, szavazási fázis (újabb 30 mp). 50%+ elfogadás = marad. Nem szavazók = elfogadás.

Technikai: `challenge.py` állapotgép, `accept_words`/`reject_words` Socket.IO eventek kezelik (nincs külön `challenge`/`cast_vote` event).

## Socket.IO eventek összefoglaló

### Kliens→Szerver (14 handler)
`connect`, `disconnect`, `set_name`, `create_room`, `join_room`, `leave_room`, `get_rooms`, `rejoin_room`, `start_game`, `place_tiles`, `exchange_tiles`, `pass_turn`, `accept_words`, `reject_words`, `send_chat`, `save_game`, `restore_game`

### Szerver→Kliens broadcast
`rooms_list`, `room_joined`, `room_code`, `game_state`, `game_started`, `action_result`, `challenge_result`, `chat_message`, `player_joined`, `player_left`, `player_disconnected`, `player_reconnected`, `room_disbanded`, `rejoin_failed`, `error`

## Rate limiting

### Socket.IO (per SID, `rate_limiter.py`)
```python
'set_name': (5, 10), 'create_room': (3, 30), 'join_room': (5, 10),
'place_tiles': (10, 10), 'exchange_tiles': (5, 10), 'pass_turn': (5, 10),
'get_rooms': (10, 5), 'accept_words': (5, 10), 'reject_words': (5, 10),
'send_chat': (10, 10), 'rejoin_room': (5, 10), 'save_game': (3, 30), 'restore_game': (3, 30),
```

### HTTP auth (IP-alapú, `config.py`)
`request_code`: 3/300mp, `login`: 10/300mp, `register`: 3/3600mp

## Konstansok
- `game.py`: `HAND_SIZE=7`, `BONUS_ALL_TILES=50`, `CHALLENGE_TIMEOUT=30`
- `server.py`: `_DISCONNECT_GRACE_PERIOD=120`, `ALLOWED_TURN_TIME_LIMITS={0,60,90,120,180,300}`
- `config.py`: `DB_PATH='scrabble.db'`, `SESSION_MAX_AGE_DAYS=30`, `VERIFICATION_CODE_EXPIRY_MINUTES=10`, `VERIFICATION_MAX_ATTEMPTS=5`

## Játék mentés / visszatöltés

### DB táblák
- **`saved_games`**: id, room_id, room_name, state_json, status, challenge_mode, owner_name, owner_token, created_at, updated_at
- **`game_players`**: id, game_id FK, user_id FK (NULL vendégnél), player_name, final_score, is_winner
- **`game_moves`**: id, game_id FK, move_number, player_name, action_type, details_json, board_snapshot_json, created_at

### Flow
- Kezdeti mentés: játék indításakor automatikusan
- Manuális mentés: owner kilépés menüből ("Mentés és kilépés")
- **Automatikus mentés**: Ha az owner végleg lecsatlakozik (120 mp grace period lejár), a rendszer automatikusan menti az állapotot feloszlatás előtt
- Restore: `restore_game` event → privát várakozó szoba → eredeti játékosok csatlakoznak → `Game.from_save_dict()`
- Befejezéskor: `finish_game()` automatikus mentés

## TODO

### Játékmenet
- [x] Challenge rendszer, [x] Mentés/visszatöltés, [x] Visszajátszás, [x] Időlimit
- [ ] AI ellenfél — nehézségi szintek

### Közösségi
- [x] Chat, [x] Privát szobák, [x] Profil oldal
- [ ] Spectator mód, [ ] Ranglista, [ ] Barátlista

### UI / UX
- [x] Sötét/világos téma, [x] Hang effektek, [x] Szótár-böngésző (Challenge fázis)
- [ ] Animációk, [ ] Szótár-böngésző (kereső), [ ] PWA, [ ] Többnyelvű felület
