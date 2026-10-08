# Magyar Scrabble Klón — fejlesztői útmutató

Webes magyar Scrabble online többjátékos támogatással. **Rust** backend (axum + socketioxide + tokio, SQLite), vanilla JS frontend (build lépés nélkül).
Egyetlen, könnyen futtatható program, otthoni szerverre is. Felhasználói leírás: [README.md](README.md); ez a fájl a **fejlesztőknek és az AI asszisztenseknek** szól.

> A korábbi Python (Flask + Socket.IO) változat a `python-final` ágon / címkén van. A Rust verzió **protokoll- és adatbázis-kompatibilis** vele: ugyanazok a HTTP útvonalak
> és Socket.IO események, ugyanaz az SQLite séma (a meglévő `scrabble.db` változtatás nélkül folytatható), a kliens (`web/`) változatlan. Az egyezést aranyfájlok
> (`tests/golden/`) és differenciális összevetés (`tests/compat/`) őrzi; a teljesítmény-összevetés a `docs/PERFORMANCE.md`-ben van.

## Dokumentáció-térkép

| Mit keresel? | Hol? |
|---|---|
| Felhasználói bemutató, képek, gyors indítás | [README.md](README.md) |
| Telepítés lépésről lépésre (Windows / macOS / Linux / Raspberry Pi), tunnel, `.env`, systemd, hibaelhárítás | [docs/INSTALL.md](docs/INSTALL.md) |
| A játék használata (játékosoknak) | [docs/USER_GUIDE.md](docs/USER_GUIDE.md) |
| Magas szintű felépítés: diagramok, egyidejűség, adatmodell, robot, szótár, biztonság | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| HTTP útvonalak, Socket.IO események, forgalomkorlátok, `game_state` | [docs/PROTOCOL.md](docs/PROTOCOL.md) |
| Az admin panel részletes specifikációja (a megvalósítás eredeti utasítása; a Python-fájlnevek megfeleltetése a fejlécben) | [docs/ADMIN_PANEL.md](docs/ADMIN_PANEL.md) |
| Python ↔ Rust mérések · a robot párharca külső motorral | [docs/PERFORMANCE.md](docs/PERFORMANCE.md) · [docs/ENGINE_DUEL.md](docs/ENGINE_DUEL.md) |
| Aranyfájlok előállítása | `tests/golden/README.md` |

## Gyors referencia

```bash
cargo build --release                   # egyszer (és frissítés után); Rust 1.85+ (edition 2024); ~2,5 perc négy magon
scripts/run.sh                          # Cloudflare tunnel-lel (publikus URL), ha van `cloudflared`
scripts/run.sh --no-tunnel              # csak helyi hálózat
# vagy közvetlenül: target/release/scrabble [--no-tunnel]
cargo run -- --no-tunnel                # fejlesztéshez
cargo test                              # minden teszt (az első futás lassabb: a függőségek optimalizáltan fordulnak)
cargo test --test admin_users           # egy tesztfájl
cargo test --lib                        # csak az egységtesztek
cargo clippy --all-targets              # lint
cargo fmt                               # rustfmt (160 karakteres sor, rustfmt.toml)
```

Böngészőben: <http://localhost:5000>. A szerver `0.0.0.0`-n figyel, leállítás: `Ctrl+C` / `SIGTERM` (kíméletes). A gépre jellemző beállítások a **`.env`** fájlba kerülnek (minta:
`.env.example`; a fájlt a `base_dir()`-ből, azaz a program mappájából olvassa a program; gitignorált, a frissítés nem írja felül); a ténylegesen beállított környezeti változó erősebb.
Otthoni szerverre: `deploy/scrabble.service` (systemd).

**Környezeti változók** (mind opcionális): `PORT` (5000), `ADMIN_EMAILS`, `ADMIN_SESSION_IDLE_MINUTES` (30), `ADMIN_SUDO_MINUTES` (10), `ADMIN_IP_ALLOWLIST`, `SMTP_HOST` (`smtp.gmail.com`) / `SMTP_PORT` (587) /
`SMTP_USER` / `SMTP_PASSWORD` / `SMTP_FROM`, `SECRET_KEY` (nélküle induláskor véletlen kulcs), `VAPID_PRIVATE_KEY` / `VAPID_SUBJECT`, `SCRABBLE_DB_PATH` (`scrabble.db`, **relatív a munkamappához**),
`SCRABBLE_BACKUP_DIR` (`backups`, ugyanígy), `SCRABBLE_BASE_DIR` (ahol a `web/` és a `dict/` van; alapból a munkamappa, a futtatható fájl környéke, végül a fordítás helye), `SCRABBLE_REJECTED_FILE` (az elutasított szavak listája; alapból `dict/hu_rejected.txt`), `WORD_REJECT_THRESHOLD` (1).
Hibás `ADMIN_IP_ALLOWLIST` esetén a szerver el sem indul (az elírás ne kapcsolja ki csendben a korlátozást).

**Platform**: a fejlesztés és a tesztek Linuxon futnak. Windowson a szerver fordul és fut (a Unix-specifikus részek `cfg(unix)` mögött vannak), de: az automatikus tunnel-indítás a PATH-ban a `cloudflared` nevet
keresi (`.exe` nélkül, `src/services/tunnel.rs`), az admin panelről indított újraindítás (`exec`, `src/admin/update.rs`) nem-Unixon nem csinál semmit, és a futó `.exe` cseréje (frissítés) sem megoldott.

## Alapszabályok (ezeket minden változtatásnál tartsd be)

1. **Zárolási sorrend: állapot → adatbázis.** Az `App.state` (`Mutex<ServerState>`) és az `App.db` (`Mutex<Connection>`) együtt csak ebben a sorrendben zárolható. A szinkron Socket.IO kezelők a zárolt állapoton dolgoznak.
2. **Nehéz munka nem a zár alatt.** A robot keresése, a tipp, a napi feladvány, az elemzés a `spawn_blocking` szálon fut; közben az állapot változhat, ezért lépés előtt újraellenőrizni kell (kör, szoba, azonosító).
3. **Az időzítők érvényteleníthetők:** a `Room` számlálói (`turn_timer_id`, `challenge_timer_id`, `bot_turn_id`) minden új ütemezésnél nőnek; a lejáró feladat összeveti a sajátját, és elavultan nem tesz semmit. A türelmi idők lecsatlakozási sorszámot kapnak.
4. **Egy kapcsolat eseményei a beérkezés sorrendjében hatnak** (`InOrder` kinyerő). Új szinkron esemény: kezelő `fn(&Arc<App>, &mut ServerState, &str, Value)` + bejegyzés a `SYNC_EVENTS` táblázatba (`src/server/mod.rs`); hosszabb munkához aszinkron kezelő az `attach_handlers`-ben.
5. **A szerver az igazság.** Tábla, kéz, pontozás, szóellenőrzés a szerveren; a kliens csak megjelenít és javasol. Minden bemenetet a szerver ellenőriz (nevek, pozíciók, e-mail, jelszó, tiltott szavak).
6. **A szerver üzenetei magyarok**, a kliens fordítja (`tServer()`, `i18n-data.js` → `en.server.exact` / `patterns`). Új felhasználói szöveg: kulcs a hu **és** en blokkba, `data-i18n*` / `t()` a kódban — a `tests/frontend_i18n.rs` hibára fut, ha hiányzik fordítás, nem használt kulcs marad, vagy egy szerver-üzenet fordítatlan. Az admin felületnek külön `web/admin/admin-i18n.js` van.
7. **Kliens: DOM API, nincs `innerHTML`** (`textContent`, `createElement`, `addEventListener`; admin kliensben a `h()` elemépítő); az admin oldalon nincs beágyazott szkript és `style=` attribútum (CSP).
8. **Adatbázis-kompatibilitás.** A séma a Python változattal azonos. Új tábla: `CREATE TABLE IF NOT EXISTS` a `src/db/schema.sql`-ben; új oszlop meglévő táblán: `ensure_column` a `Db::init`-ben. A `saved_games.state_json` és a lépésnapló formátumát (kéz, pont, szavak) ne törd el: a mentések, a visszajátszás, az elemzés és a kitüntetések ebből élnek.
9. **Forgalomkorlát minden új végponton / eseményen:** socket: `SOCKET_RATE_LIMITS` (`src/app.rs`), HTTP: `AUTH_RATE_LIMITS` (`src/config.rs`) + `app.limiter.check_ip(...)`.
10. **Admin: láthatatlanság.** Nem admin számára minden admin útvonal (rossz metódus, nem létező alútvonal, `//admin` is) azonos 404; a módosító műveletek kötelező indoklással, a művelettel egy tranzakcióban kerülnek a csak hozzáfűzhető naplóba (lásd „Admin panel”).
11. **A robot szókincsének vagy a szótárnak (elutasított lista, szűrők) a változásakor** növeld az `analysis::ANALYSIS_VERSION`-t (jelenleg 7), különben a gyorsítótárazott elemzések elavultak maradnak; a golden tesztek is érintettek lehetnek (lásd „Ismert problémák”).
12. **A `CLAUDE.md` „Összesen: N teszt” sorát az admin panel olvassa ki** (`admin::system::test_count`, regex: `Összesen:\s*(\d+)\s*teszt`) — a formátumot tartsd meg, és a számot a tesztek változásakor frissítsd.

## Fájlstruktúra

