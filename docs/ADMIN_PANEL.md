# Admin panel — részletes specifikáció (utasítás a megvalósításhoz)

> Ez a dokumentum a megvalósítás utasítása. Aki az admin panelt elkészíti (ember vagy Claude),
> ezt kövesse pontról pontra. A meglévő konvenciók (CLAUDE.md) érvényesek: DOM API `innerHTML`
> helyett, `data-i18n` / `t()` minden szövegre (hu **és** en), az Apple HIG ihletésű design rendszer
> tokenjei, rate limit minden végponton, és minden új funkcióhoz teszt.

---

## 0. Alapelvek

1. **Egyetlen (vagy néhány) kijelölt felhasználó** érheti el. Senki más nem láthatja: se a gombot, se
   a felületet, se a JavaScriptjét, se az API válaszait — még azt sem, hogy admin panel létezik.
2. **Minden jogosultság-ellenőrzés a szerveren történik.** A kliens elrejtése csak kényelem, nem
   védelem.
3. **Minden admin művelet naplózva** van (ki, mit, mikor, melyik IP-ről, milyen paraméterrel, mi volt
   előtte és utána).
4. **Romboló műveletek** (törlés, kitiltás, játék lezárása, ELO újraszámolás, szótár módosítás)
   megerősítést kérnek, a legsúlyosabbak friss jelszó-megerősítést („sudo mód”).
5. **Visszafordíthatóság, ahol lehet**: kitiltás feloldható, a játék „érvénytelenítése” csak jelölés
   (nem törlés), a szótár-módosítás visszavonható, törlés előtt mentés/export.
6. **A futó játékokat nem szabad elrontani**: a panel a meglévő függvényeken keresztül avatkozik be
   (`_emit_all_states`, `_schedule_bot_turn`, `room.invalidate_*`, `finish_game` …), nem közvetlenül
   az állapot átírásával.

---

## 1. Hozzáférés-szabályozás

### 1.1 Ki az admin

- Új konfiguráció a `config.py`-ban:
  ```python
  # Vesszővel elválasztott e-mail címek (kisbetűsítve hasonlítva). Üres → nincs admin, a panel ki van kapcsolva.
  ADMIN_EMAILS = {e.strip().lower() for e in os.environ.get('ADMIN_EMAILS', '').split(',') if e.strip()}
  ADMIN_SESSION_IDLE_MINUTES = int(os.environ.get('ADMIN_SESSION_IDLE_MINUTES', 30))
  ADMIN_SUDO_MINUTES = int(os.environ.get('ADMIN_SUDO_MINUTES', 10))
  ADMIN_IP_ALLOWLIST = {...}  # opcionális, üres = bármely IP
  ```
- Az admin státusz **az e-mail címhez kötött** (`users.email_lower ∈ ADMIN_EMAILS`), nem a
  megjelenítési névhez (az átírható) és nem egy adatbázis-oszlophoz (azt egy SQL-hiba vagy egy
  admin-végpont hibája elállíthatná). Így adminná válni csak a szerver környezeti változójával lehet.
- Csak **regisztrált, bejelentkezett** felhasználó lehet admin; vendég soha.
- Segédfüggvény az `auth.py`-ban: `is_admin_user(user) -> bool`.

### 1.2 Szerveroldali őr

- `routes.py`: új blueprint `admin_bp`, `url_prefix='/api/admin'`. Minden útvonal előtt egy
  `@admin_bp.before_request` őr:
  1. `validate_session(cookie)` → ha nincs, vagy nem admin → **404** (nem 403, hogy a létezése se
     derüljön ki), a válasz megegyezik egy nem létező útvonaléval.
  2. IP-engedélylista (ha be van állítva).
  3. Tétlenségi időkorlát: az admin utolsó admin-kérése óta eltelt idő > `ADMIN_SESSION_IDLE_MINUTES`
     → 401 `{reauth: true}` (a kliens jelszót kér).
  4. CSRF: minden nem-GET kérésnél kötelező az `X-Admin-Request: 1` fejléc **és** az `Origin`/`Referer`
     egyezése a saját hosttal (a `SameSite=Lax` mellé).
  5. IP-alapú rate limit (`AUTH_RATE_LIMITS['admin'] = (120, 60)`, romboló műveletekre külön
     `admin_danger = (20, 60)`).
- Socket.IO admin eseményeknél (pl. élő beavatkozás, közlemény) ugyanez: a SID-hez tartozó
  `user_id` → `users.email_lower ∈ ADMIN_EMAILS`, különben az esemény csendben eldobva (nincs
  hibaüzenet).
- **Sudo mód**: romboló műveletekhez a kliens `POST /api/admin/sudo {password}` hívással
  `ADMIN_SUDO_MINUTES` percre megerősítést kap (szerveroldali időbélyeg az admin sessionhöz kötve).
  Lejárt sudo → 401 `{sudo_required: true}`.
- A rossz sudo-jelszó a login rate limitjébe számít, és naplózódik.

### 1.3 Kliensoldali megjelenés — csak az adminnak

- **Külön oldal**: `GET /admin` → `templates/admin.html`. A route ugyanazt az őrt használja; nem
  adminnak **404** (a szokásos 404 oldal), így a HTML sem jut ki.
