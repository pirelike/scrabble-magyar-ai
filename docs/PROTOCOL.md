# Protokoll — HTTP útvonalak és Socket.IO események

A böngészős kliens két csatornán beszél a szerverrel: **JSON HTTP API** (fiók, ranglista, gyakorlás, visszajátszás…) és **Socket.IO**
(szobák, játékmenet, élő frissítések). Ez a dokumentum a nyilvános (nem admin) felület referenciája; az admin API-t az
[ADMIN_PANEL.md](ADMIN_PANEL.md) és a `src/admin/routes.rs` írja le. A forrás: `src/server/http.rs` (útvonaltábla a fájl végén), `src/server/mod.rs`
(`SYNC_EVENTS`) és a kezelők a `src/server/{events,play,extras,core}.rs` fájlokban.

> A szerver üzenetei **magyarul** készülnek; a kliens a `tServer()` függvénnyel fordítja angolra (`web/static/i18n-data.js` → `en.server`).
> A régi Python (Flask) változattal a protokoll **azonos**.

- [Általános szabályok](#általános-szabályok)
- [HTTP útvonalak](#http-útvonalak)
- [Socket.IO: kliens → szerver](#socketio-kliens--szerver)
- [Socket.IO: szerver → kliens](#socketio-szerver--kliens)
- [Forgalomkorlátok](#forgalomkorlátok)
- [A játékállapot (`game_state`)](#a-játékállapot-game_state)

## Általános szabályok

- **Munkamenet**: `session_token` süti (`HttpOnly`, `SameSite=Lax`, `Secure` HTTPS-en), 30 nap. Bejelentkezéskor / regisztrációkor kapja a kliens.
- **Hibaválasz**: `{"success": false, "message": "…"}` megfelelő állapotkóddal (400 hibás kérés, 401 nincs session, 403 tiltott, 404 nincs, 409 ütközés, 429 túl sok kérés, 503 a szótár nem érhető el).
- **IP-alapú forgalomkorlát** minden HTTP csoportra (alapértékek: `src/config.rs` → `AUTH_RATE_LIMITS`; az admin panelen felülírhatók).
- **Socket.IO azonosság**: a kapcsolat a `set_name` eseménnyel mutatkozik be; regisztrált felhasználónál az `auth_token` (`GET /api/auth/socket-token`, aláírt, 5 percig érvényes) **kötelező** —
  a kliens által küldött `user_id` önmagában nem elég, a név a fiókból jön. Érvénytelen token → vendég + hibaüzenet.
- **Sorrend**: egy kapcsolat szinkron eseményei a beérkezés sorrendjében hatnak (`InOrder` kinyerő).
- **Kitiltás**: a kitiltott IP-ről a Socket.IO kapcsolat el sem jön létre (`forbidden`), a kitiltott fiók kapcsolata az `account_banned` esemény után bezárul.

## HTTP útvonalak

### Oldal és PWA

| Útvonal | Leírás |
|---|---|
| `GET /` | az egyoldalas kliens (`web/templates/index.html`, `?v=` a fájlok módosítási idejéből) |
| `GET /static/…` | statikus fájlok (`web/static/`), `Cache-Control: no-cache` |
| `GET /manifest.webmanifest`, `GET /sw.js` | PWA; a service worker a gyökérről (`Service-Worker-Allowed: /`) |

### Fiók (`/api/auth/…`)

| Útvonal | Leírás |
|---|---|
| `POST /api/auth/request-code` | e-mail megerősítő kód kérése (SMTP nélkül a válasz `dev_code`-ot is tartalmaz — admin címnél soha) |
| `POST /api/auth/verify-code` | a 6 számjegyű kód ellenőrzése (10 perc, legfeljebb 5 próba) |
| `POST /api/auth/register` | jelszó + név, fiók létrehozása, automatikus belépés (csak a kóddal előzőleg megerősített címre, 30 percig érvényes, egyszer használható) |
| `POST /api/auth/login` | e-mail + jelszó |
| `POST /api/auth/logout` | munkamenet törlése |
| `GET /api/auth/me` | a süti ellenőrzése; csak az adminnál szerepel a `user.is_admin: true` (másnál a kulcs sincs) |
| `GET /api/auth/profile` | statisztikák, kitüntetések, az utolsó 20 befejezett játék |
| `GET /api/auth/socket-token` | rövid életű, aláírt token a Socket.IO `set_name`-hez |
| `GET /api/auth/saved-games` | a felhasználó folyamatban lévő / mentett játékai (a „Mentett játékok” fül) |
| `GET /api/auth/friends` | barátok (online jelzővel), beérkezett és elküldött kérések |
| `GET /api/auth/search-users?q=` | felhasználókeresés barátjelöléshez (legalább 2 karakter, legfeljebb 10 találat) |

### Játékok, visszajátszás, elemzés

| Útvonal | Leírás |
|---|---|
| `GET /api/game/{id}/moves` | a lépések listája a visszajátszáshoz (`players`, `finished`; folyamatban lévő játéknál a kezek nélkül) |
| `GET /api/game/{id}/analysis` | játékelemzés; háttérben számolódik: `status: running \| ready \| unavailable \| error` |
| `POST /api/game/{id}/share` | megosztható visszajátszás-link (csak résztvevő, befejezett játék) |
| `GET /api/replay/{token}` | nyilvános, megosztott visszajátszás |
| `POST /api/game/{id}/abandon` | a mentés tulajdonosa törli a mentett játékot (levelezős játékra nem: ott a játékban kell feladni) |
| `GET /api/async/games` | a felhasználó folyamatban lévő levelezős játékai (akinél a sor, az elöl) |

### Nyilvános és gyakorló végpontok

| Útvonal | Leírás |
|---|---|
| `GET /api/leaderboard?metric=rating\|wins\|win_rate\|avg_score\|best_game&limit=50` | ranglista (bejelentkezve a saját helyezés is: `me`, `is_me`) |
| `GET /api/daily` · `GET /api/daily/leaderboard?date=` | a napi feladvány (a saját eredmény, top 10, tegnapi megoldás) és a napi ranglista |
| `GET \| POST /api/dictionary/check` (`q` / `words`, legfeljebb 8 szó) | szó-ellenőrzés: `valid`, `tiles`, `score`, `reason`, `suggestions` |
| `GET /api/announcements` | az érvényes közlemények és a karbantartási mód (a célcsoport a bejelentkezéstől függ) |
| `GET /api/practice/quiz?n=&mode=mixed\|2\|3\|tricky` · `POST /api/practice/answer` | szókvíz: kérdések + zsetonok; a válasz állapot nélkül értékelődik |
| `GET /api/practice/short-words?length=2\|3` | a rövid szavak listája pontértékkel |
| `GET /api/practice/rack?kind=hunt\|bingo` · `POST /api/practice/rack-word` | betűvadász / bingó-edző: a 7 zseton + az összes kirakható szó; a listán nem szereplő szó bírálata (`{rack, word}`) |
| `GET \| POST /api/practice/word-review` · `POST …/undo` · `GET …/stats` | Szótár-építő (bejelentkezve): átnézendő szavak; szavazat `{word, valid}`; visszavonás; összesítő |
| `GET /api/push/public-key` · `POST /api/push/subscribe \| unsubscribe` | Web Push (bejelentkezve) |

## Socket.IO: kliens → szerver

| Esemény | Leírás |
|---|---|
| `set_name` | bemutatkozás: `{name, is_guest, user_id, auth_token}` — regisztráltnál az `auth_token` kötelező |
| `logout` | kijelentkezés: kilépés a szobából, az online azonosság törlése |
| `create_room` | `{name, max_players, challenge_mode, is_private, turn_time_limit, ai_players, hint_limit}` — `ai_players`: a robotok fokozatainak listája (1–10, `"auto"`; a régi `easy` / `medium` / `hard` is elfogadott), legfeljebb 3 és `max_players − 1`; `hint_limit`: 0 / 1 / 3 / 5 / 10 |
| `join_room` | csatlakozás `{code}` (6 jegyű kód) vagy `{room_id}` alapján |
| `leave_room` | a szoba elhagyása (azonnali) |
| `get_rooms` | a nyilvános szobák listázása (`rooms_list`, `live_games` a válasz) |
| `rejoin_room` | újracsatlakozás tokennel (türelmi idő alatt — a várakozó szobába is —, vagy a még „élőnek” hitt régi kapcsolat átvételével) |
| `start_game` | játék indítása (csak a tulajdonos) |
| `save_game` · `restore_game` | manuális mentés (csak a tulajdonos) · mentett játék visszaállítása várakozó szobaként |
| `place_tiles` | `{tiles: [{row, col, letter, is_blank}]}` |
| `exchange_tiles` · `pass_turn` | csere a zsákból · a kör passzolása |
| `accept_words` · `reject_words` | kihívás módban: lerakás elfogadása / elfogadó szavazat · elutasítás (2 játékos) / megtámadás indítása (3+ játékos) |
| `withdraw_words` | a lerakó visszavonja a szavazásra váró lerakását (amíg senki sem szavazott); időlimitnél a hátralévő idő folytatódik (legalább 10 mp) |
| `preview_move` | `{tiles}` — lerakás kipróbálása véglegesítés nélkül → `move_preview` (csendben korlátozott) |
| `request_hint` | tipp kérése (csak ha egyetlen emberi játékos van, engedélyezett és van még tipp) → `hint_result`; csak akkor fogy, ha van javasolt lépés |
| `send_chat` | `{message}` (legfeljebb 200 karakter) |
| `report_content` | `{kind: 'player' \| 'chat', target, message, reason}` — bejelentés (csak regisztrált, a szobában lévő játékos) → `report_result` |
| `set_visibility` | `{hidden}`: a böngészőlap háttérbe került-e (push értesítéshez) |
| `start_daily` · `retry_daily` · `reveal_daily` | napi feladvány indítása · új próbálkozás a befejezett után · a megoldás megmutatása (utána nem rangsorolt) |
| `create_async_game` | levelezős játék: `{name, friend_ids: [1–3 barát], turn_hours: 24 \| 48 \| 72 \| 168}`; csak regisztrált, csak barátok |
| `open_async_game` · `resign_game` | levelezős játék megnyitása `{game_id}` (másik eszközről átveszi) · feladása |
| `spectate_room` · `leave_spectate` | megfigyelés `{room_id}` (nyilvános) vagy `{code}` (privát is); csak folyamatban lévő játék · kilépés |
| `send_friend_request` · `accept_friend_request` · `decline_friend_request` · `remove_friend` | barátkezelés |
| `invite_to_room` · `respond_invite` | barát meghívása a várakozó szobába · a meghívó elfogadása / elutasítása |
| `admin_subscribe` · `admin_watch_room` | az admin panel élő eseményei és egy élő szoba figyelése — csak adminnak hat, másnak csendben semmi |

## Socket.IO: szerver → kliens

**Szobák és lobby**

| Esemény | Tartalom |
|---|---|
| `rooms_list` · `live_games` | a nyilvános szobák · a nyilvános, folyamatban lévő játékok (megfigyeléshez: név, játékosok pontokkal, néző-szám) |
| `room_joined` · `room_left` · `room_code` | csatlakozás / kilépés megerősítése · a 6 jegyű kód (csak a tulajdonosnak) |
| `player_joined` · `player_left` · `player_disconnected` · `player_reconnected` | a szoba tagjainak változása |
| `room_disbanded` · `rejoin_failed` | a szoba megszűnt · az újracsatlakozás nem sikerült |
| `spectate_joined` · `spectate_left` | megfigyelés kezdete / vége |
| `error` | `{message}` — hibaüzenet |

**Játékmenet**

| Esemény | Tartalom |
|---|---|
| `game_started` | a játék elindult |
| `game_state` | a teljes játékállapot, **személyre szabva** (lásd lent); megfigyelőnek kéz nélkül, `spectator: true` |
| `action_result` | a lerakás / csere / passz eredménye |
| `challenge_result` | `{challenge_won, message}` — a szavazás / döntés eredménye |
| `chat_message` | `{name, message}` |
| `move_preview` | `{valid, score, words: [{word, score}], message}` |
| `hint_result` | `{success, message, hints_left, moves: [{tiles, words, score}]}` |
| `game_saved` | `{game_id}` — a befejezett játék azonosítója (elemzés, visszajátszás) |
| `rating_update` · `achievements_earned` | `{rating, change}` · `{badges: […]}` |

**Napi feladvány, levelezős, barátok**

| Esemény | Tartalom |
|---|---|
| `daily_started` · `daily_result` · `daily_solution` | indulás `{date, registered, my}` · a beküldés eredménye `{score, best_score, is_best, recorded, rank, attempts}` · a megoldás |
| `async_your_turn` · `async_invited` | levelezős játék: rád került a sor · meghívtak (a lobbyban lévőnek) |
| `friend_request_result` · `friend_request_received` · `friend_request_accepted` · `friend_presence_changed` | barátkérések és online állapot |
| `game_invite` · `invite_sent` · `invite_accepted` · `invite_declined` | szobameghívó |

**Rendszer**

| Esemény | Tartalom |
|---|---|
| `announcement` | `{registered, guests}` — a közlemények / karbantartási mód élő frissítése (a kliens a sajátját választja) |
| `account_banned` | `{reason, until}` — a kapcsolat azonnal bezárul |
| `report_result` | `{success, message}` |
| `admin_subscribed`, `admin_overview`, `admin_room_update`, `admin_room_state`, `admin_alert`, `admin_report` | csak az `admin` Socket.IO szobának: élő számlálók (5 mp), egy szoba sora, egy figyelt szoba teljes állapota, tiltott szavas üzenet, új bejelentés |

## Forgalomkorlátok

**Socket.IO eventek (SID szerint, `src/app.rs` → `SOCKET_RATE_LIMITS`; kérés / ablak mp):**

```text
set_name 5/10          create_room 3/30        join_room 5/10          rejoin_room 5/10
place_tiles 10/10      exchange_tiles 5/10     pass_turn 5/10          get_rooms 10/5
accept_words 5/10      reject_words 5/10       withdraw_words 5/10     send_chat 10/10
save_game 3/30         restore_game 3/30       preview_move 30/10 (csendben eldobva)
request_hint 3/30      set_visibility 20/10    report_content 3/60
start_daily 3/30       retry_daily 10/30       reveal_daily 5/30
create_async_game 3/30 open_async_game 10/10   resign_game 3/30
spectate_room 5/10     leave_spectate 5/10
send_friend_request 5/30   accept_friend_request 10/10   decline_friend_request 10/10
remove_friend 5/30     invite_to_room 10/30    respond_invite 10/10
admin_subscribe 5/10 (csendben)   admin_watch_room 20/10 (csendben)
```

**HTTP (IP szerint, `src/config.rs` → `AUTH_RATE_LIMITS`; kérés / ablak mp):** `request_code` 3/300 · `login` 10/300 · `register` 3/3600 · `search_users` 20/60 · `leaderboard` 30/60 ·
`dictionary` 60/60 · `replay` 60/60 · `analysis` 30/60 · `daily` 60/60 · `practice` 120/60 · `push` 20/60 · `word_review` 240/60 · `announcements` 60/60 · `admin` 120/60 · `admin_danger` 20/60.

Az értékek az admin panelen (**Biztonság → Forgalomkorlátok**, illetve a Beállítások `rate_limits_http` / `rate_limits_socket`) futásidőben felülírhatók.

## A játékállapot (`game_state`)

A `game_state` minden játékosnak a **saját keze** szerint készül (`Game::get_all_states`; robotnak nem készül), a megfigyelőnek a `get_spectator_state` (kezek nélkül, `spectator: true`).
A közös mezők (`Game::shared_state`):

- `game_id`, `started`, `finished`, `turn_number`, `board` (15×15, a mezők `null` vagy a zseton adata), `tiles_remaining`;
- `players` (név, pont, `hand` csak a sajátnál, `is_bot`, `disconnected`, `difficulty`…), `current_player` (a játékos azonosítója), `current_player_name`;
- `winner` (csak ha egyetlen győztes van) és `winners` (döntetlennél több);
- `last_action` (magyar szöveg) és a szerkezetes `last_action_info` (a kliens ebből formáz), `last_move_tiles` (az utolsó lerakás mezői a kiemeléshez), `history` (a lépéstörténet, a zsetonok nélkül);
- `challenge_mode` és `pending_challenge` (a függő lerakás és a szavazás állapota); `turn_time_limit`; `hint_limit`, `hints_left`;
- levelezős játék: `async_mode`, `turn_hours`, `turn_deadline`; napi feladvány: `puzzle` (`{date, score}`).

A szerver az eseményküldéskor még hozzáteszi a `turn_timer_expires_at` (a körtimer lejárta) és a `spectator_count` (a megfigyelők száma) mezőt.
A pontos szerkezet a `src/engine/game.rs` állapot-függvényeiben és a `tests/socket_*.rs` tesztekben látható.