```
.
├── Cargo.toml, Cargo.lock, build.rs   — Rust projekt (a `scrabble` szerver + 3 karbantartó eszköz + `compare` mérés)
├── README.md, CLAUDE.md, .env.example — dokumentáció és minta beállítás
├── src/                               — a Rust forrás (lásd lent)
├── web/                               — a böngészős kliens
│   ├── static/                        —   nyilvános fájlok (`/static/…`): app.js, style.css, i18n.js, i18n-data.js, PWA, ikonok
│   ├── templates/                     —   index.html, admin.html, sw.js (sablonok; a `{{ asset_v }}` a fájlok mtime-ja)
│   └── admin/                         —   az admin kliens (`/admin/assets/…`, csak az őrzött útvonalon)
├── dict/                              — hu_HU szótár (hu_HU.aff / .dic), hu_attested.txt, hu_rejected.txt
├── tests/                             — integrációs tesztek (`*.rs`), `common/`, `frontend_support/`, `golden/`, `compat/`, `browser/`
├── benches/compare.rs                 — teljesítményteszt a Python és a Rust verzió között
├── third_party/pg-scrabble/           — a külső `scrabble` 0.1.0 motor vendorolt, 64 bites betűmaszkokkal kiszélesített másolata (MIT; csak a `engine-duel` kapcsolóval fordul)
├── scripts/                           — run.sh (indítás), perf.sh (mérés), python-baseline.sh (a régi verzió előkészítése)
├── deploy/scrabble.service            — systemd egység az otthoni szerverhez
└── docs/                              — INSTALL, USER_GUIDE, ARCHITECTURE, PROTOCOL, ADMIN_PANEL, PERFORMANCE, ENGINE_DUEL, screenshots/, engine_duel/ (a tanult maradék-értékek)
```

### `src/` — témakörönként

A moduloknak rövid, lapos elérési útjuk is van (`crate::tiles` = `crate::engine::tiles`; a `lib.rs` újraexportálja), a kód és a tesztek ezeket használják.

- `main.rs` — indulás: konfiguráció, adatbázis, szótár és szókincs betöltése, napi feladvány, levelezős játékok visszaállítása, háttérfeladatok, tunnel, szerver; `lib.rs` — modulok és újraexportok
- `app.rs` — az `App` (konfiguráció, adatbázis, beállítások, `Mutex<ServerState>`, forgalomkorlát, levelező, push, tunnel, háttérfeladatok és elemzések állapota; `emit_to` / `emit_room` / `emit_all`; `SOCKET_RATE_LIMITS`)
- `config.rs` — `.env` beolvasása (`load_env_file`), konfiguráció a környezeti változókból, `AUTH_RATE_LIMITS`, `base_dir()` / `dict_dir()`; `settings.rs` — futásidejű beállítások (`app_settings`, `definitions` / `get` / `store` / `validate`), újraindítás nélkül hatnak
- `util.rs` — időbélyegek, tokenek, szigorú egész-olvasás; `async_games.rs` — levelezős játék: `build_game`, névütközés, a lobby-lista összegzése
- `engine/` — a játék szabályai: `tiles.rs` (100 zseton, `TileBag`, `tokenize_word`, `forms_digraph`), `board.rs` (15×15, premium mezők, elhelyezés-ellenőrzés, pontozás), `game.rs` (`Game`: körök, vég, szavazás, napi feladvány, levelezős mód, `get_state` / `to_save_dict`), `player.rs`, `challenge.rs` (szavazási állapotgép)
- `robot/` — `ai.rs` (szókincs, horgonyalapú lépésgenerátor, 10 fokozat, „igazodik hozzám”, tippek), `analysis.rs` (játékelemzés, gyorsítótárazva), `daily.rs` (napi feladvány)
- `words/` — `affix.rs` (tiszta Rust, Hunspell-szerű ellenőrző, összetételi szabályok nélkül; morfológiai címkék alapján szűri a furcsa alakokat), `dictionary.rs` (szótár-API, `filter_valid` gyorsítótár, `suggest_words`, elutasított szavak), `practice.rs` (gyakorló módok motorja, állapotmentes), `word_review.rs` (szótár-építő)
- `accounts/` — `password.rs` (werkzeug-kompatibilis PBKDF2-SHA256), `socket_auth.rs` (aláírt, rövid életű socket-token), `elo.rs`, `achievements.rs`
- `services/` — `mail.rs` (SMTP), `push.rs` (Web Push: saját RFC 8291 / 8292), `ratelimit.rs`, `tunnel.rs` (cloudflared)
- `db/` — SQLite (rusqlite, WAL, egyetlen zárolt kapcsolat; `Db::with` tranzakció): `schema.sql`, `users.rs`, `games.rs`, `misc.rs`
- `server/` — `mod.rs` (útvonalak, Socket.IO bekötés, `InOrder`, `SYNC_EVENTS`, háttérfeladatok, kiszolgálás), `http.rs` (HTTP útvonalak, `public_routes`), `events.rs` (szobák, lobby, csatlakozás, türelmi idők), `play.rs` (lerakás, csere, passz, szavazás, chat, előnézet, tipp, bejelentés), `extras.rs` (napi feladvány, levelezős játék, megfigyelők, barátok, meghívók, admin események), `core.rs` (robotlépések, mentés, időzítők, push a saját körre), `net.rs` (kliens-IP, sütik), `room.rs` (`Room`), `state.rs` (`ServerState`)
- `admin/` — az admin panel: `mod.rs` (`action` / `record` napló, közös segédek), `session.rs`, `audit.rs`, `routes.rs` (őr, jogosultsági szintek, `route_table`), `api/*.rs` (végpontok: `users`, `game`, `dict`, `comm`, `system`), `users.rs`, `live.rs`, `games.rs`, `dict.rs`, `comm.rs`, `stats.rs`, `moderation.rs`, `security.rs`, `system.rs`, `mail.rs`, `update.rs`
- `engine_duel/` — a robot és egy külső motor párharca (`--features engine-duel`): `lexicon`, `bridge`, `sides`, `referee`, `leaves`, `runner` / `report` / `stats`, `crosscheck`, `spec`, `cpu`; lásd „Motor-összevetés”
- `bin/` — karbantartó eszközök: `bot_arena.rs` (fokozatok mérése: `ladder`, `match`, `adapt`), `build_attested.rs`, `word_review.rs` (`sample`, `apply`, `stats`), `engine_duel.rs`

## Játékszabályok és konstansok

**`src/engine/game.rs`**: `HAND_SIZE = 7`, `BONUS_ALL_TILES = 50`, `SCORELESS_TURNS_LIMIT = 6`, `CHALLENGE_TIMEOUT = 30` mp, `MAX_TIMEOUTS = 3` (levelezős), `ALLOWED_HINT_LIMITS = [0, 1, 3, 5, 10]`, `DEFAULT_HINT_LIMIT = 3`.
**`src/server/core.rs`**: `DISCONNECT_GRACE_PERIOD = 120` mp, `WAITING_OWNER_GRACE_PERIOD = 600` mp, `ALLOWED_TURN_TIME_LIMITS = [0, 60, 90, 120, 180, 300]` (a lobby űrlap alapértéke 120), `MIN_TIME_AFTER_WITHDRAW = 10` mp, `MAX_BOTS = 3`.
**`src/server/room.rs`**: `MAX_CHAT_MESSAGES = 100`, `MAX_SPECTATORS = 30` (a `max_spectators` beállítás felülbírálja). **`src/async_games.rs`**: `ALLOWED_TURN_HOURS = [24, 48, 72, 168]`, `DEFAULT_TURN_HOURS = 48`, `MAX_FRIENDS = 3`.
**`src/accounts/elo.rs`**: `INITIAL_RATING = 1200`, `K_PROVISIONAL = 32` (az első 10 értékelt játék), `K_ESTABLISHED = 20`, `MIN_RATED_GAMES_FOR_RANKING = 3`. **`src/accounts/achievements.rs`**: `BADGES` (10 + `daily_best`), `BIG_MOVE_SCORE = 100`, `LONG_WORD_TILES = 8`, `HIGH_GAME_SCORE = 300`.
**`src/robot/daily.rs`**: `MIN_BEST_SCORE = 30`, `MAX_BEST_SCORE = 250`, `PLIES = (8, 16)`. **`src/db/games.rs`**: `LEADERBOARD_METRICS` (`rating`, `wins`, `win_rate`, `avg_score`, `best_game`), `LEADERBOARD_MAX_LIMIT = 100`, `leaderboard_min_games()` (3 az `rating` / `win_rate` / `avg_score` mutatóknál).
**`src/config.rs`**: `SESSION_MAX_AGE_DAYS = 30`, `VERIFICATION_CODE_EXPIRY_MINUTES = 10`, `VERIFICATION_MAX_ATTEMPTS = 5`, `EMAIL_VERIFIED_WINDOW_MINUTES = 30`, `WORD_REJECT_THRESHOLD = 1`.

### Zsetonok, tábla, pontozás
- 100 zseton (2 joker), 38 betű; a kétjegyű betűk (SZ, CS, GY, LY, NY, TY, ZS) egy-egy zseton (`TILE_DISTRIBUTION`: betű, érték, darab). `tokenize_word`: szó → zsetonok (kevesebb zseton, döntetlennél több pont: KÉSZSÉG = K É S ZS É G).
- Premium mezők csak az **újonnan lerakott** zsetonokra számítanak (`board.rs::extract_word`); a középső csillag dupla szó; a joker értéke 0. A keresztszavak is képződnek és érvényesek kell legyenek. Mind a 7 zseton lerakása +50.
- **Kétjegyű betűk**: külön zsetonokból nem rakhatók ki (`tiles.forms_digraph`: S + Z, C + S, Z + S). `Board::find_split_digraph` a szótár-ellenőrzés **előtt** fut (megtámadásos módban és robotnál is), csak az **újonnan lerakott** zsetont érintő párokat nézi (a régi, megengedőbb szabállyal indult állások folytathatók). Hibaüzenet: `Kétjegyű betű (SZ) csak a saját zsetonjával rakható ki, S + Z külön zsetonnal nem.` (az angol fordítás a `server.patterns` között van). A robot a `validate_placement`-en át szűr, így a napi feladvány, a tipp és az elemzés is követi; a gyakorló módok (`practice::rack_words`, `check_rack_word`) és a kliens (`huntSubmit`, `huntType`, `formsDigraph`) ugyanígy.
- **Játék vége**: üres kéz + üres zsák, vagy **6 egymást követő pont nélküli kör** (passz, csere és elutasított lerakás is számít; a pontot érő lerakás nullázza: `Game.scoreless_turns`). A kézben maradtak levonódnak, a kiürítő megkapja az ellenfelek zsetonjainak értékét. **Döntetlen**: egyenlő pontnál mindenki győztes (`Game.winners`; `winner` csak ha egyetlen győztes van); a ranglistán / profilban mindegyiknek győzelem (`is_winner`).
- Csere csak legalább 7 zsetonos zsáknál.

