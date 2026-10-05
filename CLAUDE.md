# Magyar Scrabble Klón

## Áttekintés
Webes magyar Scrabble játék online multiplayer támogatással. **Rust** backend (axum + socketioxide + tokio, SQLite), vanilla JS frontend. Egyetlen, könnyen futtatható program (otthoni szerverre is).

> A korábbi Python (Flask + Socket.IO) változat a `python-final` ágon / címkén van. A Rust verzió **protokoll- és adatbázis-kompatibilis** vele: ugyanazok a HTTP útvonalak és Socket.IO események, ugyanaz az SQLite séma (a meglévő `scrabble.db` változtatás nélkül folytatható), a frontend (`web/`) változatlan. Az egyezést aranyfájlok (`tests/golden/`) és differenciális összevetés (`tests/compat/`) őrzi; a teljesítmény-összevetés a `docs/PERFORMANCE.md`-ben van.

## Futtatás
```bash
cargo build --release                   # egyszer (és frissítés után); Rust 1.85+ (edition 2024)
scripts/run.sh                          # Cloudflare tunnel-lel (publikus URL)
scripts/run.sh --no-tunnel              # csak helyi hálózat
# vagy közvetlenül: target/release/scrabble [--no-tunnel]
cargo run -- --no-tunnel                # fejlesztéshez
```
Böngészőben: http://localhost:5000. A gépre jellemző beállítások a `.env` fájlba kerülnek (minta: `.env.example`); otthoni szerverre: `deploy/scrabble.service` (systemd).

Környezeti változók (mind opcionális): `PORT` (5000), `ADMIN_EMAILS`, `ADMIN_SESSION_IDLE_MINUTES` (30), `ADMIN_SUDO_MINUTES` (10), `ADMIN_IP_ALLOWLIST`, `SMTP_HOST` / `SMTP_PORT` / `SMTP_USER` / `SMTP_PASSWORD` / `SMTP_FROM`, `SECRET_KEY` (nélküle induláskor véletlen kulcs), `VAPID_PRIVATE_KEY` / `VAPID_SUBJECT`, `SCRABBLE_DB_PATH` (`scrabble.db`), `SCRABBLE_BACKUP_DIR` (`backups`), `SCRABBLE_BASE_DIR` (a program mappája: ahol a `web/` és a `dict/` van), `SCRABBLE_REJECTED_FILE` (az elutasított szavak listája, alapból `dict/hu_rejected.txt`), `WORD_REJECT_THRESHOLD` (1).

## Fájlstruktúra

```
.
├── Cargo.toml, Cargo.lock, build.rs   — Rust projekt (a `scrabble` szerver + 3 eszköz + `compare` mérés)
├── README.md, CLAUDE.md, .env.example — dokumentáció és minta beállítás
├── src/                               — a Rust forrás (lásd lent)
├── web/                               — a böngészős kliens
│   ├── static/                        —   nyilvános fájlok (`/static/…`): app.js, style.css, i18n, PWA, ikonok
│   ├── templates/                     —   index.html, admin.html, sw.js
│   └── admin/                         —   az admin felület kliense (`/admin/assets/…`, csak az őrzött útvonalon)
├── dict/                              — hu_HU szótár (hu_HU.aff / .dic), hu_attested.txt, hu_rejected.txt
├── tests/                             — integrációs tesztek (`*.rs`), `common/`, `frontend_support/`, `golden/`, `compat/`, `browser/`
├── benches/compare.rs                 — teljesítményteszt a Python és a Rust verzió között
├── scripts/                           — run.sh (indítás), perf.sh (mérés), python-baseline.sh (a régi verzió előkészítése)
├── deploy/scrabble.service            — systemd egység az otthoni szerverhez
└── docs/                              — ADMIN_PANEL.md (specifikáció), PERFORMANCE.md (mérési eredmények)
```

A régi Python (Flask + Socket.IO) verzió a `python-final` ágon / címkén van (a repo fő ágából az átírás után kikerült);
a Rust szerver ugyanazt a HTTP / Socket.IO protokollt és ugyanazt az SQLite sémát beszéli, a frontend változatlan.

### `src/` — témakörönként csoportosítva
A moduloknak rövid, lapos elérési útjuk is van (`crate::tiles` = `crate::engine::tiles`, a `lib.rs` újraexportálja), a kód és a tesztek ezeket használják.

- `main.rs` — indulás: konfiguráció, adatbázis, szótár és szókincs betöltése, napi feladvány, háttérfeladatok, tunnel, szerver; `lib.rs` — a modulok és az újraexportálások
- `app.rs` — az `App` (megosztott állapot: konfiguráció, adatbázis, beállítások, `Mutex<ServerState>`, forgalomkorlát, levelező, push, tunnel, háttérfeladatok állapota); zárolási sorrend: állapot → adatbázis
- `config.rs` — a helyi `.env` fájl beolvasása (`load_env_file`; gitignorált, a frissítés nem írja felül — ide kerül pl. a `PORT`), SMTP (környezeti alapérték), DB, forgalomkorlátok és admin (`ADMIN_EMAILS`, tétlenségi / sudo idő, IP-lista) konfiguráció a környezeti változókból; `base_dir()` / `dict_dir()`
- `settings.rs` — Futásidejű beállítások (`app_settings`, `cfg.<kulcs>`): `definitions`, `get` / `store` / `validate` / `describe`, `maintenance()`; újraindítás nélkül hatnak
- `util.rs` — időbélyegek, véletlen tokenek, szigorú egész-számolvasás és egyéb apróságok; `async_games.rs` — Levelezős játék: `build_game`, névütközés-kezelés, a lobby-lista összegzése
- `engine/` — a játék szabályai
  - `tiles.rs` — Magyar betűkészlet (100 zseton), `TileBag`, `tokenize_word` (szó → zsetonok), `forms_digraph` (két külön zseton kétjegyű betűt adna-e: S+Z, C+S, Z+S… — nem megengedett; lásd „Kétjegyű betűk”)
  - `board.rs` — 15×15 tábla, premium mezők, szó elhelyezés validáció és pontozás
  - `game.rs` — Játéklogika (`Game`), körök, pontozás, játék vége (6 pont nélküli kör; döntetlennél több győztes: `winners`), challenge rendszer, kör időlimit, robotok (`add_bot`, `bot_level`), szerkezetes `last_action_info`, lépéstörténet (`get_history`, a lépésnapló a kezet is rögzíti: `rack`), előnézet (`preview_placement`), visszavonás (`withdraw_pending`), napi feladvány mód (`puzzle`), levelezős mód (`async_mode`, határidő, `expire_turn`, `resign`)
  - `player.rs` — `Player` (id, név, kéz, pontszám, disconnected állapot, `is_bot`, `difficulty`: a robot fokozata 1–10 vagy `Auto` („igazodik hozzám”), a régi `easy`/`medium`/`hard` átképeződik; `timeouts`, `resigned`)
  - `challenge.rs` — Challenge (megtámadás) logika, szavazási állapotgép, vote resolution
- `robot/`
  - `ai.rs` — Robot ellenfél: szókincs (a szótár tőszavai + ~225 000 gyakori ragozott alak, a furcsa alakok nélkül), horgonyalapú lépésgenerátor, 10 fokozatú nehézség (`parse_level`, `profile`) + „igazodik hozzám” (`adaptive_level`, `LEVEL_STRENGTH`), tippek
  - `analysis.rs` — Játékelemzés: lépésenként a legjobb lehetséges lépés és a kint maradt pont (a robot motorjával), gyorsítótárazva
  - `daily.rs` — Napi feladvány: dátumból determinisztikusan előállított tábla + kéz, játék felállítása, eredmény rögzítése
- `words/`
  - `affix.rs` — Tisztán Rust, Hunspell-szerű szóellenőrző a `dict/hu_HU.{aff,dic}` fájlokhoz: szótő, előtag, legfeljebb két toldalék, folytatási osztályok (AF aliasok), NEEDAFFIX/ONLYINCOMPOUND/FORBIDDENWORD; **összetételi szabályok nélkül**; a morfológiai címkék (AM aliasok) alapján szűri a furcsa alakokat (lásd „Furcsa szavak szűrése”); `inflected_forms` — ragozott alakok előállítása a saját szabályaiból (a robot szókincséhez)
  - `dictionary.rs` — Magyar szótár-ellenőrzés a beágyazott szóellenőrzővel (nincs rendszerfüggőség), magánhangzó nélküli rövidítések kizárása, `is_available`/`warm_up`, tömeges `filter_valid` (gyorsítótárral), `suggest_words` (egy betűnyi szerkesztés), elutasított szavak (`dict/hu_rejected.txt` + a szótár-építő szavazatai: `is_rejected`, `set_voted_rejected`, `mark_voted_rejected`, `rejected_version`)
  - `practice.rs` — Gyakorló módok motorja: szókvíz (`make_quiz`, módok: vegyes / 2 / 3 zsetonos / hosszú–rövid csapdák), betűvadász és bingó-edző (`make_rack`, `rack_words`, `check_rack_word`), a rövid (2–3 zsetonos) szavak listája; állapotmentes
  - `word_review.rs` — Szótár-építő: véletlen szavak átnézésre (`next_words`, `sample_words`), szavazat és visszavonás (`record_vote`, `undo_vote`), a szavazatokból kizárt szavak betöltése (`refresh`), összesítő (`stats`); lásd „Szótár-építő”
- `accounts/` — `password.rs` (werkzeug-kompatibilis PBKDF2-SHA256 jelszó-hash), `socket_auth.rs` (aláírt, rövid életű token a Socket.IO identitás igazolásához: `set_name`), `elo.rs` (többjátékos ELO, páronként számolva, K = 32 / 20), `achievements.rs` (Kitüntetések: 10 + 1 jelvény; játékonkénti kiértékelés a lépésnaplóból — `evaluate_game` — és összesítők — `cumulative_badges`)
- `services/`
  - `mail.rs` — A levelező szerver beállítása (az admin panelen mentett felülbírálat — `app_settings`, `mail.smtp` — vagy a környezeti `SMTP_*`: `current`, `is_configured`, `public_view` — jelszó nélkül) és az SMTP küldés (verifikációs kód, egyszerű levelek; háttérszálon), a titkosítási mód szerint (`starttls` | `ssl` | `none`), ellenőrzött tanúsítvánnyal; `check_connection` / `classify_error` — az admin panel kapcsolat-próbája (biztonságos hibakódokkal); lásd „Levelező szerver (SMTP)”
  - `push.rs` — Web Push: VAPID kulcsok, feliratkozások, „Te jössz!” üzenet (saját RFC 8291 / 8292 megvalósítás: p256, aes-gcm, hkdf — nincs külső szolgáltatás-függőség)
  - `ratelimit.rs` — Generikus rate limiter Socket.IO (SID) és HTTP (IP) endpointokhoz
  - `tunnel.rs` — Cloudflare tunnel subprocess kezelés (indítás/leállítás)
- `db/` — SQLite (rusqlite, WAL): `schema.sql` (a séma, a Python verzióval azonos), `users.rs` (regisztráció, login, session, kitiltás / némítás, belépési napló), `games.rs` (játék mentés/visszatöltés/lépésnaplózás, ranglista), `misc.rs` (beállítások, push, szótár-építő, napi feladvány, barátok…); `admin_audit` tábla (triggerekkel csak hozzáfűzhető), az admin panel táblái (lásd „Admin panel”)
- `server/` — `mod.rs` (az útvonalak és a Socket.IO bekötése: `InOrder` kinyerő, `SYNC_EVENTS` táblázat, kapcsolat-őr), `http.rs` (HTTP útvonalak: auth (+ push feliratkozás), game (lépések, megosztás, elemzés, levelezős lista), public (ranglista, szótár-ellenőrző, napi feladvány, gyakorló módok), index + PWA: `/manifest.webmanifest`, `/sw.js`), `events.rs` (szobák, lobby, csatlakozás, türelmi idők), `play.rs` (lerakás, csere, passz, szavazás, chat, előnézet, bejelentés), `extras.rs` (napi feladvány, levelezős játék, megfigyelők, barátok, admin események), `core.rs` (robotlépések — `schedule_bot_turn` / `play_bot_turn` —, mentés, időzítők, push a saját körre), `net.rs` (kliens-IP, sütik), `room.rs` (`Room`: szoba állapot, owner, beállítások, chat, timer invalidálás, megfigyelők, robotlépés-azonosító), `state.rs` (`ServerState`: szobák, játékosok, tokenek, reconnect tracking, megfigyelők, élő játékok)
- `admin/` — Admin panel: `mod.rs` (napló — `action`: kötelező indoklás, a művelettel egy tranzakcióban; `record`: `view.*` események indoklás nélkül —, közös segédek: `clean_text`, `order_by`, `parse_until`…, `ip_allowed`), `session.rs` (munkamenet: `session_status`, `touch`, `sudo_active`, `reauth`, `grant_sudo`, `end_sudo`), `audit.rs` (naplólekérdezés és CSV), `routes.rs` (az őr: 404 / IP-lista / forgalomkorlát / CSRF / tétlenség; jogosultsági szintek: `Normal` / `Danger` / `Sudo`; `route_table` a hozzáférési tesztekhez), `api/*.rs` (a végpontok: `users`, `game`, `dict`, `comm`, `system`), `users.rs`, `live.rs` (élő szobák, beavatkozások, számlálók, figyelmeztetések, grafikonok), `games.rs`, `dict.rs`, `comm.rs`, `stats.rs`, `moderation.rs`, `security.rs`, `system.rs`, `mail.rs` (a levelező szerver beállításának ellenőrzése, mentése, visszaállítása, kapcsolat-próbája), `update.rs` (frissítés GitHubról — `check`, `apply` — és újraindítás; lásd „Frissítés GitHubról és újraindítás”)
- `bin/` — karbantartó eszközök (nem részei a szervernek)
  - `bot_arena.rs` — Robot-aréna: a fokozatok erejének mérése bot–bot játékokkal (`ladder`, `match`, `adapt`; kalibrációhoz)
  - `build_attested.rs` — a `dict/hu_attested.txt` előállítása egy szógyakorisági listából (elírásszűrővel)
  - `word_review.rs` — a szótár-építő tömeges párja: `sample` (véletlen szavak átnézésre, pl. AI-nak), `apply` (az elutasított szavak felvétele a `dict/hu_rejected.txt`-be), `stats`

