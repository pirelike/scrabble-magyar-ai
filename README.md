# Magyar Scrabble

**Hungarian Scrabble** — Online multiplayer word game with full Hungarian letter support.

Webes magyar Scrabble játék online multiplayer támogatással. **Rust** backend (axum + Socket.IO, SQLite), vanilla JS frontend — egyetlen, könnyen futtatható program, otthoni szerverre is: kis memória, gyors indulás, külső szolgáltatás nélkül.

> A korábbi Python (Flask) változat a `python-final` ágon található. A Rust verzió ugyanazt a HTTP / Socket.IO protokollt és ugyanazt az SQLite sémát használja (a meglévő `scrabble.db` változtatás nélkül folytatható), a böngészős kliens változatlan. Mérések: [docs/PERFORMANCE.md](docs/PERFORMANCE.md).

---

## Gyors indítás / Quick Start

```bash
git clone <repo-url>
cd scrabble
cargo build --release                  # egyszer; Rust 1.85+ (https://rustup.rs)
target/release/scrabble --no-tunnel    # a szerver indítása
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

- **Rust 1.85+** (edition 2024) — telepítés: <https://rustup.rs>; a SQLite beépítve (bundled) fordul, ehhez C fordító kell (Linuxon `build-essential` / `gcc`, macOS-en az Xcode parancssori eszközök, Windowson a Visual Studio Build Tools)
- Opcionális: `cloudflared` (Cloudflare tunnel-hez, publikus URL-hez — lásd lent)
- Csak a teszteléshez: `git`, opcionálisan `node` (a kliens-tesztek) és Playwright + Chromium (a böngészős admin teszt)

### Fordítás

```bash
git clone <repo-url>
cd scrabble
cargo build --release          # target/release/scrabble (+ a karbantartó eszközök)
```

Linuxon, macOS-en és Windowson ugyanígy (Windowson: `target\release\scrabble.exe`). A program a mappájában keresi a `web/` (kliens) és a `dict/` (szótár) mappát; ezek a repóban vannak, rendszercsomag nem kell hozzá. Másik helyről futtatva a `SCRABBLE_BASE_DIR` környezeti változóval adható meg a mappa.

A `dict/hu_attested.txt` a nyelvtanilag lehetséges, de gyakran értelmetlen alakok (pl. melléknév + birtokos személyjel, „-ék”, „-ul/-ül” főnéven) közül a ténylegesen használtakat sorolja fel; a szótár ezeket a csoportokat csak a listán szereplő alakokkal fogadja el. Forrása Hermit Dave [FrequencyWords](https://github.com/hermitdave/FrequencyWords) gyakorisági listája (OpenSubtitles 2018, magyar), ezért a fájl CC BY-SA 4.0 licencű. Újraépítés: `target/release/build_attested hu_full.txt`.

---

## Futtatás / Running

### Helyi hálózat (LAN only)

```bash
scripts/run.sh --no-tunnel                  # szükség esetén előbb fordít
# vagy: target/release/scrabble --no-tunnel   (Windows: target\release\scrabble.exe --no-tunnel)
```

Böngészőben: **http://localhost:5000**

A `PORT` környezeti változóval (vagy a `.env` fájlban) a port módosítható (alapértelmezett: 5000).

### Publikus URL (Cloudflare Tunnel)

```bash
scripts/run.sh                              # vagy: target/release/scrabble
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

A szerver opcionális környezeti változókat olvas. Egyik sem kötelező — minden alapértelmezéssel működik. Minta: `.env.example`.

| Változó | Alapértelmezett | Leírás |
|---|---|---|
| `PORT` | `5000` | Szerver port |
| `ADMIN_EMAILS` | *(üres: nincs admin panel)* | Az admin panelhez kötött e-mail címek (vesszővel elválasztva) |
| `SECRET_KEY` | *(random generált)* | Az aláírt tokenek (socket-token) kulcsa; nélküle minden induláskor új kulcs készül |
| `SCRABBLE_DB_PATH` | `scrabble.db` | SQLite adatbázis útvonal |
| `SCRABBLE_BACKUP_DIR` | `backups` | Az admin panelről / ütemezve készült mentések mappája |
| `SCRABBLE_BASE_DIR` | *(futtatás helye / a program környéke)* | A program mappája (`web/`, `dict/`) |
| `SMTP_HOST` | `smtp.gmail.com` | SMTP szerver |
| `SMTP_PORT` | `587` | SMTP port (STARTTLS) |
| `SMTP_USER` | *(üres)* | SMTP felhasználó |
| `SMTP_PASSWORD` | *(üres)* | SMTP jelszó (app password) |
| `SMTP_FROM` | *(üres)* | Feladó email cím |
| `VAPID_PRIVATE_KEY` | *(első induláskor generált, az adatbázisban marad)* | Web Push VAPID privát kulcs (PEM, sortörések `\n`-nel; vagy a `web-push generate-vapid-keys` base64url kulcsa) |
| `VAPID_SUBJECT` | `mailto:SMTP_FROM` | Web Push `sub` mező (`mailto:` vagy `https:` cím) |
| `ADMIN_SESSION_IDLE_MINUTES`, `ADMIN_SUDO_MINUTES`, `ADMIN_IP_ALLOWLIST` | 30, 10, *(üres)* | Admin munkamenet tétlenségi ideje, sudo mód hossza, opcionális IP-engedélylista |
| `WORD_REJECT_THRESHOLD` | `1` | Ennyivel kell több „nem szó” szavazat a „rendes szó”-nál a szó kizárásához (szótár-építő) |

