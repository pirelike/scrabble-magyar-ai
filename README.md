# Magyar Scrabble

**Hungarian Scrabble** — Online multiplayer word game with full Hungarian letter support.

Webes magyar Scrabble játék online multiplayer támogatással. Flask + Socket.IO backend, vanilla JS frontend.

---

## Gyors indítás / Quick Start

```bash
git clone <repo-url>
cd scrabble
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt    # Linux / macOS
.venv/bin/python3 server.py --no-tunnel      # Start server
```

Open http://localhost:5000 in your browser.

---

## Funkciók / Features

- **1-4 játékos** — egyedül is játszható / playable solo or with up to 4 players
- **Online multiplayer** — lobby rendszer, szobák létrehozása/csatlakozás, automatikus Cloudflare tunnel publikus URL-lel
- **Nyilvános és privát szobák** — privát szoba csak 6-jegyű kóddal csatlakozható, nyilvános szobák a lobbyban listázva
- **Felhasználói fiókok** — regisztráció email verifikációval, bejelentkezés, vendég mód
- **Teljes magyar betűkészlet** — 100 zseton, beleértve a többkarakteres betűket (SZ, CS, GY, LY, NY, ZS, TY)
- **Standard Scrabble pontozás** — DL, TL, DW, TW premium mezők, 50 pont bónusz mind a 7 zseton kirakásakor
- **Szótár-böngésző (Challenge fázis)** — a megtámadás során a lerakott szavakra kattintva egy új lapon indíthatunk Google keresést (szótári fókusszal), segítve a szavazást
- **Szótár-ellenőrzés** — hunspell hu_HU szótár alapján, ragozott alakokat is felismeri
- **Drag & drop és kattintásos** betűelhelyezés
- **Joker** — üres zseton bármely betűként használható
- **Betűcsere és passz**
- **Körönkénti időlimit** — opcionális (0/60/90/120/180/300 mp), lejáratkor automatikus passz
- **Megtámadás (challenge) mód** — szobánként bekapcsolható; 2 játékosnál kötelező elfogadás/elutasítás, 3+ játékosnál szavazásos rendszer (nincs szótár-ellenőrzés, kizárólag a játékosok döntése számít)
- **Barátlista és szobameghívó** — regisztrált játékosok egymást barátnak jelölhetik (kérés küldés/elfogadás/elutasítás, barát eltávolítás), online státusz jelzővel; a várakozó szoba tulajdonosa barátait közvetlenül meghívhatja a szobába
- **In-game chat** — játék közbeni szöveges üzenetküldés a szobában lévő játékosok között
- **Játék mentés / visszatöltés** — manuális mentés (owner-only); lobby-first restore flow: a tulajdonos visszaállítja a mentést, várakozó szoba jön létre ahová az eredeti játékosok csatlakozhatnak
- **Visszajátszás** — befejezett játékok lépésről lépésre visszanézhetők (board snapshot-okkal)
- **Játékos profil** — statisztikák (játszott, győzelem, nyerési arány, átl. pontszám) és játékelőzmények
- **Sötét / világos téma** — automatikus detektálás (`prefers-color-scheme`), manuális váltás, Slate+Gold paletta
- **Hang effektek** — betű lerakás, szavazás, kör értesítő, chat, játék kezdés/vége; hangerő-szabályozó és kategóriánkénti ki/be kapcsolók (Web Audio API, nincs külső fájl)
- **Stabil újracsatlakozás** — hálózati hiba vagy manuális kilépés után is visszacsatlakozhatnak a játékosok az aktív játékba (120 mp grace period, token alapú)
- **Pinch-to-zoom** — mobilon a tábla nagyítható/kicsinyíthető csípő mozdulattal

---

## Telepítés / Installation

### Követelmények / Requirements

- **Python 3.10+**
- **pip** (Python csomagkezelő)
- Opcionális: `cloudflared` (Cloudflare tunnel-hez, publikus URL-hez — lásd lent)

### Linux

```bash
git clone <repo-url>
cd scrabble

# Virtual environment létrehozása és függőségek telepítése
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
```

A `dict/` mappában lévő beágyazott magyar szótár automatikusan működik.

<details>
<summary>Alternatív: rendszer hunspell szótár használata</summary>

