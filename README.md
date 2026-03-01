# Magyar Scrabble

Webes magyar Scrabble játék online multiplayer támogatással. Flask + Socket.IO backend, vanilla JS frontend.

## Funkciók

- **1-4 játékos** — egyedül is játszható
- **Online multiplayer** — lobby rendszer, szobák létrehozása/csatlakozás, automatikus Cloudflare tunnel publikus URL-lel
- **Nyilvános és privát szobák** — privát szoba csak 6-jegyű kóddal csatlakozható, nyilvános szobák a lobbyban listázva
- **Felhasználói fiókok** — regisztráció email verifikációval, bejelentkezés, vendég mód
- **Teljes magyar betűkészlet** — 100 zseton, beleértve a többkarakteres betűket (SZ, CS, GY, LY, NY, ZS, TY)
- **Standard Scrabble pontozás** — DL, TL, DW, TW premium mezők, 50 pont bónusz mind a 7 zseton kirakásakor
- **Szótár-ellenőrzés** — hunspell hu_HU szótár alapján, ragozott alakokat is felismeri
- **Drag & drop és kattintásos** betűelhelyezés
- **Joker** — üres zseton bármely betűként használható
- **Betűcsere és passz**
- **Megtámadás (challenge) mód** — szobánként bekapcsolható; 2 játékosnál kötelező elfogadás/elutasítás, 3+ játékosnál szavazásos rendszer (nincs szótár-ellenőrzés, kizárólag a játékosok döntése számít)
- **In-game chat** — játék közbeni szöveges üzenetküldés a szobában lévő játékosok között
- **Játék mentés / visszatöltés** — manuális mentés (owner-only), lobby-first restore flow: a tulajdonos visszaállítja a mentést, várakozó szoba jön létre ahová az eredeti játékosok csatlakozhatnak
- **Visszajátszás** — befejezett játékok lépésenkénti visszanézése board snapshot-okkal
- **Játékos profil** — statisztikák (játszott, győzelem, nyerési arány, átl. pontszám) és játékelőzmények
- **Sötét / világos téma** — automatikus detektálás (`prefers-color-scheme`), manuális váltás, `localStorage`-ban mentve, Slate+Gold paletta
- **Újracsatlakozás (grace period)** — 120 másodperc a visszacsatlakozásra ha a kapcsolat megszakad játék közben (token alapú)
- **Pinch-to-zoom** — mobilon a tábla nagyítható/kicsinyíthető csípő mozdulattal

## Telepítés

### Követelmények

- Python 3.10+
- Opcionális: `cloudflared` (Cloudflare tunnel-hez, publikus URL-hez)

### Windows

```powershell
git clone <repo-url>
cd scrabble

python -m venv .venv
.venv\Scripts\pip install -r requirements.txt
```

A `pyenchant` csomag Windows-on automatikusan tartalmazza a hunspell backendet. A magyar szótár fájlok (`hu_HU.dic`, `hu_HU.aff`) a repó `dict/` mappájában vannak, amit a program automatikusan megtalál.

### Linux

```bash
git clone <repo-url>
cd scrabble

python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
```

A `dict/` mappában lévő szótár automatikusan működik. Alternatívaként a rendszer hunspell szótár is használható:

```bash
# Arch Linux
sudo pacman -S hunspell hunspell-hu

# Debian/Ubuntu
sudo apt install hunspell hunspell-hu
```

## Futtatás

### Windows

```powershell
# Csak helyi hálózat
.venv\Scripts\python server.py --no-tunnel

# Cloudflare tunnel-lel (publikus URL)
.venv\Scripts\python server.py
```

### Linux

```bash
# Csak helyi hálózat
.venv/bin/python3 server.py --no-tunnel

# Cloudflare tunnel-lel (publikus URL)
.venv/bin/python3 server.py
```

Böngészőben: http://localhost:5000

A szerver eventlet WSGI-t használ (production-ready), a `PORT` környezeti változóból olvassa a portot (alapértelmezett: 5000).

### Cloudflare Tunnel (online multiplayer)

A Cloudflare Tunnel lehetővé teszi, hogy az interneten keresztül is elérhető legyen a szerver — portnyitás, domain vagy statikus IP nélkül. Indításkor a szerver automatikusan generál egy ideiglenes publikus URL-t (pl. `https://xyz-abc.trycloudflare.com`), amit megosztva bárki csatlakozhat.

#### Telepítés

**Windows:**

```powershell
# Winget-tel
winget install --id Cloudflare.cloudflared

# Vagy Scoop-pal
scoop install cloudflared

# Vagy Chocolatey-vel
choco install cloudflared
```