### Megtámadás (kihívás) mód — szavazásos
A felületen **„Kihívás mód”** (kódban `challenge_mode`, „megtámadás”). Szótár-ellenőrzés **teljesen kikapcsolva** (lerakáskor és szavazásnál is); kizárólag a játékosok döntenek.
- **2 játékos**: a másik elfogad / elutasít (`accept_words` / `reject_words`); elutasításkor a betűk visszakerülnek a lerakóhoz; 30 mp után automatikus elfogadás.
- **3+ játékos**: 30 mp-es ablak, bárki megtámadhat (`reject_words`) vagy elfogadhat; megtámadáskor szavazási fázis (újabb 30 mp). A lerakó és a megtámadó nem szavaz; **≥ 50% elfogadás → marad** (döntetlen = elfogadva); a nem szavazó elfogadónak számít; nincs büntetés a megtámadónak.
- A robotok **nem szavaznak** (`Game::voter_ids`); a megtámadás (`Game::challenge_applies`) csak akkor él, ha van másik **emberi** játékos — egyébként a szótár dönt. Egyjátékos módban hatástalan.
- `withdraw_words`: a lerakó visszavonhatja a szavazásra váró lerakást, amíg senki sem szavazott; időlimitnél a hátralévő köridő folytatódik (legalább 10 mp).
- Kód: `engine/challenge.rs` (állapotgép), `engine/game.rs` (`accept_pending_by_player`, `reject_pending_by_player`), `server/play.rs`; az időzítő háttérfeladat a `server/core.rs`-ben.

### Szobák, újracsatlakozás, mentés
- Szoba: játék + tulajdonos (`owner_token` — stabil azonosító az instabil SID helyett) + beállítások (név, 2–4 fő, kihívás mód, privát, időlimit, robotok, tippek) + chat (max. 100 üzenet). 6 számjegyű `join_code`; nyilvános szoba a lobbyban, privát csak kóddal. Ha a tulajdonos kilép, az első online játékos kapja a tulajdonjogot.
- **Szobát** a `room_creation` beállítás szerint hozhat létre valaki (alapérték: `everyone`); a **felület** a vendégnek elrejti az Új szoba / Mentett játékok / Barátok / Levelezős füleket és a profilt.
- **Aktív játékban a kapcsolat megszakadása vagy a manuális kilépés `disconnected` állapotba teszi a játékost** (nem törlődik); a játék átugorja (`Game::next_turn`, `skip_disconnected_current`). **Token alapú újracsatlakozás** (`rejoin_room`): 120 mp. A várakozó szoba is türelmi időt kap (a tulajdonosnak 600 mp, másnak 120 mp): a szoba és a kód megmarad, a kliens a tokennel visszatér. Az explicit `leave_room` azonnal eltávolít. Minden lecsatlakozás sorszámot kap (`mark_disconnected` → `seq`, `disconnect_is_current`), így a régi időzítő nem zár le egy újabb türelmi időt.
- `reconnect_tokens {token: TokenInfo}`, `sid_to_token`, `disconnected_players {token: DisconnectedInfo}` a `ServerState`-ben.
- **Mentés**: indításkor a teljes roster mentődik; minden lerakás / csere után lépésnapló + tábla-pillanatkép (`MoveLog`, `save_game_to_db`); befejezéskor `finish_game` (statisztika, ELO, kitüntetések). Manuális mentés a tulajdonosnak; ha a tulajdonos véglegesen lecsatlakozik, automatikus mentés a feloszlatás előtt.
- **Lobby-first visszaállítás** (`restore_game`): a tulajdonos privát várakozó szobát hoz létre, `room.expected_players` az elvárt (emberi) nevekkel; a robotok indításkor automatikusan visszakerülnek. Aki nincs ott induláskor, `disconnected` állapottal kerül a játékba, és a kóddal menet közben csatlakozhat (csak a saját fiókjával; vendég hely: azonos névvel). Szavazás közben mentett játékban a függő lerakás betűi visszakerülnek a lerakó kezébe.
- **Megfigyelők**: `Room.spectators {sid: név}` + `ServerState.spectator_rooms`; a Socket.IO szobába belépnek, de a `player_rooms`-ban nem szerepelnek, így a játékos-eventek hatástalanok; `get_spectator_state` (kezek nélkül). A lecsatlakozott játékosnak a szerver nem küld állapotot.

### Levelezős (aszinkron) játék
- `Game.async_mode`, `turn_hours`, `turn_deadline`; a lecsatlakozottat nem ugorjuk át; `expire_turn()`: automatikus passz (`timeout` lépés), három egymás utáni után `resign`; `resign()`: a játék véget ér, a feladó nem lehet győztes (az ELO-ban is mindenki mögé kerül, `resigned` a `players_data`-ban).
- A játék otthona az adatbázis (`saved_games.is_async`): `persist_async` minden változás után ment, `restore_async_games()` induláskor újraépíti a szobákat (minden játékos lecsatlakozottként, `async_games::placeholder_id`), `open_async_game` szükség esetén betölti. A percenkénti háttérfeladat `expire_async_turns`-t hív.
- Szoba: privát, `room.is_async`; létrehozás: csak regisztrált, 1–3 **barát** (`friend_ids`), nincs robot / megtámadás / tipp. Értesítés: Web Push + `async_invited` / `async_your_turn`. A mentett játékok fület, a `restore_game`-et és az `abandon`-t nem érinti.

### Robotok (`src/robot/ai.rs`, `server/core.rs`)
- `Player.is_bot` / `difficulty` (`Difficulty::Level(1–10)` vagy `Auto`; a régi `easy` / `medium` / `hard` → 3 / 6 / 10 `parse_level`-lel). Azonosító: `bot-<szoba>-<n>`; `Game.add_bot()` csak indítás előtt, összesen max. 4 játékos. Robot sosem `disconnected`; `get_all_states` nem készít neki állapotot; ember nélkül a szoba megszűnik, a robotok nem lépnek (néző jelenlétében igen).
- Konstansok: `MIN_LEVEL = 1`, `MAX_LEVEL = 10`, `DEFAULT_LEVEL = 6`, `TIME_BUDGET = 1.5` mp (csak védőkorlát), `HINT_TIME_BUDGET = 3.5`, `ADAPT_WINDOW = 6`, `ADAPT_START_LEVEL = 5.0`, `INFLECT_MAX_STEM = 6`, `INFLECT_MAX_FORM = 12`, `LEVEL_STRENGTH = [4.3, 6.1, 7.4, 10.1, 12.7, 15.7, 18.4, 21.2, 24.6, 27.0]` (mért pont/kör).
- **Szókincs**: a `dict/hu_HU.dic` tőszavai (kisbetűs, magánhangzót tartalmazó, zsetonokra bontható, 2–15 betű, önmagában érvényes) + gyakori ragozott alakok (`AffixChecker::inflected_forms(..., risky = false)`: szótő + egy végződés a szótár szabályaival, a `INFLECT_ADDS` fehérlistáról; a kockázatos levezetések nélkül) — összesen kb. 290 000 alak. Rendezett lista + bináris keresés.
- `generate_moves`: Appel–Jacobson horgonykeresés vízszintesen és (átfordított rácson) függőlegesen; a keresztszavakat **egyetlen** `filter_valid` hívás ellenőrzi; a pontozás a játék `Board::validate_placement`-jével történik egy privát másolaton. A kiválasztott lépés minden szavát újra ellenőrzi; ha nincs lépés: csere (zsák ≥ 7) vagy passz.
- **10 fokozat** (`ai::profile(level)` → `Profile`, `parse_level`): az erőt a **lépéskiválasztás** szabja meg, nem a szókincs. 1–7: *célpontszámos* (átlag `mu`, lognormális szórás, a legközelebbi pontszámú lépés); 8–10: *értékeléses* (`equity` = pont + `leave_value`, végjátékban a kézben maradók levonva; Gauss-zaj 8 / 4 / 0). Az 1. fokozat 15% eséllyel „nem talál” lépést. Jokert minden fokozat használ.
- **„Igazodik hozzám”** (`auto`): `Game.recent_stats` az emberek utolsó 6 körének átlaga (passz / csere = 0) → `level_for_average` tört fokozat → `adaptive_level` a két szomszédos fokozatot keveri. A szerver lépésenként `Game.bot_level(bot)`-ot használ.
- Szerver: `bot_tier` (1–3 `easy`, 4–7 `medium`, 8–10 `hard`) adja a nevet (Robi / Rozi / Rudi · Rita / Ricsi / Réka · Rezső / Róbert / Rómeó) és a gondolkodási időt (`think_delay`: 1,5–3 · 1,2–2,6 · 1–2,2 mp; a `bot_think_multiplier` beállítás szorozza). `schedule_bot_turn(room_id)` minden ponton hívódik, ahol a kör továbbadódhat; `play_bot_turn` a keresést `spawn_blocking`-on futtatja, majd újraellenőriz; sikertelen lépés → passz. Tesztben a robotokat `set_setting(&server, "bot_think_multiplier", json!(5.0))` tartja vissza.
- Újramérés a paraméterek módosítása után: `target/release/bot_arena ladder -n 24 -j 4`, `bot_arena match 3 6`, `bot_arena adapt 5` — és a `LEVEL_STRENGTH` frissítése.