### `web/` és `dict/`
- `dict/` — Beágyazott hu_HU hunspell szótár fájlok (hu_HU.dic, hu_HU.aff) + `hu_attested.txt` (a kockázatos levezetések ténylegesen használt alakjai, CC BY-SA 4.0) + `hu_rejected.txt` (a szótár-építő átnézésén elutasított szavak)
- `web/templates/index.html` — Egyoldalas UI: auth (3 tab), lobby (Kezdőlap / Új szoba / Mentett játékok / Barátok / Levelezős / Gyakorlás / Ranglista), várakozó szoba, játék, profil, visszajátszás (+ elemzés); közös SVG ikon-sprite, minden képernyőn egységes felső sáv (`app-topbar`); minden szöveg `data-i18n*` jelölésű
- `web/templates/admin.html` — Az admin panel oldala (nincs beágyazott szkript / stílus: szigorú CSP; a Socket.IO kliens a cdnjs-ről); csak adminnak
- `web/templates/sw.js` — Service worker (sablon, `VERSION` = kliens fájlok mtime-ja; az `/admin` soha nem kerül gyorsítótárba)
- `web/static/app.js` — Kliens logika, drag & drop, pinch-to-zoom, Socket.IO kommunikáció, auth flow, téma váltás, hang rendszer (SoundManager, SoundSettings), megfigyelő mód, ranglista, szótár-böngésző, előnézet, zsetonszámláló, tipp, gyorsbillentyűk, PWA telepítés; modulok: `Daily`, `Practice` (+ `PracticeStore`: a gyakorlás statisztikája a `localStorage`-ban), `AsyncGames`, `LobbyNav`, `Push`, `Badges`, `Replay` (elemzés, megosztás); közös segédek: `makeEl`, `makeAvatar`, `tokenizeWord` (a szerver `tokenize_word`-jének mása), `fillWordTiles`
- `web/static/i18n.js` + `web/static/i18n-data.js` — Többnyelvű felület: `t()`, `tServer()`, `I18N.setLang()`; a fordítások (hu/en) szigorú JSON-ban
- `web/static/style.css` — Apple HIG ihletésű design rendszer (tokenek, iOS-szerű komponensek), sötét/világos téma, reszponzív layout (asztali / tablet / telefon, álló és fekvő), 17. szakasz: új funkciók és animációk, 18. szakasz: Gyakorlás és Levelezős (újratervezve: közös elemek, hub, szókvíz, betűvadász, szólisták, levelezős kártyák és lap)
- `web/static/manifest.webmanifest`, `web/static/offline.html`, `web/static/icons/` — PWA
- `web/admin/` — Az admin felület kliens fájljai: `admin.js` (mag: API, munkamenet, zárolás, sudo), `admin-ui.js` (komponensek, szűrős listanézet, űrlap-párbeszéd, grafikonok, tábla), `admin-views-a.js` / `-b.js` / `-c.js` (menüpontok), `admin-main.js` (útvonalválasztó, élő kapcsolat, Ctrl+K kereső, indítás), `admin.css`, `admin-i18n.js`, `admin-boot.js`; szándékosan NEM a nyilvános `web/static/` mappában, csak az őrzött útvonalon érhetők el

### Tesztek és eszközök
- `tests/*.rs` — integrációs tesztek (lásd „Tesztek”); `tests/common/` (szerver- és kliens-segédek: `TestServer`, `Http`, `Sio`, hamis SMTP és push szolgáltatás), `tests/frontend_support/` (a kliens-tesztek segédei), `tests/golden/` (a Python implementációval előállított aranyfájlok + generátoraik), `tests/compat/` (differenciális összevetés a régi szerverrel), `tests/browser/` (a böngészős füstpróba Playwright szkriptje)
- `benches/compare.rs` + `scripts/perf.sh` — teljesítményteszt a Python és a Rust verzió között (eredmény: `docs/PERFORMANCE.md`)
- `scripts/run.sh` — a szerver indítása; `scripts/python-baseline.sh` — a régi verzió (`python-final`) munkamásolata és virtuális környezete az összehasonlításokhoz; `deploy/scrabble.service` — systemd egység

## Funkciók
- 1-4 játékos (egyedül is játszható)
- **Robot ellenfelek**: 1–3 robot, 10 nehézségi fokozat (1 újonc · 2 kezdő · 3 könnyű · 4 mérsékelt · 5 alkalmi · 6 közepes · 7 ügyes · 8 haladó · 9 erős · 10 mester) vagy **„Igazodik hozzám”** (az ember utolsó ~6 körének átlagához állítja az erejét); a robotok ragozott szavakat is raknak; egyedül játszva tipp (3 legjobb lépés, szobánként állítható / kikapcsolható limit: 0/1/3/5/10); robotos játék nem számít a ranglistába
- **Megfigyelő mód**: nyilvános, folyamatban lévő játék megfigyelése (lobby „Élő játékok”), privát játék kóddal
- **Ranglista**: **értékszám (ELO, alapértelmezett)** / győzelmek / nyerési arány / átlagpont / legjobb játék (csak regisztrált, robot nélküli, befejezett játékok)
- **Napi feladvány**: naponta egy közös tábla + kéz, cél a legtöbb pontot érő lépés; napi ranglista, tegnapi megoldás
- **Gyakorlás** (főoldal + al-nézetek): **Szókvíz** (vegyes / 2 / 3 zsetonos / hosszú–rövid csapdák, 10 vagy 20 kérdés, sorozat, magyarázat és javaslatok), **Betűvadász** (hét zsetonból minél több szó, opcionális 60 / 120 mp, tipp, bónusz szavak), **Bingó-edző** (a hét zsetonos szó megtalálása, sorozat), **Hibáim** (a kvízben eltévesztett szavak pakli, kétszeri helyes válasz kivesz), **Szólisták** (2–3 zsetonos szavak keresővel, kezdőbetű-szűrővel, rendezéssel + zsetontáblázat), **Szótár-építő** (véletlen szavak: „Rendes szó” / „Nem szó” / „Nem tudom”; a „Nem szó” kizárja a szót a játék szótárából; csak bejelentkezve; visszavonás); napi sorozat és statisztika az eszközön; a napi feladvány kiemelt kártyaként a tetején
- **Levelezős játék**: barátokkal órák / napok alatt lépkedve (24 óra – 7 nap/lépés), push értesítéssel; játékkártyák (soron lévő, hátralévő idő sáv), új játék alsó lapon (idő: szegmentált választó, barátok: kijelölhető sorok)
- **Játékelemzés**: a játék végén lépésenként a legjobb lehetséges lépés és a kint maradt pont
- **Kitüntetések** (bingó, 100+ pontos lépés, 8+ zsetonos szó, joker, 300 pontos játék, robotverő, napi feladvány…), **visszajátszás megosztása linkkel** (`/?replay=TOKEN`), **visszavonás** (függő lerakás visszavonása szavazás előtt, Ctrl+Z)
- **Web Push**: értesítés, ha rád kerül a sor, miközben nem nézed a játékot (profil → kapcsoló)
- **Admin panel** (`/admin`, csak a `ADMIN_EMAILS` címeihez kötött fióknak, másnak azonos 404): felhasználók (kitiltás, némítás, jelszó, értékszám…), élő szobák beavatkozásokkal, játékarchívum és visszajátszás, ELO újraszámolás, szótár-kezelés, moderáció (bejelentések, tiltott szavak, chat napló), közlemények és karbantartási mód, push / e-mail, statisztika, biztonság, rendszer (mentés, napló), funkciókapcsolók, csak hozzáfűzhető admin napló kötelező indoklással; lásd „Admin panel”
- **Játékos oldali moderáció**: játékos / chat üzenet bejelentése, közlemény-banner (hu / en), karbantartási mód, a kitiltás okának és lejáratának megjelenítése
- **Szótár-böngésző**: szó-ellenőrző párbeszéd (érvényes-e, pontérték, zsetonok, javaslatok)
- **Többnyelvű felület**: magyar + angol (`localStorage('scrabble-lang')`), a szótár magyar marad
- **PWA**: telepíthető, kapcsolat nélkül induló felület (service worker), ikonok, manifest
- **Animációk**: lerakás, ellenfél lépése, pontszám felugró, kör váltás, drag & drop visszajelzés (foglalt mező piros)
- **Kényelmi funkciók**: élő pontszám-előnézet · zsetonszámláló · betűtartó keverés/rendezés + gyorsbillentyűk · meghívó link (`/?join=KÓD`) · lépéstörténet + utolsó lépés kiemelése + legjobb lépés a játék végén
- Online multiplayer: lobby, szobák, Cloudflare tunnel automatikus publikus URL
- **Nyilvános és privát szobák**: privát szoba csak 6-jegyű kóddal csatlakozható, nyilvános szobák a lobbyban listázva
- Felhasználói fiók rendszer: regisztráció (email verifikáció), bejelentkezés, vendég mód
- Teljes magyar betűkészlet (SZ, CS, GY, LY, NY, ZS, TY többkarakteres betűk); a kétjegyű betű **csak a saját zsetonjával** rakható ki (külön S + Z nem; lásd „Kétjegyű betűk”)
- Standard Scrabble pontozás: DL, TL, DW, TW premium mezők
- 50 pont bónusz mind a 7 zseton kirakásakor
- Drag & drop és kattintásos betű elhelyezés
- Joker (üres zseton) bármely betűként használható
- Betűcsere és passz
- **Játék vége**: ha valaki kirakta az összes zsetonját (és a zsák üres), vagy **6 egymást követő pont nélküli kör** után (passz, csere és elutasított lerakás is számít; a pontot érő lerakás nullázza, `Game.scoreless_turns`). **Döntetlen**: egyenlő végső pontnál mindenki győztes (`Game.winners`, `winner` csak ha egyetlen győztes van); a ranglistán és a profilban mindegyikük győzelmet kap (`is_winner`), a kliens mindegyiket kiemeli
- **Körönkénti időlimit**: opcionális (0/60/90/120/180/300 mp), lejáratkor automatikus passz
- Challenge (megtámadás) mód: 2 játékosnál kötelező elfogadás, 3+ játékosnál szavazásos rendszer (nincs szótár)
- Játék közbeni chat: szöveges üzenetküldés a szobában
- **Sötét / világos téma**: automatikus detektálás (`prefers-color-scheme`), manuális váltás, `localStorage`-ban mentve, villanásmentes betöltés
- **Hang effektek**: Web Audio API (nincs külső fájl), szintetizált hangok — betű lerakás, szavazás, challenge eredmény, kör értesítő, chat, játék kezdés/vége; hangerő-csúszka + kategóriánkénti kapcsolók, `localStorage`-ban mentve
- **Újracsatlakozás (grace period)**: 120 másodperc a visszacsatlakozásra ha a kapcsolat megszakad játék közben; a várakozó szoba tulajdonosának 10 perc (token alapú)
- **Pinch-to-zoom**: mobilon a tábla nagyítható/kicsinyíthető csípő mozdulattal
- **Szótár-böngésző (Challenge fázis)**: A megtámadás során a lerakott szavak kattintható linkek, amelyek egy új lapon indítanak Google keresést az adott szóra ("A magyar nyelv értelmező szótára" fókusszal).
- Szótár-ellenőrzés: beágyazott hu_HU szótár (`src/words/affix.rs`, tisztán Rust — rendszerfüggőség nélkül működik fejlesztői gépen, Windowson és tárhelyen). A szavakat kisbetűvel keresi, így a tulajdonnevek (pl. DUNA, BUDAPEST) nem érvényesek; a Hunspell összetételi szabályait nem használja (értelmetlen összetételeket, pl. PAGONYAGY, nem fogad el); a magánhangzó nélküli tételek (KG, DB, TV, betűnevek) nem érvényesek, az indulatszavak (BRR, HM, PSZT) igen; a nyelvtanilag lehetséges, de értelmetlen alakokat (FALIM, TAROM, ÉJÉK, BLÖKIÜL, VU) kiszűri (lásd „Furcsa szavak szűrése”). A szótár-építőben „nem szó”-nak ítélt szavak (`dict/hu_rejected.txt`, felhasználói szavazatok) szintén érvénytelenek (lásd „Szótár-építő”). Ha a szótár nem tölthető be, a Szótár-eszköz 503-at ad (nem jelöl érvényesnek semmit)
- **Játék mentés / visszatöltés**: manuális mentés (owner-only) a kilépés menüből, lobby-first restore flow
- **Visszajátszás**: befejezett játékok lépésről lépésre visszanézhetők (board snapshot-okkal)
- **Kilépés menü**: owner: mentés+kilépés / kilépés mentés nélkül / mégsem; nem-owner: kilépés / mégsem
- **Profil oldal**: statisztikák (játszott, győzelem, nyerési arány, átl. pontszám, értékszám) + kitüntetések + beállítások (betűtartó helye, értesítés) + játékelőzmények (értékszám-változással); a felső sávban a lobby navigációs sora is látszik (egy kattintással bármelyik lobby-fülre)

## Biztonság
- `SECRET_KEY`: környezeti változóból (`SECRET_KEY`) vagy futásidőben generált véletlenszerű kulcs
- CORS: `cors_allowed_origins='*'` — minden origin engedélyezett (Cloudflare tunnel kompatibilitáshoz szükséges; a biztonságot session auth és rate limiting biztosítja)
- Rate limiting: minden Socket.IO event-re (SID-alapú, `src/services/ratelimit.rs`) + IP-alapú HTTP auth endpointokra
- Input validáció: játékos nevek, szoba nevek, tile placement, email, jelszó szerver oldali validálás
- Board bounds check: a `src/engine/board.rs` és a `src/server/` is ellenőrzi a pozíciók érvényességét
- Dictionary sanitizálás: szavak regex-szel validálva a szótár-keresés előtt
- Production szerver: axum (hyper) + socketioxide tokio-n, többszálú futtatókörnyezet; a Socket.IO események feldolgozása kapcsolatonként a beérkezés sorrendjében történik (`InOrder` kinyerő, `SYNC_EVENTS` táblázat a `src/server/mod.rs`-ben); a kezelők panikja nem ejti el a kapcsolatot (`catch_unwind`)
- XSS védelem: frontend innerHTML helyett DOM API (textContent, createElement, addEventListener)
- Jelszó: PBKDF2-SHA256, 260k iteráció, random salt (a werkzeug formátumával kompatibilis: `pbkdf2:sha256:260000$salt$hash`, így a régi adatbázis jelszavai is érvényesek; `src/accounts/password.rs`)
- Verifikációs kód: 6 számjegy, 10 perc lejárat, max 5 próbálkozás/kód
- Admin panel: e-mailhez kötött (`ADMIN_EMAILS`), nem adminnak minden admin útvonal azonos 404, tétlenség utáni jelszókérés, sudo mód a romboló műveletekhez, CSRF-védelem, opcionális IP-lista, csak hozzáfűzható napló; kitiltás (fiók / IP), kötelező indoklás minden módosításnál (lásd „Admin panel”)
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
- **Várakozó szoba (a játék indítása előtt) is türelmi időt kap**: ha a kapcsolat megszakad (pl. telefonon átvált az üzenetküldő appra, hogy elküldje a kódot / meghívó linket, vagy újratölt az oldal), a játékos `disconnected` lesz, a szoba és a kód megmarad, és a kliens a tokennel visszatér (`rejoin_room`). A tulajdonosnak `_WAITING_OWNER_GRACE_PERIOD = 600` mp, mindenki másnak (és az aktív játékban mindenkinek) `_DISCONNECT_GRACE_PERIOD = 120` mp jár. Lejártakor a játékos kikerül (a tulajdonjog továbbszáll, üres szoba megszűnik). A várakozó szobában a lecsatlakozott játékos „offline” jelzést kap. Az explicit kilépés (`leave_room`) továbbra is azonnal eltávolít.
- Minden lecsatlakozás sorszámot kap (`state.mark_disconnected` → `seq`, `disconnect_is_current`): ha a játékos időközben visszatért, majd újra megszakadt a kapcsolata, a régi időzítő lejárta nem zárja le az újabb türelmi időt

