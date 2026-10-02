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
- `server.py` — Flask + SocketIO szerver, lobby/szoba kezelés, Cloudflare tunnel integráció, Socket.IO event handlerek, reconnection grace period, robotlépések (`_schedule_bot_turn` / `_play_bot_turn`), megfigyelők, előnézet, tipp
- `game.py` — Játéklogika (Game osztály), körök, pontozás, játék vége, challenge rendszer, kör időlimit, robotok (`add_bot`), szerkezetes `last_action_info`, lépéstörténet (`get_history`), előnézet (`preview_placement`)
- `player.py` — Player osztály (id, név, kéz, pontszám, disconnected állapot, `is_bot`, `difficulty`)
- `ai_player.py` — Robot ellenfél: szókincs (a szótár tőszavai), horgonyalapú lépésgenerátor, nehézségi szintek, tippek
- `board.py` — 15×15 tábla, premium mezők, szó elhelyezés validáció és pontozás
- `dictionary.py` — Magyar szótár-ellenőrzés a beágyazott `affix_checker`-rel (nincs rendszerfüggőség), magánhangzó nélküli rövidítések kizárása, `is_available`/`warm_up`, tömeges `filter_valid` (gyorsítótárral), `suggest_words` (egy betűnyi szerkesztés)
- `affix_checker.py` — Tisztán Python, Hunspell-szerű szóellenőrző a `dict/hu_HU.{aff,dic}` fájlokhoz: szótő, előtag, legfeljebb két toldalék, folytatási osztályok (AF aliasok), NEEDAFFIX/ONLYINCOMPOUND/FORBIDDENWORD; **összetételi szabályok nélkül**
- `tiles.py` — Magyar betűkészlet (100 zseton), TileBag osztály, `tokenize_word` (szó → zsetonok)
- `challenge.py` — Challenge (megtámadás) logika, szavazási állapotgép, vote resolution
- `room.py` — Room osztály (szoba állapot, owner, beállítások, chat, timer invalidálás, megfigyelők, robotlépés-azonosító)
- `state.py` — ServerState singleton (szobák, játékosok, tokenek, reconnect tracking, megfigyelők, élő játékok)
- `routes.py` — Flask blueprint-ek: auth, game, public (ranglista, szótár-ellenőrző), main (index + PWA: `/manifest.webmanifest`, `/sw.js`)
- `config.py` — SMTP, auth, DB, rate limit konfigurációs konstansok (`os.environ`-ból)
- `auth.py` — SQLite DB kezelés, regisztráció, login, session, jelszó hash (PBKDF2), játék mentés/visszatöltés/lépésnaplózás
- `email_service.py` — 6 számjegyű kód generálás, SMTP küldés (háttérszálon)
- `rate_limiter.py` — Generikus rate limiter Socket.IO (SID) és HTTP (IP) endpointokhoz
- `socket_auth.py` — Aláírt, rövid életű token a Socket.IO identitás igazolásához (`set_name`)
- `tunnel.py` — Cloudflare tunnel subprocess kezelés (indítás/leállítás)
- `dict/` — Beágyazott hu_HU hunspell szótár fájlok (hu_HU.dic, hu_HU.aff)
- `templates/index.html` — Egyoldalas UI: auth (3 tab), lobby (5 fül), várakozó szoba, játék, profil, visszajátszás; közös SVG ikon-sprite, minden képernyőn egységes felső sáv (`app-topbar`); minden szöveg `data-i18n*` jelölésű
- `templates/sw.js` — Service worker (Jinja sablon, `VERSION` = kliens fájlok mtime-ja)
- `static/app.js` — Kliens logika, drag & drop, pinch-to-zoom, Socket.IO kommunikáció, auth flow, téma váltás, hang rendszer (SoundManager, SoundSettings), megfigyelő mód, ranglista, szótár-böngésző, előnézet, zsetonszámláló, tipp, gyorsbillentyűk, PWA telepítés
- `static/i18n.js` + `static/i18n-data.js` — Többnyelvű felület: `t()`, `tServer()`, `I18N.setLang()`; a fordítások (hu/en) szigorú JSON-ban
- `static/style.css` — Apple HIG ihletésű design rendszer (tokenek, iOS-szerű komponensek), sötét/világos téma, reszponzív layout (asztali / tablet / telefon, álló és fekvő), 17. szakasz: új funkciók és animációk
- `static/manifest.webmanifest`, `static/offline.html`, `static/icons/` — PWA
- `tests/` — Tesztek (pytest, 879 teszt)
- `requirements.txt` — Python függőségek (flask, flask-socketio, eventlet)
- `.venv/` — Virtual environment

