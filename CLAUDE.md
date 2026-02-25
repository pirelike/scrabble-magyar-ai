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
- `server.py` — Flask + SocketIO szerver, lobby/szoba kezelés, Cloudflare tunnel integráció, auth HTTP route-ok
- `game.py` — Játéklogika (Game, Player osztályok), körök, pontozás, játék vége
- `board.py` — 15×15 tábla, premium mezők, szó elhelyezés validáció és pontozás, szótár-ellenőrzés
- `dictionary.py` — Magyar szótár-ellenőrzés (pyenchant / hunspell CLI fallback)
- `tiles.py` — Magyar betűkészlet (100 zseton), TileBag osztály
- `config.py` — SMTP, auth, DB konfigurációs konstansok (`os.environ`-ból)
- `auth.py` — SQLite DB kezelés, regisztráció, login, session, jelszó hash (PBKDF2)
- `email_service.py` — 6 számjegyű kód generálás, SMTP küldés (háttérszálon)
- `dict/` — Beágyazott hu_HU hunspell szótár fájlok (hu_HU.dic, hu_HU.aff)
- `templates/index.html` — Egyoldalas UI: auth (3 tab), lobby, várakozó szoba, játék
- `static/app.js` — Kliens logika, drag & drop, Socket.IO kommunikáció, auth flow
- `static/style.css` — Stílusok
- `tests/` — Tesztek (pytest)
- `requirements.txt` — Python függőségek (flask, flask-socketio, pyenchant, eventlet)
- `.venv/` — Virtual environment

## Funkciók
- 1-4 játékos (egyedül is játszható)
- Online multiplayer: lobby, szobák, Cloudflare tunnel automatikus publikus URL
- Felhasználói fiók rendszer: regisztráció (email verifikáció), bejelentkezés, vendég mód
- Teljes magyar betűkészlet (SZ, CS, GY, LY, NY, ZS, TY többkarakteres betűk)
- Standard Scrabble pontozás: DL, TL, DW, TW premium mezők
- 50 pont bónusz mind a 7 zseton kirakásakor
- Drag & drop és kattintásos betű elhelyezés
- Joker (üres zseton) bármely betűként használható
- Betűcsere és passz
- Challenge (megtámadás) mód: 2 játékosnál kötelező elfogadás, 3+ játékosnál szavazásos rendszer (nincs szótár)
- Játék közbeni chat: szöveges üzenetküldés a szobában
- Szótár-ellenőrzés: pyenchant + beágyazott hu_HU szótár (cross-platform, Windows-kompatibilis)

## Biztonság
- `SECRET_KEY`: környezeti változóból (`SECRET_KEY`) vagy futásidőben generált véletlenszerű kulcs
- CORS: `cors_allowed_origins=[]` — csak same-origin kérések engedélyezve
- Rate limiting: minden Socket.IO event-re + IP-alapú HTTP auth endpointokra
- Input validáció: játékos nevek, szoba nevek, tile placement, email, jelszó szerver oldali validálás
- Board bounds check: a `board.py` és `server.py` is ellenőrzi a pozíciók érvényességét
- Dictionary sanitizálás: szavak regex-szel validálva hunspell hívás előtt
- Production szerver: eventlet WSGI (nem Werkzeug dev server), `allow_unsafe_werkzeug` nem használt
- XSS védelem: frontend innerHTML helyett DOM API (textContent, createElement, addEventListener)
- Jelszó: `werkzeug.security` PBKDF2-SHA256, 260k iteráció, random salt
- Verifikációs kód: 6 számjegy, 10 perc lejárat, max 5 próbálkozás/kód
- Session: `secrets.token_urlsafe(48)`, HttpOnly cookie, 30 nap lejárat

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
- **`users`**: id, email, email_lower, display_name, password_hash, created_at, games_played, games_won, total_score
- **`verification_codes`**: email, code, created_at, expires_at (10 perc), attempts (max 5), used
- **`sessions`**: user_id, token (64 char, `secrets.token_urlsafe`), created_at, expires_at (30 nap)

### Auth HTTP route-ok (`server.py`)
- `POST /api/auth/request-code` — email validálás, kód küldés
- `POST /api/auth/verify-code` — 6 számjegyű kód ellenőrzés
- `POST /api/auth/register` — jelszó + név, fiók létrehozás, auto-login
- `POST /api/auth/login` — email + jelszó
- `POST /api/auth/logout` — session törlés
- `GET /api/auth/me` — session cookie ellenőrzés

Session cookie: `HttpOnly` + `SameSite=Lax` + `Secure` (Cloudflare tunnel HTTPS).
IP-alapú rate limiting (kód küldés 3/5perc, login 10/5perc, regisztráció 3/óra).

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
| `tests/test_auth.py` | 33 | DB, user CRUD, jelszó hash, verifikációs kódok, session kezelés |
| `tests/test_game_logic.py` | 79 | TileBag, Board, Player, Game, Challenge szavazásos rendszer |
| `tests/test_server_auth.py` | 28 | HTTP auth route-ok, cookie flow |
| `tests/test_server_socket.py` | 42 | Socket.IO eventek, lobby, szobák, challenge szavazás, chat |