```bash
# Arch Linux
sudo pacman -S hunspell hunspell-hu

# Debian / Ubuntu
sudo apt install hunspell hunspell-hu

# Fedora
sudo dnf install hunspell hunspell-hu
```

</details>

### Windows

```powershell
git clone <repo-url>
cd scrabble

# Virtual environment létrehozása és függőségek telepítése
python -m venv .venv
.venv\Scripts\pip install -r requirements.txt
```

A `pyenchant` csomag Windows-on automatikusan tartalmazza a hunspell backendet. A magyar szótár fájlok (`hu_HU.dic`, `hu_HU.aff`) a repó `dict/` mappájában vannak, amit a program automatikusan megtalál.

### macOS

```bash
git clone <repo-url>
cd scrabble

# Homebrew-vel ha szükséges: brew install python3
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
```

Ha a `pyenchant` telepítés hibát ad, telepítsd az enchant könyvtárat:
```bash
brew install enchant
```

---

## Futtatás / Running

### Helyi hálózat (LAN only)

```bash
# Linux / macOS
.venv/bin/python3 server.py --no-tunnel

# Windows
.venv\Scripts\python server.py --no-tunnel
```

Böngészőben: **http://localhost:5000**

A szerver eventlet WSGI-t használ (production-ready). A `PORT` környezeti változóval a port módosítható (alapértelmezett: 5000).

### Publikus URL (Cloudflare Tunnel)

```bash
# Linux / macOS
.venv/bin/python3 server.py

# Windows
.venv\Scripts\python server.py
```

Ha a `cloudflared` telepítve van, a szerver indításakor automatikusan elindul a tunnel:

```
==================================================
  PUBLIKUS URL: https://xyz-abc.trycloudflare.com
  Oszd meg ezt a linket a barátaiddal!
==================================================
```

A tunnel a `--no-tunnel` kapcsolóval kikapcsolható. Regisztráció vagy Cloudflare fiók nem szükséges.

---

## Cloudflare Tunnel telepítése / Installing Cloudflare Tunnel

A Cloudflare Tunnel lehetővé teszi, hogy az interneten keresztül is elérhető legyen a szerver — portnyitás, domain vagy statikus IP nélkül.

<details>
<summary><strong>Windows</strong></summary>

```powershell
# Winget-tel (ajánlott)
winget install --id Cloudflare.cloudflared

# Vagy Scoop-pal
scoop install cloudflared

# Vagy Chocolatey-vel
choco install cloudflared
```