## Funkciók
- 1-4 játékos (egyedül is játszható)
- **Robot ellenfelek**: 1–3 robot, könnyű / közepes / nehéz; egyedül játszva tipp (3 legjobb lépés, szobánként állítható / kikapcsolható limit: 0/1/3/5/10); robotos játék nem számít a ranglistába
- **Megfigyelő mód**: nyilvános, folyamatban lévő játék megfigyelése (lobby „Élő játékok”), privát játék kóddal
- **Ranglista**: győzelmek / nyerési arány / átlagpont / legjobb játék (csak regisztrált, robot nélküli, befejezett játékok)
- **Szótár-böngésző**: szó-ellenőrző párbeszéd (érvényes-e, pontérték, zsetonok, javaslatok)
- **Többnyelvű felület**: magyar + angol (`localStorage('scrabble-lang')`), a szótár magyar marad
- **PWA**: telepíthető, kapcsolat nélkül induló felület (service worker), ikonok, manifest
- **Animációk**: lerakás, ellenfél lépése, pontszám felugró, kör váltás, drag & drop visszajelzés (foglalt mező piros)
- **Kényelmi funkciók**: élő pontszám-előnézet · zsetonszámláló · betűtartó keverés/rendezés + gyorsbillentyűk · meghívó link (`/?join=KÓD`) · lépéstörténet + utolsó lépés kiemelése + legjobb lépés a játék végén
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
- **Sötét / világos téma**: automatikus detektálás (`prefers-color-scheme`), manuális váltás, `localStorage`-ban mentve, villanásmentes betöltés
- **Hang effektek**: Web Audio API (nincs külső fájl), szintetizált hangok — betű lerakás, szavazás, challenge eredmény, kör értesítő, chat, játék kezdés/vége; hangerő-csúszka + kategóriánkénti kapcsolók, `localStorage`-ban mentve
- **Újracsatlakozás (grace period)**: 120 másodperc a visszacsatlakozásra ha a kapcsolat megszakad játék közben (token alapú)
- **Pinch-to-zoom**: mobilon a tábla nagyítható/kicsinyíthető csípő mozdulattal
- **Szótár-böngésző (Challenge fázis)**: A megtámadás során a lerakott szavak kattintható linkek, amelyek egy új lapon indítanak Google keresést az adott szóra ("A magyar nyelv értelmező szótára" fókusszal).
- Szótár-ellenőrzés: beágyazott hu_HU szótár (`affix_checker.py`, tisztán Python — ugyanúgy működik fejlesztői gépen, Windowson és tárhelyen; korábban a pyenchant/hunspell hiánya miatt a szerveren minden szó érvényesnek látszott). A szavakat kisbetűvel keresi, így a tulajdonnevek (pl. DUNA, BUDAPEST) nem érvényesek; a Hunspell összetételi szabályait nem használja (értelmetlen összetételeket, pl. PAGONYAGY, nem fogad el); a magánhangzó nélküli tételek (KG, DB, TV, betűnevek) nem érvényesek, az indulatszavak (BRR, HM, PSZT) igen. Ha a szótár nem tölthető be, a Szótár-eszköz 503-at ad (nem jelöl érvényesnek semmit)
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
- Dictionary sanitizálás: szavak regex-szel validálva a szótár-keresés előtt
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
- `POST /api/auth/register` — jelszó + név, fiók létrehozás, auto-login (csak a kóddal előzőleg megerősített email címre, 30 percig érvényes, egyszer használható)
- `POST /api/auth/login` — email + jelszó
- `POST /api/auth/logout` — session törlés
- `GET /api/auth/me` — session cookie ellenőrzés
- `GET /api/auth/profile` — statisztikák és játékelőzmények (session cookie)
- `GET /api/auth/socket-token` — rövid életű (5 perc) aláírt token a Socket.IO `set_name`-hez (session cookie)
- `GET /api/game/<int:game_id>/moves` — lépések listája (replay-hez)
- `GET /api/leaderboard?metric=wins|win_rate|avg_score|best_game&limit=50` — ranglista (nyilvános; bejelentkezve a saját helyezés is: `me`, `is_me`)
- `GET|POST /api/dictionary/check` (`q` / `words`, max. 8 szó) — szó-ellenőrzés: `valid`, `tiles`, `score`, `reason`, `suggestions`
- `GET /manifest.webmanifest`, `GET /sw.js` — PWA (a service worker a gyökérről, `Service-Worker-Allowed: /`)