### Adatstruktúrák (`src/server/state.rs` — `ServerState`, az `App` `Mutex`-e mögött)
```text
reconnect_tokens:     {token: TokenInfo {room_id, player_name, sid, auth_info}}
sid_to_token:         {sid: token}
disconnected_players: {token: DisconnectedInfo {room_id, sid, player_name}}
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

### Auth HTTP route-ok (`src/server/http.rs`)
- `POST /api/auth/request-code` — email validálás, kód küldés
- `POST /api/auth/verify-code` — 6 számjegyű kód ellenőrzés
- `POST /api/auth/register` — jelszó + név, fiók létrehozás, auto-login (csak a kóddal előzőleg megerősített email címre, 30 percig érvényes, egyszer használható)
- `POST /api/auth/login` — email + jelszó
- `POST /api/auth/logout` — session törlés
- `GET /api/auth/me` — session cookie ellenőrzés; csak az adminnál szerepel a `user.is_admin: true` (másnál a kulcs sincs), ugyanez a belépés / regisztráció válaszában
- `GET /api/auth/profile` — statisztikák és játékelőzmények (session cookie)
- `GET /api/auth/socket-token` — rövid életű (5 perc) aláírt token a Socket.IO `set_name`-hez (session cookie)
- `GET /api/game/<int:game_id>/moves` — lépések listája (replay-hez; `players`, `finished` is; folyamatban lévő játéknál a kezek — `rack` — nélkül)
- `POST /api/game/<id>/share` — megosztható replay-link (csak résztvevő, befejezett játék); `GET /api/replay/<token>` — nyilvános megosztott visszajátszás
- `GET /api/game/<id>/analysis` — játékelemzés (háttérben számolódik: `status: running|ready|unavailable|error`)
- `GET /api/async/games` — a felhasználó folyamatban lévő levelezős játékai (akinél a sor, az elöl)
- `GET /api/daily` / `GET /api/daily/leaderboard?date=` — napi feladvány, napi ranglista
- `GET /api/practice/quiz?n=&mode=mixed|2|3|tricky` (a kérdések + `tiles`), `POST /api/practice/answer`, `GET /api/practice/short-words?length=2|3`
- `GET /api/practice/rack?kind=hunt|bingo` (7 zseton + az összes kirakható szó pontértékkel), `POST /api/practice/rack-word` (`{rack, word}`: kirakható-e és érvényes-e a listán nem szereplő szó)
- `GET /api/practice/word-review?n=` (átnézésre váró szavak + `tiles` + `stats`, bejelentkezve), `POST /api/practice/word-review` (`{word, valid}`: `valid: false` kizárja a szót; `rejected`, `stats`; 409, ha a szó már nincs a szótárban), `POST /api/practice/word-review/undo` (`{word}`: a saját döntés visszavonása), `GET /api/practice/word-review/stats`
- `GET /api/push/public-key`, `POST /api/push/subscribe|unsubscribe` — Web Push (bejelentkezve)
- `GET /api/leaderboard?metric=rating|wins|win_rate|avg_score|best_game&limit=50` — ranglista (nyilvános; bejelentkezve a saját helyezés is: `me`, `is_me`)
- `GET|POST /api/dictionary/check` (`q` / `words`, max. 8 szó) — szó-ellenőrzés: `valid`, `tiles`, `score`, `reason`, `suggestions`
- `GET /api/announcements` — a most érvényes közlemények és a karbantartási mód (nyilvános; a célcsoport a bejelentkezéstől függ; `announcements` 60/perc)
- `GET /manifest.webmanifest`, `GET /sw.js` — PWA (a service worker a gyökérről, `Service-Worker-Allowed: /`)

Session cookie: `HttpOnly` + `SameSite=Lax` + `Secure` (Cloudflare tunnel HTTPS).
IP-alapú rate limiting (`src/services/ratelimit.rs`): kód küldés 3/5perc, login 10/5perc, regisztráció 3/óra, ranglista 30/perc, szótár 60/perc, admin 120/perc (romboló műveletek: 20/perc), közlemények 60/perc. Az értékek az admin panelen futásidőben felülírhatók (Biztonság → Forgalomkorlátok).

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
Ugyanide, a `.env` fájlba (nincs a git tárban) kerülhet a gépre jellemző beállítás, pl. `PORT=8080` — a GitHubos frissítés (`src/admin/update.rs`) a követett fájlokat cseréli, ezért a portot ne a forrásban írd át. Ha SMTP nincs konfigurálva, a kód a szerver konzolra íródik ki (fejlesztéshez). A környezeti változók csak az **alapérték**: az admin panel Rendszer → „Levelező szerver (SMTP)” kártyáján mentett beállítás erősebb, újraindítás nélkül hat (lásd „Levelező szerver (SMTP)”). A környezeti beállítás mindig STARTTLS-t használ.

## Tesztek

```bash
cargo test                      # minden teszt (a szótár betöltése miatt az első futás lassabb)
cargo test --test admin_users   # egy fájl
cargo test --lib                # csak az egységtesztek
cargo clippy --all-targets      # lint
```

Az integrációs tesztek **valódi szervert** indítanak egy véletlen porton, ideiglenes adatbázissal (`TestServer`), és valódi HTTP (`ureq`) és Socket.IO (`tokio-tungstenite`) kliensekkel beszélnek vele; az e-mail és a push valódi, helyi „szolgáltatásra” megy (hamis SMTP kiszolgáló és push szolgáltatás visszafejtéssel). A node-ot / Playwrightot igénylő tesztek ezek hiányában kimaradnak.

| Fájl | Tesztek | Lefedettség |
|---|---|---|
| `tests/game_logic.rs` | 43 | TileBag, Board, Player, Game, Challenge szavazásos rendszer, kör időlimit, a kódátvizsgálás során talált hibák regressziós tesztjei (zsák, dupla cella, passz-végjáték, döntetlen) |
| `tests/db_tests.rs` | 40 | DB, user CRUD, jelszó hash (a Python werkzeug-hashekkel is), verifikációs kódok, session kezelés, játék mentés / lépésnapló, ranglista, ELO, kitüntetések tárolása |
| `tests/http_auth.rs` | 23 | HTTP auth route-ok, cookie flow, e-mail megerősítés, rate limit, socket-token |
| `tests/http_public.rs` | 13 | Ranglista (DB + route, robotos játékok kizárása), szótár API, PWA végpontok, statikus fájlok |
| `tests/socket_rooms.rs` | 38 | Socket.IO eventek: lobby, szobák, privát szobák, challenge szavazás, chat, owner kilépés, kör időlimit, mentés / visszaállítás, sorrendhelyes feldolgozás |
| `tests/socket_reconnect.rs` | 27 | Újracsatlakozás (grace period), lecsatlakozott játékos átugrása, várakozó szoba türelmi ideje, késői csatlakozás |
| `tests/socket_spectator.rs` | 16 | Megfigyelő mód, élő játékok, szoba életciklus |
| `tests/socket_replay.rs` | 27 | Visszajátszás (perzisztencia, megosztási token, nyilvános replay, jogosultság), játékelemzés (kezek, gyorsítótár, háttérszámítás, API), kör időlimit és időzítők |
| `tests/socket_friends.rs` | 22 | Barát CRUD, kérések, felhasználókeresés, szobameghívó, online státusz |
| `tests/socket_bots.rs` | 22 | Robotok a szerveren (lépés, ütemezés, tipp, előnézet), „igazodik hozzám”, `last_action_info`, lépéstörténet |
| `tests/bots_model.rs` | 11 | Robotok a játékmodellben (szavazás, mentés), `Game.recent_stats` / `bot_level`, a robot motorja (fokozatok, ragozott alakok, csere / passz) |
| `tests/async_games.rs` | 55 | Levelezős játék: játéklogika (határidő, lejárat, feladás), szerver, mentés / visszaállítás, lista, útvonalak |
| `tests/daily_puzzle.rs` | 46 | Napi feladvány: előállítás (determinizmus), játék, ranglista, socket, HTTP |
| `tests/digraph_tiles.rs` | 18 | Kétjegyű betűk: `forms_digraph`, tábla (új / régi zseton, keresztszó, joker, régi állások), `Game` (lerakás, előnézet, megtámadásos mód), robot, socket |
| `tests/practice.rs` | 43 | Szókvíz (csapda mód), rövid szavak, válasz-ellenőrzés, betűvadász / bingó (kéz, szólista, szó-bírálat), API |
| `tests/word_review.rs` | 39 | Szótár-építő: elutasított szavak a szótárban (lista + szavazatok), szavazás / küszöb / visszavonás, mintavétel, API (belépés, ellenőrzés, rate limit) |
| `tests/push.rs` | 37 | VAPID, feliratkozások, titkosított küldés (valódi helyi „push szolgáltatás”, visszafejtéssel), API, „Te jössz!” kiváltása |
| `tests/push_env.rs` | 4 | Web Push a környezeti VAPID kulccsal (külön bináris: a környezeti változók a folyamatra hatnak) |
| `tests/email_service.rs` | 16 | E-mail küldés valódi (helyi) SMTP kiszolgálóra: fejlécek (feladó neve, kódolás, injekció elleni védelem), bejelentkezés, titkosítási módok, hibakódok, kapcsolat-próba |
| `tests/admin_access.rs` | 84 | Admin hozzáférés: nem admin → mindenhol azonos 404 (oldal, asset, API, rossz metódus, `//`) — az **összes** útvonalra az útvonaltérképből, `ADMIN_EMAILS` üres, e-mailhez kötött adminság, `is_admin` csak az adminnál, tétlenség és újraigazolás, CSRF (fejléc + azonos eredet) minden módosító útvonalon, sudo (és a `danger` / `sudo_required` védelmek), IP-lista, forgalomkorlát, Socket.IO, service worker, CSP, az admin cím nem foglalható le |
| `tests/admin_audit.rs` | 39 | Admin napló: csak hozzáfűzhető (trigger), kötelező indoklás, a művelettel egy tranzakcióban (visszagördülés), szűrők / lapozás / LIKE-menekítés, CSV (képlet-injekció elleni védelem), végpont és export naplózása |
| `tests/admin_users.rs` | 58 | Felhasználók: lista / szűrők (ékezetfüggetlen), részletek, név / e-mail / jelszó, kitiltás (belépés, session, socket, futó játék, lejárat), némítás, szótár-építő tiltás / visszavonás, értékszám, kitüntetések, export, törlés / anonimizálás, jegyzetek, védett admin fiókok |
| `tests/admin_rooms.rs` | 51 | Élő szobák: lista / szűrők / elakadás-érzékelés, részletek, kezek naplózása, beavatkozások (üzenet, kirúgás, tulajdonjog, kör átugrása, időzítő, szavazás, robot, mentés, befejezés, érvénytelenítés, feloszlatás), zsetonmegmaradás, élő admin események |
| `tests/admin_games.rs` | 43 | Játékarchívum, érvénytelenítés / visszavonás, ELO újraszámolás (= a lépésenkénti számolás), gyanús minták, levelezős beavatkozások, ranglista |
| `tests/admin_dictionary.rs` | 39 | Szó-vizsgáló, kizárt lista (írás, visszagördülés, diff), felülbírálatok, saját szavak, szavazatok törlése, bírálók, küszöb, gyorsítótárak |
| `tests/admin_comm.rs` | 43 | Közlemények (érvényességi idő, célcsoport, nyilvános végpont), karbantartási mód, push, e-mail (tömeges: megerősítés, korlát), beállítások (érvényesítés, naplózás, hatás futásidőben), statisztika, napi feladvány |
| `tests/admin_system.rs` | 57 | Rendszer (konfiguráció titkok nélkül, mentés, takarítás, naplók, folyamatok), biztonság (belépési napló, IP tiltás, forgalomkorlát, kódok, munkamenetek), áttekintés és figyelmeztetések, statisztika |
| `tests/admin_system_env.rs` | 2 | A konfiguráció a környezeti változókból: a titkok sosem látszanak (külön bináris) |
| `tests/admin_log_capture.rs` | 1 | A konzolra írt üzenetek gyűjtése a naplónézethez (külön bináris: a folyamat kimenetét átirányítja) |
| `tests/admin_moderation.rs` | 20 | Tiltott szavak, chat napló (élő / tartós), játékos oldali bejelentés, nevek átnézése |
| `tests/admin_mail.rs` | 31 | Levelező szerver: tárolás és érvényes beállítás (mentett > környezeti), érvényesítés, jelszó sosem látszik (válasz, napló), jelszó megtartása csak ugyanahhoz a kiszolgálóhoz, visszaállítás, kapcsolat-próba (sudo, csak a saját címre), élő hatás |
| `tests/admin_update.rs` | 31 | Frissítés GitHubról valódi helyi git-tárakkal: állapot, ellenőrzés (fetch), fast-forward, ágváltás, tiszta munkafa, eltérő előzmény, nem fordítható kód → visszagörgetés (valódi `cargo build`), ágnév-ellenőrzés, újraindítás, végpontok (sudo, indoklás) |
| `tests/admin_browser.rs` | 1 | Az admin felület valódi böngészőben (Playwright): minden menüpont betöltődik, nincs JS hiba, művelet-párbeszéd, sudo, kereső, élő Socket.IO események, telefon, nyelvváltás (kimarad, ha nincs node / Playwright / Chromium) |
| `tests/frontend_i18n.rs` | 27 | Fordítások teljessége (hu / en, kulcsok, helyőrzők), szerverüzenet-lefedettség (a Rust forrásból kigyűjtve), HTML lefedettség, a fordító futtatása node-ban |
| `tests/frontend_admin.rs` | 42 | Admin kliens: JS szintaxis (minden fájl), fordítások teljessége (hu/en, szerverüzenetek a Rust forrásból), a hívott API útvonalak és metódusok léte (az útvonaltérképből), minden végponthoz van felület, elem-azonosítók, menüpontok, nincs `innerHTML` / beágyazott kód, a kliens magja és a fordítás node-ban |
| `tests/frontend_client.rs` | 35 | Kliens ↔ szerver: konstansok (TILE_VALUES, premium mezők), elem-azonosítók, robot-fokozat választó, Socket.IO események, API útvonalak, JS szintaxis, a profil navigációs sora, a betűtartó CSS-változói; a kliens állapotkezelése és gyakorló logikája node-ban (félkész lerakás, kétjegyű betű, zsetonokra bontás a szerverrel egyezik, magyar ábécé-rendezés, napi sorozat, „Hibáim” pakli) |
| `tests/tools.rs` | 10 | A `word_review`, `bot_arena` és `build_attested` eszközök valódi futtatással |
| `tests/ai_golden.rs` | 1 | A robot lépésgenerátora a Python implementációval előállított aranyfájllal egyezik (az összes lehetséges lépés táblánként és kézenként) |
| `tests/dictionary_golden.rs` | 5 | A szóellenőrző (tőszavak, ragozott alakok, mutációk), a levezetés-magyarázat és a javaslatok egyeznek a Python aranyfájlokkal |
| `tests/practice_golden.rs` | 8 | Rövid szavak, szótövek, zsetonokra bontás, kvíz-válaszok és betűvadász szólisták egyeznek a Python aranyfájllal |
| `tests/vocabulary_golden.rs` | 2 | A robot szókincse (darabszám, ellenőrző összeg, minta) egyezik a Python szókincsével |
| `tests/python_compat.rs` | 4 | A Python verzió által írt adatok olvashatók: játékállapotok, SQLite fájl (jelszó-hash, munkamenetek, mentett játékok, lépésnapló) |
| `src/**` (`#[cfg(test)]`) | 96 | Egységtesztek a modulokban: tábla, zsetonok, challenge, szobák / állapot, forgalomkorlát, ELO, kitüntetések, jelszó-hash, socket-token, push titkosítás, beállítások, `.env` fájl, admin segédek, gyakorló módok belső segédei… |

