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
- **Teljes magyar betűkészlet** — 100 zseton, beleértve a többkarakteres betűket (SZ, CS, GY, LY, NY, ZS, TY) — a kétjegyű betű csak a saját zsetonjával rakható ki (külön S + Z nem)
- **Standard Scrabble pontozás** — DL, TL, DW, TW premium mezők, 50 pont bónusz mind a 7 zseton kirakásakor
- **Szótár-böngésző (Challenge fázis)** — a megtámadás során a lerakott szavakra kattintva egy új lapon indíthatunk Google keresést (szótári fókusszal), segítve a szavazást
- **Szótár-ellenőrzés** — beágyazott hu_HU szótár (rendszerfüggőség nélkül): a szótári szavakat és ragozott alakjaikat fogadja el; tulajdonnevek, rövidítések, idegen írásmódú szavak és a szótárban nem szereplő összetételek nem érvényesek; a nyelvtanilag lehetséges, de a használatban nem előforduló alakokat (FALIM, ÉJÉK, BLÖKIÜL) kiszűri
- **Drag & drop és kattintásos** betűelhelyezés
- **Joker** — üres zseton bármely betűként használható
- **Betűcsere és passz**
- **Körönkénti időlimit** — opcionális (0/60/90/120/180/300 mp), lejáratkor automatikus passz
- **Megtámadás (challenge) mód** — szobánként bekapcsolható; 2 játékosnál kötelező elfogadás/elutasítás, 3+ játékosnál szavazásos rendszer (nincs szótár-ellenőrzés, kizárólag a játékosok döntése számít)
- **Barátlista és szobameghívó** — regisztrált játékosok egymást barátnak jelölhetik (kérés küldés/elfogadás/elutasítás, barát eltávolítás), online státusz jelzővel; a várakozó szoba tulajdonosa barátait közvetlenül meghívhatja a szobába
- **In-game chat** — játék közbeni szöveges üzenetküldés a szobában lévő játékosok között
- **Játék mentés / visszatöltés** — manuális mentés (owner-only); lobby-first restore flow: a tulajdonos visszaállítja a mentést, várakozó szoba jön létre ahová az eredeti játékosok csatlakozhatnak
- **Visszajátszás** — befejezett játékok lépésről lépésre visszanézhetők (board snapshot-okkal), elemzéssel és megosztható linkkel
- **Visszavonás** — megtámadás módban a lerakó visszavonhatja a még el nem döntött lerakását; `Ctrl+Z` az utolsó lerakott betűt veszi vissza
- **Játékos profil** — statisztikák (játszott, győzelem, nyerési arány, átl. pontszám, értékszám), kitüntetések, beállítások és játékelőzmények (értékszám-változással); a lobby navigációja a profilban is elérhető
- **Sötét / világos téma** — automatikus detektálás (`prefers-color-scheme`), manuális váltás, Slate+Gold paletta
- **Hang effektek** — betű lerakás, szavazás, kör értesítő, chat, játék kezdés/vége; hangerő-szabályozó és kategóriánkénti ki/be kapcsolók (Web Audio API, nincs külső fájl)
- **Stabil újracsatlakozás** — hálózati hiba vagy manuális kilépés után is visszacsatlakozhatnak a játékosok az aktív játékba (120 mp grace period, token alapú); a **várakozó szoba** sem szűnik meg azonnal, ha a tulajdonos kapcsolata megszakad (pl. telefonon átvált az üzenetküldő appra a kód elküldéséhez): 10 percig megmarad, és a tulajdonos visszatérhet
- **Pinch-to-zoom** — mobilon a tábla nagyítható/kicsinyíthető csípő mozdulattal
- **Robot ellenfelek (AI)** — egyedül is játszható 1–3 számítógépes ellenfél ellen, 10 nehézségi fokozattal vagy **„Igazodik hozzám”** móddal (a robot az utolsó köreid átlagához állítja az erejét); a robotok tőszavakat és gyakori ragozott alakokat is raknak. Robotos játék a ranglistába nem számít. Egyedül játszva **tipp** kérhető (a három legjobb lépés)
- **Megfigyelő mód** — folyamatban lévő nyilvános játék megfigyelése játékos nélkül (lobby „Élő játékok”, privát játék kóddal); a megfigyelő nem lát kezeket, nem lép és nem chatel
- **Ranglista** — **értékszám (ELO)**, győzelmek, nyerési arány, átlagpont és legjobb játék szerint (csak regisztrált játékosok, robot nélküli, befejezett játékokból)
- **Napi feladvány** — naponta egy közös táblaállás és betűkészlet mindenkinek: keresd meg a legtöbb pontot érő lépést; napi ranglista, tegnapi megoldás
- **Gyakorlás** — szókvíz (hosszú–rövid csapdákkal), betűvadász, bingó-edző, a tévesztett szavak újragyakorlása („Hibáim”), szólisták keresővel; napi sorozat és statisztika
- **Levelezős játék** — barátokkal órák vagy napok alatt lépkedve (24 óra – 7 nap lépésenként); a játék közben bezárhatod az alkalmazást, és értesítést kapsz, ha rád kerül a sor
- **Értesítések (Web Push)** — a profilban bekapcsolható „Te jössz!” értesítés a telefonodra / böngésződbe
- **Játékelemzés** — a játék végén lépésenként a legjobb lehetséges lépés és a kint maradt pont, játékosonkénti hatékonysággal
- **Kitüntetések** — bingó (mind a 7 zseton), 100+ pontos lépés, 8+ zsetonos szó, joker, 300 pontos játék, győzelem erős robot ellen, napi feladvány legjobb lépése, 10 győzelem, 25 játék…
- **Visszajátszás megosztása** — a befejezett játék visszajátszásának linkje (`/?replay=…`) bejelentkezés nélkül is megnyitható
- **Szótár-böngésző** — bárhonnan megnyitható szó-ellenőrző: érvényes-e a szó, hány pontot ér prémium nélkül, zsetonokra bontva, javaslatokkal elgépelés esetén
- **Animációk** — betű lerakás, ellenfél lépésének becsúszása, pontszám felugró, kör váltás jelzése; húzás közben a foglalt mezők pirossal jelölve (`prefers-reduced-motion` esetén kikapcsolva)
- **PWA** — telepíthető alkalmazás (manifest, service worker, ikonok); kapcsolat nélkül is elindul a felület, és érthető üzenetet mutat
- **Többnyelvű felület** — magyar és angol (automatikus nyelvfelismerés, kézi váltás a felső sávban); a szótár magyar marad, a szerver üzeneteit a kliens fordítja
- **Kényelmi funkciók** — élő pontszám-előnézet lerakás közben · zsetonszámláló („mi van még a zsákban?”) · betűtartó keverés / rendezés + gyorsbillentyűk · a betűtartó a tábla jobb oldalán (alapértelmezett) vagy alatta áll, a profilban vagy a játék „Elrendezés” gombjával állítható · meghívó link (`/?join=KÓD`) megosztással · lépéstörténet, az utolsó lépés kiemelése a táblán és „legjobb lépés” a játék végén

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

