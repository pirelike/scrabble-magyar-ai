# Erőforrás-kezelés és Subagent Delegáció
Az iterációs ciklusok és a memóriaterhelés optimalizálása érdekében rendelkezel egy dedikált elemző eszközzel: a Gemini CLI-vel.

SZABÁLYOK A GEMINI HASZNÁLATÁRA:
- Ha egy feladat a teljes projekt áttekintését (több száz fájl) igényli, vagy "Read-Heavy" (pl. elavult kódmintázatok keresése).
- Ha naprakész webes információra van szükséged egy új API-ról vagy CVE sebezhetőségről.
- Ha független kód-felülvizsgálatot (Second Opinion / Code Review) szeretnél kérni a megírt kódodra.

ILYENKOR HASZNÁLD AZ Gemini CLI-t a feladatok leadása érdekében és kontextusablak megtakarítás érdekében:

A Gemini kimenetét tekintsd desztillált ténynek, és használd fel a saját, precíziós kódolási folyamatodban.

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
- **Szótár-böngésző (Challenge fázis)**: A megtámadás során a lerakott szavak kattintható linkek, amelyek egy új lapon indítanak Google keresést az adott szóra ("A magyar nyelv értelmező szótára" fókusszal).
- Szótár-ellenőrzés: pyenchant + beágyazott hu_HU szótár (cross-platform, Windows-kompatibilis)
- **Játék mentés / visszatöltés**: manuális mentés (owner-only) a kilépés menüből, lobby-first restore flow
- **Visszajátszás**: befejezett játékok lépésről lépésre visszanézhetők (board snapshot-okkal)
- **Kilépés menü**: owner: mentés+kilépés / kilépés mentés nélkül / mégsem; nem-owner: kilépés / mégsem
- **Profil oldal**: statisztikák (játszott, győzelem, nyerési arány, átl. pontszám) + játékelőzmények

## Biztonság
- `SECRET_KEY`: környezeti változóból (`SECRET_KEY`) vagy futásidőben generált véletlenszerű kulcs
- CORS: `cors_allowed_origins='*'` — minden origin engedélyezett (Cloudflare tunnel kompatibilitáshoz szükséges; a biztonságot session auth és rate limiting biztosítja)
- Rate limiting: minden Socket.IO event-re (SID-alapú, `rate_limiter.py`) + IP-alapú HTTP auth endpointokra
- Input validáció: játékos nevek, szoba nevek, tile placement, email, jelszó szerver oldali validálás
- Board bounds check: a `board.py` és `server.py` is ellenőrzi a pozíciók érvényességét
- Dictionary sanitizálás: szavak regex-szel validálva hunspell hívás előtt
- Production szerver: eventlet WSGI (nem Werkzeug dev server), `allow_unsafe_werkzeug` nem használt
- XSS védelem: frontend innerHTML helyett DOM API (textContent, createElement, addEventListener)
- Jelszó: `werkzeug.security` PBKDF2-SHA256, 260k iteráció, random salt
- Verifikációs kód: 6 számjegy, 10 perc lejárat, max 5 próbálkozás/kód
- Session: `secrets.token_urlsafe(48)`, HttpOnly cookie, 30 nap lejárat

## Szoba rendszer

### Nyilvános és privát szobák
- Szoba létrehozásakor a játékos megadhatja: szoba név, max játékosszám (2-4), challenge mód, privát/nyilvános, körönkénti időlimit
- Minden szobához egyedi 6-jegyű csatlakozási kód generálódik
- **Nyilvános szobák**: megjelennek a lobby listájában, csatlakozhatók kóddal vagy room ID-val
- **Privát szobák**: NEM jelennek meg a listában, kizárólag 6-jegyű kóddal csatlakozhatók
- A szoba tulajdonosa (owner) az első csatlakozó játékos; a tulajdonjogot stabil token (`owner_token`) azonosítja az instabil session ID helyett
- Ha a tulajdonos kilép, az ownership átadódik az első online játékosnak
- Csak regisztrált felhasználók hozhatnak létre szobát

### Várakozó szoba
- Játékosok listája (névvel, ready státusszal)
- Challenge mód, privát mód és időlimit badge-ek
- A tulajdonos indíthatja a játékot (min 1 játékos)
- Bármely játékos elhagyhatja a szobát

## Újracsatlakozás és Roster megőrzés