**Összesen: 1270 teszt**

Segédek: `tests/common/mod.rs` — `TestServer` (szerver ideiglenes mappával, `start_with` a konfiguráció módosításához, `start_in` másik programmappához), `Http` (süti-kezelés, admin kérések: `admin_get` / `admin_post` / `sudo()`), `Sio` (Socket.IO kliens: `emit`, `call`, `wait`, `settle`, `wait_code`, `my_turn`…), `with_room`; `tests/common/admin.rs` — `make_user`, `audit_rows`, `finished_game`, `live_room`, `set_setting`…; `tests/common/smtp.rs`, `push.rs` — hamis SMTP kiszolgáló és push szolgáltatás; `tests/frontend_support/` — a kliens-tesztek segédei (node futtatás, HTML bejárás, a Rust forrás üzeneteinek kigyűjtése).

Tudnivalók a tesztek írásához:
- **A folyamat-szintű állapot** (szótár: elutasított lista, szavazatok; környezeti változók; frissítési zár) ütközik a párhuzamos tesztek között: az ilyen tesztek egy `SERIAL` zár mögött futnak, a környezeti változós / kimenet-átirányítós tesztek külön binárisban (`*_env.rs`, `admin_log_capture.rs`).
- A robotok a tesztekben ne lépjenek maguktól: `set_setting(&server, "bot_think_multiplier", json!(5.0))`.
- Időzítés: `Sio::settle()` a szerver feldolgozás alatt álló eseményeinek számlálóját (`events_in_flight`) is megvárja; fix `sleep` helyett `wait_code`, `wait_registered` jellegű várakozás kell.

**Aranyfájlok** (`tests/golden/`): a régi Python implementációval előállított adatok (szóellenőrzés, robot lépések, gyakorló módok, szókincs, játékállapotok, SQLite fájl), amelyekkel a Rust tesztek a viselkedés egyezését őrzik; a generátorok és az újragenerálás leírása a `tests/golden/README.md`-ben van. **Differenciális összevetés** (`tests/compat/`): ugyanazok a forgatókönyvek a régi és az új szerveren (`tests/compat/run.sh`). **Teljesítmény**: `scripts/perf.sh` (`benches/compare.rs`), eredmények: `docs/PERFORMANCE.md`.

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
- `src/engine/challenge.rs`: Challenge állapotgép, szavazás indítás, vote resolution
- `src/engine/game.rs`: `accept_pending_by_player()` játékos elfogadás, `reject_pending_by_player()` 2 játékos elutasítás
- `src/server/play.rs`: `accept_words`, `reject_words` Socket.IO eventek; az időzítő háttérfeladat a `src/server/core.rs`-ben
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
| `create_room` | Szoba létrehozása (név, max_players, challenge_mode, is_private, turn_time_limit, `ai_players`: a robotok fokozatainak (1–10) listája, a régi `easy`/`medium`/`hard` is elfogadott; max. 3 és `max_players-1`, `hint_limit`: 0/1/3/5/10, alapért. 3) |
| `join_room` | Csatlakozás kóddal vagy room_id-val |
| `leave_room` | Szoba elhagyása |
| `get_rooms` | Nyilvános szobák listázása |
| `rejoin_room` | Újracsatlakozás tokennel (grace period alatt — várakozó szobába is —, vagy a még „élőnek” hitt régi kapcsolat átvételével — pl. háttérbe került telefon, újratöltött oldal) |
| `start_game` | Játék indítása (owner only) |
| `start_daily` / `retry_daily` / `reveal_daily` | Napi feladvány indítása / új próbálkozás a befejezett után / a megoldás megmutatása (utána nem rangsorolt) |
| `create_async_game` | Levelezős játék: `{name, friend_ids: [1–3 barát], turn_hours: 24/48/72/168}`; csak regisztrált, csak barátok |
| `open_async_game` | Folyamatban lévő levelezős játék megnyitása `{game_id}` (másik eszközről átveszi) |
| `resign_game` | Levelezős játék feladása (a feladó nem lehet győztes) |
| `set_visibility` | `{hidden}`: a böngészőlap háttérbe került-e (push értesítéshez) |
| `spectate_room` | Megfigyelés: `{room_id}` (nyilvános) vagy `{code}` (privát is); csak folyamatban lévő játék |
| `leave_spectate` | Kilépés a megfigyelésből |
| `admin_subscribe` | Belépés az `admin` Socket.IO szobába (élő admin események). Az admin oldal `{auth_token}`-nel (aláírt socket-token, nem számít online játékosnak), a játékból bejelentkezett admin anélkül. Csak adminnak hat (`admin_subscribed` + `admin_overview` a válasz); másnak csendben semmi. Kijelentkezéskor és azonosságváltáskor a kapcsolat kilép a szobából |
| `admin_watch_room` | `{room_id}` (azonosító vagy kód): egy élő szoba figyelése az admin panelről (nem néző, nem számít a korlátba); a szoba minden változásáról `admin_room_state` érkezik. Csak adminnak, másnak csendben semmi |
| `report_content` | Játékos vagy chat üzenet bejelentése `{kind: 'player'\|'chat', target, message, reason}` (csak regisztrált, a szobában lévő játékos) → `report_result` |

### Játékmenet (kliens→szerver)
| Event | Leírás |
|---|---|
| `place_tiles` | Betűk lerakása a táblára |
| `exchange_tiles` | Betűk cseréje a zsákból |
| `pass_turn` | Kör passzolása |
| `accept_words` | Lerakás elfogadása / challenge elfogadás / elfogadó szavazat |
| `reject_words` | Lerakás elutasítása (2 játékos) / megtámadás indítása (3+ játékos) |
| `withdraw_words` | A lerakó visszavonja a szavazásra váró lerakását (amíg senki sem szavazott); időlimitnél a kör hátralévő ideje folytatódik (legalább `_MIN_TIME_AFTER_WITHDRAW` = 10 mp), nem indul újra |
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
| `daily_started` / `daily_result` / `daily_solution` | Napi feladvány: indulás (`{date, registered, my}`), beküldés eredménye (`{score, best_score, is_best, recorded, rank, attempts}`), a megoldás |
| `achievements_earned` | `{badges: [...]}` — új kitüntetések |
| `rating_update` | `{rating, change}` — új értékszám a befejezett játék után |
| `game_saved` | `{game_id}` — a befejezett játék azonosítója (elemzés / visszajátszás) |
| `async_your_turn` / `async_invited` | Levelezős játék: rád került a sor / meghívtak (a lobbyban lévőnek) |
| `announcement` | `{registered, guests}` — a közlemények / karbantartási mód élő frissítése (a kliens a sajátját választja) |
| `account_banned` | `{reason, until}` — a fiók kitiltása: a kapcsolat azonnal bezárul |
| `report_result` | `{success, message}` — a bejelentés eredménye |
| `admin_overview` / `admin_room_update` / `admin_room_state` / `admin_alert` / `admin_report` | Csak az `admin` szobának: élő számlálók (5 mp), egy szoba sora, egy figyelt szoba teljes állapota, tiltott szavas üzenet, új bejelentés |
| `error` | Hibaüzenet |

## Rate limiting

### Socket.IO eventek (per SID, `src/services/ratelimit.rs`)
```text
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
'admin_subscribe': (5, 10),     # csendben eldobva
'admin_watch_room': (20, 10),   # csendben eldobva
'report_content': (3, 60),
'leave_spectate': (5, 10),
'withdraw_words': (5, 10),
'set_visibility': (20, 10),
'start_daily': (3, 30), 'retry_daily': (10, 30), 'reveal_daily': (5, 30),
'create_async_game': (3, 30), 'open_async_game': (10, 10), 'resign_game': (3, 30),
```

### HTTP auth (IP-alapú, `src/config.rs` → `AUTH_RATE_LIMITS`)
- `request_code`: 3 kérés / 300 mp
- `login`: 10 kérés / 300 mp
- `register`: 3 kérés / 3600 mp
- `replay` 60/perc, `analysis` 30/perc, `daily` 60/perc, `practice` 120/perc, `word_review` 240/perc, `push` 20/perc, `admin` 120/perc, `admin_danger` 20/perc (IP-alapú, `src/config.rs`)

## UI felépítés

### Képernyők
1. **Auth képernyő**: 3 tab (Bejelentkezés, Regisztráció, Vendég), lebegő téma gomb
2. **Lobby**: középre igazított szegmentált navigáció (Kezdőlap / Új szoba / Mentett játékok / Barátok / Levelezős / Gyakorlás / Ranglista; keskeny képernyőn görgethető: a kijelölt fül középre görgetődik, a széleken elhalványul — `LobbyNav`; telefonon a cím röviden, „Scrabble”), szoba létrehozás (regisztráltaknak; robotok száma + nehézsége), kóddal csatlakozás / megfigyelés, nyilvános szobák és élő játékok listája
3. **Várakozó szoba**: badge-ek (challenge/privát/időlimit), csatlakozási kód, játékoslista, start gomb (owner)
4. **Játék képernyő**: info panel + tábla + betűtartó (lásd lent)
5. **Profil** és **Visszajátszás**

Minden képernyőn (az auth kivételével) ugyanaz a **sticky felső sáv** (`.app-topbar`) látszik: vissza gomb + cím balra, profil / hang / téma / kijelentkezés jobbra. **Kivétel: a játék képernyő álló nézetben** (telefon és tablet) **és fekvő érintőképernyős telefonon** — ott a hely szűkös, ezért nincs felső sáv: a szobanév és a gombok (kilépés · profil · hang · téma · kijelentkezés) egyetlen sorban, a `.game-nav` menüsorban vannak az alsó, görgethető panelen belül. Asztali gépen és fekvő tableten a játékban is marad a felső sáv. `env(safe-area-inset-*)` kezeli a notchot és a home indicatort (`viewport-fit=cover`).

### Játék képernyő elrendezés
Három elrendezés, CSS media query-kkel (`static/style.css` 11–13. szakasz):

- **Alap (fekvő, asztali gép, fekvő tablet)** — két oszlop: bal oldali `side-panel` (300px, sticky, saját görgetéssel) + tábla és betűtartó. A tábla mérete (`--board-size`) a képernyő magasságából is számolódik, így tábla + betűtartó görgetés nélkül elfér. **A betűtartó helye beállítható** (`HandLayout`, `localStorage('scrabble-hand-position')`, a `<html data-hand="right|bottom">` attribútumon át): alapértelmezés `right` — a betűtartó függőlegesen a tábla jobb oldalán áll (CSS 11b. szakasz; 820px-nél szélesebb, legalább 541px magas fekvő ablakban; a `--tile` itt csak a magasságtól függ, különben körkörös lenne a függés a tábla méretével), `bottom` — a tábla alatt (az alap elrendezés). Választható a profil beállításai között és a játék eszközsorának „Elrendezés” gombjával (ez csak ott látszik, ahol a beállítás hat). Álló nézetben és 820px-nél keskenyebb ablakban a betűtartó mindig alul van, kompakt fekvő telefonon mindig oldalt.
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

### src/engine/game.rs
- `HAND_SIZE = 7`
- `SCORELESS_TURNS_LIMIT = 6` — ennyi egymást követő pont nélküli kör után véget ér a játék
- `BONUS_ALL_TILES = 50`
- `CHALLENGE_TIMEOUT = 30` (mp)
- `MAX_TIMEOUTS = 3` — levelezős játék: ennyi egymás utáni lejárt határidő után a játékos feladja

### src/server/
- `_DISCONNECT_GRACE_PERIOD = 120` (mp)
- `_WAITING_OWNER_GRACE_PERIOD = 600` (mp) — a várakozó szoba tulajdonosának türelmi ideje
- `ALLOWED_TURN_TIME_LIMITS = {0, 60, 90, 120, 180, 300}`
- `MAX_BOTS = 3`, `_BOT_THINK_DELAY` (szintenként min/max mp), `_BOT_NAMES`
- `Room.MAX_SPECTATORS = 30`

### src/engine/game.rs (tippek)
- `ALLOWED_HINT_LIMITS = (0, 1, 3, 5, 10)`, `DEFAULT_HINT_LIMIT = 3`; `Game.hint_limit` / `hints_used` (mentésbe kerül, régi mentésnél az alapérték), `hints_left()`, `use_hint()`; az állapotban `hint_limit`, `hints_left`

### src/robot/ai.rs
- `MIN_LEVEL = 1`, `MAX_LEVEL = 10`, `DEFAULT_LEVEL = 6`, `DIFFICULTIES = (1..10)`, `LEGACY_LEVELS = {'easy': 3, 'medium': 6, 'hard': 10}`
- `_TIME_BUDGET = 1.5` mp keresési időkeret (minden fokozaton), `_HINT_TIME_BUDGET = 3.5` (tipp); `_PROFILES` (fokozatonkénti `mu` / `sigma` / `pass_p`), `_TARGET_CV = 0.5`
- `ADAPTIVE = 'auto'`, `LEVEL_STRENGTH` (mért pont/kör fokozatonként), `ADAPT_WINDOW = 6`, `ADAPT_START_LEVEL = 5.0`; `INFLECT_MAX_STEM = 6`, `INFLECT_MAX_FORM = 12`, `_INFLECT_ADDS` (a vett végződések)

### src/db/
- `LEADERBOARD_METRICS` (`rating`, `wins`, `win_rate`, `avg_score`, `best_game`), `LEADERBOARD_MIN_GAMES` (`rating`, `win_rate`, `avg_score`: 3), `LEADERBOARD_MAX_LIMIT = 100`

### src/admin/ / src/config.rs (admin)
- `ADMIN_EMAILS` (üres → nincs admin), `ADMIN_SESSION_IDLE_MINUTES = 30`, `ADMIN_SUDO_MINUTES = 10`, `ADMIN_IP_ALLOWLIST` (üres → bárhonnan; hibás elemnél indulási hiba), `AUTH_RATE_LIMITS['admin'] = (120, 60)`, `['admin_danger'] = (20, 60)`
- `MIN_REASON_LEN = 3`, `MAX_REASON_LEN = 500`, `TOUCH_INTERVAL_SECONDS = 15`, `AUDIT_DEFAULT_LIMIT = 50`, `AUDIT_MAX_LIMIT = 200`, `AUDIT_EXPORT_MAX = 10000`
- `config.BACKUP_DIR` (`SCRABBLE_BACKUP_DIR`, alapért. `backups/`, a `.gitignore`-ban); `admin_live.STUCK_*` (elakadás-érzékelés), `admin_security.FAILED_*` (gyanús belépések), `admin_comm.MAX_*` / `BULK_EMAIL_COOLDOWN`, `admin_system.LOG_BUFFER_SIZE = 2000`

### src/accounts/elo.rs / achievements.rs, src/robot/daily.rs, src/async_games.rs
- `INITIAL_RATING = 1200`, `K_PROVISIONAL = 32` (az első 10 értékelt játék), `K_ESTABLISHED = 20`, `MIN_RATED_GAMES_FOR_RANKING = 3`
- `BADGES` (10 + `daily_best`), `BIG_MOVE_SCORE = 100`, `LONG_WORD_TILES = 8`, `HIGH_GAME_SCORE = 300`
- `daily.MIN_BEST_SCORE = 30`, `MAX_BEST_SCORE = 250`, `PLIES = (8, 16)`
- `ALLOWED_TURN_HOURS = (24, 48, 72, 168)`, `DEFAULT_TURN_HOURS = 48`, `MAX_FRIENDS = 3`