Session cookie: `HttpOnly` + `SameSite=Lax` + `Secure` (Cloudflare tunnel HTTPS).
IP-alapú rate limiting (`rate_limiter.py`): kód küldés 3/5perc, login 10/5perc, regisztráció 3/óra, ranglista 30/perc, szótár 60/perc.

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
| `tests/test_server_auth.py` | 52 | HTTP auth route-ok, cookie flow |
| `tests/test_server_socket.py` | 67 | Socket.IO eventek, lobby, szobák, privát szobák, challenge szavazás, chat, owner kilépés, kör időlimit |
| `tests/test_challenge.py` | 17 | Challenge szavazásos rendszer |
| `tests/test_dictionary.py` | 59 | Szótár-ellenőrzés (valódi szótárral: kisbetűs keresés, tulajdonnevek, rövidítések, értelmetlen szavak pl. SALYT), elérhetőség, javaslatok, tábla-validáció |
| `tests/test_affix_checker.py` | 39 | Beépített szóellenőrző: toldalékok, előtagok, folytatási osztályok, speciális jelzők, a valódi szótár (hunspellel összevetve) |
| `tests/test_email_service.py` | 4 | Email küldés |
| `tests/test_room.py` | 12 | Room osztály |
| `tests/test_friends.py` | 27 | Barát CRUD, kérések, felhasználókeresés, szobameghívó, online státusz |
| `tests/test_timer_and_replay.py` | 27 | Kör időlimit, replay perzisztencia |
| `tests/test_regressions.py` | 70 | Kódátvizsgálás során talált hibák: zsák, dupla cella, passz-végjáték, mentés szavazás közben, IP rate limit, e-mail megerősítés, socket-token, session átvétel, visszaállítás/késői csatlakozás |
| `tests/test_ai_player.py` | 39 | Robot motor: szókincs, lépésgenerátor (pontszám = játék pontozása), nehézségi szintek, csere/passz, tipp |
| `tests/test_bots.py` | 106 | Robotok a játékmodellben (szavazás, mentés), szerver (lépés, ütemezés, tipp, előnézet), `last_action_info`, lépéstörténet |
| `tests/test_spectator.py` | 30 | Megfigyelő mód, élő játékok, szoba életciklus |
| `tests/test_public_api.py` | 52 | Ranglista (DB + route, robotos játékok kizárása), szótár API, PWA végpontok |
| `tests/test_tiles_dictionary.py` | 30 | `tokenize_word`, `filter_valid`, `suggest_words` |
| `tests/test_i18n.py` | 27 | Fordítások teljessége, szerverüzenet-lefedettség (AST), HTML lefedettség, a fordító futtatása node-ban |
| `tests/test_frontend_consistency.py` | 20 | Kliens ↔ szerver: konstansok (TILE_VALUES, premium mezők), elem-azonosítók, Socket.IO események, API útvonalak, JS szintaxis |

**Összesen: 879 teszt**