### disconnected állapot
- Aktív játék közben a kapcsolat megszakadásakor **vagy manuális kilépéskor** a játékost nem távolítja el a rendszer
- A szerver a játékost `disconnected=True` állapotúra állítja a `Game` objektumban (nem törli a listából)
- A játék automatikusan átugorja a lecsatlakozott játékosokat a körök váltásakor (`_next_turn`)
- Token alapú újracsatlakozás: `rejoin_room` event a korábbi tokennel bármikor az aktív játék alatt
- A játékoslista (roster) a kezdés pillanatában rögzül az adatbázisban is

### Adatstruktúrák (`state.py` — ServerState singleton)
```python
# ServerState 18 metódusa kezeli ezeket:
_reconnect_tokens = {token: {room_id, player_name, sid, auth_info}}
_sid_to_token = {sid: token}
_disconnected_players = {token: {room_id, sid, player_name}}
```

## Felhasználói fiók rendszer

### Regisztrációs flow
```
[1. Email megadás] → [2. Kód emailben (6 számjegy)] → [3. Kód beírása] → [4. 2x jelszó mező + megjelenítési név] → [5. Fiók létrehozva, auto-login] → [6. Lobby]
```

### Bejelentkezési flow
```
[1. Email + jelszó] → [2. Lobby]
```

Vendég mód: a régi név-megadós flow megmarad (statisztikák nem mentődnek).

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
- `GET /api/auth/profile` — statisztikák és játékelőzmények (session cookie)
- `GET /api/game/<int:game_id>/moves` — lépések listája (replay-hez)

Session cookie: `HttpOnly` + `SameSite=Lax` + `Secure` (Cloudflare tunnel HTTPS).
IP-alapú rate limiting (`rate_limiter.py`): kód küldés 3/5perc, login 10/5perc, regisztráció 3/óra.

### Frontend auth (`index.html` + `app.js`)
Az `auth-screen` 3 tabbal:
- **Bejelentkezés** tab: email + jelszó form
- **Regisztráció** tab: 3 lépéses wizard (email → kód → jelszó 2x + név)
- **Vendég** tab: régi név-megadós flow

Oldal betöltéskor `GET /api/auth/me` → ha van érvényes session, automatikus belépés a lobby-ba.

### Környezeti változók (SMTP)
```
SMTP_HOST=smtp.gmail.com
SMTP_PORT=587
SMTP_USER=yourscrabble@gmail.com
SMTP_PASSWORD=abcd-efgh-ijkl-mnop
SMTP_FROM=yourscrabble@gmail.com
```
Ha SMTP nincs konfigurálva, a kód a szerver konzolra íródik ki (fejlesztéshez).

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

Fixture: `tests/conftest.py` — temp_db (auto-applied, ideiglenes SQLite DB minden teszthez)

## Challenge (megtámadás) rendszer — szavazásos

### Működés
Szoba létrehozásakor bekapcsolható a "Megtámadás mód" (checkbox). A rendszer játékosszám-függő:

#### 2 játékos
1. Amikor egy játékos lerak szavakat, a másik játékos látja a lerakott szavakat
2. A másik játékos "Elfogad" vagy "Elutasít" gombbal reagálhat
3. "Elfogad" → lerakás véglegesítve, pontok jóváírva
4. "Elutasít" → lerakás visszavonva, betűk visszakerülnek a lerakó kezébe
5. Ha 30 mp-en belül nem reagál, automatikusan elfogadódik
6. **Nincs szótár-ellenőrzés** — kizárólag a másik játékos döntése számít

#### 3+ játékos
1. Amikor egy játékos lerak szavakat, 30 másodperces ablak nyílik
2. A többi játékos "Megtámad" vagy "Elfogad" gombbal reagálhat
3. Ha valaki megnyomja a "Megtámad" gombot, **szavazási fázis** indul (újabb 30 mp)
4. Ha mindenki elfogad (vagy lejár az idő), a lerakás véglegesítődik
5. **Nincs szótár-ellenőrzés** — kizárólag a játékosok szavazata dönt

#### Szavazási fázis (3+ játékos)
- A **lerakó** nem szavaz (ő rakta le)
- A **megtámadó** nem szavaz (ő indította a szavazást)
- A **többi játékos** szavaz: "Elfogad" vagy "Elutasít"
- **50% vagy több elfogadás → szó marad** (döntetlen = elfogadva)
- **Kevesebb mint 50% → szó elutasítva**, betűk visszakerülnek a lerakó kezébe
- Nem szavazók (timeout) elfogadásnak számítanak
- **Nincs kör kihagyás büntetés** a megtámadónak

#### Példák
- **4 játékos**: 2 szavazó (lerakó és megtámadó kizárva). 1 elfogad + 1 elutasít = 50% → elfogadva
- **3 játékos**: 1 szavazó. Ő egyedül dönti el