Ha az SMTP változók nincsenek beállítva, a verifikációs kódok a szerver konzolra íródnak ki (fejlesztéshez elegendő).
Az admin panelen (Rendszer → „Levelező szerver (SMTP)”) a levelező szerver újraindítás nélkül is beállítható és kipróbálható; az ott mentett beállítás erősebb a környezeti változóknál. Ugyanott a Rendszer oldalon a program GitHubról is frissíthető (a legfrissebb vagy egy megadott ágra; ha a Rust forrás változott, a `cargo build --release` is lefut), majd újraindítható.

A Web Push saját megvalósítás (külső csomag nem kell); az értesítésekhez a böngészőnek HTTPS (vagy `localhost`) kell; iPhone-on az alkalmazást előbb a Főképernyőhöz kell adni.

**Gépre jellemző beállítások (pl. másik `PORT`)**: a program mappájában lévő `.env` fájlt a szerver indításkor magától beolvassa (soronként `KULCS=érték`, opcionális `export`; a ténylegesen beállított környezeti változó erősebb). A fájl nincs a git tárban, ezért az admin panelről indított GitHubos frissítés sosem írja felül.

<details>
<summary>Példa .env fájl</summary>

```bash
PORT=8080
ADMIN_EMAILS=te@example.com
SMTP_HOST=smtp.gmail.com
SMTP_PORT=587
SMTP_USER=yourscrabble@gmail.com
SMTP_PASSWORD=abcd-efgh-ijkl-mnop
SMTP_FROM=yourscrabble@gmail.com
```

</details>

---

## Üzemeltetés otthoni szerveren / Home server

- **Folyamatos futtatás**: a `deploy/scrabble.service` egy kész systemd egység (`/opt/scrabble`, `.env` a beállításokhoz); `sudo systemctl enable --now scrabble`.
- **Fordított proxy** (nginx / Caddy): a websocket (`Upgrade`) átengedése és a `X-Forwarded-For` fejléc átadása kell; a szerver a kliens IP-jét csak a helyi (loopback) proxy fejlécéből fogadja el. Cloudflare tunnellel mindez magától működik.
- **Frissítés**: `git pull && cargo build --release`, majd újraindítás — vagy az admin panelen Rendszer → „Frissítés GitHubról” (fast-forward, szükség esetén újrafordítás, visszagörgetés hiba esetén, majd újraindítás).
- **Mentés**: az adatbázis egyetlen fájl (`scrabble.db`, WAL módban); az admin panelről kérhető konzisztens mentés / letöltés, és beállítható napi automatikus mentés (`backup_daily`, `backup_keep`).
- **Régi adatbázis**: a Python verzió `scrabble.db` fájlja változtatás nélkül használható (a migrációk futnak indításkor).

### Teljesítmény a Python verzióhoz képest

Ugyanazon a gépen (4 mag), ugyanarról a mintaadatbázisról (400 felhasználó, 4000 játék) indított két szerver összevetése; a teljes táblázat, a módszer és az újrafuttatás: [docs/PERFORMANCE.md](docs/PERFORMANCE.md).

| Mérőszám | Python | Rust |
|---|---|---|
| Indulás az első kérésig | 7,3 mp | 1,9 mp |
| Memória üresjáratban | 164 MiB | 66 MiB |
| Teljes játékok (szoba + passzok + mentés) | 17,5 játék/mp | 101,5 játék/mp |
| Chat körülfordulás p99 | 42,9 ms | 3,4 ms |
| Szó-ellenőrzés (8 szó) | 130 kérés/mp | 1432 kérés/mp |
| Bejelentkezés (PBKDF2) | 12 kérés/mp | 105 kérés/mp |
| Robot-aréna (bot–bot játékok, egy szál) | 32,1 mp | 7,7 mp |

---

## Karbantartó eszközök / Tools

| Eszköz | Mire jó |
|---|---|
| `target/release/word_review` | Szótár-építő tömeges párja: `sample` (véletlen szavak átnézésre), `apply` (elutasított szavak felvétele a `dict/hu_rejected.txt`-be), `stats` |
| `target/release/bot_arena` | A robot-fokozatok erejének mérése bot–bot játékokkal (`ladder`, `match`, `adapt`) |
| `target/release/build_attested` | A `dict/hu_attested.txt` előállítása szógyakorisági listából |
| `scripts/perf.sh` | Teljesítményteszt a Python és a Rust verzió között (`docs/PERFORMANCE.md`) |
| `target/release/engine_duel` | A robot párharca egy külső Scrabble motorral (`cargo build --release --features engine-duel --bin engine_duel`; leírás és eredmények: [docs/ENGINE_DUEL.md](docs/ENGINE_DUEL.md)) |