Alternatívaként a `cloudflared.exe` letölthető közvetlenül a [Cloudflare GitHub Releases](https://github.com/cloudflare/cloudflared/releases) oldalról — tedd a PATH-ba vagy a projekt mappájába.

**Linux (Arch):**

```bash
sudo pacman -S cloudflared
```

**Linux (Debian/Ubuntu):**

```bash
curl -fsSL https://pkg.cloudflare.com/cloudflare-main.gpg | sudo tee /usr/share/keyrings/cloudflare-main.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/cloudflare-main.gpg] https://pkg.cloudflare.com/cloudflared $(lsb_release -cs) main" | sudo tee /etc/apt/sources.list.d/cloudflared.list
sudo apt update && sudo apt install cloudflared
```

#### Használat

Ha a `cloudflared` telepítve van, a szerver indításakor automatikusan elindul a tunnel:

```
==================================================
  PUBLIKUS URL: https://xyz-abc.trycloudflare.com
  Oszd meg ezt a linket a barátaiddal!
==================================================
```

A tunnel a `--no-tunnel` kapcsolóval kikapcsolható. Regisztráció vagy Cloudflare fiók nem szükséges.

## Projekt struktúra

```
server.py          — Flask + Socket.IO szerver, lobby/szoba kezelés, auth route-ok, Cloudflare tunnel
game.py            — Játéklogika (Game, Player osztályok), körök, pontozás, challenge rendszer
board.py           — 15×15 tábla, premium mezők, szóelhelyezés validáció és pontozás
dictionary.py      — Magyar szótár-ellenőrzés (pyenchant / hunspell)
tiles.py           — Magyar betűkészlet (100 zseton), TileBag osztály
config.py          — SMTP, auth, DB konfigurációs konstansok
auth.py            — SQLite DB, regisztráció, login, session, jelszó hash, játék mentés
room.py            — Room osztály (szoba állapot, chat, challenge timer)
challenge.py       — Challenge (megtámadás) logika, szavazás
email_service.py   — Email verifikációs kód küldés (SMTP / konzol fallback)
dict/              — Beágyazott hu_HU hunspell szótár fájlok
templates/
  index.html       — Egyoldalas UI (auth, lobby, várakozó szoba, játék)
static/
  app.js           — Kliens logika, drag & drop, pinch-to-zoom, Socket.IO, auth flow, téma váltás
  style.css        — Stílusok, sötét/világos téma (Slate+Gold paletta), reszponzív layout
tests/             — Tesztek (pytest, 200 teszt)
```

## Játékszabályok

- A játékosok felváltva raknak le betűket a 15×15-ös táblára
- Az első szónak a középső (csillag) mezőt kell fednie, és legalább 2 betűből kell állnia
- Minden további szónak csatlakoznia kell meglévő betűkhöz
- A betűknek egy sorban vagy oszlopban, folytonosan kell elhelyezkedniük
- A lerakott szavakat a hunspell magyar szótár ellenőrzi (kivéve challenge módban, ahol nincs szótár-ellenőrzés — kizárólag a játékosok döntése számít)
- Premium mezők: dupla/tripla betű (DL/TL) és dupla/tripla szó (DW/TW)
- Ha valaki mind a 7 zsetonját lerakja, 50 pont bónuszt kap
- A játék véget ér, ha valaki elfogyasztja az összes zsetonját (és a zsák üres), vagy ha mindenki 2× egymás után passzol

## Tesztek

```bash
.venv/bin/python -m pytest tests/ -v
```

200 teszt: auth (33), játéklogika (93), szerver auth route-ok (28), Socket.IO eventek (46).

## TODO

### Játékmenet
- [x] Challenge rendszer — szó megkérdőjelezése más játékos által (megtámadás mód, szavazásos rendszer)
- [x] Játék mentés / visszatöltés — manuális mentés, lobby-first restore flow
- [x] Visszajátszás — befejezett játék lépéseinek visszanézése
- [ ] Időlimit a körökre — opcionális időzítő, lejáratkor automatikus passz
- [ ] AI ellenfél — egyjátékos mód számítógépes ellenfél(ek)kel

### Közösségi funkciók
- [x] Chat — játék közbeni üzenetküldés a szobában
- [x] Privát szobák — 6-jegyű kóddal csatlakozás, lobby-ban nem listázott szobák
- [x] Játékos profil oldal — statisztikák, játékelőzmények, visszajátszás
- [ ] Spectator mód — játék megfigyelése
- [ ] Ranglista / leaderboard
- [ ] Barátlista / meghívó rendszer

### Hálózat
- [x] Újracsatlakozás (grace period) — 120 mp-es ablak a visszacsatlakozásra játék közben
- [x] Pinch-to-zoom — mobilos tábla nagyítás/kicsinyítés

### UI / UX
- [x] Sötét / világos téma váltás — Slate+Gold paletta, auto-detektálás, localStorage mentés
- [ ] Hang effektek
- [ ] Animációk (betű lerakás, pontszám, kör váltás)
- [ ] Szótár-böngésző
- [ ] PWA támogatás (offline, telepíthető)
- [ ] Többnyelvű felület