- Az admin JS/CSS **nem** a `static/` mappában van (az bárki számára letölthető), hanem
  `admin_assets/admin.js`, `admin_assets/admin.css`, és egy őrzött route szolgálja ki
  (`GET /admin/assets/<path>`), szintén 404-gyel nem adminnak. A közös `style.css` és `i18n.js`
  újrahasznosítható.
- A `GET /api/auth/me` válaszában **csak az adminnál** jelenik meg `is_admin: true`; másnál a kulcs
  egyáltalán nem szerepel (ne `false` legyen, hogy a mező léte se áruljon el semmit).
- Belépési pont a fő alkalmazásban: ha `is_admin`, a JS **futásidőben** szúr be egy „Admin” gombot
  a felső sávba (`.app-topbar`, pajzs ikon) és a profil oldal beállításaiba. Az `index.html`-ben
  nincs statikus admin elem; az admin fordítási kulcsok az admin oldalon töltődnek be
  (`admin_assets/admin-i18n.js`), így a nyilvános `i18n-data.js` sem tartalmaz admin szöveget.
- Service worker: a `/admin` és `/admin/assets/` **soha** nem kerül gyorsítótárba (a `sw.js`
  kizáró listájára fel kell venni, a `/api/`-hoz hasonlóan).
- Az admin oldalon `<meta name="robots" content="noindex,nofollow">`, `Cache-Control: no-store`,
  `X-Frame-Options: DENY`, szigorú `Content-Security-Policy`.

### 1.4 Admin munkamenet

- A fejlécben: bejelentkezett admin neve, sudo állapot (visszaszámláló), „Kilépés az admin
  panelből” (vissza a lobbyba), „Sudo lezárása”.
- Tétlenségi időkorlát után a panel elhomályosul és jelszót kér (nem dob ki a játékból).

---

## 2. Napló (audit log) — ezt kell először megcsinálni

- Új tábla:
  ```sql
  CREATE TABLE IF NOT EXISTS admin_audit (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      admin_user_id INTEGER NOT NULL,
      action TEXT NOT NULL,            -- pl. 'user.ban', 'room.force_end', 'dict.reject_add'
      target_type TEXT,                -- 'user' | 'room' | 'game' | 'word' | 'setting' | ...
      target_id TEXT,
      details_json TEXT,               -- paraméterek, előtte/utána állapot, indoklás
      ip TEXT,
      user_agent TEXT,
      created_at TEXT NOT NULL DEFAULT (datetime('now'))
  );
  CREATE INDEX IF NOT EXISTS idx_admin_audit_created ON admin_audit(created_at);
  CREATE INDEX IF NOT EXISTS idx_admin_audit_target ON admin_audit(target_type, target_id);
  ```
- Minden módosító admin végpont egy `audit(action, target_type, target_id, details)` segéddel ír
  bele, **a művelettel egy tranzakcióban** (ha a napló nem írható, a művelet sem történik meg).
- Az olvasó (GET) műveletek közül a **személyes adat megtekintése** (felhasználó részletei, chat
  napló, kezek megtekintése) is naplózódik `view.*` akcióval.
- A napló **nem törölhető és nem szerkeszthető** a panelről.
- A felületen: szűrés adminra, akcióra, célpontra, időszakra; keresés; CSV/JSON export; egy sorra
  kattintva a teljes `details_json` (előtte/utána diff).
- Minden felhasználó / játék / szó részletező oldalán „Admin előzmények” blokk (az őt érintő
  naplósorok).

---

## 3. Felépítés és navigáció

- Bal oldali menü (asztali gépen), telefonon alsó lap / hamburger; a meglévő design rendszer
  komponenseivel (`.settings-group`, listasorok, `.dialog-sheet`, `.toggle-switch`, gombváltozatok).
- Sötét / világos téma és magyar / angol nyelv, mint a fő alkalmazásban.
- Globális kereső a fejlécben (Ctrl+K): felhasználó (név / e-mail / id), szoba (kód / név / id),
  játék (id), szó — találatok csoportosítva, Enter → részletező oldal.
- URL-alapú navigáció (`/admin#users/42`, `/admin#rooms/ABC123`), hogy a böngésző vissza gombja és
  a linkmásolás működjön.
- Minden listában: lapozás (szerveroldali, `limit`/`offset` vagy kurzor), rendezés oszlopra,
  szűrők, a szűrők az URL-ben, CSV export, „frissítés” gomb, élő oldalakon automatikus frissítés
  (Socket.IO admin szoba: `admin` room, csak adminoknak).
- Üres és hibás állapotok kidolgozva (`.empty-state`), betöltésnél vázlat (skeleton).
- Megerősítő párbeszéd romboló műveleteknél: a művelet pontos leírása, a célpont neve, **kötelező
  indoklás** mező (a naplóba kerül), a legsúlyosabbaknál a célpont nevének begépelése.

Menüpontok:

1. Áttekintés (dashboard)
2. Felhasználók
3. Élő szobák és játékok
4. Játékok (archívum)
5. Levelezős játékok
6. Ranglista és értékszám
7. Szótár
8. Napi feladvány
9. Moderáció (chat, nevek, bejelentések)
10. Kommunikáció (közlemény, push, e-mail)
11. Statisztika
12. Biztonság
13. Rendszer
14. Beállítások (funkciókapcsolók)
15. Admin napló