### src/config.rs
- `DB_PATH = 'scrabble.db'` (vagy `SCRABBLE_DB_PATH` env var)
- `SESSION_MAX_AGE_DAYS = 30`
- `VERIFICATION_CODE_EXPIRY_MINUTES = 10`
- `VERIFICATION_MAX_ATTEMPTS = 5`
- `SMTP_CONFIGURED` — bool, automatikusan kalkulált
- `AUTH_RATE_LIMITS` — dict, IP-alapú rate limit konfigok
- `WORD_REJECT_THRESHOLD = 1` (vagy env var) — ennyivel kell több „nem szó” szavazat a „rendes szó”-nál a szó kizárásához (szótár-építő)

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
- A felső sorban a lobby navigációja (`#profile-nav`, a lobbyé másolata): fülre kattintva `Profile.openLobbyTab` a lobbyba lép és az adott fület nyitja (`Lobby.switchTab`); a jelvények és a „nyitott szoba” fül a lobby állapotát tükrözik (`Profile.syncNav`). A lobby saját fülkezelője csak a `#lobby-nav` gombjaira van kötve, a görgetés-elhalványítás (`LobbyNav`) mindkét sorra működik.
- `GET /api/auth/profile` — statisztikák + utolsó 20 befejezett játék
- `GET /api/game/<id>/moves` — lépések listája replay-hez
- Visszajátszás: lépésenkénti navigáció board snapshot-okkal

## Robot ellenfelek (AI)

### Játékmodell
- `Player.is_bot` / `Player.difficulty`; id: `bot-<szoba>-<n>` (nem SID, a mentésben is stabil). `Game.add_bot()` csak indítás előtt, max. 4 játékos összesen.
- A robotok **nem szavaznak** (`_get_voter_ids`): a megtámadás (`_challenge_applies`) csak akkor él, ha van másik **emberi** játékos. Ha nincs, a lerakást a szótár ellenőrzi.
- `Game.get_all_states()` nem készít állapotot robotnak; a robot sosem `disconnected` (`from_save_dict` is kényszeríti). `human_players()` / `has_connected_human()` a szoba életciklusához: ember nélkül a szoba megszűnik, a robotok nem lépnek (néző jelenlétében igen).
- Restore: a `expected_players` csak embereket tartalmaz, a robotok az indításkor automatikusan visszakerülnek.

### Szerver (`src/server/core.rs`)
- `schedule_bot_turn(room_id)` — minden olyan ponton hívódik, ahol a kör továbbadódhat (lerakás/csere/passz, szavazás lezárása, időzítők, újracsatlakozás, indítás, megfigyelő csatlakozása). A `room.invalidate_bot_turn()` azonosítóval a régi ütemezések érvénytelenednek.
- `play_bot_turn(room_id, turn_id, turn_number)` — a keresés külön blokkoló szálon fut (`spawn_blocking`), közben az állapot megváltozhat, ezért a lépés előtt újraellenőriz. Sikertelen lépés → passz, hogy a játék ne akadjon el. A tesztek a robotok önálló lépését a `bot_think_multiplier` beállítással (5.0) tartják vissza, vagy a függvényt közvetlenül hívják.
- `emit_all_states` háttérfeladatból is hívható (nem kér kapcsolat-környezetet).

### Motor (`src/robot/ai.rs`)
- Szókincs: `dict/hu_HU.dic` tőszavai (csak kisbetűs, magánhangzót tartalmazó, tiltott betűk nélküli — zsetonokra bontható —, 2–15 betű, a szótár szerint önmagában érvényes (`AffixChecker.has_stem`: nincs VU, COS, MADAME); ~65 000 szó) **+ gyakori ragozott alakok** (`AffixChecker.inflected_forms`: szótő + egy végződés a szótár saját szabályaival, a tővégi a/e nyúlásával, legfeljebb 6 betűs szótövekre és 12 betűs alakokra; a végződések az `_INFLECT_ADDS` fehérlistán: többes szám, tárgyrag, esetragok, birtokos személyjelek, igei végződések; a kockázatos levezetések (melléknév + birtokos, -ék, -ul/-ül főnéven, -né) nélkül, akkor is, ha a használati listán szerepelnek — `inflected_forms(..., risky=False)` — ~225 000 alak, ~+35 MB). Rendezett lista + `bisect` prefix- és tagsági kereséssel (nincs trie és nincs külön halmaz: kevés memória; a keresés így is ~12 ms/állás).
- `generate_moves`: Appel–Jacobson horgonykeresés vízszintesen és (átfordított rácson) függőlegesen; a többkarakteres zsetonok (SZ, CS...) több karaktert lépnek a prefixben; joker bármely betű. A keresztszavak érvényességét **egyetlen** `filter_valid` hívás dönti el az egész táblára. A pontozás a játék saját `Board.validate_placement`-jével történik egy privát másolaton.
- **10 fokozat** (`_PROFILES`, `_ordered_candidates`): az erőt a **lépéskiválasztás** szabja meg, nem a szókincs (a szókincs szűkítése mérés szerint alig gyengít, mert a robot a megmaradtak közül is a legjobbat választja). Az 1–7. fokozat *célpontszámos*: minden körben célpontszámot sorsol (átlag `mu`, lognormális szórás `_TARGET_CV`), és a hozzá legközelebbi pontszámú lépést rakja le. A 8–10. fokozat *értékeléses*: a legnagyobb `equity` (= pont + `leave_value`, végjátékban a kézben maradó zsetonok levonva), Gauss-zajjal (`sigma` 8 / 4 / 0). Az 1. fokozat 15% eséllyel „nem talál" lépést (csere, ha a zsák ≥ 7, különben passz). Jokert minden fokozat használ.
- **Mért erősség** (bot–bot önjáték, átlagos pont/kör, ragozott szókinccsel, a furcsa alakok nélkül): 4,3 · 6,1 · 7,4 · 10,1 · 12,7 · 15,7 · 18,4 · 21,2 · 24,6 · 27,0 (`LEVEL_STRENGTH`; újramérés után ezt is frissíteni kell). A régi szintek helye: könnyű = 3, közepes = 6 (alapértelmezett), nehéz = 10. A régi mentések és kliensek `easy`/`medium`/`hard` értéke `parse_level`-lel képeződik át (a `Player` is normalizál). Újramérés a paraméterek módosítása után: `target/release/bot_arena ladder -n 24 -j 4` (fokozatonként), `target/release/bot_arena match 3 6` (két fokozat egymás ellen); a bot–bot játék kevésbé szór, mint egy ember, ezért ott a szomszédos fokozatok közti győzelmi arány élesebb, mint embernél.
- A kiválasztott lépés minden szavát `_first_valid` ellenőrzi a játék szótárával; ha nincs lépés: csere (zsák ≥ 7) vagy passz.
- A keresés időkerete (`_TIME_BUDGET`) csak védőkorlát: a keresés a legtöbb táblán jóval hamarabb véget ér, így a fokozatok ereje nem függ a szerver sebességétől; lejártakor az addigi legjobbal dolgozik.
- **„Igazodik hozzám” (`'auto'`)**: `Game.recent_stats` az emberi játékosok utolsó 6 körének átlagát számolja (passz/csere = 0), `ai_player.level_for_average` ezt a mért skálán tört fokozattá képezi, `adaptive_level` a két szomszédos fokozatot véletlenszerűen keveri (kevés adatnál az induló 5. fokozat felé húz). A szerver lépésenként `Game.bot_level(bot)`-ot használ. Ellenőrzés: `target/release/bot_arena adapt 5`.
- Szerver: a robot neve és gondolkodási ideje a fokozat sávjától függ (`_bot_tier`: 1–3 könnyű, 4–7 közepes, 8–10 nehéz). A lobbyban egy `select` (`#room-ai-difficulty`, 10 opció, `ai.level_N` fordításokkal) állítja az összes robot fokozatát; a szerver robotonként külön fokozatot is kezel.

## Megfigyelő mód

- `Room.spectators {sid: név}` + `ServerState.spectator_rooms {sid: room_id}`; a megfigyelő a Socket.IO szobába is belép (chat, események), de a `player_rooms`-ban nem szerepel, így a játékos-eventek (lerakás, chat küldés...) hatástalanok.
- `_emit_all_states` a megfigyelőknek `Game.get_spectator_state()`-et küld (kezek nélkül, `spectator: true`); minden játékos állapotában `spectator_count`. A lecsatlakozott (`disconnected`) játékosnak nem küld állapotot: a SID-je még élhet a lobbyban (pl. kilépett, vagy levelezős játékból bezárta a nézetet), és a kliense visszaugrana a játékba.
- Életciklus: `disconnect`/`logout`/`leave_spectate` eltávolítja; `_cleanup_room` és `_disband_active_room` `room_disbanded`-et küld nekik. Kliens oldalon újracsatlakozáskor `Spectate.resume()` újraindítja a megfigyelést.
- A `get_rooms` a `live_games` eseményt is kiküldi; játék indításakor/végén broadcast.

## Ranglista

- A listát `auth.get_leaderboard()` a `game_players` ⨝ `saved_games(finished, has_bots=0)` ⨝ `users` összesítéséből számolja (nem a `users` számlálóiból), így a robotos játékok nem számítanak. A rendezés SQL-részlete rögzített (`_LEADERBOARD_ORDER`), a metrika fehérlistás.

## Többnyelvű felület (i18n)

- `static/i18n-data.js`: `window.I18N_DATA = {hu: {...}, en: {..., server: {exact, patterns}}}` — szigorú JSON (a tesztek `json.loads`-szal olvassák). Kulcsok `névtér.kulcs`; `_one` végű kulcs az angol egyes számhoz (`params.n === 1`).
- HTML: `data-i18n` (szöveg), `data-i18n-placeholder`, `data-i18n-title` (title + aria-label), `data-i18n-aria`. JS: `t('kulcs', {n})`, a szerver magyar üzeneteihez `tServer(msg)`; nyelvváltáskor `langchange` esemény, amire a modulok újrarajzolják a gyorsítótárazott adataikat.
- A szerver szerkezetes `last_action_info`-t is küld; a kliens ebből formázza a „utolsó akció” szöveget (a magyar `last_action` szöveg tartalék).
- Új felhasználói szöveg hozzáadásakor: kulcs a hu **és** en blokkba, `data-i18n`/`t()` a kódban; a `tests/frontend_i18n.rs` hibára fut, ha hiányzik fordítás, nem használt kulcs marad, vagy egy szerver-üzenet fordítatlan.

## PWA

- `/sw.js` (`templates/sw.js`): install → váz előtöltése (`SHELL_URLS`), activate → régi gyorsítótárak törlése, fetch: navigáció hálózat-először (offline: gyorsítótárazott `/`, végső esetben `offline.html`); `/socket.io/` és `/api/` soha; statikus fájlok és a CDN-es Socket.IO/betűtípus stale-while-revalidate.
- `asset_version()` (`src/server/http.rs`) a kliens fájlok mtime-ja: az `index.html` `?v=` paramétere és a SW `VERSION`-je ugyanez.
- Telepítés gomb: `beforeinstallprompt` (iOS Safari-n kézi útmutató).

## Animációk és húzás-visszajelzés

- `GameBoard.renderBoard` az előző rajzoláshoz képest különbséget számol: új betű → `tile-pop` (saját lerakás) / `tile-drop` (más lépése); utolsó lépés: `last-move`; pontszám-változás: `.score-pop`; kör váltás: `turn-pulse`; saját kör: `.my-turn` fény a táblán.
- Húzás közben a tábla `is-dragging` osztályt kap, a foglalt mezők csíkozva; `dragenter` **és** `dragover` elfogadása szükséges (különben a böngésző a `body`-t teszi céllá, és a mezők nem kapnak eseményt). A `prefers-reduced-motion` az összes animációt kikapcsolja.

## Napi feladvány

- `daily.generate_puzzle(dátum)`: a dátumból indított determinisztikus bot–bot játék néhány lépés után, a soron lévő robot keze a feladvány; a legjobb lépés a robot motorjával (`MIN_BEST_SCORE`…`MAX_BEST_SCORE`). Az első kérésnél (vagy indításkor) készül, a `daily_puzzles` táblában rögzül. A dátum magyar idő szerint értendő.
- `Game.puzzle`: egyjátékos játék, egyetlen lerakás után vége (nincs passz, csere, végső elszámolás); sosem mentődik az előzményekbe. Szoba: privát, `room.is_puzzle`. Vendég is játszhat, de csak regisztrált kerül a ranglistára (`daily_scores`: legjobb pont → kevesebb próbálkozás → korábbi idő). A megoldás megnézése (`reveal_daily`) lezárja a ranglistás részvételt; ha valaki a legjobbnál többet ér el, az lesz az új legjobb. `daily_best` kitüntetés.
- `GET /api/daily`: a saját eredmény, top 10, tegnapi megoldás; a feladvány legjobb pontszáma csak a már próbálkozóknak látszik.

## Kétjegyű betűk

A kétjegyű betű (SZ, CS, GY, LY, NY, TY, ZS) egy zseton: **külön zsetonokból nem rakható ki** (pl. S + Z egymás mellett nem ad SZ-t). A szerver a magyar Scrabble szabálya szerint kezeli; a tábla és a gyakorló módok is ezt követik.
- `tiles.forms_digraph(első, második)`: két szomszédos, egybetűs zseton kétjegyű betűt adna-e. Csak a SZ, CS, ZS pár fordulhat elő valóban (önálló Y zseton nincs, a GY / LY / NY / TY második betűje nem külön zseton), de a szabály általános.
- `Board._find_split_digraph` (a `_validate_words` hívja a szótár-ellenőrzés **előtt**, `skip_dictionary` esetén — megtámadásos módban, robotnál — is): minden képzett szóban (fő- és keresztszó) megnézi a szomszédos zsetonpárokat. **Csak az újonnan lerakott zsetont érintő párokat** nézi, így a régi, megengedőbb szabállyal indult állások folytathatók. A Z + SZ (vízszint) és S + SZ (asszony) rendben van, mert a második zseton kétjegyű. A joker a választott betűjével számít. A hibaüzenet (`Kétjegyű betű (SZ) csak a saját zsetonjával rakható ki, S + Z külön zsetonnal nem.`) a lerakásnál és az élő előnézetben is megjelenik; az angol fordítás a `server.patterns` között van.
- A robot a `validate_placement`-en át szűr (a generátor eldobja az ilyen lépéseket), így a napi feladvány, a tipp és az elemzés is követi; a szabály bevezetésekor az `analysis.ANALYSIS_VERSION` nőtt (6).
- Gyakorló módok: `practice.rack_words` és `practice.check_rack_word` (`_assign`) sem használja külön zsetonokból a kétjegyű betűt; a `check_rack_word` külön `split_digraph` okot ad, ha csak ez volt az akadály (a kliens `hunt.reason_split_digraph`). A kliensben a `huntSubmit` is elutasítja, a `huntType` (billentyűzet) pedig az S után gépelt Z-t a szabad SZ zsetonnal váltja fel (`formsDigraph` a szerver mása; a `tests/frontend_client.rs` összeveti).

## Furcsa szavak szűrése