Alternatívaként a `cloudflared.exe` letölthető közvetlenül a [Cloudflare GitHub Releases](https://github.com/cloudflare/cloudflared/releases) oldalról — tedd a PATH-ba vagy a projekt mappájába.

</details>

<details>
<summary><strong>Linux (Arch)</strong></summary>

```bash
sudo pacman -S cloudflared
```

</details>

<details>
<summary><strong>Linux (Debian / Ubuntu)</strong></summary>

```bash
curl -fsSL https://pkg.cloudflare.com/cloudflare-main.gpg | sudo tee /usr/share/keyrings/cloudflare-main.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/cloudflare-main.gpg] https://pkg.cloudflare.com/cloudflared $(lsb_release -cs) main" | sudo tee /etc/apt/sources.list.d/cloudflared.list
sudo apt update && sudo apt install cloudflared
```

</details>

<details>
<summary><strong>macOS</strong></summary>

```bash
brew install cloudflared
```

</details>

---

## Környezeti változók / Environment Variables

A szerver opcionális környezeti változókat olvas. Egyik sem kötelező — minden alapértelmezéssel működik.

| Változó | Alapértelmezett | Leírás |
|---|---|---|
| `PORT` | `5000` | Szerver port |
| `SECRET_KEY` | *(random generált)* | Flask session kulcs |
| `SCRABBLE_DB_PATH` | `scrabble.db` | SQLite adatbázis útvonal |
| `SMTP_HOST` | `smtp.gmail.com` | SMTP szerver |
| `SMTP_PORT` | `587` | SMTP port |
| `SMTP_USER` | *(üres)* | SMTP felhasználó |
| `SMTP_PASSWORD` | *(üres)* | SMTP jelszó (app password) |
| `SMTP_FROM` | *(üres)* | Feladó email cím |

Ha az SMTP változók nincsenek beállítva, a verifikációs kódok a szerver konzolra íródnak ki (fejlesztéshez elegendő).

<details>
<summary>Példa .env fájl (opcionális, manuálisan kell source-olni)</summary>

```bash
export SMTP_HOST=smtp.gmail.com
export SMTP_PORT=587
export SMTP_USER=yourscrabble@gmail.com
export SMTP_PASSWORD=abcd-efgh-ijkl-mnop
export SMTP_FROM=yourscrabble@gmail.com
```

</details>

---

## Projekt struktúra / Project Structure

```
server.py          — Flask + Socket.IO szerver, lobby/szoba kezelés, Socket.IO event handlerek
game.py            — Játéklogika (Game osztály), körök, pontozás, challenge rendszer, kör időlimit
player.py          — Player osztály (id, név, kéz, pontszám, disconnected állapot)
board.py           — 15×15 tábla, premium mezők, szóelhelyezés validáció és pontozás
dictionary.py      — Magyar szótár-ellenőrzés (pyenchant / hunspell)
tiles.py           — Magyar betűkészlet (100 zseton), TileBag osztály
challenge.py       — Challenge (megtámadás) logika, szavazási állapotgép
room.py            — Room osztály (szoba állapot, owner, chat, timer kezelés)
state.py           — ServerState singleton (szobák, játékosok, tokenek, reconnect tracking)
routes.py          — Flask blueprint-ek: auth route-ok, game route-ok, index
config.py          — Konfigurációs konstansok (SMTP, auth, DB, rate limit)
auth.py            — SQLite DB, regisztráció, login, session, jelszó hash, játék mentés
email_service.py   — Email verifikációs kód küldés (SMTP / konzol fallback)
rate_limiter.py    — Generikus rate limiter (Socket.IO + HTTP)
tunnel.py          — Cloudflare tunnel subprocess kezelés
dict/              — Beágyazott hu_HU hunspell szótár fájlok
templates/
  index.html       — Egyoldalas UI (auth, lobby, várakozó szoba, játék, profil, replay)
static/
  app.js           — Kliens logika, drag & drop, pinch-to-zoom, Socket.IO, auth, téma, hang
  style.css        — Stílusok, sötét/világos téma (Slate+Gold paletta), reszponzív layout
tests/             — Tesztek (pytest, 413 teszt)
```

---

## Mentés és Roster konzisztencia

A játék kiemelt figyelmet fordít a multiplayer sessionök stabilitására:
- **Fix játékoslista**: A játék kezdésekor (start) a rendszer rögzíti a résztvevőket és azonnal menti az állapotot az adatbázisba.
- **Lecsatlakozás kezelése**: Ha egy játékos kilép vagy megszakad a kapcsolata, nem törlődik a játékból, csak `disconnected` állapotba kerül. A játék automatikusan átugorja őt a körök során.
- **Bármikori visszatérés**: Az érintett játékosok bármikor visszakapcsolódhatnak az aktív játékba az újracsatlakozási tokenjük segítségével.
- **Automatikus mentés**: Ha a szoba tulajdonosa (lobby leader) végleg lecsatlakozik (120 mp grace period lejár), a rendszer automatikusan menti a játékállást, mielőtt feloszlatná a szobát, így semmi nem vész el.
- **Konzisztens mentések**: A manuális mentések minden játékost megőriznek, így a játék később pontosan ugyanabban a felállásban folytatható.

---

## Játékszabályok / Game Rules

- A játékosok felváltva raknak le betűket a 15×15-ös táblára
- Az első szónak a középső (csillag) mezőt kell fednie, és legalább 2 betűből kell állnia
- Minden további szónak csatlakoznia kell meglévő betűkhöz
- A betűknek egy sorban vagy oszlopban, folytonosan kell elhelyezkedniük
- A lerakott szavakat a hunspell magyar szótár ellenőrzi (kivéve challenge módban, ahol nincs szótár-ellenőrzés — kizárólag a játékosok döntése számít)
- Premium mezők: dupla/tripla betű (DL/TL) és dupla/tripla szó (DW/TW)
- Ha valaki mind a 7 zsetonját lerakja, 50 pont bónuszt kap
- A játék véget ér, ha valaki elfogyasztja az összes zsetonját (és a zsák üres), vagy ha mindenki 2× egymás után passzol

---

## Tesztek / Tests

```bash
.venv/bin/python -m pytest tests/ -v
```

| Fájl | Tesztek | Lefedettség |
|---|---|---|
| `tests/test_auth.py` | 63 | DB, user CRUD, jelszó hash, verifikációs kódok, session kezelés |
| `tests/test_game_logic.py` | 138 | TileBag, Board, Player, Game, Challenge, kör időlimit |
| `tests/test_server_auth.py` | 48 | HTTP auth route-ok, cookie flow |
| `tests/test_server_socket.py` | 62 | Socket.IO eventek, lobby, szobák, challenge, chat, owner kilépés |
| `tests/test_challenge.py` | 17 | Challenge szavazásos rendszer |
| `tests/test_dictionary.py` | 19 | Szótár-ellenőrzés |
| `tests/test_email_service.py` | 4 | Email küldés |
| `tests/test_room.py` | 12 | Room osztály |
| `tests/test_friends.py` | 23 | Barát CRUD, kérések, felhasználókeresés, szobameghívó, online státusz |
| `tests/test_timer_and_replay.py` | 27 | Körszámláló UI, kör időlimit, replay perzisztencia |

**Összesen: 413 teszt**

---

## Hibaelhárítás / Troubleshooting

<details>
<summary><strong>pyenchant telepítési hiba</strong></summary>

**Linux**: Telepítsd az enchant könyvtárat:
```bash
# Debian / Ubuntu
sudo apt install libenchant-2-dev

# Arch
sudo pacman -S enchant

# Fedora
sudo dnf install enchant2-devel
```

**macOS**:
```bash
brew install enchant
```

**Windows**: A `pyenchant` pip csomag automatikusan tartalmazza a szükséges DLL-eket.

</details>

<details>
<summary><strong>eventlet telepítési hiba</strong></summary>

Egyes rendszereken az `eventlet` fordítási hibát adhat. Próbáld:
```bash
pip install --upgrade pip setuptools wheel
pip install eventlet
```

</details>

<details>
<summary><strong>A szótár nem ismeri fel a szavakat</strong></summary>

Ellenőrizd, hogy a `dict/hu_HU.dic` és `dict/hu_HU.aff` fájlok megvannak a projekt mappában. Ezek a beágyazott szótár fájlok, amelyeket a program automatikusan használ.

</details>

<details>
<summary><strong>Cloudflare tunnel nem indul el</strong></summary>

Ellenőrizd, hogy a `cloudflared` parancs elérhető a PATH-ban:
```bash
cloudflared --version
```

Ha nem telepítetted, használd a `--no-tunnel` kapcsolót a helyi futtatáshoz.

</details>

---

## TODO

### Játékmenet
- [x] Challenge rendszer — szó megkérdőjelezése más játékos által (megtámadás mód, szavazásos rendszer)
- [x] Játék mentés / visszatöltés — manuális mentés, lobby-first restore flow
- [x] Visszajátszás — befejezett játék lépéseinek visszanézése
- [x] Időlimit a körökre — opcionális időzítő, lejáratkor automatikus passz
- [ ] AI ellenfél — egyjátékos mód számítógépes ellenfél(ek)kel

### Közösségi funkciók
- [x] Chat — játék közbeni üzenetküldés a szobában
- [x] Privát szobák — 6-jegyű kóddal csatlakozás, lobby-ban nem listázott szobák
- [x] Játékos profil oldal — statisztikák, játékelőzmények, visszajátszás
- [x] Barátlista / meghívó rendszer — barátnak jelölés, online státusz, szobameghívó
- [ ] Spectator mód
- [ ] Ranglista / leaderboard

### Hálózat
- [x] Újracsatlakozás (grace period) — 120 mp-es ablak a visszacsatlakozásra játék közben
- [x] Pinch-to-zoom — mobilos tábla nagyítás/kicsinyítés

### UI / UX
- [x] Sötét / világos téma váltás — Slate+Gold paletta, auto-detektálás, localStorage mentés
- [x] Hang effektek — Web Audio API, 8 szintetizált hang, hangerő csúszka, kategóriánkénti kapcsolók
- [x] Szótár-böngésző (Challenge fázis) — szavazásnál kattintható szavak keresése
- [ ] Animációk (betű lerakás, pontszám, kör váltás)
- [ ] Szótár-böngésző (kereső/validáló)
- [ ] PWA támogatás (offline, telepíthető)
- [ ] Többnyelvű felület
