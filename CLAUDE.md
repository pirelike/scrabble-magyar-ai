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
- `server.py` — Flask + SocketIO szerver, lobby/szoba kezelés, Cloudflare tunnel integráció
- `game.py` — Játéklogika (Game, Player osztályok), körök, pontozás, játék vége
- `board.py` — 15×15 tábla, premium mezők, szó elhelyezés validáció és pontozás, szótár-ellenőrzés
- `dictionary.py` — Magyar szótár-ellenőrzés (pyenchant / hunspell CLI fallback)
- `tiles.py` — Magyar betűkészlet (100 zseton), TileBag osztály
- `dict/` — Beágyazott hu_HU hunspell szótár fájlok (hu_HU.dic, hu_HU.aff)
- `templates/index.html` — Egyoldalas UI: név, lobby, várakozó szoba, játék
- `static/app.js` — Kliens logika, drag & drop, Socket.IO kommunikáció
- `static/style.css` — Stílusok
- `requirements.txt` — Python függőségek (flask, flask-socketio, pyenchant)
- `.venv/` — Virtual environment

## Funkciók
- 1-4 játékos (egyedül is játszható)
- Online multiplayer: lobby, szobák, Cloudflare tunnel automatikus publikus URL
- Teljes magyar betűkészlet (SZ, CS, GY, LY, NY, ZS, TY többkarakteres betűk)
- Standard Scrabble pontozás: DL, TL, DW, TW premium mezők
- 50 pont bónusz mind a 7 zseton kirakásakor
- Drag & drop és kattintásos betű elhelyezés
- Joker (üres zseton) bármely betűként használható
- Betűcsere és passz
- Szótár-ellenőrzés: pyenchant + beágyazott hu_HU szótár (cross-platform, Windows-kompatibilis)

## Biztonság
- `SECRET_KEY`: környezeti változóból (`SECRET_KEY`) vagy futásidőben generált véletlenszerű kulcs
- CORS: `cors_allowed_origins=[]` — csak same-origin kérések engedélyezve
- Rate limiting: minden Socket.IO event-re (konfigurálható küszöbök `_RATE_LIMITS` dict-ben)
- Input validáció: játékos nevek, szoba nevek, tile placement adatok szerver oldali validálás
- Board bounds check: a `board.py` és `server.py` is ellenőrzi a pozíciók érvényességét
- Dictionary sanitizálás: szavak regex-szel validálva hunspell hívás előtt
- Debug mód kikapcsolva, `allow_unsafe_werkzeug` eltávolítva
- XSS védelem: frontend innerHTML helyett DOM API (textContent, createElement, addEventListener)

## Következő lépés: Felhasználói fiók rendszer

### Regisztrációs flow
```
[1. Email megadás] → [2. Kód emailben (6 számjegy)] → [3. Kód beírása] → [4. 2x jelszó mező + megjelenítési név] → [5. Fiók létrehozva, auto-login] → [6. Lobby]
```

### Bejelentkezési flow
```
[1. Email + jelszó] → [2. Lobby]
```

Vendég mód: a régi név-megadós flow megmarad (statisztikák nem mentődnek).

### Új fájlok
| Fájl | Tartalom |
|---|---|
| `config.py` | SMTP, auth, DB konfigurációs konstansok (`os.environ`-ból) |
| `auth.py` | SQLite DB kezelés, regisztráció, login, session, jelszó hash (`werkzeug.security` PBKDF2) |
| `email_service.py` | 6 számjegyű kód generálás, SMTP küldés (`smtplib`, háttérszálon) |

### Adatbázis: SQLite (`scrabble.db`)
- **`users`**: id, email, email_lower, display_name, password_hash, created_at, games_played, games_won, total_score
- **`verification_codes`**: email, code, created_at, expires_at (10 perc), attempts (max 5), used
- **`sessions`**: user_id, token (64 char, `secrets.token_urlsafe`), created_at, expires_at (30 nap)

### Szerver változások (`server.py`)
Új HTTP POST route-ok (auth a WebSocket előtt történik):
- `POST /api/auth/request-code` — email validálás, kód küldés
- `POST /api/auth/verify-code` — 6 számjegyű kód ellenőrzés
- `POST /api/auth/register` — jelszó + név, fiók létrehozás, auto-login
- `POST /api/auth/login` — email + jelszó
- `POST /api/auth/logout` — session törlés
- `GET /api/auth/me` — session cookie ellenőrzés

Session cookie: `HttpOnly` + `SameSite=Lax` + `Secure` (Cloudflare tunnel HTTPS).
IP-alapú rate limiting az auth endpointokra (kód küldés 3/5perc, login 10/5perc, regisztráció 3/óra).

### Frontend (`index.html` + `app.js`)
A `name-screen` helyett `auth-screen` 3 tabbal:
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

### Biztonság
- Jelszó: `werkzeug.security` PBKDF2-SHA256, 260k iteráció, random salt (zero extra dependency)
- Verifikációs kód: 6 számjegy, 10 perc lejárat, max 5 próbálkozás/kód
- Session: `secrets.token_urlsafe(48)`, HttpOnly cookie, 30 nap lejárat
- Nincs új dependency (sqlite3, smtplib, hashlib, secrets mind standard library)

### Implementáció sorrendje
1. `config.py` — konfigurációs konstansok
2. `auth.py` — DB séma + init_db() + jelszó hash + user CRUD + session kezelés
3. `email_service.py` — email küldés + kód generálás
4. `server.py` — HTTP auth route-ok + IP rate limiting + connect módosítás
5. `templates/index.html` — auth UI (3 tab, 3 lépéses regisztráció)
6. `static/app.js` — auth flow kliens logika (fetch API)
7. `static/style.css` — auth form stílusok, input selector kibővítés email/password típusokra

## Ismert problémák / TODO
- Nincs challenge rendszer (szó megkérdőjelezése más játékos által)
- Nincs chat
- Nincs spectator mód