A hu_HU szótár helyesírás-ellenőrzésre készült: minden nyelvtanilag lehetséges alakot elfogad, a Scrabble-ban viszont ez sok értelmetlen szót engedne (a robot ilyeneket rakott le, pl. FALIM, TAROM, ÉJÉK, VU). Az `affix_checker` ezért a szótár morfológiai címkéit is beolvassa (`AM` aliasok: a szócikk szófaja `po:`, a toldalék fajtája `is:` / `ds:`):
- **Csak kötőjellel toldalékolható szócikkek** (`al:szó-`, főnév: rövidítések, mértékegységek, idegen írásmódú szavak — VU, UV, COS, KCAL, SZJA, MADAME, CROISSANT): nem érvényesek (`_KIND_FOREIGN`).
- **Kockázatos levezetések** (`_rule_risk`, `_is_risky`): csak akkor érvényesek, ha az alak a `dict/hu_attested.txt` listán szerepel (ténylegesen használt):
  - melléknév (vagy -s/-i/-bb/-nyi… képzős melléknév, ill. -s foglalkozásnév) + birtokos személyjel / birtokjel: KEDVESEM, DRÁGÁM, GYILKOSA igen — TAROM, FALIJA, DOMBOSOM nem;
  - -ék családi többes: SZOMSZÉDÉK, ANYÁMÉK igen — ÉJÉK, CIRMOSÉK nem;
  - -ul/-ül **főnéven**: FELESÉGÜL, AJÁNDÉKUL igen — BLÖKIÜL nem (melléknéven ez a szabályos határozószó: ROSSZUL, VÉLETLENÜL — mindig érvényes);
  - -né képző: KIRÁLYNÉ, SÓGORNÉ igen — ALMÁNÉ nem.
  - A -ó/-ő, -andó/-endő melléknévi igenév főnévként viselkedik (TANULÓM, FAGYASZTÓMBÓL rendben).
- `dict/hu_attested.txt`: a `build_attested` eszköz (`src/bin/build_attested.rs`) állítja elő a FrequencyWords (OpenSubtitles 2018, magyar, CC BY-SA 4.0) gyakorisági listából: azok az alakok, amelyek csak kockázatos levezetéssel érvényesek és legalább 2-szer előfordulnak; kiszűrve az elírások (egy ≥10× gyakoribb érvényes szó ékezet nélkül, egy kimaradt vagy megkettőzött betűvel: TAROM ← TARTOM, KORUL ← KÖRÜL) és a tulajdonnévből képzettek (ALISA ← Ali). A fájl hiányában a kockázatos levezetések mind érvényesek (a hunspell viselkedése). A listán maradt kevés zaj (pl. MAID, MARISA) ritka, hosszú vagy kevéssé valószínű alak.
- A robot a kockázatos alakokat akkor sem rakja le, ha a listán szerepelnek (`inflected_forms(..., risky=False)`), így a lépései és a napi feladvány táblája természetes szavakból állnak.
- Ellenőrzés: `AffixChecker.needs_attestation(szó)` — csak kockázatos levezetéssel érvényes-e.

## Gyakorló módok

A Gyakorlás lap főoldalból (statisztika-sáv: napi sorozat / mai gyakorlat / kvíz-pontosság; a napi feladvány kiemelt kártyája; a módok iOS-szerű csoportosított listája) és al-nézetekből áll (`.practice-view`, `Practice.open(nézet)` / `back()`; a telefonon nincs egyetlen hosszú görgetés). Az eredmények és a „Hibáim” pakli az eszközön élnek (`PracticeStore`, `localStorage('scrabble-practice')`: napok, mai szám, kvíz-számlálók, betűvadász legjobb pontja időkorlátonként, bingó-sorozat, `missed`), vendégnek is működnek.

- **Szókvíz** (`practice.make_quiz(count, mode)`): a fele érvényes szó (tőszavak + ~30% ragozott alak), a fele hihető félreírás, amelyet a szótár elutasít; minden hamis szó más forrásszóból készül. Módok: `mixed` (3–9 zsetonos, egy zsetonnyi módosítás), `2` / `3` (csak annyi zsetonos szavak), `tricky` (csapdák: egy magánhangzó hosszúságának cseréje, a↔á, e↔é, i↔í, o↔ó, ö↔ő, u↔ú, ü↔ű — `_mutate_length`). A kérdés csak a szót és a zsetonjait adja, a választ `POST /api/practice/answer` a játék szótárával értékeli (állapot nélkül), hibás válasznál javaslatokkal. A kliens a szót játékbeli zsetonokkal rajzolja (`fillWordTiles`); ←/→ billentyű: nem érvényes / érvényes, Enter / szóköz: következő. A tévesztett szó a „Hibáim” pakliba kerül (legfeljebb 200); a pakliból a kétszer egymás után helyes válasz kivesz, egy újabb tévedés nullázza a számlálót.
- **Betűvadász / Bingó-edző** (`practice.make_rack(kind)`): 7 zseton (joker nélkül, a játék zsákjának darabszámai szerint, 2–4 magánhangzóval). `rack_words` a robot szókincsében prefix-vágással keresi az összes kirakható szót (a szótárral megerősítve; kb. 3 ms), a pont a zsetonértékek összege, mind a hét zseton +`BINGO_BONUS` (50). `hunt`: 12–150 szó, legalább egy 5+ zsetonos; `bingo`: a kéz egy 7 zsetonos szóból készül (75% szótári tőszó, hogy ne furcsa ragozott alak legyen), a lista a 7 zsetonos szavak. A kliens a listából azonnal pontoz; a listán nem szereplő, de érvényes szót `POST /api/practice/rack-word` (`check_rack_word`: `too_short` / `invalid_chars` / `not_in_rack` / `not_a_word`, a kétjegyű betű egy vagy két zseton is lehet, a több pontot érő kirakás számít) bírálja el, és bónusz szóként számít. Bevitel: zsetonokra koppintás, vagy billentyűzet (a kétjegyű betű két billentyű, ha nincs külön S / C… zseton); Enter / Backspace / szóköz (keverés). Időkorlát (nincs / 60 / 120 mp) csak a vadászaton; az óra megáll, ha a lap nem látszik. A tipp a legtöbb pontot érő megtalálatlan szó hosszát és kezdőbetűit adja (ismétléskor több betűt).
- **Szólisták**: `practice.short_words(2|3)` — az összes érvényes 2 / 3 zsetonos szó pontértékkel (lustán számolva, gyorsítótárazva; `practice.warm_up()` indításkor előállítja). A kliens a magyar ábécé szerint (`HU_ALPHABET`, a kétjegyű betűk külön betűk) rendezi, kereső (a szó bármely részére), kezdőbetű-szűrő, ABC / pont rendezés; a „Zsetonok” fül a 100 zseton értéke és darabszáma pontérték szerint csoportosítva (a `TILE_VALUES` / `TILE_COUNTS` kliensoldali mása).
- A `tokenizeWord` (JS) a szerver `tokenize_word`-jével azonos dinamikus programozás (kevesebb zseton, döntetlennél több pont: KÉSZSÉG = K É S ZS É G); a `tests/frontend_client.rs` több ezer szón összeveti.
- A végpontok közös őre: `routes._practice_guard()` (IP-alapú rate limit `practice`: 120 / perc, és a szótár elérhetősége — hiányában 503).

## Szótár-építő

A hu_HU szótár helyesírás-ellenőrzésre készült, ezért a Scrabble-ban sok olyan alakot is elfogad, amelyet senki sem tekintene rendes szónak (pl. a gépiesen képzett „…gyűlölet” / „…ellenesség” népnevekkel, értelmetlen számnevek, torz alakok). A szótár-építő az emberi (és AI-s) átnézéssel tisztítja a szótárt.

- **Felület**: Gyakorlás → „Szótár-építő” (`WordBuilder` modul az `app.js`-ben, `#practice-wordbuilder`). Egy szó látszik játékbeli zsetonokkal és „Keresés a magyar értelmező szótárban” hivatkozással; **Nem szó** (←), **Rendes szó** (→), **Nem tudom** (↓, nincs szavazat), **Visszavonás** (⌫, a legutóbbi döntés). A döntés azonnal továbblép (a szavakat 20-asával kapja a kliens, a küldés a háttérben, sorban megy; hálózati hibánál a szó újra sorra kerül). Számlálók: ma átnézett / összes / kizárt. Csak bejelentkezve (vendégnek magyarázó kártya), mert a szavazat mindenki játékát érinti. A lenyomva tartott billentyű nem dönt (`e.repeat`).
- **Mintavétel** (`word_review.sample_words`): a robot szókincséből (tőszavak és gyakori ragozott alakok 7 : 3 arányban), csak olyan szó, amelyet a szótár most elfogad és amelyet még senki sem nézett át (az elfogadottak sem térnek vissza). Az eszköz (`src/bin/word_review.rs`) ugyanezt használja, bármekkora mintára.
- **Döntések és kizárás**: `word_reviews` tábla (szó kisbetűsen, felhasználónként egy szavazat: 1 rendes szó, 0 nem szó). Egy szót akkor zár ki a szótár, ha `nem szó − rendes szó ≥ config.WORD_REJECT_THRESHOLD` (alapérték 1: egyetlen elutasítás elég; több emberes átnézésnél emelhető). A kizárás pontos szóalakra szól (a ragozott alakokra nem), azonnal él (`dictionary.mark_voted_rejected`: a lerakás, a robot, a kvíz, a szólisták és a szótár-böngésző is érvénytelennek veszi), és indításkor az adatbázisból töltődik vissza (`word_review.refresh`). Csak a szótár által éppen elfogadott szóra lehet szavazni.
- **Tartós lista**: `dict/hu_rejected.txt` (soronként egy kisbetűs szó, `#` megjegyzés) — a szótárral együtt töltődik be, a robot szókincséből is kimarad; a szavazatoktól független. Karbantartása a `word_review` eszközzel (`src/bin/word_review.rs`) (vagy kézzel); a szavazatok nem kerülnek bele automatikusan.
- **AI-s átnézés**: `target/release/word_review sample 5000 --seed N --out sample.txt` (átnézendő szavak), az ítéletek (a nem rendes szavak) egy fájlba, majd `target/release/word_review apply rejected.txt` (csak a szótár által elfogadott szó kerülhet a listára; `--dry-run`). Első kör (2026-10-03): 5000 véletlen szó, 188 elutasítás. Második kör: további 20 000 szó (`--seed 20261004 --exclude` az első minta), első szűrőként Claude Haikuval (szavankénti ítélet, csomagonként kanári-szavakkal; 3315 jelölés), de **minden jelölést egyenként felülbíráltam**, 436 elutasítással — a lista így 622 szó. A mérce mindkét körben: nem használatos, gépiesen képzett alak (ritka népnév + -gyűlölet / -ellenesség / -üldözés / -barát / -imádat / -tanítás / -centrikus, értelmetlen számnevek, személyes névmás + névutó torzítások, nem személyre vonatkozó „-é” alakok) vagy elírás; a bizonytalan szót az átnézés megtartotta, ezért a lista óvatos. **A Haiku önmagában nem megbízható bírálónak**: a kalibráló mintán (a saját 33 ítéletem) a recall 85% volt, de a pontosság csak ~15% (valódi szavakat is megjelölt: hágcsó, kényszeredett, hoznál, kétlek), a „javított alakot” kérő és a szkeptikus ellenőrző körök pedig nem javítottak rajta (az utóbbi a jó jelöléseket is elejtette), ezért csak recall-szűrőként használható, az ítéletet ember (vagy erősebb modell) hozza. Ugyanaz a szó kétszer ne kerüljön átnézésre: a `--exclude` kihagyja a korábbi minták fájljait. Új lista után az elemzés gyorsítótárának verzióját (`analysis.ANALYSIS_VERSION`) növelni kell.
- **Gyorsítótárak**: `dictionary.rejected_version()` nő minden változásnál; a számolt gyorsítótárak (`practice.short_words`) ebből veszik észre, hogy elavultak; a `filter_valid` gyorsítótárából a kizárt szó törlődik. A szavazatból kizártak folyamatszintű állapotot jelentenek, ezért a róluk szóló tesztek külön binárisban vagy soros zárral futnak, és minden teszt után törlik őket (`dictionary::set_voted_rejected`).
- **Biztonság / visszaélés**: csak regisztrált felhasználó szavazhat; IP-alapú rate limit (`word_review` 240/perc); a szó alakját és szótári érvényességét a szerver ellenőrzi. Mivel az alapértelmezett küszöb 1, egy fiók egyedül is kizárhat szavakat — nagyobb közösségnél érdemes a `WORD_REJECT_THRESHOLD`-ot 2–3-ra állítani.

## Levelezős (aszinkron) játék

- `Game.async_mode`, `turn_hours`, `turn_deadline`: a lecsatlakozott játékost nem ugorjuk át (`_next_turn`), a soron lévőnek `turn_hours` órája van. `expire_turn()`: automatikus passz (`timeout` lépés), három egymás utáni után `resign`. `resign()`: a játék véget ér, a feladó nem lehet győztes (az ELO-ban is mindenki mögé kerül, `resigned` a `players_data`-ban).
- A játék otthona az adatbázis: `saved_games.is_async`; `_persist_async` minden változás után ment (`_emit_all_states` hívja), `restore_async_games()` induláskor újraépíti a szobákat (minden játékos lecsatlakozottként, helyettesítő azonosítóval: `async_games.placeholder_id`), `open_async_game` szükség esetén betölti. `_async_sweeper` percenként `_expire_async_turns()`-t hív. A mentett játékok fülön / `restore_game` / `abandon` nem érinti őket.
- Szoba: privát, `room.is_async`; lecsatlakozás / `leave_room` csak a nézetet zárja be (nincs türelmi idő, nincs feloszlatás). Játék vége után a szoba megszűnik, ha senki sincs bent.
- Létrehozás: csak regisztrált, 1–3 **barát** (`friend_ids`), nincs robot, nincs megtámadás, nincs tipp. Értesítés: Web Push (meghívó / „Te jössz!”), `async_invited` / `async_your_turn` esemény a lobbyban lévőknek, jelvény a Levelezős fülön.
- Felület (`AsyncGames`): a lista játékkártyákból áll (`.async-card`: a soron lévő monogramja, pontállás, „Te jössz!” / „X következik” jelvény, hátralévő idő és elfogyó sáv; sürgős, ha 6 óránál / a határidő 15%-ánál kevesebb van, vagy lejárt); üres állapotban magyarázat és gomb. Az **új játék alsó lap** (`#async-new-dialog`, `.dialog-sheet`): név, gondolkodási idő szegmentált választóval (24 ó / 48 ó / 3 nap / 7 nap), barátok kijelölhető sorokban (monogram, online jelző, `n/3` számláló; három kijelölése után a többi zárolt), a „Játék indítása” gomb legalább egy barátnál él. Barátok nélkül a lap a Barátok fülre irányít.

## Web Push

- `src/services/push.rs`: VAPID kulcspár (`VAPID_PRIVATE_KEY` környezeti változó: PEM vagy a `web-push` eszköz base64url formája — a nyilvános kulcs ebből számolódik; ennek híján az első induláskor generálódik és az `app_settings` táblában marad), `VAPID_SUBJECT` (alapért.: `mailto:SMTP_FROM`). A küldés saját megvalósítás (RFC 8291 aes128gcm + VAPID ES256), külső csomag nem kell. Küldés háttérfeladatban; a 404/410 válaszú feliratkozás törlődik.
- `push_subscriptions` tábla (felhasználónként több eszköz, nyelvvel: hu/en). A szerver `_maybe_push_turn`-nel értesít, ha a soron lévő regisztrált ember távol van (lecsatlakozott, vagy a lapja a `set_visibility` szerint háttérben): körönként egyszer, robotnak / vendégnek / napi feladványban soha.
- Service worker: `push` (értesítés) és `notificationclick` (az alkalmazás előtérbe hozása). Kliens: profil → kapcsoló (`Push` modul, engedélykérés, iOS-n a Főképernyőhöz adás tanácsa).