## Challenge (megtámadás) rendszer — szavazásos

### Működés
Szoba létrehozásakor bekapcsolható a "Megtámadás mód" (checkbox). A rendszer játékosszám-függő:

#### 2 játékos
1. Amikor egy játékos lerak szavakat, a másik játékos látja a lerakott szavakat
2. A másik játékos "Elfogad" gombbal elfogadhatja (nincs "Megtámad" gomb)
3. Ha 30 mp-en belül nem nyom Elfogad-ot, automatikusan elfogadódik
4. **Nincs szótár-ellenőrzés**, a szó mindig elfogadásra kerül

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
- `game.py`: `challenge()` szavazást indít, `cast_vote()` szavazat leadás, `accept_pending_by_player()` játékos elfogadás, `_resolve_votes()` kiértékelés
- `server.py`: `challenge`, `accept_words`, `cast_vote` Socket.IO eventek, `_start_challenge_timer()` háttérfolyamat
- Egyjátékos módban a challenge mód nincs hatással (nincs ki megtámadja)
- Szótár-ellenőrzés teljesen kikapcsolva challenge módban (lerakáskor és szavazásnál is)

### Socket.IO eventek
- `challenge` (kliens→szerver): szavazás indítása (3+ játékos)
- `accept_words` (kliens→szerver): lerakás elfogadása (vagy elfogadó szavazat)
- `cast_vote` (kliens→szerver): `{vote: 'accept'|'reject'}` — szavazat leadás
- `challenge_result` (szerver→szoba): `{challenge_won, message}` — szavazás eredménye

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

## Legutóbbi javítások

### Drag & drop premium mezőkön (2026-02-24)
- **Probléma:** Drag & drop nem működött megbízhatóan a speciális (DL, TL, DW, TW, ★) mezőkön, mert a premium label `<span>` elkapta a drag eventeket egyes böngészőkben a `pointer-events: none` ellenére.
- **Megoldás:** Per-cell drag handlerek helyett board-level event delegation (`e.target.closest('.cell')`). A `dragleave` most csak akkor törli a highlight-ot, ha tényleg elhagyja a táblát.

### UI méretezés játék indításakor (2026-02-24)
- **Probléma:** A `.game-layout` `min-height: 100vh`-ja feleslegesen megnövelte az oldal magasságát, scrollbar jelent meg.
- **Megoldás:** Eltávolítva a `min-height: 100vh` a `.game-layout`-ról, `#game-screen.screen` `align-items: flex-start`. A tábla mérete most figyelembe veszi a viewport magasságát is: `min(70vw, 600px, calc(100vh - 180px))`.

### Premium label túlcsordulás (2026-02-24)
- **Probléma:** A premium labelek (pl. "DUPLA BETŰ") egy sorban renderelődtek és túlcsordultak a szomszéd cellákba, mert a HTML a `\n`-t whitespace-ként kezelte.
- **Megoldás:** `white-space: pre-line` a `.premium-label`-re (kétsoros megjelenítés), `overflow: hidden` a `.cell`-re.

## Ismert problémák / TODO

### Játékmenet
- [x] Challenge rendszer — szó megkérdőjelezése más játékos által (30 mp ablak, megtámadás/elfogadás)
- [ ] Időlimit a körökre — opcionális időzítő (pl. 2 perc/kör), lejáratkor automatikus passz
- [ ] AI ellenfél — egyjátékos mód számítógépes ellenfél(ek)kel, nehézségi szintek
- [ ] Játék mentés / visszatöltés — félbehagyott játék folytatása (szerver újraindítás után is)
- [ ] Visszajátszás — befejezett játék lépéseinek visszanézése

### Közösségi funkciók
- [x] Chat — játék közbeni szöveges üzenetküldés a játékosok között
- [ ] Spectator mód — folyamatban lévő játék megfigyelése játékos nélkül
- [ ] Ranglista / leaderboard — regisztrált játékosok összesített statisztikái
- [ ] Játékos profil oldal — saját statisztikák, játékelőzmények megtekintése
- [ ] Barátlista / meghívó rendszer — közvetlen meghívás barátoknak

### UI / UX
- [ ] Hang effektek — betű lerakás, érvénytelen lépés, játék vége hangok
- [ ] Animációk — betű lerakás, pontszám felugró, kör váltás animáció
- [ ] Sötét / világos téma váltás
- [ ] Szótár-böngésző — szavak keresése és validálása játékon kívül
- [ ] PWA támogatás — offline mód, alkalmazásként telepíthető
- [ ] Többnyelvű felület — angol és egyéb nyelvű UI (a szótár marad magyar)
- [ ] Drag & drop vizuális visszajelzés javítása — foglalt cellák jelölése drop közben