### Technikai részletek
- `challenge.py`: Challenge állapotgép, szavazás indítás, vote resolution
- `game.py`: `accept_pending_by_player()` játékos elfogadás, `reject_pending_by_player()` 2 játékos elutasítás
- `server.py`: `accept_words`, `reject_words` Socket.IO eventek + `_start_challenge_timer()` háttérfolyamat
- Egyjátékos módban a challenge mód nincs hatással (nincs ki megtámadja)
- Szótár-ellenőrzés teljesen kikapcsolva challenge módban (lerakáskor és szavazásnál is)

### Socket.IO eventek
- `accept_words` (kliens→szerver): lerakás elfogadása, challenge elfogadás, vagy elfogadó szavazat (kontextus-függő)
- `reject_words` (kliens→szerver): lerakás elutasítása (2 játékos) vagy megtámadás indítása (3+ játékos)
- `challenge_result` (szerver→szoba): `{challenge_won, message}` — szavazás/döntés eredménye

## In-game Chat

### Működés
Játék közben a side panelen chat szekció érhető el:
- Üzenetek max 200 karakter hosszúak
- Rate limiting: max 10 üzenet / 10 mp
- Üzenetek a szoba összes játékosának broadcastolva
- Max 100 üzenet tárolva szobánként (memóriában)

### Socket.IO eventek
- `send_chat` (kliens→szerver): `{message}` — üzenet küldése
- `chat_message` (szerver→szoba): `{name, message}` — üzenet broadcastolás

## Socket.IO eventek összefoglaló

### Szoba kezelés (kliens→szerver)
| Event | Leírás |
|---|---|
| `set_name` | Játékosnév / auth adatok beállítása |
| `create_room` | Szoba létrehozása (név, max_players, challenge_mode, is_private, turn_time_limit) |
| `join_room` | Csatlakozás kóddal vagy room_id-val |
| `leave_room` | Szoba elhagyása |
| `get_rooms` | Nyilvános szobák listázása |
| `rejoin_room` | Újracsatlakozás tokennel (grace period alatt) |
| `start_game` | Játék indítása (owner only) |

### Játékmenet (kliens→szerver)
| Event | Leírás |
|---|---|
| `place_tiles` | Betűk lerakása a táblára |
| `exchange_tiles` | Betűk cseréje a zsákból |
| `pass_turn` | Kör passzolása |
| `accept_words` | Lerakás elfogadása / challenge elfogadás / elfogadó szavazat |
| `reject_words` | Lerakás elutasítása (2 játékos) / megtámadás indítása (3+ játékos) |
| `send_chat` | Chat üzenet küldése |
| `save_game` | Manuális mentés (owner only) |
| `restore_game` | Mentett játék visszaállítása (várakozó szoba létrehozás) |

### Szerver broadcast (szerver→kliens)
| Event | Leírás |
|---|---|
| `rooms_list` | Nyilvános szobák frissített listája |
| `room_joined` | Szobához csatlakozás megerősítése |
| `room_code` | 6-jegyű csatlakozási kód (csak a tulajdonosnak) |
| `game_state` | Teljes játékállapot (személyre szabva) |
| `game_started` | Játék elindult |
| `action_result` | Lerakás/csere/passz eredménye |
| `challenge_result` | Challenge/szavazás eredménye |
| `chat_message` | Chat üzenet broadcast |
| `player_joined` | Új játékos csatlakozott |
| `player_left` | Játékos kilépett |
| `player_disconnected` | Játékos kapcsolata megszakadt |
| `player_reconnected` | Játékos visszacsatlakozott |
| `room_disbanded` | Owner kilépett aktív játékból, szoba feloszlatva |
| `rejoin_failed` | Újracsatlakozás sikertelen |
| `error` | Hibaüzenet |

## Rate limiting

### Socket.IO eventek (per SID, `rate_limiter.py`)
```python
'set_name': (5, 10),        # 5 kérés / 10 mp
'create_room': (3, 30),     # 3 kérés / 30 mp
'join_room': (5, 10),
'place_tiles': (10, 10),
'exchange_tiles': (5, 10),
'pass_turn': (5, 10),
'get_rooms': (10, 5),
'accept_words': (5, 10),
'reject_words': (5, 10),
'send_chat': (10, 10),
'rejoin_room': (5, 10),
'save_game': (3, 30),
'restore_game': (3, 30),
```

### HTTP auth (IP-alapú, `config.py` → `AUTH_RATE_LIMITS`)
- `request_code`: 3 kérés / 300 mp
- `login`: 10 kérés / 300 mp
- `register`: 3 kérés / 3600 mp