## Értékszám (ELO)

- `elo.rating_changes`: minden játékospárra külön (a nagyobb pont nyer), a változás az ellenfelek számával osztva; K = 32 az első 10 értékelt játékig, utána 20. Csak befejezett, robot nélküli játék, legalább két regisztrált játékossal (a vendégek nem számítanak); a változás a `game_players.rating_before/after`-ben is rögzül. A ranglistára 3 értékelt játék után lehet felkerülni.

## Kitüntetések

- `achievements.evaluate_game` a lépésnaplóból (bingó = 7 zseton egy lépésben, 100+ pontos lépés, 8+ zsetonos szó, joker lerakása) és a végeredményből (300+ pontos játék, győzelem 8–10. fokozatú robot ellen); `cumulative_badges` az összesítőkből (első játék / győzelem, 10 győzelem, 25 játék). `daily_best` a napi feladványból. Rögzítés: `auth.grant_achievements` (egyszer, kulcsonként), a kliens az `achievements_earned` eseményből értesül.

## Játékelemzés és megosztás

- A lépésnapló minden lépésnél tartalmazza a kezet a lépés előtt (`rack`); `analysis.analyze_game` az előző lépés tábla-pillanatképén megkeresi a legjobb lépést (`best_moves`), és összeveti a játszottal (kint maradt pont, játékosonkénti hatékonyság). Háttérben fut (`_run_analysis`), az eredmény a `game_analysis` táblában gyorsítótárazott (`ANALYSIS_VERSION` — a robot szókincsének változásakor növelni kell). Régi játékoknál nincs kéz → `unavailable`. Kliens: a visszajátszás képernyőn „Elemzés indítása”, lépésenként a legjobb lépés + „Legjobb lépés mutatása” (a lépés előtti tábla halvány zsetonokkal).
- Megosztás: `saved_games.share_token`; a `/?replay=TOKEN` link bejelentkezés nélkül is megnyitja a visszajátszást (a végeredménnyel).

## Admin panel

Részletes specifikáció: `docs/ADMIN_PANEL.md`. **Állapot: mind a nyolc lépés kész** — hozzáférés / őr / sudo / napló, áttekintés, felhasználók, élő szobák, játékarchívum, levelezős játékok, ranglista / értékszám, szótár, napi feladvány, moderáció, kommunikáció, statisztika, biztonság, rendszer, beállítások, admin napló; globális kereső (Ctrl+K), élő frissítés Socket.IO-n, magyar / angol felület, telefonon is használható.

### Ki az admin
- Az `ADMIN_EMAILS` környezeti változó (vesszővel elválasztott e-mail címek, kisbetűsítve hasonlítva) határozza meg; üresen a panel ki van kapcsolva (minden admin útvonal 404). Nem adatbázis-oszlop és nem a megjelenítési névhez kötött: `auth.is_admin_user(user)`. Vendég soha nem admin. **Csak olyan címet adj meg, amelynek a fiókja már a tiéd** (vagy SMTP-vel regisztrálj): az admin címnél a `request-code` SMTP nélkül sem adja vissza a kódot (`dev_code`), az a szerver konzolján olvasható. Az admin címek védettek: más fiók nem veheti fel őket, az admin fiókot nem lehet kitiltani, törölni, némítani (és saját magadat sem).
- Opcionális: `ADMIN_IP_ALLOWLIST` (IP-k / CIDR-ek; hibás elemnél a szerver el sem indul), `ADMIN_SESSION_IDLE_MINUTES` (30), `ADMIN_SUDO_MINUTES` (10).

### Láthatatlanság (nem admin semmit sem tud meg)
- Az őr (`src/admin/routes.rs`) az egész alkalmazás elé kerül (axum middleware, az útvonal-illesztés előtt), ezért a **rossz metódus** (405), a **nem létező alútvonal** és a `//admin` is ugyanazt a szokásos 404-et adja nem adminnak. Az admin válaszok fejlécei (`no-store`, CSP, `X-Frame-Options`...) csak az adminnak mennek (`g.admin_user`), így a 404 fejlécei sem árulkodnak. A `tests/admin_access.rs` az **összes** admin útvonalat az alkalmazás útvonaltérképéből állítja elő (új végpont automatikusan bekerül): nem adminnak mind azonos 404, CSRF nélkül 403, tétlenség után 401 `reauth`.
- Az oldal (`GET /admin` → `templates/admin.html`) és az assetek (`GET /admin/assets/<fájl>` ← `admin_assets/`) ugyanazt az őrt használják. Az admin JS / CSS / fordítás **nincs** a `static/` mappában, a nyilvános `i18n-data.js` nem tartalmaz `admin.` kulcsot, az `index.html` pedig semmilyen admin elemet; az „Admin” gombot a `static/app.js` `AdminEntry` modulja futásidőben szúrja be, ha a `/api/auth/me` `is_admin`-t jelez. A service worker az `/admin` útvonalakat nem érinti.
- CSP: `script-src 'self' https://cdnjs.cloudflare.com; style-src 'self'; connect-src 'self' ws: wss:` — az admin oldalon nincs beágyazott szkript és `style=` attribútum (a téma / nyelv beállítása az `admin-boot.js`-ben; a JS csak CSSOM-on át állít stílust). A Socket.IO kliens ugyanonnan töltődik, mint a nyilvános oldalon; ha nem érhető el, a nézetek időzítővel frissülnek.

### Az őr lépései (sorrendben)
1. Azonosítás (`session_token` süti → `validate_session` → `is_admin_user`), különben 404. 2. IP-engedélylista (kívülről 404). 3. IP-alapú forgalomkorlát (`admin`). 4. CSRF: nem biztonságos metódusnál kötelező az `X-Admin-Request: 1` fejléc **és** az `Origin` (vagy `Referer`) hostja egyezzen a kérés hostjával (különben 403). 5. Tétlenség: az utolsó admin-kérés óta eltelt idő > `ADMIN_SESSION_IDLE_MINUTES` (vagy nincs időbélyeg) → 401 `{reauth: true}`; kivétel az oldal, az assetek, `GET /api/admin/session` és `POST /api/admin/reauth` (ezek nem frissítik az időzítőt). A sikeres kérés ritkítottan (15 mp) frissíti a `sessions.admin_seen_at`-et. A friss belépés (jelszó) is beállítja.
- **Sudo mód**: `POST /api/admin/sudo {password}` → `ADMIN_SUDO_MINUTES` percre a `sessions.sudo_until`-ban (a sessionhöz kötve, tétlenség / újraigazolás lezárja); `DELETE /api/admin/sudo` lezárja. A romboló végpontokra `@admin_routes.sudo_required` (401 `{sudo_required: true}`), a sudo nélküli, de veszélyesekre `@admin_routes.danger` (`admin_danger` forgalomkorlát) kerül. A kliens (`Api.call`) a 401-et felismeri: jelszót kér, majd megismétli a kérést. A rossz jelszó a `login` forgalomkorlátjába számít és naplózódik (`admin.sudo_failed` / `admin.reauth_failed`).

### Napló (`admin_audit`)
- Tábla: id, admin_user_id, action, target_type, target_id, details_json, ip, user_agent, created_at. **Nem törölhető és nem szerkeszthető**: `BEFORE UPDATE` / `BEFORE DELETE` trigger. A napló nem törlődik a felhasználó törlésekor sem (nincs idegen kulcs).
- **Módosító műveletek** mindig így íródnak: `with admin.action(ctx, 'user.ban', 'user', uid, reason=indoklás, details={...}) as act: act.conn.execute(...); act.details['before'] = ...; act.details['after'] = ...`. Az indoklás kötelező (3–500 karakter; hiányában a művelet el sem indul; csak ott hagyható el, ahol a tartalom maga az indoklás: közlemény, push, e-mail — `require_reason=False`), a naplósor a művelettel **egy tranzakcióban** íródik (hiba vagy naplózási hiba esetén minden visszagördül). A memóriabeli mellékhatásokat (socket bontás, szoba) a blokk UTÁN kell elvégezni; a memóriabeli beavatkozásoknál (szoba) a naplósor a művelet ELŐTT íródik, így naplózási hiba esetén a művelet nem történik meg. Személyes adat megtekintése: `admin.record(conn, ctx, 'view.…', …)` (indoklás nélkül: `view.user`, `view.game`, `view.racks`, `view.chat`, `view.report`, `view.codes`, `view.logins_export`, `view.audit_export`). Az `AdminContext(admin_user_id, ip, user_agent)` a kérésből jön (`g.admin_ctx`).
- Végpontok (`/api/admin`, mind JSON): `GET /session`, `POST /reauth`, `POST|DELETE /sudo`, `GET /audit?admin=&action=&target_type=&target_id=&since=&until=&q=&limit=&offset=` (lapozott lista, legfeljebb 200); `format=csv` vagy `download=1` a letöltés (legfeljebb 10 000 sor; a CSV-ben a `=`, `+`, `-`, `@` kezdetű cellák elé `'` kerül).

### Menüpontok és végpontok (`/api/admin`)
| Menüpont | Végpontok (kivonat) | Sudo |
|---|---|---|
| **Áttekintés** | `GET /overview` (élő számlálók, mai adatok, szerver, szolgáltatások, figyelmeztetések), `GET /charts?days=` (30 napos grafikonok), `GET /search?q=` (felhasználó / szoba / játék / szó) | – |
| **Felhasználók** | `GET /users` (szűrők: `q status online admin from to min_games inactive_days sort order`, CSV), `GET /users/<id>` (részletek, `view.user`), `/profile` (olvasható nézet, megszemélyesítés nincs), `PATCH /users/<id>` (név; e-mail sudo-val, a régi címre értesítés), `POST /users/<id>/ban` (sudo; 1h/1d/7d/30d/végleges), `unban`, `mute`, `unmute`, `logout-all`, `reset-password` (sudo; SMTP nélkül egyszer a válaszban), `rating` (sudo), `badges`, `recompute-stats`, `reviews/revert` (sudo), `reviews/block`, `push-test`, `push/<id>` (törlés), `notes`, `export` (GDPR JSON), `DELETE /users/<id>` (sudo; anonimizálás / törlés, a név begépelése) | részben |
| **Élő szobák** | `GET /rooms`, `GET /rooms/<id\|kód>` (tábla, zsák, szavazás, történet, chat, türelmi idők), `/racks` (kezek, `view.racks`), `POST /rooms/<id>/action {action}`: `message kick transfer skip extend pause resume resolve_vote reschedule_bot save end void disband` (`end`, `void`, `disband`: sudo) | részben |
| **Játékok** | `GET /games` (szűrők, CSV), `/games/<id>` (`view.game`), `/moves` (visszajátszás), `/export`, `POST /void` `unvoid` (az ELO újraszámolódik) `unshare` `status` `reanalyze`, `DELETE /games/<id>` (sudo), `GET|POST /games/cleanup` | részben |
| **Levelezős** | `GET /async`, `POST /async/<id>/extend expire resign remind void load unload`, `POST /async/sweep` | – |
| **Ranglista** | `GET /ratings?metric=`, `/ratings/suspicious`, `POST /ratings/recompute {dry_run}` (alkalmazás: sudo) | részben |
| **Szótár** | `GET /dictionary/word?w=` (döntési lánc), `/rejected` (+ `preview`, `download`, `export-votes` sudo), `/overrides`, `/additions`, `DELETE /votes` (sudo), `/second-opinion`, `/reviewers`, `/review-summary`, `/threshold`, `POST /caches/clear` | részben |
| **Napi feladvány** | `GET /daily?date=`, `/daily/archive`, `/daily/preview`, `POST /daily/<dátum>/regenerate`, `DELETE /scores/<user>`, `POST /scores/<user>/reset-revealed` | ha voltak próbálkozások |
| **Moderáció** | `GET /reports`, `/reports/<id>` (`view.report`), `PATCH /reports/<id>`, `GET /moderation/chat?source=live\|log` (`view.chat`), `/moderation/words` (+ `POST`, `DELETE`), `/moderation/names` | – |
| **Kommunikáció** | `GET|POST /announcements`, `PATCH|DELETE /announcements/<id>`, `GET|POST /maintenance`, `POST /push/preview`, `POST /push` (csoportnál `confirm`), `POST /email`, `GET /email/preview`, `POST /email/bulk` (sudo, percenként egy, max. 500) | részben |
| **Statisztika** | `GET /stats?metric=registrations\|active\|retention\|games\|bots\|words\|challenges\|practice\|review\|heatmap&range=7\|30\|90\|365` (CSV) | – |
| **Biztonság** | `GET /security/logins` (gyanús minták jelölve, CSV), `/security/rate-limits` (+ `POST /unblock`), `/security/ip-bans` (+ `POST` sudo, `DELETE`), `/security/codes` (`view.codes`), `/security/sessions` (+ `DELETE`, `POST revoke-all` sudo, `POST close-admins`) | részben |
| **Rendszer** | `GET /system` (szerver, szolgáltatások, konfiguráció titkok nélkül, verziók, adatbázis, mentések, háttérfolyamatok, push, tunnel), `/system/logs`, `GET|POST /system/backup` (letöltés: sudo), `POST /system/vacuum`, `GET|POST /system/cleanup` (sudo), `POST /system/tunnel/restart` (sudo), `smtp-test`, `push-test`, **levelező szerver**: `GET|PATCH|DELETE /system/mail` (PATCH / DELETE sudo), `POST /system/mail/check` (sudo), **frissítés**: `GET /system/update`, `POST /system/update/check`, `POST /system/update/apply` (sudo), `POST /system/restart` (sudo) | részben |
| **Beállítások** | `GET /settings`, `PATCH /settings {changes: {kulcs: érték \| null}, reason}` (`null` = vissza az alapra) | – |
| **Admin napló** | `GET /audit` | – |

Leállítás gomb **nincs** (a folyamatot a gazdagép kezeli): helyette karbantartási mód. Újraindítás **van**, de csak a frissítés (és a Rendszer oldal „Újraindítás” gombja) része: sudo + indoklás, a folyamat önmagára cserélődik (`os.execv`); lásd „Frissítés GitHubról és újraindítás”.