Fixture: `tests/conftest.py` — temp_db (auto-applied, ideiglenes SQLite DB minden teszthez)
Segédek: `tests/helpers.py` — `registered_set_name_payload()` (érvényes socket-tokennel), `verify_email()`

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
| `set_name` | Játékosnév / auth adatok beállítása. Regisztrált felhasználónál az `auth_token` (lásd `/api/auth/socket-token`) kötelező: a kliens által küldött `user_id` önmagában nem elég, a név a fiókból jön. Érvénytelen token → vendég + hibaüzenet |
| `logout` | Kijelentkezés: kilépés a szobából, online azonosság törlése |
| `create_room` | Szoba létrehozása (név, max_players, challenge_mode, is_private, turn_time_limit, `ai_players`: nehézségek listája, max. 3 és `max_players-1`, `hint_limit`: 0/1/3/5/10, alapért. 3) |
| `join_room` | Csatlakozás kóddal vagy room_id-val |
| `leave_room` | Szoba elhagyása |
| `get_rooms` | Nyilvános szobák listázása |
| `rejoin_room` | Újracsatlakozás tokennel (grace period alatt, vagy a még „élőnek” hitt régi kapcsolat átvételével — pl. háttérbe került telefon, újratöltött oldal) |
| `start_game` | Játék indítása (owner only) |
| `spectate_room` | Megfigyelés: `{room_id}` (nyilvános) vagy `{code}` (privát is); csak folyamatban lévő játék |
| `leave_spectate` | Kilépés a megfigyelésből |

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
| `preview_move` | Lerakás kipróbálása véglegesítés nélkül `{tiles}` → `move_preview` (csendben rate limitelt) |
| `request_hint` | Tipp kérése (csak ha egyetlen emberi játékos van, a szobában engedélyezett és van még tipp) → `hint_result` `{success, message, hints_left, moves}`; csak akkor fogy, ha van javasolt lépés |

### Szerver broadcast (szerver→kliens)
| Event | Leírás |
|---|---|
| `rooms_list` | Nyilvános szobák frissített listája |
| `live_games` | Nyilvános, folyamatban lévő játékok (megfigyeléshez): név, játékosok pontokkal, néző-szám |
| `spectate_joined` / `spectate_left` | Megfigyelés megkezdése / vége |
| `move_preview` | `{valid, score, words:[{word,score}], message}` |
| `hint_result` | `{success, message, moves:[{tiles, words, score}]}` |
| `room_joined` | Szobához csatlakozás megerősítése |
| `room_code` | 6-jegyű csatlakozási kód (csak a tulajdonosnak) |
| `game_state` | Teljes játékállapot (személyre szabva; megfigyelőnek kéz nélkül, `spectator: true`; `history`, `last_move_tiles`, `last_action_info`, `spectator_count`) |
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
'preview_move': (30, 10),   # csendben eldobva
'request_hint': (3, 30),
'spectate_room': (5, 10),
'leave_spectate': (5, 10),
```

### HTTP auth (IP-alapú, `config.py` → `AUTH_RATE_LIMITS`)
- `request_code`: 3 kérés / 300 mp
- `login`: 10 kérés / 300 mp
- `register`: 3 kérés / 3600 mp

## UI felépítés

### Képernyők
1. **Auth képernyő**: 3 tab (Bejelentkezés, Regisztráció, Vendég), lebegő téma gomb
2. **Lobby**: középre igazított szegmentált navigáció (Kezdőlap / Új szoba / Mentett játékok / Barátok / Ranglista), szoba létrehozás (regisztráltaknak; robotok száma + nehézsége), kóddal csatlakozás / megfigyelés, nyilvános szobák és élő játékok listája
3. **Várakozó szoba**: badge-ek (challenge/privát/időlimit), csatlakozási kód, játékoslista, start gomb (owner)
4. **Játék képernyő**: info panel + tábla + betűtartó (lásd lent)
5. **Profil** és **Visszajátszás**

Minden képernyőn (az auth kivételével) ugyanaz a **sticky felső sáv** (`.app-topbar`) látszik: vissza gomb + cím balra, profil / hang / téma / kijelentkezés jobbra. **Kivétel: a játék képernyő álló nézetben** (telefon és tablet) **és fekvő érintőképernyős telefonon** — ott a hely szűkös, ezért nincs felső sáv: a szobanév és a gombok (kilépés · profil · hang · téma · kijelentkezés) egyetlen sorban, a `.game-nav` menüsorban vannak az alsó, görgethető panelen belül. Asztali gépen és fekvő tableten a játékban is marad a felső sáv. `env(safe-area-inset-*)` kezeli a notchot és a home indicatort (`viewport-fit=cover`).

### Játék képernyő elrendezés
Három elrendezés, CSS media query-kkel (`static/style.css` 11–13. szakasz):

- **Alap (fekvő, asztali gép, fekvő tablet)** — két oszlop: bal oldali `side-panel` (300px, sticky, saját görgetéssel) + tábla és betűtartó. A tábla mérete (`--board-size`) a képernyő magasságából is számolódik, így tábla + betűtartó görgetés nélkül elfér.
- **Álló (`orientation: portrait`: telefon, tablet álló)** — fent a tábla és a betűtartó, alul **fix, görgethető panel** (`.side-panel`, lekerekített felső sarkokkal): pontszámok → infó → időzítő → lépés-gombok (**egyetlen sorban**) → menüsor (szobanév + gombok) → chat. Aktív szavazásnál a challenge szekció a panel tetejére kerül (`order: -1`), és új szavazásnál a panel a tetejére görget. **A panel soha nem takarhatja el a táblát vagy a zsetonokat:** előbb a tábla mérete (`--board-size`) számolódik a szélességből és abból, hogy a betűtartóval együtt elférjen a minimális panel (`--panel-min`) fölött; a panel magassága (`--panel-h`) a maradék hely (max. 480px), és a tartalom alatt pontosan ennyi hely van fenntartva. Az oldal így nem is görgethető. A betűtartó mérete (`--tile`) csak a szélességtől függ (nincs körkörös függés).
- **Fekvő telefon (`orientation: landscape` és `max-height: 540px`)** — kompakt két oszlop, a betűtartó függőlegesen a tábla mellett; érintőképernyős telefonon a menüsor az oldalpanel tetején van (felső sáv nélkül, így nagyobb a tábla).
- **Chat telefonon**: a chat a panel alján van; ha a chat ablak nem látszik (`Chat._isVisible()`), a másik játékos üzenete értesítésként (toast) is megjelenik.

A panel tartalma: megfigyelő sáv (csak megfigyelőnek), pontszámok (aktív játékos kiemelve, robotok ikonnal), játék infó (zsák, aktuális játékos, utolsó akció — a `last_action_info` alapján lokalizálva), kör visszaszámláló (`TurnTimerUI`), challenge szekció (dinamikus), akciógombok, élő előnézet, eszközök (keverés, rendezés, zsetonok, tipp), lépéstörténet (`<details>`), chat. Megfigyelőként (`#game-screen.spectating`) a lépés-/chatgombok és a betűtartó rejtett.