## UI felépítés

### Képernyők
1. **Auth képernyő**: 3 tab (Bejelentkezés, Regisztráció, Vendég)
2. **Lobby**: szoba létrehozás (regisztráltaknak), kóddal csatlakozás, nyilvános szobák listája
3. **Várakozó szoba**: játékosok listája, challenge/privát/időlimit badge-ek, start gomb (owner)
4. **Játék képernyő**: bal panel (270px) + tábla + kéz

### Játék képernyő elrendezés
- **Bal oldali panel** (`side-panel`, 270px, sticky):
  - Pontszámok (scoreboard, aktív játékos kiemelve)
  - Játék infó (zsákban maradt zsetonok, aktuális játékos, utolsó akció)
  - Kör visszaszámláló (ha időlimit be van állítva, `TurnTimerUI`)
  - Challenge szekció (gombok, időzítő, szavazás — dinamikus)
  - Akciógombok 2×2-es rácsban: Lerak, Csere, Passz, Visszavon
  - Chat szekció (üzenetek + input mező)
- **Tábla terület**: 15×15 rács, premium mezők labelekkel, board-level event delegation drag & drop-hoz
- **Kéz**: 7 zseton, drag & drop / kattintásos elhelyezés, csere mód

### Téma rendszer
- Slate+Gold színpaletta mindkét módban
- Automatikus detektálás: `prefers-color-scheme` media query
- Manuális váltás: téma gomb (jobb felső sarok)
- Mentés: `localStorage('scrabble-theme')`
- CSS változók: `--bg-*`, `--text-*`, `--accent-*`, `--border-*` stb.

### Reszponzív design
- **Desktop**: side panel bal oldalon (270px sticky) + tábla középen
- **Tablet** (769-1024px): kisebb tábla (`min(60vw, 550px)`)
- **Mobil portrait** (<768px): panel alulra kerül (fixed), tábla teljes szélességű, pinch-to-zoom
- **Mobil landscape** (<768px landscape): egymás melletti elrendezés, panel 200px

## Hang rendszer

### Architektúra (`static/app.js`)
- `SoundManager` — hangszintetizátor Web Audio API-val; beállítások `localStorage('scrabble-sound')`-ban
- `SoundSettings` — beállítások UI; `.btn-sound-settings` osztályú gombok event delegationnel nyitják

### Hang kategóriák és triggerek
| Kategória kulcs | Label | Triggerek |
|---|---|---|
| `tile_place` | Betű lerakás | `placeTileOnBoard()`, blank dialog megerősítés |
| `vote` | Szavazás | Challenge szekció megjelenésekor (nem-rakónak), elfogad/elutasít gomb |
| `challenge_result` | Szavazás eredménye | `challenge_result` socket event (`challenge_won` alapján) |
| `your_turn` | Te következel | `game_state` event — `current_player` változás detektálása (`_prevCurrentPlayer`) |
| `chat` | Chat üzenet | `chat_message` event, csak más játékostól érkező üzenetnél |
| `game_events` | Játék események | `game_started` event → fanfár; `GameOver.show()` → záró motívum |

### localStorage séma
```json
{
  "volume": 0.65,
  "enabled": {
    "tile_place": true,
    "vote": true,
    "challenge_result": true,
    "your_turn": true,
    "chat": true,
    "game_events": true
  }
}
```

## Konstansok

### game.py
- `HAND_SIZE = 7`
- `BONUS_ALL_TILES = 50`
- `CHALLENGE_TIMEOUT = 30` (mp)

### server.py
- `_DISCONNECT_GRACE_PERIOD = 120` (mp)
- `ALLOWED_TURN_TIME_LIMITS = {0, 60, 90, 120, 180, 300}`

### config.py
- `DB_PATH = 'scrabble.db'` (vagy `SCRABBLE_DB_PATH` env var)
- `SESSION_MAX_AGE_DAYS = 30`
- `VERIFICATION_CODE_EXPIRY_MINUTES = 10`
- `VERIFICATION_MAX_ATTEMPTS = 5`
- `SMTP_CONFIGURED` — bool, automatikusan kalkulált
- `AUTH_RATE_LIMITS` — dict, IP-alapú rate limit konfigok

## Játék mentés / visszatöltés

### Adatbázis táblák
- **`saved_games`**: id, room_id, room_name, state_json, status ('active'/'finished'/'abandoned'), challenge_mode, owner_name, owner_token, created_at, updated_at
- **`game_players`**: id, game_id FK, user_id FK (NULL vendégnél), player_name, final_score, is_winner
- **`game_moves`**: id, game_id FK, move_number, player_name, action_type, details_json, board_snapshot_json, created_at