---

## 4. Áttekintés (dashboard)

Élő számlálók (Socket.IO-n, 5 mp-enként frissülve):

- Online felhasználók (regisztrált / vendég), csatlakozott socketek száma
- Szobák: várakozó / aktív / levelezős / napi feladvány; nyilvános / privát
- Futó játékok, ebből robotos; megfigyelők összesen
- Szavazásra váró lerakások, futó kör-időzítők
- Lecsatlakozott, türelmi időben lévő játékosok
- Mai: regisztrációk, befejezett játékok, napi feladvány próbálkozások, szótár-építő döntések,
  kizárt szavak
- Szerver: üzemidő, memória (RSS), CPU, gevent greenletek száma, adatbázis mérete, a szótár
  betöltve-e (`dictionary.is_available()`), a robot szókincse betöltve-e, push elérhető-e
  (`pywebpush`, VAPID), SMTP beállítva-e, tunnel URL és állapota, `asset_version()`, git commit

Grafikonok (utolsó 30 nap, napi bontás): regisztrációk, aktív felhasználók (DAU), befejezett
játékok (emberi / robotos), napi feladvány résztvevők.

Figyelmeztetések kártyái (piros/sárga):
- a szótár nem töltődött be (a szótár-eszköz 503-at ad)
- SMTP nincs beállítva (a kódok a konzolra mennek)
- push nem elérhető
- elakadt játék (a soron lévő játékosnak nincs időzítője, és X perce nem történt semmi; robot
  köre, de nincs ütemezett robotlépés)
- sok sikertelen bejelentkezés egy IP-ről / egy fiókra
- a szótár-építőben egy felhasználó szokatlanul sok „nem szó” szavazatot adott rövid idő alatt
- lejárt határidejű levelezős játék, amelyet a `_async_sweeper` nem kezelt
- az adatbázis mentése régebbi, mint 24 óra

Gyorslinkek: „Közlemény küldése”, „Karbantartási mód”, „Adatbázis mentés letöltése”.

---

## 5. Felhasználók

### 5.1 Lista

Oszlopok: id, megjelenítési név, e-mail, regisztráció dátuma, utolsó belépés, online (zöld pont),
játszott / nyert, értékszám, státusz (aktív / kitiltott / némított / törölt), push eszközök száma.
Szűrők: online, kitiltott, admin, regisztráció időszaka, legalább N játék, inaktív X napja.
Keresés név, e-mail és id szerint (kis-nagybetű és ékezet független).

### 5.2 Részletező oldal

- **Adatok**: id, e-mail, név, regisztráció, utolsó belépés és IP, aktív sessionök (létrehozás,
  lejárat, IP, böngésző — ehhez a `sessions` táblába `ip`, `user_agent`, `last_seen` oszlop kell
  migrációval), push feliratkozások (nyelv, létrehozás), barátok és függő kérések.
- **Statisztika**: a profil oldal összes adata + értékszám-előzmény grafikon
  (`game_players.rating_before/after`), kitüntetések, napi feladvány eredmények, szótár-építő
  szavazatai (mennyi, milyen arányban „nem szó”, utolsó 50).
- **Játékok**: az összes játéka (befejezett, folyamatban, mentett, levelezős), visszajátszás linkkel.
- **Élő**: ha online, melyik szobában van, mióta; „Ugrás a szobához”.
- **Admin előzmények**: a rá vonatkozó naplósorok, korábbi kitiltások.
- **Belső jegyzet**: szabad szöveges admin-megjegyzés a felhasználóhoz (`user_admin_notes` tábla,
  időbélyeggel, ki írta).

### 5.3 Műveletek