### Napi feladvány, gyakorló módok, szótár-építő
- **Napi feladvány** (`daily::generate_puzzle(dátum)`): a dátumból indított determinisztikus bot–bot játék néhány lépés után, a soron lévő robot keze a feladvány; a legjobb lépés a robot motorjával. A `daily_puzzles` táblában rögzül; a dátum magyar idő szerint. `Game.puzzle`: egyjátékos, egyetlen lerakás után vége, sosem mentődik az előzményekbe; szoba privát, `room.is_puzzle`. Vendég is játszhat, de csak regisztrált kerül a ranglistára (`daily_scores`: legjobb pont → kevesebb próbálkozás → korábbi idő). A `reveal_daily` lezárja a ranglistás részvételt. `daily_best` kitüntetés.
- **Gyakorló módok** (`words/practice.rs`, állapotmentes; végpontok közös őre `practice_guard` (`http.rs`): `practice` forgalomkorlát + szótár elérhetősége, hiányában 503):
  - *Szókvíz* (`make_quiz`; `QUESTION_COUNT = 10`, `MAX_QUESTIONS = 30`): fele érvényes szó (tőszavak + ~30% ragozott alak), fele hihető félreírás; módok `mixed` / `2` / `3` / `tricky` (hosszú–rövid csapdák: `mutate_length`). A választ `POST /api/practice/answer` a játék szótárával értékeli.
  - *Betűvadász / Bingó-edző* (`make_rack`, `rack_words`, `check_rack_word`): 7 zseton (joker nélkül, 2–4 magánhangzó), a robot szókincsében prefix-vágással keresett kirakható szavak; a pont a zsetonértékek összege, mind a hét +`BINGO_BONUS` (50). A listán nem szereplő szót a `rack-word` végpont bírálja el (`too_short` / `invalid_chars` / `not_in_rack` / `split_digraph` / `not_a_word`).
  - *Szólisták* (`short_words(2|3)`, gyorsítótárazva; `practice::warm_up()` indításkor), *Hibáim* (az eszközön, `PracticeStore`), a tokenizáló mása a kliensben (`tokenizeWord`; a `tests/frontend_client.rs` több ezer szón összeveti).
- **Szótár-építő** (`words/word_review.rs`, `word_reviews` tábla): véletlen szavak (a robot szókincséből, tőszó : ragozott = 7 : 3, csak a szótár által most elfogadott, még át nem nézett); szavazat felhasználónként: 1 rendes szó, 0 nem szó. Kizárás, ha `nem szó − rendes szó ≥ WORD_REJECT_THRESHOLD`; pontos szóalakra szól, azonnal él (`dictionary::mark_voted_rejected`), indításkor az adatbázisból töltődik vissza (`word_review::refresh`). Csak regisztrált szavazhat; `word_review` forgalomkorlát 240/perc.
  - **Tartós lista**: `dict/hu_rejected.txt` (soronként egy kisbetűs szó, `#` megjegyzés; ~3200 szó) — a szótárral együtt töltődik, a robot szókincséből is kimarad; karbantartása a `word_review` eszközzel: `sample 5000 --seed N --out sample.txt` → az ítéletek egy fájlba → `apply rejected.txt` (csak a szótár által elfogadott szó kerülhet rá; `--dry-run`; `--exclude` a korábbi minták fájljaival).
  - Átnézési tapasztalat: a Claude Haiku önmagában **nem megbízható bíráló** (a kalibráló mintán a recall 85% volt, de a pontosság csak ~15%: valódi szavakat is megjelölt), csak recall-szűrőként használható; az ítéletet ember (vagy erősebb modell) hozza. A bizonytalan szót meg kell tartani.
  - `dictionary::rejected_version()` nő minden változásnál (a számolt gyorsítótárak, pl. `short_words`, ebből látják, hogy elavultak); a szavazatból kizártak **folyamatszintű állapot** (a tesztek soros zár mögött / külön binárisban futnak, és takarítanak: `dictionary::set_voted_rejected`).

### Szótár és furcsa szavak szűrése
- A szótár a `dict/hu_HU.{aff,dic}` fájlokból töltődik be rendszercsomag nélkül (`Szótár: beágyazott hu_HU (… szótő, … elutasított szó)` az induláskor; ha nem tölthető be: `FIGYELEM: A szótár nem tölthető be`, a Szótár-eszköz 503-at ad). A szavakat kisbetűvel keresi (tulajdonnevek nem érvényesek), a **Hunspell összetételi szabályait nem használja** (PAGONYAGY nem érvényes), a magánhangzó nélküli tételek (KG, DB, TV, betűnevek) nem érvényesek, az indulatszavak (BRR, HM, PSZT) igen. Támogatott: szótő, előtag, legfeljebb két toldalék, folytatási osztályok (AF aliasok), `NEEDAFFIX` / `ONLYINCOMPOUND` / `FORBIDDENWORD`.
- **Morfológiai címkék** (`AM` aliasok: `po:` szófaj, `is:` / `ds:` toldalékfajta):
  - csak kötőjellel toldalékolható szócikkek (`al:szó-`, főnév: rövidítések, mértékegységek, idegen írásmód — VU, UV, COS, KCAL, SZJA, MADAME) nem érvényesek (`KIND_FOREIGN`);
  - **kockázatos levezetések** (`rule_risk`, `is_risky`) csak akkor érvényesek, ha az alak a `dict/hu_attested.txt` listán szerepel: melléknév (vagy -s/-i/-bb/-nyi… képzős melléknév, ill. -s foglalkozásnév) + birtokos személyjel (KEDVESEM, DRÁGÁM igen — TAROM, FALIJA nem); -ék családi többes (SZOMSZÉDÉK igen — ÉJÉK nem); -ul/-ül **főnéven** (FELESÉGÜL igen — BLÖKIÜL nem; melléknéven ROSSZUL, VÉLETLENÜL mindig érvényes); -né képző (KIRÁLYNÉ igen — ALMÁNÉ nem). A -ó/-ő, -andó/-endő melléknévi igenév főnévként viselkedik (TANULÓM rendben).
  - `hu_attested.txt`: a `build_attested` eszköz állítja elő a FrequencyWords (OpenSubtitles 2018, magyar, **CC BY-SA 4.0**) listából: csak kockázatos levezetéssel érvényes, legalább 2-szer előforduló alakok, az elírások (egy ≥10× gyakoribb érvényes szó ékezet nélkül / egy kimaradt vagy megkettőzött betűvel) és a tulajdonnévből képzettek nélkül. Hiányában a kockázatos levezetések mind érvényesek (a hunspell viselkedése).
  - A robot a kockázatos alakokat akkor sem rakja le, ha a listán szerepelnek. Ellenőrzés: `AffixChecker::needs_attestation(szó)`.

### Értékszám (ELO), kitüntetések, elemzés, megosztás
- `elo::rating_changes`: minden játékospárra külön (a nagyobb pont nyer), a változás az ellenfelek számával osztva; csak befejezett, **robot nélküli** játék, legalább két regisztrált játékossal; a változás a `game_players.rating_before/after`-ben is rögzül; a ranglistára 3 értékelt játék után lehet felkerülni. A ranglistát `get_leaderboard()` a `game_players ⨝ saved_games(finished, has_bots = 0) ⨝ users` összesítéséből számolja (nem a `users` számlálóiból), így a robotos játékok nem számítanak; a rendezés SQL-része rögzített, a metrika fehérlistás.
- **Kitüntetések**: `achievements::evaluate_game` a lépésnaplóból (bingó, 100+ pontos lépés, 8+ zsetonos szó, joker) és a végeredményből (300+ pontos játék, győzelem 8–10. fokozatú robot ellen); `cumulative_badges` az összesítőkből (első játék / győzelem, 10 győzelem, 25 játék); `daily_best` a napi feladványból. Rögzítés: `grant_achievements` (kulcsonként egyszer), a kliens az `achievements_earned` eseményből értesül.
- **Elemzés**: a lépésnapló minden lépésnél tartalmazza a kezet a lépés előtt (`rack`); `analysis::analyze_game` az előző lépés tábla-pillanatképén megkeresi a legjobb lépést, és összeveti a játszottal. Háttérben fut (`run_analysis`, `http.rs`), a `game_analysis` táblában gyorsítótárazott (`ANALYSIS_VERSION`). Régi játékoknál nincs kéz → `unavailable`.
- **Megosztás**: `saved_games.share_token`; a `/?replay=TOKEN` link bejelentkezés nélkül megnyitja a visszajátszást. **Push**: VAPID kulcspár (`VAPID_PRIVATE_KEY`, ennek híján az első induláskor generálódik és az `app_settings`-ben marad), saját RFC 8291 aes128gcm + ES256 megvalósítás; a 404 / 410 válaszú feliratkozás törlődik; `maybe_push_turn` értesít, ha a soron lévő regisztrált ember távol van (körönként egyszer; robotnak / vendégnek / napi feladványban soha).

## Szerver

- **Protokoll**: [docs/PROTOCOL.md](docs/PROTOCOL.md) — az összes HTTP útvonal és Socket.IO esemény, a forgalomkorlátok és a `game_state` mezői. Új útvonal: `public_routes()` (`http.rs`); új esemény: lásd az Alapszabályok 4. pontját.
- **Azonosság**: a kapcsolat a `set_name`-mel mutatkozik be; regisztráltnál az `auth_token` kötelező (`/api/auth/socket-token`, 5 perc). `session_token` süti: `HttpOnly`, `SameSite=Lax`, `Secure`, 30 nap. A kliens IP-jét a proxy fejlécekből (`CF-Connecting-IP`, `X-Forwarded-For`) csak loopback proxy esetén fogadja el (`net.rs`).
- **Regisztráció**: e-mail → kód (6 számjegy, 10 perc, ≤ 5 próba) → jelszó + név. SMTP nélkül a válasz `dev_code`-ot tartalmaz, amit a kliens magától kitölt (admin címnél soha: az csak a konzolon látszik). Az admin címek védettek (nem foglalhatók le, nem tilthatók ki / törölhetők / némíthatók).
- **Jelszó**: PBKDF2-SHA256, 260 000 iteráció, véletlen só, `pbkdf2:sha256:260000$salt$hash` (a régi hashek érvényesek). **CORS**: minden origin engedélyezett (a tunnelhez).
- **Rate limit**: Socket.IO SID szerint, HTTP IP szerint; az admin panelen futásidőben felülírható (`rate_limits_http` / `rate_limits_socket`).
- **Tunnel** (`services/tunnel.rs`): a `cloudflared tunnel --url http://localhost:PORT` gyermekfolyamat; a publikus cím kiolvasása a kimenetből (`*.trycloudflare.com`); a `--no-tunnel` kikapcsolja.
- **Háttérfeladatok** (`server::spawn_background_tasks`): percenként levelezős határidők, 5 mp-enként admin számlálók (ha nézi valaki), óránként napi mentés + chat napló takarítás.
- **Napló**: a szerver konzolra írt üzeneteit az admin panel naplónézete gyűjti (`admin::system::install_log_capture`).