---

## Projekt struktúra / Project Structure

```
Cargo.toml, build.rs   — Rust projekt (szerver + eszközök + mérés)
src/                   — a Rust forrás, témakörönként:
  engine/              —   játékszabályok: zsetonok, tábla, játékmenet, játékosok, megtámadás
  robot/               —   robot ellenfél (lépéskeresés, 10 fokozat, „igazodik hozzám”), napi feladvány, játékelemzés
  words/               —   hu_HU szóellenőrző (ragozott alakokkal), szótár, gyakorló módok, szótár-építő
  accounts/            —   jelszó-hash, socket-token, értékszám (ELO), kitüntetések
  services/            —   levelezés (SMTP), Web Push, Cloudflare tunnel, forgalomkorlát
  db/                  —   SQLite réteg (séma: schema.sql)
  server/              —   HTTP útvonalak, Socket.IO események, szobák és szerverállapot, robotlépések
  admin/               —   admin panel (logika, őr, végpontok)
  bin/                 —   karbantartó eszközök (word_review, bot_arena, build_attested)
web/
  static/              —   nyilvános kliens: app.js, i18n, style.css, PWA (manifest, ikonok, offline oldal)
  templates/           —   index.html, admin.html, sw.js (service worker)
  admin/               —   az admin felület kliense (csak az őrzött /admin útvonalon érhető el)
dict/                  — beágyazott hu_HU szótár (hu_HU.aff / .dic), használati lista (hu_attested.txt), elutasított szavak
tests/                 — integrációs tesztek (*.rs), segédek, aranyfájlok (golden/), összevetés a régi szerverrel (compat/)
benches/compare.rs     — teljesítményteszt a Python és a Rust verzió között
scripts/, deploy/      — indító és mérő szkriptek, systemd egység
docs/                  — ADMIN_PANEL.md (specifikáció), PERFORMANCE.md (mérések)
```

A részletes leírás a `CLAUDE.md`-ben van.

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
- Az erősség újramérése: `target/release/bot_arena ladder -n 24 -j 4`; az „igazodó” robot ellenőrzése: `target/release/bot_arena adapt 5`.

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
cargo test                        # minden teszt
cargo test --test admin_users     # egy fájl
cargo clippy --all-targets        # lint
```

Az integrációs tesztek valódi szervert indítanak ideiglenes adatbázissal, és valódi HTTP / Socket.IO kliensekkel beszélnek vele (az e-mail és a Web Push is valódi, helyi „szolgáltatásra” megy). A node-ot / Playwrightot igénylő tesztek ezek hiányában kimaradnak.

| Terület | Tesztek |
|---|---|
| Játéklogika, adatbázis, auth, HTTP API | 119 |
| Socket.IO: szobák, újracsatlakozás, megfigyelés, visszajátszás, barátok, robotok | 163 |
| Levelezős játék, napi feladvány, kétjegyű betűk, gyakorlás, szótár-építő, push, e-mail | 258 |
| Admin panel (hozzáférés, napló, felhasználók, szobák, játékok, szótár, kommunikáció, rendszer, moderáció, levelezés, frissítés, böngészős próba) | 500 |
| Kliens: fordítások, admin kliens, kliens ↔ szerver összhang, node-os logika | 104 |
| Eszközök, aranyfájlok (egyezés a Python verzióval) | 30 |
| Egységtesztek a modulokban | 96 |

**Összesen: 1270 teszt**

---

## Hibaelhárítás / Troubleshooting

<details>
<summary><strong>Szótár (értelmetlen szavak is érvényesnek látszanak)</strong></summary>

A szótár a `dict/hu_HU.aff` és `dict/hu_HU.dic` fájlokból töltődik be, külön rendszercsomag (enchant, hunspell) nélkül. Indításkor a szerver kiírja: `Szótár: beágyazott hu_HU (… szótő)`. Ha ehelyett a `FIGYELEM: A szótár nem tölthető be` sor jelenik meg, hiányzik vagy sérült a `dict/` mappa, és a szavak ellenőrzése ki van kapcsolva (a Szótár-eszköz ilyenkor hibát jelez, nem "érvényes"-t).

</details>

<details>
<summary><strong>Fordítási hiba (cargo build)</strong></summary>

Frissítsd a Rust toolchaint (`rustup update`; legalább 1.85 kell), és ellenőrizd, hogy van C fordító (a beépített SQLite-hoz). Linuxon: `sudo apt install build-essential`.

</details>

<details>
<summary><strong>„A program nem találja a web/ vagy a dict/ mappát”</strong></summary>

A program a futtatás mappájában, a futtatható fájl környékén, végül a fordítás helyén keresi a `web/` és a `dict/` mappát. Máshonnan indítva add meg: `SCRABBLE_BASE_DIR=/út/a/repohoz target/release/scrabble`.

</details>

<details>
<summary><strong>A szótár nem ismeri fel a szavakat</strong></summary>

Ellenőrizd, hogy a `dict/hu_HU.dic` és `dict/hu_HU.aff` fájlok megvannak a program mappájában. Ezek a beágyazott szótár fájlok, amelyeket a program automatikusan használ.

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