A tábla cellái `container-type: inline-size` + `cqw` egységekkel méreteződnek (betű, érték, premium felirat), a rövid premium felirat (`2× BETŰ`) a tábla szélességétől függően jelenik meg (`@container board`). Többkarakteres betűknél (SZ, CS...) a JS `long-letter` osztályt ad.

### Téma rendszer
- iOS-szerű paletta (`#007AFF` / sötét módban `#0A84FF`), rendszerfont-stack (SF Pro az Apple eszközökön, Inter fallback)
- Automatikus detektálás: `prefers-color-scheme`; a `<head>` inline szkriptje a megjelenítés előtt beállítja a `data-theme` attribútumot a `<html>` elemen (nincs villanás)
- Manuális váltás: `.btn-theme-toggle` osztályú gombok (egy közös, delegált kezelő: `toggleTheme()`)
- Mentés: `localStorage('scrabble-theme')`; a `<meta name="theme-color">` is frissül
- CSS változók: `--bg-*`, `--text-*`, `--accent*`, `--border-*`, `--color-fill*`, `--radius-*`, `--space-*`, `--text-*` méretek, `--z-*` rétegek

### Komponens-konvenciók
- Gombok: alap = kitöltött kiemelt; `.secondary` (szürke), `.tinted` (halvány kiemelt), `.danger` (piros), `.link-btn`, `.small-btn`. A változatok a `--btn-bg` / `--btn-bg-hover` / `--btn-fg` változókat állítják. A `:hover` csak `(hover: hover)` eszközön él (iPaden nincs "beragadt" hover).
- Párbeszédek: `.dialog` + `.dialog-content`; `.dialog-sheet` telefonon (≤600px) alsó lapként (bottom sheet) jelenik meg. Esc és háttérre koppintás bezárja (`Dialogs` a `app.js`-ben). A "Mégsem" gomb mindig `.secondary`.
- Kapcsolók: iOS-stílusú `.toggle-switch`; csoportosított beállítások `.settings-group`.
- Listasorok (szoba, mentett játék, előzmény, barát) közös stílust kapnak; üres állapot: `.empty-state > .empty-msg`.