## Adatbázis

A séma: `src/db/schema.sql` (27 tábla). Fő táblák: `users` (id, email, email_lower, display_name, password_hash, created_at, statisztika, `rating`, `rated_games`, `reconnect_token`, kitiltás / némítás / törlés oszlopok…), `sessions` (token, lejárat, `ip`, `user_agent`, `admin_seen_at`, `sudo_until`), `verification_codes`, `verified_emails`, `saved_games` (`state_json`, `status` = `active` / `finished` / `abandoned` / `voided`, `challenge_mode`, `owner_name`, `owner_token`, `has_bots`, `is_async`, `share_token`), `game_players` (`final_score`, `is_winner`, `rating_before/after`), `game_moves` (`move_number`, `action_type`, `details_json`, `board_snapshot_json`), `game_analysis`, `achievements`, `friendships`, `push_subscriptions`, `daily_puzzles`, `daily_scores`, `word_reviews`, `usage_counters`, `login_events`, `ip_bans`, és az admin táblák: `admin_audit` (BEFORE UPDATE / DELETE triggerek tiltják a módosítást és a törlést; nincs idegen kulcs, így a felhasználó törlése sem törli), `user_admin_notes`, `rating_adjustments` (kézi értékszám-módosítás; az újraszámolás figyelembe veszi), `word_overrides`, `word_additions`, `announcements`, `reports`, `chat_log`, `banned_words`, `app_settings`.
- Kitiltás: `validate_session` kitiltott / törölt felhasználóra `None` (a session azonnal érvénytelen), a socket-kapcsolatok bezárulnak (`account_banned`), a futó játékban lecsatlakozottá válik; belépéskor az ok és a lejárat látszik. IP tiltás: a HTTP őrben és a socket `connect`-nél (30 mp-es gyorsítótár). Némítás: a `send_chat` csendben eldobja. Karbantartási mód: új szoba / levelezős játék / napi próbálkozás letiltva (az admin kivétel), bannerrel.
- Anonimizálás: név → „Törölt felhasználó #id”, e-mail és jelszó törölve, a játékok és az értékszám-előzmények megmaradnak; teljes törlés külön opció. Az ELO újraszámolás (`admin::games::compute_ratings`) a befejezett, robot nélküli játékokat `updated_at, id` szerint játssza újra, és a lépésenkénti számolással azonos eredményt ad (teszt). Érvénytelenített játék (`voided`): nem számít az értékszámba / statisztikába / ranglistába.
- Egy adatbázis-hozzáférés: `Db::with(|tx| …)` (tranzakció: commit a végén, hiba esetén visszagörgetés), `Db::raw()` (VACUUM, mentés).

## Kliens (`web/`)

- `web/templates/index.html` — egyoldalas UI: auth (3 fül), lobby (Kezdőlap / Új szoba / Mentett játékok / Barátok / Levelezős / Gyakorlás / Ranglista), várakozó szoba, játék, profil, visszajátszás (+ elemzés); közös SVG ikon-sprite; minden képernyőn egységes felső sáv (`.app-topbar`). `admin.html` — az admin oldal; `sw.js` — service worker sablon.
- `web/static/app.js` — a kliens logikája; modulok (globális `const`-ok): `AppState`, `Auth`, `Lobby`, `WaitingRoom`, `GameBoard`, `HandLayout`, `Preview`, `Tracker`, `Hint`, `Shortcuts`, `BlankDialog`, `TurnTimerUI`, `ChallengeUI`, `Chat`, `BoardZoom`, `GameOver`, `Reconnection`, `ExitGame`, `Daily`, `AsyncGames`, `PracticeStore`, `WordBuilder`, `Practice`, `LobbyNav`, `Push`, `Badges`, `AdminEntry`, `Profile`, `Replay` (`load`, `share`, elemzés), `SoundManager`, `SoundSettings`, `Friends`, `Dialogs`, `Leaderboard`, `DictionaryTool`, `Spectate`, `PWA`, `Announcements`, `Report`; közös segédek: `makeEl`, `makeAvatar`, `tokenizeWord`, `fillWordTiles`, `formsDigraph`. A Socket.IO kliens (`socket`) a cdnjs-ről töltődik; ha nem érhető el, egy „offline” tartalék-objektum és egy sáv jelzi.
- `i18n.js` + `i18n-data.js` — `t()`, `tServer()`, `I18N.setLang()`; `{hu: {…}, en: {…, server: {exact, patterns}}}`, szigorú JSON; kulcsok `névtér.kulcs`; az `_one` végű kulcs az angol egyes számhoz (`params.n === 1`); HTML-ben `data-i18n` / `-placeholder` / `-title` / `-aria`; nyelvváltáskor `langchange` esemény. Új nyelv: a blokk és az `i18n.js` `SUPPORTED` listája.
- `style.css` — Apple HIG ihletésű design rendszer (tokenek: `--bg-*`, `--text-*`, `--accent*`, `--border-*`, `--color-fill*`, `--radius-*`, `--space-*`, `--z-*`), sötét / világos téma (`prefers-color-scheme`, a `<head>` inline szkriptje a megjelenítés előtt állítja a `data-theme`-et; `localStorage('scrabble-theme')`), reszponzív layout.
- **Elrendezések**: asztali / fekvő tablet (két oszlop: oldalpanel + tábla + betűtartó; `HandLayout`: `right` alapértelmezés / `bottom`, `localStorage('scrabble-hand-position')`, `<html data-hand>`), **álló telefon / tablet** (tábla fent, alul fix, görgethető panel; a `--board-size` a szélességből és a minimális panelből számolódik, a panel soha nem takarhatja el a táblát), **fekvő telefon** (kompakt két oszlop). Álló nézetben és fekvő érintőképernyős telefonon nincs felső sáv: a szobanév és a gombok a `.game-nav` menüsorban vannak. `env(safe-area-inset-*)`, `100dvh`, `prefers-reduced-motion` tiszteletben tartva. A tábla cellái `container-type: inline-size` + `cqw` egységekkel méreteződnek.
- **Komponens-konvenciók**: gombok `.secondary` / `.tinted` / `.danger` / `.link-btn` / `.small-btn` (a `--btn-bg` / `--btn-fg` változókat állítják; a `:hover` csak `(hover: hover)` eszközön él); párbeszédek `.dialog` + `.dialog-content`, telefonon `.dialog-sheet` (alsó lap); kapcsolók `.toggle-switch`; listasorok közös stílus, üres állapot `.empty-state > .empty-msg`.
- **Animációk**: `GameBoard.renderBoard` különbséget számol az előző rajzoláshoz képest (`tile-pop` / `tile-drop` / `last-move` / `.score-pop` / `turn-pulse` / `.my-turn`); húzás közben `is-dragging`, a foglalt mezők csíkozva (`dragenter` **és** `dragover` elfogadása kell).
- **PWA**: `/sw.js` (install: váz előtöltése; navigáció hálózat-először, offline: gyorsítótárazott `/`, végső esetben `offline.html`; `/socket.io/` és `/api/` soha; statikus fájlok és a CDN stale-while-revalidate; az `/admin` soha). `asset_version()` (`http.rs`) a kliens fájlok mtime-ja: az `index.html` `?v=` paramétere és a SW `VERSION`-je. Telepítés: `beforeinstallprompt` (iOS-en kézi útmutató).
- **Hang**: `SoundManager` (Web Audio API, külső fájl nélkül), kategóriák: `tile_place`, `vote`, `challenge_result`, `your_turn`, `chat`, `game_events`; `localStorage('scrabble-sound')`: `{volume, enabled: {…}}`.
- **Kliens-oldali tárolás**: `scrabble-lang`, `scrabble-theme`, `scrabble-sound`, `scrabble-hand-position`, `scrabble-practice` (gyakorlás: napok, számlálók, betűvadász legjobb pont, bingó-sorozat, `missed`).
- **Linkek**: `/?join=KÓD` (csatlakozás), `/?spectate=KÓD` (megfigyelés), `/?replay=TOKEN` (megosztott visszajátszás).

## Admin panel

Specifikáció: [docs/ADMIN_PANEL.md](docs/ADMIN_PANEL.md). Állapot: kész — felhasználók, élő szobák, játékarchívum, levelezős játékok, ranglista / értékszám, szótár, napi feladvány, moderáció, kommunikáció, statisztika, biztonság, rendszer, beállítások, admin napló; globális kereső (Ctrl+K), élő frissítés Socket.IO-n, magyar / angol felület, telefonon is használható.