### Levelező szerver (SMTP)
- **Kétféle forrás**: az admin panelen mentett felülbírálat (`app_settings`, `mail.smtp` kulcs, JSON: `host`, `port`, `security` = `starttls` | `ssl` | `none`, `username`, `password`, `from_address`, `from_name`, `verify_tls`, `updated_by/at`) — ez az erősebb —, ennek híján a környezeti `SMTP_*`. A `mail_config.current()` hívási időben számolja ki (`source`: `database` | `environment`, `configured`); az `email_service` minden küldésnél ezt olvassa, ezért a mentés **újraindítás nélkül** hat. Környezeti forrásnál a `configured` a korábbi szabály (`config.SMTP_CONFIGURED`: user + jelszó + feladó); mentett beállításnál: kiszolgáló + port + feladó, és ha van felhasználónév, jelszó is (felhasználónév nélkül bejelentkezés nélkül küld, pl. helyi levelező).
- **Jelszó**: az adatbázisban van (mint a push VAPID kulcsa, ezért egy adatbázis-mentés tartalmazza), de **sosem kerül ki**: a válaszban csak `password_set`, a naplóban csak `password_changed`. Üresen hagyva (`null`) a mentett marad, **de csak ugyanahhoz a kiszolgálóhoz és felhasználónévhez** — másik címre nem küldhető el újragépelés nélkül (mentésnél és próbánál sem).
- **Végpontok** (`src/admin/mail.rs` ellenőriz és naplóz): `GET /system/mail` (jelszó nélkül + a környezeti alapérték), `PATCH` (sudo, kötelező indoklás; előtte/utána a naplóban), `DELETE` (sudo, vissza a környezeti beállításra; nincs mentett → 409), `POST /system/mail/check` (sudo; a még nem mentett űrlapot próbálja ki: csatlakozás, titkosítás, bejelentkezés, `send: true` esetén teszt levél **csak a saját admin címedre**; a válasz biztonságos hibakód: `dns`, `timeout`, `refused`, `connect`, `tls`, `certificate`, `auth`, `unsupported`, `sender`, `recipient`, `disconnected`, `protocol`, `error` — a kiszolgáló szövege nem megy a felületre). A próba a naplóba kerül (`system.mail_check`). `smtp-test`: a mentett beállítással teszt levél magadnak (hibánál `code`).
- **Tanúsítvány**: alapértelmezésben ellenőrzött (`ssl.create_default_context()`); az űrlapon kikapcsolható (`verify_tls`) önaláírt tanúsítványú saját levelezőhöz. A fejlécek sortörés ellen védettek (feladó neve / címe), a levél kap `Date` és `Message-ID` fejlécet, a feladó neve a `From`-ban (`Név <cím>`), a boríték feladója a bare cím.
- **Felület** (Rendszer → „Levelező szerver (SMTP)” kártya): állapot és forrás, űrlap (a titkosítás váltása a szokásos portra állítja a portot, titkosítás nélkül figyelmeztetés), „Kapcsolat kipróbálása”, „Mentés”, „Teszt e-mail magamnak”, „Visszaállítás az alapra”.

### Frissítés GitHubról és újraindítás
- **Mit tud** (`src/admin/update.rs`, Rendszer → „Frissítés GitHubról” kártya): a program mappája git tár; az állapot (ág, commit, távoli cím **hozzáférési adatok nélkül**, helyi módosítások, futó verzió), „Ellenőrzés a GitHubon” (`git fetch --prune origin`: ágak listája, a kiválasztott ágra váltva/frissítve a beérkező commitok és a változó fájlok), „Frissítés” (a legfrissebb vagy egy megadott ágra), „Újraindítás”.
- **Biztonsági korlátok**: csak a rögzített `origin` távoli tárral dolgozik; az ág neve szigorúan ellenőrzött (`^[A-Za-z0-9][A-Za-z0-9._/-]{0,99}$`, nincs `..`), és a GitHubról éppen lekért ágak között kell lennie; csak **fast-forward** (`git merge --ff-only`), tiszta munkafán (a követett fájlok módosítása blokkol — a nem követettek, pl. az adatbázis, nem); nincs `reset --hard` az admin által; egyszerre egy frissítés fut. Sudo + kötelező indoklás; a naplósor a művelet **előtt** íródik (`system.update`), az eredmény utána (`system.update_done` / `system.update_failed`).
- **Visszagörgetés**: ha a Rust forrás (vagy a `Cargo.*`) változott és a „Függőségek telepítése” (újrafordítás) kapcsoló be van kapcsolva, a frissítés után lefut a `cargo build --release`; hibánál (422) visszaáll az előző ágra és commitra (`Az új kód nem fordítható le, a frissítés visszaállt az előző állapotra.`). Eltérő előzménynél (helyi commit) nem ír felül semmit. A `requirements_changed` jelző a kliensben a „fordítás szükséges” jelzést adja (`Cargo.toml`, `Cargo.lock`, `build.rs`, `src/**/*.rs` változott).
- **Újraindítás**: a futó folyamat az induláskori commitját rögzíti (`_RUNNING_COMMIT`), ezért a `restart_needed` jelzi, ha a lemezen újabb kód van. Az újraindítás (`POST /system/restart`, vagy „Újraindítás a frissítés után”) sudo + indoklás, röviddel a válasz után a tunnel leáll és a folyamat `os.execv`-vel önmagára cserélődik (azonos parancssor); a kapcsolatok megszakadnak, a kliensek újracsatlakoznak, a tunnel címe megváltozhat (gyors tunnelnél). A felület `/api/admin/session` lekérdezésével várja meg, hogy a szerver visszajöjjön, majd újratölt. Ha az új kód mégsem indul el, a gazdagépen kell helyreállítani. Tesztben a `_restart_process` és a `_pip_install` hamisítva van.
- Hiba a lekérésnél: 502 `detail`-lel (a git kimenete, a `https://felhasznalo:token@` részek maszkolva).

### Beállítások (`src/settings.rs`, `app_settings` tábla, `cfg.<kulcs>`)
`registration_open`, `guest_allowed`, `room_creation` (everyone / registered / none), `max_spectators`, `default_hint_limit`, `bots_enabled`, `max_bots`, `default_bot_level`, `bot_think_multiplier`, `feature_daily` / `feature_async` / `feature_practice` / `feature_word_review`, `word_reject_threshold`, `grace_disconnect`, `grace_waiting_owner`, `chat_max_length`, `chat_rate_count` / `chat_rate_window`, `banned_word_action` (mask / drop), `chat_log_enabled` / `chat_log_days`, `backup_daily` / `backup_keep`, `rate_limits_http` / `rate_limits_socket` (felülírások), rejtetten `maintenance`. Olvasás: `settings.get(kulcs, alapérték)` (gyorsítótárazott, `auth.DB_PATH`-hoz kötve); a módosítás után a `server.apply_runtime_settings()` érvényesíti (forgalomkorlátok, szótár-építő küszöb, chat). Az érték mellett látszik az alapérték, a „visszaállítás alapra” és a legutóbbi módosító. Új funkciókapcsoló: `DEFINITIONS` + `SETTING_TEXT` az `admin-views-c.js`-ben + fordítás.

### Adatbázis (az admin panel miatti bővítések, migrációval)
- `users`: `banned_until` (végleges: `9999-12-31 23:59:59`), `ban_reason`, `chat_muted_until`, `review_blocked`, `last_login_at`, `last_login_ip`, `deleted_at`; `sessions`: `ip`, `user_agent`, `last_seen`, `admin_seen_at`, `sudo_until`.
- Új táblák: `admin_audit`, `user_admin_notes`, `rating_adjustments` (kézi értékszám-módosítás; az ELO újraszámolás ezt is figyelembe veszi), `word_overrides`, `word_additions`, `announcements`, `reports`, `login_events`, `ip_bans`, `chat_log`, `usage_counters` (gyakorló módok napi hívásszáma), `banned_words`, `app_settings`.
- Kitiltás: a `validate_session` kitiltott / törölt felhasználóra `None`-t ad (a session azonnal érvénytelen), a socket-kapcsolatok bezárulnak (`admin_live.kick_user`, `account_banned`), a futó játékban lecsatlakozottá válik; belépéskor a felhasználó látja az okot és a lejáratot. IP tiltás: `@app.before_request` és a socket `connect` (30 mp-es gyorsítótár). Némítás: a `send_chat` csendben eldobja. Karbantartási mód: új szoba / levelezős játék / napi próbálkozás letiltva (az admin kivétel), bannerrel.
- Anonimizálás: név → „Törölt felhasználó #id”, e-mail és jelszó törölve, a játékok és az értékszám-előzmények megmaradnak; teljes törlés külön opció. Az ELO újraszámolás (`admin_games.compute_ratings`) a befejezett, robot nélküli játékokat `updated_at, id` szerint játssza újra: a lépésenkénti számolással azonos eredményt ad (teszt). Érvénytelenített játék: `saved_games.status = 'voided'`, nem számít az értékszámba / statisztikába / ranglistába.

### Kliens felépítése (`admin_assets/`)
- Szabályok: kizárólag DOM API (`h()` elemépítő: a szöveg mindig szövegcsomópont; `innerHTML` nincs), **minden szöveg** `t('admin.kulcs')` vagy `data-i18n` (a kulcsok csak `[a-z_]`, szó szerint a kódban — nincs összefűzés; hu **és** en az `admin-i18n.js`-ben, a szerver magyar üzenetei az `en.server.exact`-ban), a hálózati hívás egyetlen helyen (`Api.request`, minden kérésen az `X-Admin-Request` fejléc), a kulcsok és az API útvonalak létét a tesztek ellenőrzik.
- `Router`: `#menüpont/azonosító?szűrők` (a szűrők, a lapozás, a rendezés és a fül az URL-ben: vissza gomb, linkmásolás); `registerSection({id, order, icon, labelKey, render})`; `Router.onLeave` / `Router.interval` a nézet elhagyásakor takarít. `mountList` — szűrős, rendezhető, lapozott lista CSV exporttal; `Act.open({title, fields, confirmName, run, done})` — minden módosító művelet űrlap-párbeszéde (kötelező indoklás, célpont-név begépelése, a mezőnkénti szerverhiba kiemelése, sudo-kérés a 401 után); `Chart` (SVG oszlop / vonal / halmozott / hőtérkép / vízszintes oszlop), `renderBoard` (15×15 tábla, a premium mezőkkel), `loadInto` (vázlat, hiba, újrapróbálás).
- Élő frissítés: `Live` (Socket.IO `admin` szoba; `admin_overview` ötmásodpercenként, `admin_room_update`, `admin_room_state` a figyelt szobára, `admin_alert`, `admin_report` → toast + menü-jelvény); Socket.IO nélkül (CDN nem elérhető) a nézetek időzítővel frissülnek.
- Telefonon a menü oldalról csúszó lap, a táblázatok soronként kártyák, a lapok görgethetők.

- **Nagy képernyő és görgethető sorok**: a tartalom (`.admin-main`) középre igazított, 1600 / 1920 / 2400 px fölött szélesebb (`max-width`: 1400 / 1560 / 1840 — nem nyúlik a képernyő széléig), az oldalmenü a topbar alatti teljes magasságot kitölti (sticky, saját háttérrel), a betűméret-tokenek és a tábla legnagyobb mérete (`--admin-board-max`) nő; a tábla rácsa rögzített 15×15 sáv (`minmax(0, 1fr)`), hogy az üres sorok ne essenek össze. A vízszintesen görgethető fülsor (`UI.tabs`) és az oldalmenü a kijelölt elemre gördül (`revealInScroller`, `ScrollRow.watch`: elhalványuló szélek a `can-scroll-left/right` osztályokkal). A lépéstípusok emberi felirata: `moveTypeLabel`.
- **Elrendezési konvenciók** (2K-n ellenőrizve, hu / en, világos / sötét, 390–2560 px): a szűrőűrlap egy sorba törő flex (azonos szélességű mezők, a jelölőnégyzet is keretes „mező”, a gombok jobbra; egyetlen mezős űrlapnál a gomb a mező mellett); a fülön belüli lista (`mountList`, `keep: ['tab']`) nem kap saját címsort (a frissítés gomb az eszköztárba kerül); a táblázat rövid cellái (≤ 28 karakter: időpont, IP, azonosító) `cell-short` jelölést kapnak és nem törnek, a hosszú szöveg `overflow-wrap: break-word`-del törik; a kártyán belüli táblázat első / utolsó cellája a kártya szélével egy vonalban kezdődik; `UI.kv` sorai (`.admin-kv-row`) széles helyen több oszlopba rendeződnek, a hosszú érték (commit, cím, böngésző) `[címke, érték, {wide: true}]`-val a teljes sort kapja; `UI.cardGrid` egymás melletti kártyákat ad (a táblázatos / tábla / napló kártya a teljes sort kapja); a `Chart` az SVG-t a tároló tényleges szélességére rajzolja (`ResizeObserver`, 1:1 léptékkel, „szép” tengelylépés, darabszámnál egész értékek); a Beállítások nézet középre igazított keskeny oszlop (`.admin-narrow`), a logikai kapcsoló a játékos oldali `.toggle-switch`, a „Mentés” csak módosítás után él.

### Új admin funkció hozzáadása (teendők)
1. Logika az `src/admin/*.rs` megfelelő moduljában (vagy újban), módosításnál `admin::action(...)`. 2. Végpont az `src/admin/api/*.rs`-ben, a jogosultsági szinttel (`Level::Danger` / `Level::Sudo`: `post_danger`, `post_sudo`…). 3. Nézet az `admin-views-*.js`-ben (`registerSection` új menüponthoz), **minden szöveg** az `admin-i18n.js`-ben (hu **és** en; az új szerverüzenet is az `en.server.exact`-ban). 4. Teszt a funkcióhoz; a hozzáférési tesztek az új útvonalat automatikusan lefedik (útvonaltérkép), de a `tests/frontend_admin.rs` számon kéri, hogy minden végpontnak legyen felülete. 5. Kliens-oldali hiba kereséséhez: `tests/admin_browser.rs` (valódi szerver + Playwright; kimarad, ha nincs node / Playwright / Chromium — a `PLAYWRIGHT_MODULE`, `PLAYWRIGHT_CHROMIUM` környezeti változóval felülírható).

## Ismert problémák / TODO

### Játékmenet
- [x] Challenge rendszer — szó megkérdőjelezése más játékos által (30 mp ablak, megtámadás/elfogadás)
- [x] Játék mentés / visszatöltés — manuális mentés (owner-only), lobby-first restore flow
- [x] Visszajátszás — befejezett játék lépéseinek visszanézése
- [x] Időlimit a körökre — opcionális időzítő (0/60/90/120/180/300 mp), lejáratkor automatikus passz
- [x] AI ellenfél — egyjátékos mód számítógépes ellenfél(ek)kel, nehézségi szintek (tőszavak + gyakori ragozott alakok), „igazodik hozzám” mód
- [x] Napi feladvány, gyakorló módok, levelezős játék, játékelemzés, ELO, kitüntetések, replay-megosztás, visszavonás

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
- [x] Admin panel — felhasználók, élő szobák, archívum, szótár, moderáció, kommunikáció, statisztika, biztonság, rendszer, beállítások (lásd „Admin panel”)
- [x] Játékos oldali újdonságok az admin panelhez: bejelentés (játékos / chat üzenet), közlemény-banner, karbantartási mód, kitiltás üzenete

### Ötletek
- [ ] Szótár-építő: az AI által elutasított szavak emberi második véleménye (külön mintavétel a `hu_rejected.txt` szavaiból), a felhasználói szavazatok exportja a tartós listába, mintázat-alapú átvizsgálás (a népnév + -gyűlölet / -ellenesség / -üldözés … családok teljes végigjárása), szavankénti szerepkör / küszöb
- [ ] Robot: kétszeres ragozás és hosszabb szótövek (memória!), tapasztalati értékelés (szimuláció), emberszerűbb lépések (kevesebb egyzsetonos lépés az alsó fokozatokon), gyakori szavak listája a ritka szavak elkerülésére, fokozat robotonként a felületen
- [ ] Levelezős játék: nyitott (ismeretlen ellenfeles) játékok, e-mail értesítés push híján, chat, emlékeztető a határidő előtt
- [ ] Több nyelv a felületen (a `i18n-data.js` blokkja és a `SUPPORTED` lista bővítésével)