### Reszponzív design
- **Asztali gép** (>900px): lobby fejléc egy sorban (cím · navigáció · gombok), játék két oszlopban
- **Keskeny ablak / tablet** (≤900px): a lobby navigáció új sorba kerül, teljes szélességű szegmentált vezérlő
- **Telefon** (≤600px): kompaktabb fejléc, 16px-es beviteli mezők (iOS nem nagyít rá), a nagyobb párbeszédek alsó lapok, listasorok egymás alá törnek
- **Nagyon keskeny** (≤374px): kisebb gombok és betűméretek
- **Pinch-to-zoom**: érintőképernyőn a tábla nagyítható/kicsinyíthető csípő mozdulattal
- `100dvh` (mobil böngésző címsor), `prefers-reduced-motion` tiszteletben tartva

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
- `MAX_BOTS = 3`, `_BOT_THINK_DELAY` (szintenként min/max mp), `_BOT_NAMES`
- `Room.MAX_SPECTATORS = 30`

### game.py (tippek)
- `ALLOWED_HINT_LIMITS = (0, 1, 3, 5, 10)`, `DEFAULT_HINT_LIMIT = 3`; `Game.hint_limit` / `hints_used` (mentésbe kerül, régi mentésnél az alapérték), `hints_left()`, `use_hint()`; az állapotban `hint_limit`, `hints_left`

### ai_player.py
- `DIFFICULTIES = ('easy', 'medium', 'hard')`, `_TIME_BUDGET` (1.0 / 2.0 / 3.5 mp keresési időkeret)

### auth.py
- `LEADERBOARD_METRICS`, `LEADERBOARD_MIN_GAMES` (`win_rate`, `avg_score`: 3), `LEADERBOARD_MAX_LIMIT = 100`

### config.py
- `DB_PATH = 'scrabble.db'` (vagy `SCRABBLE_DB_PATH` env var)
- `SESSION_MAX_AGE_DAYS = 30`
- `VERIFICATION_CODE_EXPIRY_MINUTES = 10`
- `VERIFICATION_MAX_ATTEMPTS = 5`
- `SMTP_CONFIGURED` — bool, automatikusan kalkulált
- `AUTH_RATE_LIMITS` — dict, IP-alapú rate limit konfigok

## Játék mentés / visszatöltés

### Adatbázis táblák
- **`saved_games`**: id, room_id, room_name, state_json, status ('active'/'finished'/'abandoned'), challenge_mode, owner_name, owner_token, `has_bots` (robot is volt a játékban → a ranglista nem számolja; migrációval kerül a régi adatbázisokba), created_at, updated_at
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
- **Aki nincs ott a kezdésnél**, az `disconnected=True` státusszal kerül a játékba, és a szoba kódjával (`join_room`) bármikor becsatlakozhat menet közben — csak a saját fiókjával (vendég hely: vendégként, azonos névvel). Ha a soron lévő játékos hiányzik, a kör továbbadódik
- A visszaállított játék új mentést kap, a korábbi lépések átkerülnek (a visszajátszás teljes marad)
- Szavazás közben mentett játékban a függő lerakás betűi visszakerülnek a lerakó kezébe (nem vesznek el zsetonok)

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

## Robot ellenfelek (AI)

### Játékmodell
- `Player.is_bot` / `Player.difficulty`; id: `bot-<szoba>-<n>` (nem SID, a mentésben is stabil). `Game.add_bot()` csak indítás előtt, max. 4 játékos összesen.
- A robotok **nem szavaznak** (`_get_voter_ids`): a megtámadás (`_challenge_applies`) csak akkor él, ha van másik **emberi** játékos. Ha nincs, a lerakást a szótár ellenőrzi.
- `Game.get_all_states()` nem készít állapotot robotnak; a robot sosem `disconnected` (`from_save_dict` is kényszeríti). `human_players()` / `has_connected_human()` a szoba életciklusához: ember nélkül a szoba megszűnik, a robotok nem lépnek (néző jelenlétében igen).
- Restore: a `expected_players` csak embereket tartalmaz, a robotok az indításkor automatikusan visszakerülnek.