### Mentési logika
- **Kezdeti mentés**: A játék indításakor (`start_game`) a rendszer automatikusan menti a teljes játékoslistát
- **Manuális mentés (owner-only)**: A szoba tulajdonosa bármikor mentheti a játékot a kilépés menüből ("Mentés és kilépés")
- **Automatikus mentés**: Ha a szoba tulajdonosa (owner) végleg lecsatlakozik (120 mp grace period lejár), a rendszer automatikusan menti a játékállást feloszlatás előtt
- **Roster megőrzés**: A mentés minden játékost tartalmaz, a lecsatlakozottakat is
- **Folyamatos lépés-naplózás**: Minden sikeres lerakás/csere után board snapshot és move rögzítés történik (`_record_move()`)
- Játék befejezésekor (természetes vég) automatikusan mentődik `finish_game()` hívással

### Lobby-first restore flow
- `restore_game` Socket.IO event: a mentés tulajdonosa privát várakozó szobát hoz létre
- `room.expected_players` tartalmazza a mentett játékosok neveit
- Csak az elvárt nevű játékosok csatlakozhatnak (név-alapú validáció)
- A várakozó szoba mutatja mely játékosok csatlakoztak és kik hiányoznak
- Owner indítja a játékot → `Game.from_save_dict()` visszaállítja az állapotot
- **Aki nincs ott a kezdésnél**, az `disconnected=True` státusszal kerül a játékba, és bármikor visszacsatlakozhat menet közben

### Kilépés menü
- Topbar-ban kilépés gomb → megerősítő dialog
- **Owner** (aktív játék): "Mentés és kilépés" / "Kilépés mentés nélkül" / "Mégsem"
- **Nem-owner** (vagy befejezett játék): "Kilépés" / "Mégsem"
- "Mentés és kilépés": `save_game` emit, majd `leave_room`
- Aktív játéknál a kilépés csak `disconnected` állapotba teszi a játékost, nem távolítja el végleg

### Profil oldal
- `GET /api/auth/profile` — statisztikák + utolsó 20 befejezett játék
- `GET /api/game/<id>/moves` — lépések listája replay-hez
- Visszajátszás: lépésenkénti navigáció board snapshot-okkal

## Ismert problémák / TODO

### Játékmenet
- [x] Challenge rendszer — szó megkérdőjelezése más játékos által (30 mp ablak, megtámadás/elfogadás)
- [x] Játék mentés / visszatöltés — manuális mentés (owner-only), lobby-first restore flow
- [x] Visszajátszás — befejezett játék lépéseinek visszanézése
- [x] Időlimit a körökre — opcionális időzítő (0/60/90/120/180/300 mp), lejáratkor automatikus passz
- [ ] AI ellenfél — egyjátékos mód számítógépes ellenfél(ek)kel, nehézségi szintek

### Közösségi funkciók
- [x] Chat — játék közbeni szöveges üzenetküldés a játékosok között
- [x] Privát szobák — 6-jegyű kóddal csatlakozás, lobby-ban nem listázott szobák
- [x] Játékos profil oldal — saját statisztikák, játékelőzmények megtekintése, visszajátszás
- [ ] Spectator mód — folyamatban lévő játék megfigyelése játékos nélkül
- [ ] Ranglista / leaderboard — regisztrált játékosok összesített statisztikái
- [ ] Barátlista / meghívó rendszer — közvetlen meghívás barátoknak

### Hálózat
- [x] Újracsatlakozás (grace period) — 120 mp-es ablak a visszacsatlakozásra játék közben
- [x] Pinch-to-zoom — mobilos tábla nagyítás/kicsinyítés

### UI / UX
- [x] Sötét / világos téma váltás — Slate+Gold paletta, auto-detektálás, localStorage mentés
- [x] Hang effektek — Web Audio API, 8 szintetizált hang, hangerő csúszka + kategóriánkénti kapcsolók
- [x] Szótár-böngésző (Challenge fázis) — szavazásnál kattintható szavak keresése
- [ ] Animációk — betű lerakás, pontszám felugró, kör váltás animáció
- [ ] Szótár-böngésző (kereső/validáló)
- [ ] PWA támogatás — offline mód, alkalmazásként telepíthető
- [ ] Többnyelvű felület — angol és egyéb nyelvű UI (a szótár marad magyar)
- [ ] Drag & drop vizuális visszajelzés javítása — foglalt cellák jelölése drop közben