| Művelet | Részletek | Sudo |
|---|---|---|
| Név módosítása | validálás a regisztrációval azonos szabályokkal; a futó szobákban is frissül | – |
| E-mail módosítása | egyediség-ellenőrzés; értesítés a régi címre | ✔ |
| Jelszó-visszaállítás | e-mailben kód / ideiglenes jelszó, minden session érvénytelenítése | ✔ |
| Kijelentkeztetés mindenhonnan | `sessions` törlés + a socketek bontása (`disconnect`) | – |
| Kitiltás | időtartam (1 óra / 1 nap / 7 nap / 30 nap / végleges), indoklás; azonnal kidobja a socketjeit, a futó játékban `disconnected` lesz; belépéskor a felhasználó látja az okot és a lejáratot | ✔ |
| Kitiltás feloldása | | – |
| Chat némítás | időtartammal; a `send_chat` csendben eldobja | – |
| Szótár-építő tiltása | a szavazatai nem számítanak, új szavazatot nem adhat | – |
| Szótár-építő szavazatainak visszavonása | mind / időszak szerint; a kizárások újraszámolódnak (`word_review.refresh`) | ✔ |
| Értékszám kézi módosítása | új érték + indoklás, külön sorként az értékszám-előzményben | ✔ |
| Statisztika újraszámolása | `users` számlálói a `game_players`-ből | – |
| Kitüntetés adása / elvétele | a `BADGES` listából | – |
| Push teszt üzenet | az összes eszközére | – |
| Push feliratkozás törlése | eszközönként | – |
| Adatok exportja (GDPR) | JSON: fiók, játékok, lépések, szavazatok, kitüntetések, barátok | – |
| Fiók törlése / anonimizálása | alapértelmezés: anonimizálás (név → „Törölt felhasználó #id”, e-mail és jelszó törölve, a játékok és a ranglista-előzmények megmaradnak); teljes törlés külön opció. A célpont nevének begépelése kötelező | ✔ |
| Belépés helyette | **nincs** — megszemélyesítés biztonsági okból nem készül; helyette a „Profil megtekintése, ahogy ő látja” csak olvasható nézet | – |

Saját magát az admin nem tilthatja ki, nem törölheti, és nem veheti el a saját adminságát (az
amúgy is csak a környezeti változóval változik).

Adatbázis-változás: `users` új oszlopai migrációval — `banned_until TEXT`, `ban_reason TEXT`,
`chat_muted_until TEXT`, `review_blocked INTEGER DEFAULT 0`, `last_login_at TEXT`,
`last_login_ip TEXT`, `deleted_at TEXT`. A `validate_session`, `login`, `set_name` és a socket
események ellenőrzik a kitiltást.

---

## 6. Élő szobák és játékok

### 6.1 Lista

Az összes szoba a `state` singletonból (privát, levelezős, napi feladvány is): kód, név, típus,
állapot (várakozik / folyik / szavazás / vége), tulajdonos, játékosok (online/offline jelzéssel,
robotok fokozattal), megfigyelők száma, időlimit, megtámadás mód, létrehozva, utolsó lépés óta
eltelt idő. Szűrők: típus, állapot, „elakadt”, robotos.

### 6.2 Szoba részletei

- **Tábla élőben** (a megfigyelő nézet újrahasznosítva), pontállás, zsák tartalma, soron lévő
  játékos, időzítő, függő lerakás / szavazás állása (ki szavazott mit), lépéstörténet.
- **Kezek megtekintése** gomb: kifejezett kattintásra mutatja a játékosok kezét — naplózott
  (`view.racks`), és csak befejezés utáni elemzéshez / vitás esethez való.
- **Chat napló** (a `room.chat_messages`), időbélyeggel (ehhez az üzenetekhez időbélyeget kell
  tenni).
- Kapcsolódó tokenek / SID-ek, türelmi időben lévő játékosok hátralévő ideje.

### 6.3 Beavatkozások

| Művelet | Megvalósítás |
|---|---|
| Megfigyelés | normál megfigyelő mód, a 30-as limit alól kivéve, a játékosok listájában nem látszik |
| Rendszerüzenet a szobába | `chat_message` „Rendszer” névvel, kiemelt stílussal |
| Játékos kirúgása | a játékból `disconnected` + a token érvénytelenítése; várakozó szobából eltávolítás |
| Tulajdonjog átadása | `room.transfer_ownership` |
| Kör átugrása | a soron lévőnek passz (`timeout` jellegű lépés, a naplóban „admin” jelöléssel) |
| Időzítő hosszabbítása / szüneteltetése | `room.invalidate_turn_timer` + új időzítő |
| Szavazás lezárása | elfogadás vagy elutasítás kikényszerítése a meglévő challenge függvényekkel |
| Robotlépés újraütemezése | `_schedule_bot_turn` (elakadt robotnál) |
| Mentés | `save_game` megfelelője a tulajdonos nélkül |
| Játék lezárása | „befejezés a jelenlegi állással” (normál `finish_game`, ranglistába számít) **vagy** „érvénytelenítés” (nem számít a ranglistába, ELO nélkül) |
| Szoba feloszlatása | `_disband_active_room` / `_cleanup_room`, opcionális mentéssel, `room_disbanded` a klienseknek indoklással |

Minden beavatkozás után `_emit_all_states` és szükség esetén `_schedule_bot_turn`.

---

## 7. Játékok (archívum)

- Lista a `saved_games`-ből: id, név, státusz (`active` / `finished` / `abandoned` / új:
  `voided`), játékosok és pontjaik, győztes(ek), robotos-e, levelezős-e, megtámadás mód, lépések
  száma, létrehozva / frissítve, van-e megosztási link, van-e elemzés.
- Szűrők: státusz, időszak, felhasználó, robotos, levelezős, „gyanús” (lásd 9.3).
- Részletek: visszajátszás (a meglévő `Replay` modul), lépésnapló táblázatosan (kezekkel), elemzés,
  végeredmény, értékszám-változások.
- Műveletek:
  - **Érvénytelenítés** (`status='voided'`): kikerül a ranglistából, az érintettek ELO-ja
    újraszámolódik (lásd 8.), a kitüntetések nem vesznek el automatikusan (külön opció).
  - Érvénytelenítés visszavonása.
  - Megosztási link visszavonása (`share_token = NULL`).
  - Elemzés újrafuttatása (gyorsítótár törlése + `_run_analysis`).
  - Mentett játék státuszának módosítása (`abandoned` ↔ `active`).
  - Export JSON (állapot + lépések).
  - Végleges törlés (sudo, csak ha nem befejezett ranglistás játék, vagy külön megerősítéssel).
- Tömeges művelet: régi (`abandoned`, X napnál régebbi) mentések törlése előnézettel.

---

## 8. Levelezős játékok

- Lista: játékosok, soron lévő, határidő és hátralévő idő, egymás utáni lejárt körök
  (`timeouts`), utolsó lépés, betöltve-e memóriába.
- Műveletek: határidő meghosszabbítása (órával), a kör azonnali lejáratása (`expire_turn`),
  feladás valaki nevében (`resign`, indoklással), push emlékeztető a soron lévőnek, a játék
  érvénytelenítése, betöltés / kiürítés a memóriából, a `_async_sweeper` utolsó futása és kézi
  indítása.

---

## 9. Ranglista és értékszám

1. Az összes metrika (`LEADERBOARD_METRICS`) a nyilvánossal azonos nézetben, de a minimum
   játékszám nélkül is megtekinthető.
2. **Teljes ELO újraszámolás** a befejezett, nem érvénytelenített, robot nélküli játékokból
   időrendben (`elo.rating_changes`), előnézettel: ki mennyit változna; utána alkalmazás (sudo).
3. **Gyanús minták** (csak jelzés, automatikus büntetés nincs):
   - ugyanaz a két fiók sokszor egymás ellen, egyoldalú eredménnyel (győzelem-tologatás)
   - ugyanarról az IP-ről érkező ellenfelek
   - szokatlanul gyors, sorozatos passzolós játékok
   - kiugróan magas átlagos lépésérték a robot legjobb lépéséhez mérve (az elemzésből: ha valaki
     rendszeresen a legjobb lépést rakja, lehet, hogy segédprogramot használ)
4. Kézi értékszám-módosítás (lásd 5.3), napló sorral.

---

## 10. Szótár

A szótár a játék lelke, ezért ez a legrészletesebb rész.

### 10.1 Szó-vizsgáló

Egy szó beírására a teljes döntési lánc:
- érvényes-e a játékban; zsetonokra bontás és pontérték
- az `affix_checker` levezetése: szótő, előtag, toldalékok, morfológiai címkék
- kockázatos levezetés-e (`needs_attestation`), szerepel-e a `hu_attested.txt`-ben
- szerepel-e a `hu_rejected.txt`-ben, és van-e rá szavazat (ki, mit, mikor; egyenleg a
  küszöbhöz képest)
- benne van-e a robot szókincsében
- javaslatok (`suggest_words`)

### 10.2 Kizárt szavak kezelése

- **Tartós lista** (`dict/hu_rejected.txt`): keresés, lapozás, szó hozzáadása / eltávolítása,
  tömeges hozzáadás (beillesztett lista, előnézettel: melyik nem is érvényes most), letöltés.
  Az írás atomikus (ideiglenes fájl + átnevezés), a memóriában azonnal érvényesül
  (`dictionary` újratöltés vagy `mark_voted_rejected` megfelelője), `rejected_version()` nő.
  Megjegyzés: ez git-ben követett fájl — a panel jelezze, hogy a változás a szerveren él, és adjon
  „Változások letöltése” (diff) gombot a repóba való átvezetéshez.
- **Szavazatokból kizárt szavak**: lista (szó, nem szó / rendes szó szavazatok, ki, mikor),
  szavazatok törlése szavanként, „Visszaengedés” (admin felülbírálás).
- **Admin felülbírálás**: új tábla `word_overrides (word, verdict 'allow'|'reject', admin_user_id,
  reason, created_at)`. Az `allow` akkor is érvényesnek veszi a szót, ha a szavazatok kizárnák (a
  hunspell által nem ismert szót viszont **nem** teszi érvényessé — az külön döntés, lásd 10.4).
- **Szavazat → tartós lista** exportja: a szavazatokból kizárt szavak átvezetése a
  `hu_rejected.txt`-be (a CLAUDE.md TODO-ja).
- A `WORD_REJECT_THRESHOLD` futásidejű módosítása (`app_settings`-ben tárolva, a környezeti
  változó az alapérték).
- Második vélemény sor: véletlen minta a `hu_rejected.txt`-ből újraértékelésre (a CLAUDE.md
  TODO-ja).

### 10.3 Szótár-építő felügyelet

- Összesítő: napi döntések, átnézett / kizárt szavak, aktív bírálók.
- Bírálók listája: döntések száma, „nem szó” arány, egyezés a többiekkel; gyanús bíráló kiemelve
  (pl. > 80% „nem szó” 50 döntés felett).
- Műveletek bírálónként: tiltás, szavazatok visszavonása (5.3).

### 10.4 Saját szavak (opcionális, később)

- `word_additions` tábla: a szótárban nem szereplő, de a játékban elfogadott szavak (pl. új
  szavak). Csak admin adhat hozzá, szavanként indoklással; a robot szókincsébe nem kerül
  automatikusan.

### 10.5 Gyorsítótárak

- `filter_valid` gyorsítótár ürítése, `practice.short_words` újraszámolása, robot szókincs
  újraépítése, `analysis.ANALYSIS_VERSION`-höz hasonló elemzés-gyorsítótár ürítése
  (`game_analysis` törlése). Mindegyik gombbal, a futási idő kijelzésével.

---

## 11. Napi feladvány

- Mai feladvány: tábla, kéz, legjobb lépés, résztvevők, próbálkozások eloszlása, ranglista.
- Előre generált napok előnézete (következő 7 nap, `generate_puzzle(dátum)`), a nehézség
  becslésével (legjobb pontszám).
- Újragenerálás egy napra (csak ha még senki nem próbálkozott, különben figyelmeztetés + sudo).
- Ranglista-bejegyzés törlése (csalás esetén), a `revealed` jelző visszaállítása.
- Archívum: korábbi napok statisztikái.

---

## 12. Moderáció

- **Chat napló**: az összes futó szoba chatje egy helyen, élőben, kereséssel; tiltott szó
  találatok kiemelve. (Opcionálisan a chat üzenetek tartós tárolása `chat_log` táblában, X napig
  megőrizve — adatvédelmi okból alapból kikapcsolva, beállítással.)
- **Tiltott szavak listája**: szoba- és játékosnevekre és chatre; találatnál a név elutasítva, a
  chat üzenet kicsillagozva vagy eldobva (beállítható).
- **Bejelentés funkció** (új, a játékosoknak): a játékban egy játékos / chat üzenet bejelenthető
  („Bejelentés” menüpont), `reports` tábla; az adminnál sor kezelőfelülettel (új / kezelt /
  elutasított, a bejelentett tartalom pillanatképével, gyorslinkkel a felhasználóhoz és a
  szobához). Az adminnak push értesítés új bejelentésről.
- **Nevek átnézése**: legutóbb regisztrált / módosított nevek listája, egy kattintással
  átnevezés.

---

## 13. Kommunikáció

- **Közlemény (banner)**: szöveg (hu + en), típus (info / figyelmeztetés / karbantartás),
  érvényesség kezdete és vége, célcsoport (mindenki / bejelentkezettek / vendégek). Tárolás:
  `announcements` tábla; a kliens betöltéskor (`GET /api/announcements`, nyilvános) és élőben
  (`announcement` Socket.IO esemény) kapja; a felhasználó bezárhatja (a bezárás a
  `localStorage`-ban). Szerkesztés, előnézet, visszavonás.
- **Karbantartási mód**: bekapcsolva nem hozható létre új szoba / levelezős játék / napi
  próbálkozás, a futó játékok folytatódnak; visszaszámlálós banner („A szerver 10 perc múlva
  újraindul”); a vége automatikusan vagy kézzel. Az admin kivétel.
- **Push üzenet**: egy felhasználónak, egy csoportnak vagy mindenkinek; előnézet; a kiküldés
  eredménye (sikeres / sikertelen / törölt feliratkozás).
- **E-mail**: egy felhasználónak (SMTP-n, sablonnal); tömeges e-mail csak megerősítéssel és
  rate limittel.

---

## 14. Statisztika

Grafikonok és táblázatok, időszak-választóval (7 / 30 / 90 / 365 nap), CSV exporttal:
- regisztrációk, DAU / WAU / MAU, megtartás (az X. napon visszatérők aránya)
- játékok száma típus szerint (emberi, robotos, levelezős, napi), átlagos hossz (lépés, idő),
  befejezett / félbehagyott arány
- robot fokozatok népszerűsége, emberek nyerési aránya fokozatonként (a kalibráláshoz:
  összevethető a `LEVEL_STRENGTH`-szel)
- leggyakoribb lerakott szavak, legtöbb pontot érő lépések, bingók száma
- megtámadások: hány lerakás, hány elutasítás
- gyakorló módok használata (szerveroldali: kvíz és betűvadász API hívások száma)
- szótár-építő: döntések naponta, kizárt szavak
- napszakos / heti aktivitás hőtérkép

---

## 15. Biztonság

- **Belépési napló**: sikeres és sikertelen bejelentkezések (`login_events` tábla: user_id vagy
  e-mail, IP, user agent, eredmény, idő), szűrhetően; gyanús minták kiemelve.
- **Rate limiter állapota**: a `rate_limiter` memóriájából a jelenleg korlátozott IP-k és SID-ek,
  egy IP feloldása.
- **IP tiltás**: `ip_bans` tábla (IP vagy CIDR, indoklás, lejárat); a HTTP és a Socket.IO
  kapcsolat elején ellenőrizve.
- **Ellenőrző kódok**: függő regisztrációs kódok (e-mail, lejárat, próbálkozások) — ha nincs
  SMTP, itt látszik a kód (kényelmesebb, mint a konzol); érvénytelenítés.
- **Aktív sessionök** globálisan, tömeges érvénytelenítés (pl. „mindenki kijelentkeztetése a
  SECRET_KEY csere után”).
- **Admin sessionök**: az adminok saját sessionjei külön, „Minden más admin session bezárása”.

---

## 16. Rendszer

- **Konfiguráció** (csak olvasható): a `config.py` értékei és a releváns környezeti változók,
  a titkok maszkolva (`SMTP_PASSWORD`, `SECRET_KEY`, `VAPID_PRIVATE_KEY` → `••••` + beállítva /
  nincs).
- **Adatbázis**: méret, táblánkénti sorszám, utolsó mentés; **mentés letöltése** (SQLite backup
  API-val konzisztens pillanatkép, sudo), ütemezett napi mentés a szerveren (utolsó N megtartva),
  `VACUUM` / `ANALYZE`, takarítás (lejárt kódok, sessionök, `verified_emails`, régi
  `abandoned` mentések) előnézettel.
- **Naplók**: a szerver naplójának utolsó N sora (gyűrűpuffer `logging.Handler`, szint szerinti
  szűrés, élő frissítés); hibák csoportosítva (kivétel típusa + hely, darabszám, utolsó előfordulás).
- **Folyamatok**: háttérfeladatok (`_async_sweeper`, elemzések, push küldés) állapota és utolsó
  futása; futó elemzések sora.
- **Tunnel**: állapot, publikus URL (másolás gomb), újraindítás.
- **Push / VAPID**: nyilvános kulcs, feliratkozások száma, teszt üzenet magamnak.
- **SMTP**: teszt e-mail magamnak.
- **Verzió**: git commit, `asset_version()`, Python és csomagverziók, a tesztek száma (a
  CLAUDE.md-ből).
- **Újraindítás / leállítás** gomb: **nem** készül a panelre (a szerver folyamatot a gazdagép
  kezeli); helyette karbantartási mód.

---

## 17. Beállítások (funkciókapcsolók)

Az `app_settings` táblában, futásidőben módosíthatók, újraindítás nélkül hatnak (a szerver egy
gyorsítótárazott `settings.get(key, default)` segédet használ, a módosításkor frissül):

- regisztráció nyitva / zárva (meghívó kóddal?) · vendég mód engedélyezve
- új szoba létrehozása (mindenki / csak regisztrált / senki — karbantartás)
- robotok: engedélyezve, max. robot szám, alapértelmezett fokozat, gondolkodási idő szorzó
- tipp: alapértelmezett limit
- napi feladvány, levelezős játék, gyakorló módok, szótár-építő: ki / be
- `WORD_REJECT_THRESHOLD`
- türelmi idők (`_DISCONNECT_GRACE_PERIOD`, `_WAITING_OWNER_GRACE_PERIOD`) — korlátok között
- chat: max hossz, rate limit, tiltott szó kezelés
- rate limitek (HTTP és Socket.IO) felülírása
- a megfigyelők max. száma
- chat tartós naplózása (be / ki, megőrzési idő)

Minden kapcsoló mellett: aktuális érték, alapérték, „Visszaállítás alapra”, utolsó módosítás
(ki, mikor). Minden változás a naplóba.

---

## 18. API (javasolt végpontok)

Mind `/api/admin` alatt, JSON, az 1.2 őrével. Példák:

```
GET    /api/admin/overview
GET    /api/admin/search?q=
GET    /api/admin/users?q=&status=&online=&sort=&limit=&offset=
GET    /api/admin/users/<id>
PATCH  /api/admin/users/<id>                  {display_name?, email?}
POST   /api/admin/users/<id>/ban              {until|null, reason}        (sudo)
POST   /api/admin/users/<id>/unban
POST   /api/admin/users/<id>/mute             {until, reason}
POST   /api/admin/users/<id>/logout-all
POST   /api/admin/users/<id>/reset-password                               (sudo)
POST   /api/admin/users/<id>/rating           {rating, reason}            (sudo)
POST   /api/admin/users/<id>/badges           {badge, grant: bool}
POST   /api/admin/users/<id>/reviews/revert   {since?}                    (sudo)
GET    /api/admin/users/<id>/export
DELETE /api/admin/users/<id>                  {mode: anonymize|delete, confirm_name} (sudo)
GET    /api/admin/rooms
GET    /api/admin/rooms/<room_id>
POST   /api/admin/rooms/<room_id>/action      {action, ...}               (kick, message, skip, extend, resolve_vote, end, void, disband)
GET    /api/admin/games?status=&user=&...
GET    /api/admin/games/<id>
POST   /api/admin/games/<id>/void | /unvoid | /unshare | /reanalyze
GET    /api/admin/async
POST   /api/admin/async/<id>/extend | /expire | /resign
POST   /api/admin/ratings/recompute           {dry_run}                   (sudo, ha nem dry_run)
GET    /api/admin/dictionary/word?w=
GET|POST|DELETE /api/admin/dictionary/rejected
GET|POST|DELETE /api/admin/dictionary/overrides
GET    /api/admin/dictionary/reviewers
POST   /api/admin/dictionary/caches/clear     {which}
GET    /api/admin/daily?date= ; POST /api/admin/daily/<date>/regenerate
GET|POST|PATCH|DELETE /api/admin/announcements
POST   /api/admin/maintenance                 {enabled, message, until}
POST   /api/admin/push                        {target, title, body}
POST   /api/admin/email                       {user_id, subject, body}
GET    /api/admin/reports ; PATCH /api/admin/reports/<id>
GET    /api/admin/stats?metric=&range=
GET    /api/admin/security/logins | /rate-limits | /ip-bans | /codes | /sessions
GET    /api/admin/system ; GET /api/admin/system/logs ; GET /api/admin/system/backup (sudo)
GET|PATCH /api/admin/settings
GET    /api/admin/audit?admin=&action=&target=&from=&to=
POST   /api/admin/sudo                        {password}
```

Socket.IO: `admin_subscribe` (belépés az `admin` szobába az élő dashboardhoz), szerver→admin:
`admin_overview`, `admin_room_update`, `admin_alert`, `admin_report`.

A szerverüzenetek magyarul, a kliens `tServer`-rel fordítja (az admin fordításokban).

---

## 19. Fájlok

Új:
- `admin.py` — az admin logika (lekérdezések, műveletek, `audit()`), a Flask kéréstől független,
  így tesztelhető
- `admin_routes.py` (vagy a `routes.py`-ban `admin_bp`) — HTTP réteg, őr
- `settings.py` — futásidejű beállítások (`app_settings` gyorsítótárral)
- `templates/admin.html`, `admin_assets/admin.js`, `admin_assets/admin.css`,
  `admin_assets/admin-i18n.js`
- `tests/test_admin_access.py`, `tests/test_admin_users.py`, `tests/test_admin_rooms.py`,
  `tests/test_admin_dictionary.py`, `tests/test_admin_misc.py`

Módosul: `config.py`, `auth.py` (migrációk, `is_admin_user`, kitiltás ellenőrzése, belépési
napló), `server.py` (kitiltás / némítás / karbantartás ellenőrzése az eseményekben, admin
Socket.IO események, beavatkozások), `routes.py` (`/admin`, `/api/auth/me` `is_admin`,
`/api/announcements`), `templates/sw.js` (kizárás), `static/app.js` (admin gomb, közlemény
banner, bejelentés), `static/i18n-data.js` (csak a nyilvános új szövegek: banner, bejelentés,
kitiltás üzenete), `CLAUDE.md` (dokumentáció).

---

## 20. Tesztek (kötelező)

**Hozzáférés** — ez a legfontosabb, minden admin végpontra paraméterezve:
- nincs session → 404; vendég → 404; nem admin regisztrált → 404; admin → 200
- `ADMIN_EMAILS` üres → minden admin végpont és a `/admin` oldal 404
- a nem admin `/api/auth/me` válaszában nincs `is_admin` kulcs
- a nyilvános `index.html`, `app.js`, `i18n-data.js` nem tartalmaz admin felületet / admin
  fordítási kulcsot; a `/admin/assets/*` nem adminnak 404
- `X-Admin-Request` / `Origin` nélküli POST → elutasítva
- sudo nélküli romboló művelet → 401 `sudo_required`; lejárt sudo; rossz jelszó naplózva
- admin Socket.IO esemény nem admin SID-ről → hatástalan
- a `sw.js` nem gyorsítótárazza a `/admin`-t

**Napló**: minden módosító végpont pontosan egy naplósort ír, a helyes adatokkal; ha a naplózás
hibát dob, a művelet nem történik meg.

**Funkciók**: kitiltás (belépés, socket, futó játék, lejárat), némítás, anonimizálás (a játékok
megmaradnak, a ranglistáról eltűnik a név), ELO újraszámolás = a lépésenkénti számolással azonos
eredmény, érvénytelenítés kiveszi a ranglistából, szoba-beavatkozások után konzisztens állapot és
robotlépés, szótár: kizárás / felülbírálás / visszavonás azonnal hat a `is_valid`-ra és a
`rejected_version()` nő, közlemény megjelenik a nyilvános végponton csak érvényességi időben,
karbantartási mód blokkolja az új szobát, beállítások futásidőben hatnak.

**Kliens**: `test_frontend_consistency.py` mintájára az admin JS szintaxisa és az általa hívott
API útvonalak léteznek; `test_i18n.py` mintájára az admin fordítások teljessége.

---

## 21. Megvalósítási sorrend

1. **Alapok**: `ADMIN_EMAILS`, őr (404), `/admin` oldal + őrzött assetek, `is_admin` a `/me`-ben,
   admin gomb, `admin_audit`, sudo mód, hozzáférési tesztek.
2. **Áttekintés + Felhasználók** (lista, részletek, kitiltás, némítás, kijelentkeztetés, név).
3. **Élő szobák** (lista, részletek, megfigyelés, rendszerüzenet, kirúgás, feloszlatás, lezárás).
4. **Szótár** (szó-vizsgáló, kizárt szavak, felülbírálás, szavazatok, gyorsítótárak).
5. **Játékok, levelezős, ranglista** (érvénytelenítés, ELO újraszámolás).
6. **Kommunikáció** (közlemény, karbantartási mód, push).
7. **Biztonság, rendszer, beállítások** (belépési napló, IP tiltás, mentés, naplók, kapcsolók).
8. **Statisztika, moderáció** (bejelentések, tiltott szavak), napi feladvány.

Minden lépés után: teljes tesztsor zöld, a CLAUDE.md frissítve (fájlstruktúra, végpontok, rate
limitek, tesztszám), és a panel telefonon is használható.

---

## 22. Elfogadási feltételek

- Egy nem admin felhasználó semmilyen módon (UI, forráskód, API, service worker gyorsítótár,
  hibaüzenet) nem tudja meg, hogy admin panel létezik.
- Az admin a telefonjáról is el tud végezni minden műveletet.
- Minden módosítás nyoma ott van az admin naplóban, indoklással.
- A futó játékokban egyetlen admin beavatkozás sem okoz elakadást vagy zsetonvesztést.
- A teljes tesztsor (a régi 1458 + az újak) zöld.