### Szerver (`server.py`)
- `_schedule_bot_turn(room_id)` — minden olyan ponton hívódik, ahol a kör továbbadódhat (lerakás/csere/passz, szavazás lezárása, időzítők, újracsatlakozás, indítás, megfigyelő csatlakozása). A `room.invalidate_bot_turn()` azonosítóval a régi ütemezések érvénytelenednek.
- `_play_bot_turn(room_id, turn_id, turn_number)` — a keresés alatt (`yield_fn=socketio.sleep(0)`) az állapot megváltozhat, ezért a lépés előtt újraellenőriz. Sikertelen lépés → passz, hogy a játék ne akadjon el. A tesztek ezt a függvényt közvetlenül hívják (`start_background_task` kikapcsolva).
- `_emit_all_states` már `socketio.emit`-et használ, így háttérszálból is hívható.

### Motor (`ai_player.py`)
- Szókincs: `dict/hu_HU.dic` tőszavai (csak kisbetűs, magánhangzót tartalmazó, 2–15 betű; ~68 000 szó), rendezett lista + `bisect` prefixkereséssel (nincs trie: kevés memória).
- `generate_moves`: Appel–Jacobson horgonykeresés vízszintesen és (átfordított rácson) függőlegesen; a többkarakteres zsetonok (SZ, CS...) több karaktert lépnek a prefixben; joker bármely betű. A keresztszavak érvényességét **egyetlen** `filter_valid` hívás dönti el az egész táblára. A pontozás a játék saját `Board.validate_placement`-jével történik egy privát másolaton.
- Szintek: könnyű (≤4 zseton, a gyengébb fél), közepes (felső ~30% véletlen), nehéz (`equity` = pont + `leave_value`, végjátékban a kézben maradó zsetonok levonva). A kiválasztott lépés minden szavát `_first_valid` ellenőrzi a játék szótárával; ha nincs lépés: csere (zsák ≥ 7) vagy passz.
- A keresés időkerete szintenként korlátos (`_TIME_BUDGET`); lejártakor a addigi legjobbal dolgozik.

## Megfigyelő mód

- `Room.spectators {sid: név}` + `ServerState.spectator_rooms {sid: room_id}`; a megfigyelő a Socket.IO szobába is belép (chat, események), de a `player_rooms`-ban nem szerepel, így a játékos-eventek (lerakás, chat küldés...) hatástalanok.
- `_emit_all_states` a megfigyelőknek `Game.get_spectator_state()`-et küld (kezek nélkül, `spectator: true`); minden játékos állapotában `spectator_count`.
- Életciklus: `disconnect`/`logout`/`leave_spectate` eltávolítja; `_cleanup_room` és `_disband_active_room` `room_disbanded`-et küld nekik. Kliens oldalon újracsatlakozáskor `Spectate.resume()` újraindítja a megfigyelést.
- A `get_rooms` a `live_games` eseményt is kiküldi; játék indításakor/végén broadcast.

## Ranglista

- A listát `auth.get_leaderboard()` a `game_players` ⨝ `saved_games(finished, has_bots=0)` ⨝ `users` összesítéséből számolja (nem a `users` számlálóiból), így a robotos játékok nem számítanak. A rendezés SQL-részlete rögzített (`_LEADERBOARD_ORDER`), a metrika fehérlistás.

## Többnyelvű felület (i18n)

- `static/i18n-data.js`: `window.I18N_DATA = {hu: {...}, en: {..., server: {exact, patterns}}}` — szigorú JSON (a tesztek `json.loads`-szal olvassák). Kulcsok `névtér.kulcs`; `_one` végű kulcs az angol egyes számhoz (`params.n === 1`).
- HTML: `data-i18n` (szöveg), `data-i18n-placeholder`, `data-i18n-title` (title + aria-label), `data-i18n-aria`. JS: `t('kulcs', {n})`, a szerver magyar üzeneteihez `tServer(msg)`; nyelvváltáskor `langchange` esemény, amire a modulok újrarajzolják a gyorsítótárazott adataikat.
- A szerver szerkezetes `last_action_info`-t is küld; a kliens ebből formázza a „utolsó akció” szöveget (a magyar `last_action` szöveg tartalék).
- Új felhasználói szöveg hozzáadásakor: kulcs a hu **és** en blokkba, `data-i18n`/`t()` a kódban; a `tests/test_i18n.py` hibára fut, ha hiányzik fordítás, nem használt kulcs marad, vagy egy szerver-üzenet fordítatlan.