- **Ki az admin**: az `ADMIN_EMAILS` (vesszővel elválasztott, kisbetűsítve hasonlított címek; üresen a panel ki van kapcsolva) — nem adatbázis-oszlop és nem a megjelenítési névhez kötött (`is_admin_email`). Vendég soha nem admin. **Csak olyan címet adj meg, amelynek a fiókja a tiéd**: az admin címnél az `request-code` SMTP nélkül sem adja vissza a kódot (a szerver konzolján olvasható).
- **Őr** (`src/admin/routes.rs`, axum middleware az útvonal-illesztés előtt): 1. azonosítás (`session_token` → `validate_session` → admin?), különben 404; 2. IP-engedélylista (kívülről 404); 3. forgalomkorlát (`admin`); 4. CSRF: nem biztonságos metódusnál kötelező az `X-Admin-Request: 1` fejléc **és** az `Origin` (vagy `Referer`) hostja egyezzen a kérés hostjával (különben 403) — fordított proxynál az eredeti `Host` fejlécet át kell adni; 5. tétlenség: az utolsó admin-kérés óta eltelt idő > `ADMIN_SESSION_IDLE_MINUTES` → 401 `{reauth: true}` (kivétel: az oldal, az assetek, `GET /api/admin/session`, `POST /api/admin/reauth`). A nem admin számára a rossz metódus, a nem létező alútvonal és a `//admin` is azonos 404; az admin válaszok biztonsági fejlécei (`no-store`, CSP, `X-Frame-Options`) csak az adminnak mennek.
- **Sudo mód**: `POST /api/admin/sudo {password}` → `ADMIN_SUDO_MINUTES` percre (`sessions.sudo_until`, a sessionhöz kötve); `DELETE` lezárja. Szintek az útvonalaknál: `Level::Danger` (`post_danger`, `patch_danger`, `delete_danger`: `admin_danger` forgalomkorlát) és `Level::Sudo` (`post_sudo`, `get_sudo`, `patch_sudo`, `delete_sudo`: 401 `{sudo_required: true}`); a kliens (`Api.call`) felismeri, jelszót kér, és megismétli a kérést. A rossz jelszó a `login` forgalomkorlátjába számít és naplózódik.
- **Napló** (`admin_audit`; id, admin_user_id, action, target_type, target_id, details_json, ip, user_agent, created_at): nem törölhető, nem szerkeszthető. **Módosító műveletek** mindig `admin::action(db, ctx, "user.ban", Some("user"), Some(id), reason, details, require_reason, |act| { … })` alakban: az indoklás kötelező (`MIN_REASON_LEN = 3`, `MAX_REASON_LEN = 500`; csak ott hagyható el, ahol a tartalom maga az indoklás: közlemény, push, e-mail — `require_reason = false`), a naplósor a művelettel **egy tranzakcióban** íródik (hiba esetén minden visszagördül); a memóriabeli mellékhatásokat (socket bontás, szoba) a blokk UTÁN kell elvégezni; a memóriabeli beavatkozásoknál (szoba) a naplósor a művelet ELŐTT íródik. Személyes adat megtekintése: `admin::record(tx, ctx, "view.…", …)` indoklás nélkül (`view.user`, `view.game`, `view.racks`, `view.chat`, `view.report`, `view.codes`, `view.logins_export`, `view.audit_export`). Az `AdminContext { admin_user_id, ip, user_agent }` a kérésből jön.
- **Végpontok** (`/api/admin`, JSON; a teljes lista a `src/admin/routes.rs` `route_table`-jében és az `api/*.rs` fájlokban): áttekintés (`overview`, `charts`, `search`), felhasználók (`users…`), élő szobák (`rooms…`, `POST /rooms/{key}/action`), játékok (`games…`), levelezős (`async…`), ranglista (`ratings…`), szótár (`dictionary…`), napi feladvány (`daily…`), moderáció (`reports`, `moderation/…`), kommunikáció (`announcements`, `maintenance`, `push`, `email`), statisztika (`stats`), biztonság (`security/…`), rendszer (`system…`, `system/mail`, `system/update`, `system/restart`), `settings`, `audit`. **Leállítás gomb nincs** (karbantartási mód helyette); az **újraindítás** a frissítés (és a Rendszer oldal gombja) része: sudo + indoklás, a folyamat önmagára cserélődik (`exec`, Unixon).
- **Levelező szerver (SMTP)**: kétféle forrás — az admin panelen mentett felülbírálat (`app_settings`, `mail.smtp`: `host`, `port`, `security` = `starttls` \| `ssl` \| `none`, `username`, `password`, `from_address`, `from_name`, `verify_tls`) erősebb, ennek híján a környezeti `SMTP_*` (mindig STARTTLS). A `Mailer::current()` hívási időben számol, így a mentés újraindítás nélkül hat. A jelszó az adatbázisban van, de **sosem kerül ki** (válaszban csak `password_set`, naplóban `password_changed`); üresen hagyva a mentett marad, **de csak ugyanahhoz a kiszolgálóhoz és felhasználónévhez**. `POST /system/mail/check` (sudo): a még nem mentett űrlapot próbálja ki; teszt levél csak a saját admin címre; a válasz biztonságos hibakód (`dns`, `timeout`, `refused`, `connect`, `tls`, `certificate`, `auth`, `unsupported`, `sender`, `recipient`, `disconnected`, `protocol`, `error`). A tanúsítvány alapból ellenőrzött (`verify_tls` kikapcsolható önaláírthoz); a fejlécek sortörés ellen védettek.
- **Frissítés GitHubról** (`admin/update.rs`): a program mappája git tár; állapot (ág, commit, távoli cím hozzáférési adatok nélkül), ellenőrzés (`git fetch --prune origin`), frissítés (a legfrissebb vagy egy megadott ágra), újraindítás. Korlátok: csak az `origin`; az ágnév szigorúan ellenőrzött (`^[A-Za-z0-9][A-Za-z0-9._/-]{0,99}$`, nincs `..`) és a lekért ágak között kell lennie; csak **fast-forward** (`git merge --ff-only`) tiszta munkafán (a követett fájlok módosítása blokkol; a nem követettek — adatbázis, `.env` — nem); nincs `reset --hard`; egyszerre egy frissítés. Sudo + indoklás; a naplósor a művelet **előtt** íródik (`system.update`), az eredmény utána (`system.update_done` / `_failed`). Ha a Rust forrás (vagy `Cargo.*`) változott és a „Függőségek telepítése” be van kapcsolva, lefut a `cargo build --release`; hibánál (422) visszaáll az előző ágra és commitra. A `restart_needed` jelzi, ha a lemezen újabb kód van a futónál (`running_commit`). Tesztben az `exec` kikapcsolt (`SCRABBLE_TEST_NO_EXEC`).
- **Beállítások** (`settings.rs`, `app_settings`; olvasás `settings.get*`, gyorsítótárazott; módosítás után `App::apply_runtime_settings()`): `registration_open`, `guest_allowed`, `room_creation` (`everyone` / `registered` / `none`), `max_spectators`, `default_hint_limit`, `bots_enabled`, `max_bots`, `default_bot_level`, `bot_think_multiplier`, `feature_daily` / `feature_async` / `feature_practice` / `feature_word_review`, `word_reject_threshold`, `grace_disconnect`, `grace_waiting_owner`, `chat_max_length`, `chat_rate_count` / `chat_rate_window`, `banned_word_action` (`mask` / `drop`), `chat_log_enabled` / `chat_log_days`, `backup_daily` / `backup_keep`, `rate_limits_http` / `rate_limits_socket`, rejtetten `maintenance`. Új funkciókapcsoló: `definitions` (`settings.rs`) + `SETTING_TEXT` az `admin-views-c.js`-ben + fordítás.
- **Kliens** (`web/admin/`): `admin.js` (mag: `Api.request` — minden kérésen az `X-Admin-Request` fejléc —, munkamenet, zárolás, sudo), `admin-ui.js` (komponensek: `mountList`, `Act.open`, `Chart`, `renderBoard`, `loadInto`, `UI.tabs`, `UI.kv`, `UI.cardGrid`), `admin-views-a/b/c.js` (menüpontok: `registerSection({id, order, icon, labelKey, render})`), `admin-main.js` (`Router`: `#menüpont/azonosító?szűrők`; `Live`: Socket.IO `admin` szoba; Ctrl+K kereső), `admin.css`, `admin-i18n.js`, `admin-boot.js`. Szabályok: kizárólag DOM API (`h()`), **minden szöveg** `t('admin.kulcs')` (a kulcsok szó szerint a kódban, összefűzés nélkül; hu **és** en; a szerver magyar üzenetei az `en.server.exact`-ban), a hálózati hívás egyetlen helyen, a kulcsok és az API útvonalak létét a tesztek ellenőrzik. A kliens szándékosan **nem** a nyilvános `web/static/` mappában van, csak az őrzött `/admin/assets/…` útvonalon érhető el; az „Admin” gombot az `AdminEntry` modul futásidőben szúrja be, ha a `/api/auth/me` `is_admin`-t jelez.
- **Elrendezési konvenciók** (2K-n ellenőrizve, hu / en, világos / sötét, 390–2560 px): a szűrőűrlap egy sorba törő flex (azonos szélességű mezők, a gombok jobbra); a fülön belüli lista (`mountList`, `keep: ['tab']`) nem kap saját címsort; a táblázat rövid cellái (≤ 28 karakter) `cell-short`, nem törnek; `UI.kv` sorai széles helyen több oszlopba rendeződnek (a hosszú érték `{wide: true}`); a `Chart` az SVG-t a tároló tényleges szélességére rajzolja (`ResizeObserver`); a tartalom (`.admin-main`) középre igazított, nagy képernyőn szélesebb (`max-width`: 1400 / 1560 / 1840 px), a tábla rácsa rögzített 15×15 sáv.

## Motor-összevetés (robot ↔ külső Scrabble motor)

Részletek, módszertan, eredmények: [docs/ENGINE_DUEL.md](docs/ENGINE_DUEL.md).
- A külső motor a crates.io `scrabble` 0.1.0 (Pranav Gundu, MIT) vendorolt másolata (`third_party/pg-scrabble`, csomag `pg-scrabble`), 64 bites betűmaszkokkal (a magyar ábécé 38 betűje nem fér az eredeti 30-ba); a változtatások: `third_party/pg-scrabble/PATCHES.md`. A csomag a munkaterület tagja, de a szerver alap-fordítását nem érinti (opcionális függőség: `engine-duel`).
- Fordítás és futtatás: `cargo build --release --features engine-duel --bin engine_duel`; `engine_duel duel bot:10 eng:leaves:sim+eg --leaves docs/engine_duel/leaves-hu.json --pairs 400 --first-pair 100001 -j 4`.
- A játékvezető a saját `Game`-ünk; a motor csak lépést javasol. Közös szókincs: a robot szókincse minden jogos zsetonbontásban; a robot a párharcban a szókincsére szűrt (`bot:10`), az éles működés külön (`bot:10:full`). Tükrözött párok; pármagok: fejlesztői < 100 000, a végleges mérés 100 001-től; a tanító magok ≥ 10⁹ (a kód ellenőrzi).
- A motor magyar maradék-értékét önjátékból tanítjuk (`engine_duel train` / `solve`, a robot kódját nem használja); a végleges táblázat `docs/engine_duel/leaves-hu.json`. Új oldal: `src/engine_duel/spec.rs` (leírás-nyelv), `sides.rs`; a `tests/engine_duel.rs` számon kéri a szabályazonosságot és az egyező lépéshalmazt. Az `.perf/duel/` (gitignorált) a naplókat tartalmazza.