### Windows

```powershell
git clone <repo-url>
cd scrabble

# Virtual environment létrehozása és függőségek telepítése
python -m venv .venv
.venv\Scripts\pip install -r requirements.txt
```

A magyar szótár fájlok (`hu_HU.dic`, `hu_HU.aff`) a repó `dict/` mappájában vannak, amit a program automatikusan megtalál; semmilyen rendszercsomag nem kell hozzá.

A `dict/hu_attested.txt` a nyelvtanilag lehetséges, de gyakran értelmetlen alakok (pl. melléknév + birtokos személyjel, „-ék”, „-ul/-ül” főnéven) közül a ténylegesen használtakat sorolja fel; a szótár ezeket a csoportokat csak a listán szereplő alakokkal fogadja el. Forrása Hermit Dave [FrequencyWords](https://github.com/hermitdave/FrequencyWords) gyakorisági listája (OpenSubtitles 2018, magyar), ezért a fájl CC BY-SA 4.0 licencű. Újraépítés: `python tools/build_attested.py hu_full.txt`.

### macOS

```bash
git clone <repo-url>
cd scrabble

# Homebrew-vel ha szükséges: brew install python3
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
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

A szerver gevent WSGI-t használ websocket támogatással (production-ready). A `PORT` környezeti változóval a port módosítható (alapértelmezett: 5000).

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
| `VAPID_PRIVATE_KEY` | *(első induláskor generált, az adatbázisban marad)* | Web Push VAPID privát kulcs (PEM, sortörések `\n`-nel; vagy a `web-push generate-vapid-keys` base64url kulcsa) |
| `VAPID_SUBJECT` | `mailto:SMTP_FROM` | Web Push `sub` mező (`mailto:` vagy `https:` cím) |

Ha az SMTP változók nincsenek beállítva, a verifikációs kódok a szerver konzolra íródnak ki (fejlesztéshez elegendő).

A Web Push a `pywebpush` csomagot használja (a `requirements.txt` tartalmazza); ha nincs telepítve, az értesítések kikapcsolnak, a játék többi része változatlanul működik. Az értesítésekhez a böngészőnek HTTPS (vagy `localhost`) kell; iPhone-on az alkalmazást előbb a Főképernyőhöz kell adni.

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
server.py          — Flask + Socket.IO (gevent) szerver, lobby/szoba kezelés, Socket.IO event handlerek, robotlépések, napi feladvány, levelezős játékok, push
game.py            — Játéklogika (Game osztály), körök, pontozás, challenge rendszer, kör időlimit, lépéstörténet, előnézet
player.py          — Player osztály (id, név, kéz, pontszám, disconnected állapot, robot jelző)
ai_player.py       — Robot ellenfél: szókincs (tőszavak + gyakori ragozott alakok), lépésgenerátor, 10 fokozat + „igazodik hozzám”, tippek
achievements.py    — Kitüntetések kiértékelése
analysis.py        — Játékelemzés (legjobb lépés / kint maradt pont lépésenként)
async_games.py     — Levelezős játék felállítása, lista
daily.py           — Napi feladvány előállítása és eredmény-rögzítés
elo.py             — ELO értékszám
practice.py        — Szókvíz, betűvadász / bingó-edző, rövid szavak listája
push_service.py    — Web Push (VAPID, feliratkozások, küldés)
board.py           — 15×15 tábla, premium mezők, szóelhelyezés validáció és pontozás
dictionary.py      — Magyar szótár-ellenőrzés (beágyazott), tömeges ellenőrzés, javaslatok
affix_checker.py   — Hunspell-szerű, függőségmentes szóellenőrző a dict/hu_HU fájlokhoz (ragozott alakok előállításával)
tiles.py           — Magyar betűkészlet (100 zseton), TileBag osztály, szó → zsetonok felbontás
challenge.py       — Challenge (megtámadás) logika, szavazási állapotgép
room.py            — Room osztály (szoba állapot, owner, chat, timer kezelés, megfigyelők)
state.py           — ServerState singleton (szobák, játékosok, tokenek, reconnect tracking, megfigyelők)
routes.py          — Flask blueprint-ek: auth, game, publikus API (ranglista, szótár), index, PWA végpontok
config.py          — Konfigurációs konstansok (SMTP, auth, DB, rate limit)
auth.py            — SQLite DB, regisztráció, login, session, jelszó hash, játék mentés
email_service.py   — Email verifikációs kód küldés (SMTP / konzol fallback)
rate_limiter.py    — Generikus rate limiter (Socket.IO + HTTP)
socket_auth.py     — Aláírt socket-token a Socket.IO identitás igazolásához
tunnel.py          — Cloudflare tunnel subprocess kezelés
dict/              — Beágyazott hu_HU hunspell szótár fájlok + hu_attested.txt (használati lista, CC BY-SA 4.0)
templates/
  index.html       — Egyoldalas UI (auth, lobby, várakozó szoba, játék, profil, replay)
  sw.js            — Service worker (Jinja sablon: a verzió a kliens fájlok módosítási idejéből jön)
static/
  app.js           — Kliens logika, drag & drop, pinch-to-zoom, Socket.IO, auth, téma, hang, megfigyelés, ranglista, szótár
  i18n.js          — Fordító (t(), data-i18n attribútumok, szerverüzenet-fordítás)
  i18n-data.js     — Fordítások (hu / en) — szigorú JSON, a tesztek is ezt olvassák
  style.css        — Stílusok, sötét/világos téma (Slate+Gold paletta), reszponzív layout, animációk
  manifest.webmanifest, offline.html, icons/ — PWA: manifest, kapcsolat nélküli oldal, ikonok
tests/             — Tesztek (pytest, 1360 teszt)
```

---

## Mentés és Roster konzisztencia

A játék kiemelt figyelmet fordít a multiplayer sessionök stabilitására:
- **Fix játékoslista**: A játék kezdésekor (start) a rendszer rögzíti a résztvevőket és azonnal menti az állapotot az adatbázisba.
- **Lecsatlakozás kezelése**: Ha egy játékos kilép vagy megszakad a kapcsolata, nem törlődik a játékból, csak `disconnected` állapotba kerül. Az élő játék automatikusan átugorja őt a körök során (a **levelezős** játékban nem: ott a határidő számít, és az adatbázis a játék tartós otthona).
- **Bármikori visszatérés**: Az érintett játékosok bármikor visszakapcsolódhatnak az aktív játékba az újracsatlakozási tokenjük segítségével.
- **Automatikus mentés**: Ha a szoba tulajdonosa (lobby leader) végleg lecsatlakozik (120 mp grace period lejár), a rendszer automatikusan menti a játékállást, mielőtt feloszlatná a szobát, így semmi nem vész el.
- **Konzisztens mentések**: A manuális mentések minden játékost megőriznek, így a játék később pontosan ugyanabban a felállásban folytatható.

---

## Robot ellenfelek / Robots

Új szoba létrehozásakor 1–3 robot ellenfél kérhető (a robotok is férőhelyet foglalnak). Egyedül, robotok ellen is játszhatsz.

| Fokozat | Név | Mért erő (pont/kör) |
|---|---|---|
| 1 – 3 | újonc · kezdő · könnyű | 4,3 · 6,1 · 7,4 |
| 4 – 7 | mérsékelt · alkalmi · közepes · ügyes | 10,1 · 12,7 · 15,7 · 18,4 |
| 8 – 10 | haladó · erős · mester | 21,2 · 24,6 · 27,0 |
| **Igazodik hozzám** | a robot az utolsó ~6 köröd átlagához állítja az erejét | — |

- Az alsó fokozatok célpontszámot sorsolnak, és a hozzá legközelebbi lépést rakják le; a felsők a pont + a kézben maradó zsetonok értéke alapján a legjobb lépést választják (egyre kisebb zajjal). Jokert minden fokozat használ.
- A robotok szókincse a szótár **tőszavai** (kb. 65 000 szó) és azok **gyakori ragozott alakjai** (többes szám, tárgyrag, esetragok, birtokos és igei végződések; kb. 225 000 alak, a szótár saját szabályaival előállítva; a nyelvtanilag lehetséges, de furcsa alakok — pl. FALIM, ÉJÉK — nélkül). A keresztszavakat és a kiválasztott lépés szavait a teljes szótár ellenőrzi.
- **Igazodik hozzám**: a robot az emberi játékosok utolsó 6 körének átlagát (a passz 0 pont) a mért skálán fokozattá képezi; tört fokozatnál a két szomszédos fokozatot keveri. Az első néhány körben az 5. fokozat felé húz.
- Megtámadás módban a robotok **nem szavaznak**: ha a lerakónak nincs emberi ellenfele, a szótár dönt; a robot lerakására az emberek szavaznak.
- Egyedül (robotok ellen) játszva a **Tipp** gomb a három legjobb lépést mutatja; az „Elhelyez” a táblára teszi, a lerakást te hagyod jóvá.
- A tippek **korlátozottak**: szoba létrehozásakor (lobby → Új szoba → *Tippek száma*) kikapcsolhatók, vagy 1 / 3 (alapértelmezett) / 5 / 10 tipp engedélyezhető játékonként. A gomb a hátralévő számot mutatja (`Tipp (2)`), kikapcsolt tippnél nem jelenik meg.
- A robotos játékok a profil statisztikájában szerepelnek, de a **ranglistában és az értékszámban nem**.
- Emberi néző vagy játékos nélkül a robotok nem játszanak egymás ellen.
- Az erősség újramérése: `python tools/bot_arena.py ladder -n 24 -j 4`; az „igazodó” robot ellenőrzése: `python tools/bot_arena.py adapt 5`.

## Megfigyelő mód / Spectating

- A lobby **Élő játékok** listájában minden nyilvános, folyamatban lévő játék megfigyelhető; privát játék a 6 jegyű kóddal (**Megfigyelés** gomb a kódmező mellett, vagy `/?spectate=KÓD` link).
- A megfigyelő látja a táblát, a pontokat, a lépéstörténetet és a chatet, de **kezeket nem**, nem léphet és nem chatelhet. A játékosok látják a megfigyelők számát.
- Szobánként legfeljebb 30 megfigyelő lehet. Ha a szoba megszűnik, a megfigyelők visszakerülnek a lobbyba.

## Ranglista / Leaderboard

Lobby → **Ranglista**: **értékszám (ELO**, min. 3 értékelt játék), győzelmek, nyerési arány (min. 3 játék), átlagpont (min. 3 játék) és legjobb játék szerint. Csak regisztrált játékosok szerepelnek, csak befejezett, **robot nélküli** játékokból. A saját helyezésed akkor is látszik, ha nem vagy a top 50-ben.

**Értékszám**: mindenki 1200-ról indul; a játék végén minden játékospárra külön számolódik a változás (a nagyobb pontszám nyer, az egyenlő döntetlen), az első 10 értékelt játékban gyorsabban (K = 32), utána lassabban (K = 20). Csak legalább két regisztrált játékos közti, robot nélküli játék számít; aki feladja a játékot, pontszámától függetlenül veszít.

## Napi feladvány, gyakorlás / Daily puzzle, practice

Lobby → **Gyakorlás**:
- **Napi feladvány**: naponta (magyar idő szerint) egy közös táblaállás és betűkészlet; a cél a legtöbb pontot érő lépés. Próbálkozhatsz többször (a legjobb számít, holtversenynél a kevesebb próbálkozás és a korábbi idő), az élő előnézet segít. A *Megoldás* gomb megmutatja a legjobb lépést, de utána már nem kerülhetsz a napi ranglistára. Vendégként is játszhatsz, de csak regisztrált játékos kerül a ranglistára. A tegnapi megoldás is látszik.
- **Szókvíz**: 10 vagy 20 kérdés, hogy a szó érvényes-e. Módok: vegyes szavak, csak két- vagy háromzsetonosak, vagy **hosszú–rövid csapdák** (a↔á, o↔ó, ö↔ő, u↔ú…: az érvénytelen szó egy érvényes szó egyetlen magánhangzójának hosszúságcseréjével készül). A szót játékbeli zsetonokkal látod; a válasz után megtudod, miért (pontérték, javaslatok), a végén listázza a tévesztéseket. Billentyűzettel: ← nem érvényes, → érvényes, Enter következő.
- **Betűvadász**: hét zsetonból építs minél több érvényes szót (koppintással vagy gépeléssel; a pont a zsetonok értéke, mind a hét zseton +50). Időkorlát nélkül, 60 mp-cel vagy 2 perccel; tipp, bónusz szavak (érvényesek, de nem szerepeltek a listánkon), a végén a kimaradt szavak és az eredményed a lehetséges pontokhoz képest.
- **Bingó-edző**: a hét zsetonodból biztosan kirakható egy hét zsetonos szó – találd meg! Tipp (a szó kezdőbetűi), feladás után megmutatja a megoldást; a bingók sorozata számolódik.
- **Hibáim**: a kvízben eltévesztett szavak (az eszközön tárolva); egy szó akkor kerül ki a pakliból, ha kétszer egymás után helyesen válaszolsz rá.
- **Szólisták**: az összes érvényes két- és háromzsetonos szó pontértékkel – keresővel, kezdőbetű-szűrővel és ABC / pont rendezéssel –, valamint a 100 zseton értéke és darabszáma.

## Levelezős játék / Correspondence games

Lobby → **Levelezős**: barátaiddal (1–3 fő) órák vagy napok alatt lépkedhettek.
- Új játéknál (a „Új játék” gombra megnyíló alsó lapon) megadod a barátokat és a gondolkodási időt (24 óra / 48 óra / 3 nap / 7 nap lépésenként). A játék azonnal elindul; a barátok a saját Levelezős fülükön látják, és értesítést kapnak.
- A listában az elöl áll, ahol te jössz; a fül jelvénye mutatja, hány játékban vagy soron. A játékból bármikor kiléphetsz, a játék megmarad (a szerver minden lépést elment, újraindítás után is folytatható).
- Ha a határidőig nem lépsz, **automatikusan passzolsz**; három egymás utáni lejárt határidő után a játékot feladottnak tekintjük. A *Játék feladása* gombbal te is feladhatod: a játék véget ér, és nem lehetsz a győztes.
- Értesítés: Web Push (a profilban kapcsolható), és az alkalmazáson belül is felugró üzenet.

## Kényelmi funkciók / Quality of life

| Funkció | Leírás |
|---|---|
| Élő előnézet | lerakás közben a szerver véglegesítés nélkül kiszámolja a szavakat és a pontszámot (hibaüzenettel, ha érvénytelen) |
| Zsetonszámláló | *Zsetonok* gomb: mely betűk lehetnek még a zsákban / az ellenfelek kezében |
| Betűtartó | *Keverés*, *Rendez*; a sorrend az új húzások után is megmarad |
| Meghívó link | a várakozó szobában a *Meghívó link* gomb megosztja / másolja a `/?join=KÓD` címet; megnyitva belépés után automatikusan csatlakozik |
| Lépéstörténet | *Lépések* lista a panelen; az utolsó lépés betűi kiemelve a táblán és a visszajátszásban; a játék végén a legjobb lépés |

**Gyorsbillentyűk** (játék közben, nyitott ablak nélkül): `Enter` lerak · `Esc` visszavon · `Backspace` / `Ctrl+Z` az utolsó lerakott betű vissza · `S` keverés · `R` rendezés.

**Elemzés és megosztás**: a profil előzményeiben (és a játék végén) a *Visszajátszás* mellett *Megosztás* gomb készít linket; a visszajátszás képernyőn az *Elemzés indítása* lépésenként megmutatja a legjobb lehetséges lépést, a kint maradt pontokat és a játékosonkénti hatékonyságot (csak az elemzés bevezetése utáni játékokra, mert a lépésnapló azóta őrzi a kezeket).

**Kitüntetések**: a profilon látszanak; új kitüntetésről játék végén értesít a játék.

## Nyelvek / Languages

A felület **magyar** és **angol** nyelvű. Az első indításkor a böngésző nyelve dönt, a felső sávban (belépő képernyőn lebegő gombként) váltható; a választás a `localStorage`-ban (`scrabble-lang`) marad meg. Új nyelv felvételéhez a `static/i18n-data.js`-be kell felvenni a nyelvi blokkot (a tesztek ellenőrzik a kulcsok és helyőrzők teljességét), és a `static/i18n.js` `SUPPORTED` listájába a nyelvkódot. A szótár és a szóellenőrzés mindig magyar.

## Telepíthető alkalmazás (PWA)

Támogatott böngészőben a lobby felső sávjában megjelenik a **Telepítés** gomb (iOS Safari: *Megosztás → Főképernyőhöz adás*). A service worker (`/sw.js`) az alkalmazás vázát gyorsítótárazza (HTML, CSS, JS, ikonok), a játék forgalmát (`/socket.io/`, `/api/`) soha. Kapcsolat nélkül a gyorsítótárazott felület indul, a kapcsolati sáv jelzi a hibát. A gyorsítótár verziója a kliens fájlok módosítási idejéből számolódik, így új kiadásnál magától frissül. A service worker csak HTTPS-en (a Cloudflare tunnelen) vagy `localhost`-on működik.

---

## Játékszabályok / Game Rules

- A játékosok felváltva raknak le betűket a 15×15-ös táblára
- Az első szónak a középső (csillag) mezőt kell fednie, és legalább 2 betűből kell állnia
- Minden további szónak csatlakoznia kell meglévő betűkhöz
- A betűknek egy sorban vagy oszlopban, folytonosan kell elhelyezkedniük
- A lerakott szavakat a beágyazott magyar szótár ellenőrzi (szótári szavak és ragozott alakjaik; tulajdonnév, rövidítés, betűnév nem érvényes) (kivéve challenge módban, ahol nincs szótár-ellenőrzés — kizárólag a játékosok döntése számít)
- Premium mezők: dupla/tripla betű (DL/TL) és dupla/tripla szó (DW/TW)
- Ha valaki mind a 7 zsetonját lerakja, 50 pont bónuszt kap
- A játék véget ér, ha valaki elfogyasztja az összes zsetonját (és a zsák üres), vagy ha 6 egymást követő pont nélküli kör volt (passz, csere és elutasított lerakás is számít); egyenlő pontnál döntetlen, mindenki nyer

---

## Tesztek / Tests

```bash
.venv/bin/python -m pytest tests/ -v
```

| Fájl | Tesztek | Lefedettség |
|---|---|---|
| `tests/test_auth.py` | 63 | DB, user CRUD, jelszó hash, verifikációs kódok, session kezelés |
| `tests/test_game_logic.py` | 160 | TileBag, Board, Player, Game, Challenge, kör időlimit |
| `tests/test_server_auth.py` | 52 | HTTP auth route-ok, cookie flow |
| `tests/test_server_socket.py` | 68 | Socket.IO eventek, lobby, szobák, challenge, chat, owner kilépés |
| `tests/test_challenge.py` | 17 | Challenge szavazásos rendszer |
| `tests/test_dictionary.py` | 99 | Szótár-ellenőrzés (valódi szótárral: kisbetűs keresés, tulajdonnevek, rövidítések, értelmetlen szavak pl. SALYT), elérhetőség, javaslatok, tábla-validáció |
| `tests/test_affix_checker.py` | 46 | Beépített szóellenőrző: toldalékok, előtagok, folytatási osztályok, speciális jelzők, a valódi szótár (hunspellel összevetve) |
| `tests/test_email_service.py` | 4 | Email küldés |
| `tests/test_room.py` | 12 | Room osztály |
| `tests/test_friends.py` | 27 | Barát CRUD, kérések, felhasználókeresés, szobameghívó, online státusz |
| `tests/test_timer_and_replay.py` | 30 | Körszámláló UI, kör időlimit, replay perzisztencia |
| `tests/test_regressions.py` | 82 | Kódátvizsgálás során talált hibák regressziós tesztjei |
| `tests/test_ai_player.py` | 82 | Robot: szókincs (ragozott alakok), lépésgenerátor, nehézségi szintek, tipp |
| `tests/test_bots.py` | 114 | Robotok a játékmodellben és a szerveren, lépéstörténet, előnézet, tipp |
| `tests/test_spectator.py` | 30 | Megfigyelő mód, élő játékok listája |
| `tests/test_public_api.py` | 52 | Ranglista (DB + route), szótár-ellenőrző API, PWA végpontok |
| `tests/test_tiles_dictionary.py` | 30 | Zseton-felbontás, tömeges szótár-ellenőrzés, javaslatok |
| `tests/test_i18n.py` | 27 | Fordítások teljessége (kulcsok, helyőrzők, szerverüzenetek), a böngészős fordító futtatása node-ban |
| `tests/test_frontend_consistency.py` | 23 | Kliens ↔ szerver összhang: konstansok, elem-azonosítók, Socket.IO események, JS szintaxis |

| `tests/test_adaptive_bot.py` | 40 | „Igazodik hozzám” robot |
| `tests/test_achievements.py` | 27 | Kitüntetések |
| `tests/test_elo.py` | 27 | ELO értékszám, értékszám szerinti ranglista |
| `tests/test_replay_share.py` | 11 | Megosztható visszajátszás |
| `tests/test_analysis.py` | 25 | Játékelemzés |
| `tests/test_daily.py` | 50 | Napi feladvány és ranglistája |
| `tests/test_practice.py` | 47 | Szókvíz, betűvadász / bingó, rövid szavak |
| `tests/test_practice_client.py` | 15 | A gyakorló felület kliensoldali logikája (node) |
| `tests/test_push.py` | 44 | Web Push |
| `tests/test_async_games.py` | 56 | Levelezős játék |

**Összesen: 1360 teszt** (a node-ot igénylő tesztek node nélkül kimaradnak)

---

## Hibaelhárítás / Troubleshooting

<details>
<summary><strong>Szótár (értelmetlen szavak is érvényesnek látszanak)</strong></summary>

A szótár a `dict/hu_HU.aff` és `dict/hu_HU.dic` fájlokból töltődik be, külön rendszercsomag (enchant, hunspell) nélkül. Indításkor a szerver kiírja: `Szótár: beágyazott hu_HU (… szótő)`. Ha ehelyett a `FIGYELEM: A szótár nem tölthető be` sor jelenik meg, hiányzik vagy sérült a `dict/` mappa, és a szavak ellenőrzése ki van kapcsolva (a Szótár-eszköz ilyenkor hibát jelez, nem "érvényes"-t).

</details>

<details>
<summary><strong>gevent telepítési hiba</strong></summary>

Egyes rendszereken a `gevent` fordítási hibát adhat. Próbáld:
```bash
pip install --upgrade pip setuptools wheel
pip install gevent gevent-websocket
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
- [x] AI ellenfél — egyjátékos mód számítógépes ellenfél(ek)kel, 10 nehézségi fokozat, „igazodik hozzám” mód, ragozott szavak, tipp
- [x] Levelezős (aszinkron) játék, napi feladvány, gyakorló módok, játékelemzés, kitüntetések, ELO, Web Push

### Közösségi funkciók
- [x] Chat — játék közbeni üzenetküldés a szobában
- [x] Privát szobák — 6-jegyű kóddal csatlakozás, lobby-ban nem listázott szobák
- [x] Játékos profil oldal — statisztikák, játékelőzmények, visszajátszás
- [x] Barátlista / meghívó rendszer — barátnak jelölés, online státusz, szobameghívó
- [x] Spectator mód — folyamatban lévő játék megfigyelése játékos nélkül
- [x] Ranglista / leaderboard — győzelmek, nyerési arány, átlagpont, legjobb játék

### Hálózat
- [x] Újracsatlakozás (grace period) — 120 mp-es ablak a visszacsatlakozásra játék közben
- [x] Pinch-to-zoom — mobilos tábla nagyítás/kicsinyítés

### UI / UX
- [x] Sötét / világos téma váltás — Slate+Gold paletta, auto-detektálás, localStorage mentés
- [x] Hang effektek — Web Audio API, 8 szintetizált hang, hangerő csúszka, kategóriánkénti kapcsolók
- [x] Szótár-böngésző (Challenge fázis) — szavazásnál kattintható szavak keresése
- [x] Animációk (betű lerakás, pontszám, kör váltás)
- [x] Drag & drop vizuális visszajelzés — foglalt mezők jelölése húzás közben
- [x] Szótár-böngésző (kereső/validáló)
- [x] PWA támogatás (offline, telepíthető)
- [x] Többnyelvű felület (magyar, angol)
- [x] Kényelmi funkciók — élő pontszám-előnézet, zsetonszámláló, betűtartó keverés/rendezés + gyorsbillentyűk, meghívó link, lépéstörténet + utolsó lépés kiemelése