## PWA

- `/sw.js` (`templates/sw.js`): install → váz előtöltése (`SHELL_URLS`), activate → régi gyorsítótárak törlése, fetch: navigáció hálózat-először (offline: gyorsítótárazott `/`, végső esetben `offline.html`); `/socket.io/` és `/api/` soha; statikus fájlok és a CDN-es Socket.IO/betűtípus stale-while-revalidate.
- `asset_version()` (`routes.py`) a kliens fájlok mtime-ja: az `index.html` `?v=` paramétere és a SW `VERSION`-je ugyanez.
- Telepítés gomb: `beforeinstallprompt` (iOS Safari-n kézi útmutató).

## Animációk és húzás-visszajelzés

- `GameBoard.renderBoard` az előző rajzoláshoz képest különbséget számol: új betű → `tile-pop` (saját lerakás) / `tile-drop` (más lépése); utolsó lépés: `last-move`; pontszám-változás: `.score-pop`; kör váltás: `turn-pulse`; saját kör: `.my-turn` fény a táblán.
- Húzás közben a tábla `is-dragging` osztályt kap, a foglalt mezők csíkozva; `dragenter` **és** `dragover` elfogadása szükséges (különben a böngésző a `body`-t teszi céllá, és a mezők nem kapnak eseményt). A `prefers-reduced-motion` az összes animációt kikapcsolja.

## Ismert problémák / TODO

### Játékmenet
- [x] Challenge rendszer — szó megkérdőjelezése más játékos által (30 mp ablak, megtámadás/elfogadás)
- [x] Játék mentés / visszatöltés — manuális mentés (owner-only), lobby-first restore flow
- [x] Visszajátszás — befejezett játék lépéseinek visszanézése
- [x] Időlimit a körökre — opcionális időzítő (0/60/90/120/180/300 mp), lejáratkor automatikus passz
- [x] AI ellenfél — egyjátékos mód számítógépes ellenfél(ek)kel, nehézségi szintek (a robot a tőszavakból épít; ragozott főszavak nem generálódnak)

### Közösségi funkciók
- [x] Chat — játék közbeni szöveges üzenetküldés a játékosok között
- [x] Privát szobák — 6-jegyű kóddal csatlakozás, lobby-ban nem listázott szobák
- [x] Játékos profil oldal — saját statisztikák, játékelőzmények megtekintése, visszajátszás
- [x] Spectator mód — folyamatban lévő játék megfigyelése játékos nélkül
- [x] Ranglista / leaderboard — regisztrált játékosok összesített statisztikái (robot nélküli játékok)
- [x] Barátlista / meghívó rendszer — közvetlen meghívás barátoknak

### Hálózat
- [x] Újracsatlakozás (grace period) — 120 mp-es ablak a visszacsatlakozásra játék közben
- [x] Pinch-to-zoom — mobilos tábla nagyítás/kicsinyítés

### UI / UX
- [x] Sötét / világos téma váltás — Slate+Gold paletta, auto-detektálás, localStorage mentés
- [x] Hang effektek — Web Audio API, 8 szintetizált hang, hangerő csúszka + kategóriánkénti kapcsolók
- [x] Szótár-böngésző (Challenge fázis) — szavazásnál kattintható szavak keresése
- [x] Animációk — betű lerakás, pontszám felugró, kör váltás animáció
- [x] Szótár-böngésző (kereső/validáló)
- [x] PWA támogatás — offline váz, alkalmazásként telepíthető
- [x] Többnyelvű felület — magyar és angol UI (a szótár marad magyar)
- [x] Drag & drop vizuális visszajelzés javítása — foglalt cellák jelölése drop közben
- [x] Kényelmi funkciók — élő előnézet, zsetonszámláló, keverés/rendezés + gyorsbillentyűk, meghívó link, lépéstörténet

### Ötletek
- [ ] Robot: ragozott főszavak (tőszó + toldalék szótár-ellenőrzéssel), tapasztalati értékelés (szimuláció)
- [ ] Több nyelv a felületen (a `i18n-data.js` blokkja és a `SUPPORTED` lista bővítésével)
- [ ] Értesítések (Web Push) a saját körre