## Tesztek

Az integrációs tesztek **valódi szervert** indítanak véletlen porton, ideiglenes adatbázissal (`TestServer`), valódi HTTP (`ureq`) és Socket.IO (`tokio-tungstenite`) kliensekkel; az e-mail és a push valódi, helyi „szolgáltatásra” megy (hamis SMTP kiszolgáló, visszafejtéssel ellenőrzött push). A node-ot / Playwrightot igénylő tesztek ezek hiányában kimaradnak.

| Fájl | Tesztek | Lefedettség |
|---|---|---|
| `tests/game_logic.rs` | 43 | TileBag, Board, Player, Game, kihívás-szavazás, kör időlimit; a kódátvizsgálás hibáinak regressziós tesztjei (zsák, dupla cella, passz-végjáték, döntetlen) |
| `tests/db_tests.rs` | 40 | DB, user CRUD, jelszó-hash (a Python werkzeug-hashekkel is), verifikációs kódok, session, játék mentés / lépésnapló, ranglista, ELO, kitüntetések tárolása |
| `tests/http_auth.rs` | 23 | auth útvonalak, cookie flow, e-mail megerősítés, rate limit, socket-token |
| `tests/http_public.rs` | 13 | ranglista (DB + útvonal, robotos játékok kizárása), szótár API, PWA végpontok, statikus fájlok |
| `tests/socket_rooms.rs` | 38 | lobby, szobák, privát szobák, kihívás-szavazás, chat, owner kilépés, kör időlimit, mentés / visszaállítás, sorrendhelyes feldolgozás |
| `tests/socket_reconnect.rs` | 27 | újracsatlakozás (türelmi idő), lecsatlakozott játékos átugrása, várakozó szoba türelmi ideje, késői csatlakozás |
| `tests/socket_spectator.rs` | 16 | megfigyelő mód, élő játékok, szoba életciklus |
| `tests/socket_replay.rs` | 27 | visszajátszás (perzisztencia, megosztási token, nyilvános replay, jogosultság), játékelemzés (gyorsítótár, háttérszámítás, API), időzítők |
| `tests/socket_friends.rs` | 22 | barátkezelés, kérések, felhasználókeresés, szobameghívó, online státusz |
| `tests/socket_bots.rs` | 22 | robotok a szerveren (lépés, ütemezés, tipp, előnézet), „igazodik hozzám”, `last_action_info`, lépéstörténet |
| `tests/bots_model.rs` | 11 | robotok a játékmodellben (szavazás, mentés), `recent_stats` / `bot_level`, a robot motorja (fokozatok, ragozott alakok, csere / passz) |
| `tests/async_games.rs` | 55 | levelezős játék: játéklogika (határidő, lejárat, feladás), szerver, mentés / visszaállítás, lista, útvonalak |
| `tests/daily_puzzle.rs` | 46 | napi feladvány: előállítás (determinizmus), játék, ranglista, socket, HTTP |
| `tests/digraph_tiles.rs` | 18 | kétjegyű betűk: `forms_digraph`, tábla (új / régi zseton, keresztszó, joker, régi állások), `Game`, robot, socket |
| `tests/practice.rs` | 43 | szókvíz (csapda mód), rövid szavak, válasz-ellenőrzés, betűvadász / bingó, API |
| `tests/word_review.rs` | 39 | szótár-építő: elutasított szavak (lista + szavazatok), szavazás / küszöb / visszavonás, mintavétel, API |
| `tests/push.rs` | 37 | VAPID, feliratkozások, titkosított küldés (valódi helyi „push szolgáltatás”, visszafejtéssel), API, „Te jössz!” |
| `tests/push_env.rs` | 4 | Web Push a környezeti VAPID kulccsal (külön bináris) |
| `tests/email_service.rs` | 16 | e-mail küldés valódi (helyi) SMTP kiszolgálóra: fejlécek, kódolás, injekció elleni védelem, titkosítási módok, hibakódok, kapcsolat-próba |
| `tests/admin_access.rs` | 84 | admin hozzáférés: nem admin → mindenhol azonos 404 (az **összes** útvonalra az útvonaltérképből), `is_admin` csak az adminnál, tétlenség / újraigazolás, CSRF, sudo, IP-lista, forgalomkorlát, Socket.IO, service worker, CSP |
| `tests/admin_audit.rs` | 39 | admin napló: csak hozzáfűzhető (trigger), kötelező indoklás, egy tranzakció (visszagördülés), szűrők / lapozás, CSV (képlet-injekció elleni védelem) |
| `tests/admin_users.rs` | 58 | felhasználók: lista / szűrők, részletek, név / e-mail / jelszó, kitiltás, némítás, szótár-építő tiltás, értékszám, kitüntetések, export, törlés / anonimizálás, jegyzetek, védett admin fiókok |
| `tests/admin_rooms.rs` | 51 | élő szobák: lista / szűrők / elakadás-érzékelés, részletek, kezek naplózása, beavatkozások, zsetonmegmaradás, élő admin események |
| `tests/admin_games.rs` | 43 | játékarchívum, érvénytelenítés / visszavonás, ELO újraszámolás (= a lépésenkénti számolás), gyanús minták, levelezős beavatkozások, ranglista |
| `tests/admin_dictionary.rs` | 39 | szó-vizsgáló, kizárt lista (írás, visszagördülés, diff), felülbírálatok, saját szavak, szavazatok törlése, bírálók, küszöb, gyorsítótárak |
| `tests/admin_comm.rs` | 43 | közlemények, karbantartási mód, push, e-mail (tömeges: megerősítés, korlát), beállítások (érvényesítés, naplózás, futásidejű hatás), statisztika, napi feladvány |
| `tests/admin_system.rs` | 57 | rendszer (konfiguráció titkok nélkül, mentés, takarítás, naplók, folyamatok), biztonság (belépési napló, IP tiltás, forgalomkorlát, kódok, munkamenetek), áttekintés, statisztika |
| `tests/admin_system_env.rs` | 2 | a konfiguráció a környezeti változókból: a titkok sosem látszanak (külön bináris) |
| `tests/admin_log_capture.rs` | 1 | a konzolra írt üzenetek gyűjtése a naplónézethez (külön bináris: a folyamat kimenetét átirányítja) |
| `tests/admin_moderation.rs` | 20 | tiltott szavak, chat napló (élő / tartós), játékos oldali bejelentés, nevek átnézése |
| `tests/admin_mail.rs` | 31 | levelező szerver: mentett > környezeti beállítás, érvényesítés, a jelszó sosem látszik, visszaállítás, kapcsolat-próba (sudo, csak a saját címre), élő hatás |
| `tests/admin_update.rs` | 31 | frissítés GitHubról valódi helyi git-tárakkal: ellenőrzés, fast-forward, ágváltás, tiszta munkafa, nem fordítható kód → visszagörgetés (valódi `cargo build`), ágnév-ellenőrzés, újraindítás |
| `tests/admin_browser.rs` | 1 | az admin felület valódi böngészőben (Playwright): minden menüpont, nincs JS hiba, művelet-párbeszéd, sudo, kereső, élő Socket.IO események, telefon, nyelvváltás (kimarad, ha nincs node / Playwright / Chromium) |
| `tests/frontend_i18n.rs` | 27 | fordítások teljessége (hu / en, kulcsok, helyőrzők), szerverüzenet-lefedettség (a Rust forrásból kigyűjtve), HTML lefedettség, a fordító futtatása node-ban |
| `tests/frontend_admin.rs` | 42 | admin kliens: JS szintaxis, fordítások teljessége, a hívott API útvonalak létezése, minden végponthoz van felület, elem-azonosítók, nincs `innerHTML` / beágyazott kód |
| `tests/frontend_client.rs` | 35 | kliens ↔ szerver: konstansok, elem-azonosítók, robot-fokozat választó, Socket.IO események, API útvonalak, JS szintaxis; a kliens állapotkezelése és gyakorló logikája node-ban |
| `tests/tools.rs` | 10 | a `word_review`, `bot_arena` és `build_attested` eszközök valódi futtatással |
| `tests/ai_golden.rs` | 1 | **elbukik**: a robot lépésgenerátora a Python aranyfájllal (minden lehetséges lépés táblánként és kézenként) — lásd „Ismert problémák” |
| `tests/dictionary_golden.rs` | 5 | szóellenőrző, levezetés-magyarázat, javaslatok a Python aranyfájlokkal; **1 elbukik** (`dictionary_lookup_matches_python_including_rejected_lists`) |
| `tests/practice_golden.rs` | 8 | rövid szavak, szótövek, zsetonokra bontás, kvíz-válaszok, betűvadász szólisták a Python aranyfájllal; **2 elbukik** (`short_words_match_python`, `rack_words_match_python`) |
| `tests/vocabulary_golden.rs` | 2 | a robot szókincse (darabszám, ellenőrző összeg, minta) a Python szókincsével; **1 elbukik** |
| `tests/python_compat.rs` | 4 | a Python verzió által írt adatok olvashatók: játékállapotok, SQLite fájl (jelszó-hash, munkamenetek, mentett játékok, lépésnapló) |
| `src/**` (`#[cfg(test)]`) | 96 | egységtesztek a modulokban: tábla, zsetonok, challenge, szobák / állapot, forgalomkorlát, ELO, kitüntetések, jelszó-hash, socket-token, push titkosítás, beállítások, `.env` fájl, admin segédek, gyakorló módok… |

Ezen felül, a `--features engine-duel` kapcsolóval: `tests/engine_duel.rs` (30 teszt: szabályazonosság, híd, a két független lépésgenerátor egyezése, játékvezető és hibakezelés, ismételhetőség, tanítás, futtató / jelentés) és 13 egységteszt az `engine_duel` modulban; a vendorolt motor tesztjei: `cargo test -p pg-scrabble --all-features` (196).

Segédek: `tests/common/mod.rs` — `TestServer` (szerver ideiglenes mappával, `start_with` a konfiguráció módosításához, `start_in` másik programmappához), `Http` (süti-kezelés, admin kérések: `admin_get` / `admin_post` / `sudo()`), `Sio` (Socket.IO kliens: `emit`, `call`, `wait`, `settle`, `wait_code`, `my_turn`…), `with_room`; `tests/common/admin.rs` — `make_user`, `audit_rows`, `finished_game`, `live_room`, `set_setting`…; `tests/common/smtp.rs`, `push.rs` — hamis SMTP és push szolgáltatás; `tests/frontend_support/` — node futtatás, HTML bejárás, a Rust forrás üzeneteinek kigyűjtése.

Tudnivalók a tesztek írásához:
- **A folyamat-szintű állapot** (szótár: elutasított lista, szavazatok; környezeti változók; frissítési zár) ütközik a párhuzamos tesztek között: az ilyen tesztek egy `SERIAL` zár mögött futnak, a környezeti változós / kimenet-átirányítós tesztek külön binárisban (`*_env.rs`, `admin_log_capture.rs`).
- A robotok a tesztekben ne lépjenek maguktól: `set_setting(&server, "bot_think_multiplier", json!(5.0))`.
- Időzítés: a `Sio::settle()` a szerver feldolgozás alatt álló eseményeinek számlálóját (`events_in_flight`) is megvárja; fix `sleep` helyett `wait_code`, `wait_registered` jellegű várakozás kell.
- A hozzáférési tesztek (`admin_access.rs`) az **összes** admin útvonalat az útvonaltérképből állítják elő: új admin végpont automatikusan bekerül (nem adminnak azonos 404, CSRF nélkül 403, tétlenség után 401 `reauth`); a `frontend_admin.rs` számon kéri, hogy minden végponthoz legyen felület.
- **Aranyfájlok** (`tests/golden/`): a régi Python implementációval előállított adatok (szóellenőrzés, robot lépések, gyakorló módok, szókincs, játékállapotok, SQLite fájl); a generátorok és az újragenerálás leírása: `tests/golden/README.md`. **Differenciális összevetés**: `tests/compat/run.sh` (ugyanazok a forgatókönyvek a régi és az új szerveren). **Teljesítmény**: `scripts/perf.sh` (`benches/compare.rs`) → `docs/PERFORMANCE.md`.
- A párharc tesztjei: `cargo test --features engine-duel --test engine_duel`; a vendorolt motoré: `cargo test -p pg-scrabble --all-features`.

## Hogyan bővítsd (receptek)

| Feladat | Teendők |
|---|---|
| **Új HTTP végpont** | kezelő + útvonal a `public_routes()`-ban (`src/server/http.rs`); forgalomkorlát-csoport az `AUTH_RATE_LIMITS`-ben, ha új; bejegyzés a [PROTOCOL.md](docs/PROTOCOL.md)-be; teszt (`tests/http_*.rs`); a kliens hívása (és a `tests/frontend_client.rs` útvonal-ellenőrzése) |
| **Új Socket.IO esemény** | kezelő (`src/server/{events,play,extras}.rs`) + `SYNC_EVENTS` (vagy aszinkron az `attach_handlers`-ben) + `SOCKET_RATE_LIMITS`; kliens `socket.emit` / `socket.on`; [PROTOCOL.md](docs/PROTOCOL.md); teszt (`tests/socket_*.rs`) |
| **Új felhasználói szöveg** | kulcs a `i18n-data.js` hu **és** en blokkjába; `data-i18n*` / `t()`; szerver-üzenetnél `en.server.exact` / `patterns`; `cargo test --test frontend_i18n` |
| **Új adatbázis-mező / tábla** | `schema.sql` (`CREATE TABLE IF NOT EXISTS`) vagy `ensure_column` a `Db::init`-ben; teszt a `tests/db_tests.rs`-ben; a régi adatbázisnak továbbra is meg kell nyílnia |
| **Új futásidejű beállítás** | `definitions` a `settings.rs`-ben (+ az érték tényleges használata `settings.get*`-gal) + `SETTING_TEXT` az `admin-views-c.js`-ben + admin fordítás hu / en; teszt az `admin_comm.rs`-ben |
| **Új admin funkció** | logika az `src/admin/*.rs`-ben, módosításnál `admin::action(...)`; végpont az `src/admin/api/*.rs`-ben a jogosultsági szinttel (`post_danger`, `post_sudo`…); nézet az `admin-views-*.js`-ben (`registerSection` új menüponthoz); minden szöveg az `admin-i18n.js`-ben (hu **és** en, a szerverüzenet is az `en.server.exact`-ban); teszt (a hozzáférési tesztek az útvonaltérképből automatikusan lefedik) |
| **Új kitüntetés** | a `BADGES`-be + kiértékelés az `achievements.rs`-ben + név / leírás a kliens fordításaiban (`badge.<kulcs>`, `badge.<kulcs>_desc`) + ikon a `BADGE_ICONS`-ban (`app.js`) |
| **A robot módosítása** | `ai.rs`; `bot_arena ladder` újramérés → `LEVEL_STRENGTH`; `ANALYSIS_VERSION` növelése; az aranyfájlok érintettek lehetnek |
| **A szótár módosítása** | `hu_rejected.txt` (`word_review apply`), `hu_attested.txt` (`build_attested`) vagy az `affix.rs` szabályai; `ANALYSIS_VERSION` növelése; aranyfájlok újragenerálása (`tests/golden/README.md`) |
| **Új gyakorló mód** | motor a `words/practice.rs`-ben (állapotmentes), végpont a `http.rs`-ben a `practice` őrrel, nézet a `Practice` modulban (`.practice-view`, `Practice.open(nézet)`), fordítás, teszt a `tests/practice.rs`-ben |
| **Új nyelv a felületen** | a `i18n-data.js` blokkja + az `i18n.js` `SUPPORTED` listája (a tesztek ellenőrzik a kulcsok és helyőrzők teljességét) |

## Ismert problémák és ötletek

**Ismert problémák** (a repó állapota a dokumentáció frissítésekor):
- **Elavult aranyfájlok.** A `bb4ca81` (a `dict/hu_rejected.txt` bővítése 2595 gépi átnézésen kizárt szóval, 622 → 3217 szó) óta 5 teszt bukik, mert a Python-ból előállított aranyfájlok a régi elutasított listával készültek: `tests/ai_golden.rs`, `tests/dictionary_golden.rs` (`dictionary_lookup_matches_python_including_rejected_lists`), `tests/practice_golden.rs` (`short_words_match_python`, `rack_words_match_python`), `tests/vocabulary_golden.rs`. **Ellenőrizve:** a régi (622 szavas) listával mind a négy fájl zöld (16 teszt): `git show bb4ca81^:dict/hu_rejected.txt > /tmp/old_rejected.txt`, majd `SCRABBLE_REJECTED_FILE=/tmp/old_rejected.txt cargo test --test ai_golden --test dictionary_golden --test practice_golden --test vocabulary_golden`. Tartós javítás: az aranyfájlok újragenerálása az új listával (`tests/golden/README.md`), vagy a golden összevetésből az elutasított lista kizárása (a tesztek a régi listát használják).
- **Admin Rendszer kártya: Python-korszakbeli feliratok.** A „Szerver” kártya a Rust-verziót „Python” címkével, a tokio-feladatok számát „Greenletek” címkével mutatja (`admin.srv_python`, `admin.srv_greenlets`, `src/admin/system.rs` `python` / `greenlets` mezők) — csak kozmetikai.
- **Windows:** az automatikus tunnel-indítás nem találja a `cloudflared.exe`-t (a keresés `.exe` nélkül történik), és az admin panelről indított újraindítás nem-Unixon nem csinál semmit (lásd „Gyors referencia”).
- A `dict/hu_rejected.txt` fejléce még a régi `tools/word_review.py` eszközre hivatkozik (a mai: `target/release/word_review`); az `docs/ADMIN_PANEL.md` a Python-korszak specifikációja (a fejlécében a fájlnevek megfeleltetésével).
- A repó gyökerében nincs `LICENSE` fájl (a vendorolt motoré és a szótáré a saját mappájában / fejlécében van).

**Ötletek**
- [ ] Szótár-építő: az AI által elutasított szavak emberi második véleménye (külön mintavétel a `hu_rejected.txt` szavaiból), a felhasználói szavazatok exportja a tartós listába, mintázat-alapú átvizsgálás (népnév + -gyűlölet / -ellenesség / -üldözés … családok), szavankénti szerepkör / küszöb
- [ ] Robot: kétszeres ragozás és hosszabb szótövek (memória!), tapasztalati értékelés (szimuláció), emberszerűbb lépések (kevesebb egyzsetonos lépés az alsó fokozatokon), gyakori szavak listája a ritka szavak elkerülésére, fokozat robotonként a felületen
- [ ] Levelezős játék: nyitott (ismeretlen ellenfeles) játékok, e-mail értesítés push híján, chat, emlékeztető a határidő előtt
- [ ] Több nyelv a felületen; Windows-támogatás finomítása (tunnel, újraindítás); Dockerfile

**Összesen: 1270 teszt** (a legutóbbi teljes `cargo test` futás, Linux: 1265 sikeres, 5 elbukó — mind aranyfájlos, lásd „Ismert problémák”)
