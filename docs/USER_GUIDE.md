# Használati útmutató

Ez az útmutató a **játékosoknak** szól: hogyan indíts játékot, mit tudnak a robotok, mire jó a gyakorlás és a napi feladvány,
hogyan játssz levelezősen a barátaiddal. A program telepítéséről a [telepítési útmutató](INSTALL.md) szól.

> **Tartalom**
> 1. [Belépés és a lobby](#1-belépés-és-a-lobby)
> 2. [Játék indítása](#2-játék-indítása)
> 3. [A játék képernyő](#3-a-játék-képernyő)
> 4. [Játékszabályok](#4-játékszabályok)
> 5. [Robot ellenfelek és tippek](#5-robot-ellenfelek-és-tippek)
> 6. [Napi feladvány és gyakorlás](#6-napi-feladvány-és-gyakorlás)
> 7. [Levelezős játék](#7-levelezős-játék)
> 8. [Közösség: barátok, megfigyelés, chat](#8-közösség-barátok-megfigyelés-chat)
> 9. [Ranglista, profil, kitüntetések](#9-ranglista-profil-kitüntetések)
> 10. [Visszajátszás, elemzés, megosztás](#10-visszajátszás-elemzés-megosztás)
> 11. [Mentés és újracsatlakozás](#11-mentés-és-újracsatlakozás)
> 12. [Beállítások: téma, nyelv, hang, telepítés](#12-beállítások-téma-nyelv-hang-telepítés)

---

## 1. Belépés és a lobby

A belépő képernyőn három fül van:

- **Bejelentkezés** — e-mail + jelszó.
- **Regisztráció** — három lépés: e-mail cím → 6 számjegyű kód → jelszó (kétszer) és megjelenítendő név. Ha a szerver üzemeltetője nem állított be
  levelezést, a kód magától kitöltődik.
- **Vendég** — csak egy név. Vendégként csatlakozhatsz nyilvános szobához vagy kóddal, megfigyelhetsz és gyakorolhatsz, de **szobát létrehozni, robot ellen játszani,
  barátokat felvenni, levelezős játékot indítani és profilt nézni csak regisztrált fiókkal lehet** (a vendég statisztikája nem mentődik).

A belépés után a **lobby** fogad. A felső sáv középső részén a fülek: **Kezdőlap · Új szoba · Mentett játékok · Barátok · Levelezős · Gyakorlás · Ranglista**
(vendégnek csak a Kezdőlap, a Gyakorlás és a Ranglista látszik). Jobbra: profil, szótár-ellenőrző, hang, nyelv (HU / EN), téma és kijelentkezés.

![A lobby](screenshots/lobby.png)

A **Kezdőlap** három dolgot mutat:

- **Csatlakozás kóddal** — a 6 jegyű szobakód beírása után *Csatlakozás* (játékosként) vagy *Megfigyelés* (nézőként).
- **Nyitott szobák** — a nyilvános, még nem indult szobák; egy kattintás a csatlakozás.
- **Élő játékok** — a folyamatban lévő nyilvános játékok, amelyeket megfigyelhetsz.

Lejjebb a **Korábbi meccsek** listája látszik visszajátszás gombbal.

## 2. Játék indítása

### Új szoba

**Új szoba** fül:

![Új szoba](screenshots/new-room.png)

| Beállítás | Jelentés |
|---|---|
| Szoba neve | a lobbyban ez látszik (legfeljebb 30 karakter) |
| Játékosok száma | 2–4 (a robotok is foglalnak férőhelyet) |
| Körönkénti időlimit | nincs / 1 / 1,5 / 2 (alapérték) / 3 / 5 perc; lejáratkor automatikus passz |
| Robot ellenfelek | 0–3 robot; **egyedül is játszhatsz** ellenük |
| Robot nehézsége | 1–10 fokozat, vagy **Igazodik hozzám** ([5. rész](#5-robot-ellenfelek-és-tippek)) |
| Tippek száma | 0 / 1 / 3 / 5 / 10 — csak akkor kérhető tipp, ha egyetlen emberi játékos van |
| Kihívás mód | a szavak érvényességéről a játékosok szavaznak, nincs szótár-ellenőrzés ([4. rész](#4-játékszabályok)) |
| Privát szoba | nem jelenik meg a listában, csak a 6 jegyű kóddal lehet csatlakozni |

A **Szoba létrehozása** után a **várakozó szobába** kerülsz:

![Várakozó szoba](screenshots/waiting-room.png)

- **Csatlakozási kód** — add meg a barátaidnak, vagy nyomd meg a **Másolás** gombot.
- **Meghívó link** — megosztható `…/?join=KÓD` hivatkozás; megnyitva belépés után a vendég / barát automatikusan csatlakozik a szobához.
- **Barátok meghívása** — a barátlistádból közvetlenül meghívhatsz online barátokat.
- **Játék indítása** — csak a szoba tulajdonosa indíthat (egyedül is lehet).

> Ha csak a telefonodra váltasz át egy üzenetküldő alkalmazásra, hogy elküldd a kódot, a szoba nem szűnik meg: a tulajdonosnak **10 percig**,
> a többieknek **2 percig** van ideje visszatérni.

### Csatlakozás

Nyilvános szobához a lobbyban kattintasz; privát szobához (vagy ha a link a kezedben van) a **6 jegyű kódot** írod be, vagy megnyitod a
`/?join=KÓD` linket. A szoba telt, ha elérte a férőhelyek számát.

## 3. A játék képernyő

![A játék képernyő](screenshots/game.png)

**Bal oldali panel**

- **Pontok** — a játékosok pontszáma (a soron lévő kiemelve; a robotokat kis robot-ikon jelöli), a zsákban maradt zsetonok száma, ki következik és az utolsó lépés.
- **Lerak · Csere · Passz · Visszavon** — a lépés gombjai. A **Lerak** a táblán kijelölt zsetonokat véglegesíti; a **Csere** a kijelölt zsetonokat
  a zsákból cseréli (csak ha a zsákban legalább 7 zseton van); a **Passz** kihagyja a kört; a **Visszavon** visszaveszi a még le nem rakott zsetonokat.
- **Élő előnézet** — már lerakás közben látszik a képzett szavak listája és a pontszám (zölden), vagy a hiba oka (pirosan). A szerver számolja, véglegesítés nélkül.
- **Keverés · Rendez · Zsetonok · Tipp · Elrendezés** — a betűtartó átrendezése; a *Zsetonok* gomb megmutatja, mely betűk lehetnek még a zsákban és az ellenfelek kezében;
  a *Tipp* a három legjobb lépést mutatja; az *Elrendezés* a betűtartót a tábla jobb oldalára vagy alá teszi.
- **Lépések** — a lépéstörténet (lenyitható); **Chat** — üzenetküldés a szoba játékosainak.

**Zsetonok lerakása**: húzd a betűt a mezőre (drag & drop), vagy koppints a betűre, majd a mezőre. A foglalt mezők húzás közben csíkozottak. A **joker** (üres zseton)
lerakásakor a program megkérdezi, melyik betűt helyettesítse. A **Lerak** gombbal vagy az `Enter` billentyűvel véglegesíted a lépést.

**Gyorsbillentyűk** (játék közben, ha nincs nyitva ablak): `Enter` lerak · `Esc` visszavon · `Backspace` vagy `Ctrl+Z` az utoljára lerakott betű visszavétele · `S` keverés · `R` rendezés.

### Tipp

Ha egyedül játszol robot ellen, a **Tipp** gomb megmutatja a három legjobb lépést; az **Elhelyez** a táblára teszi (de te rakod le, azaz te hagyod jóvá).
A tippek száma szobánként korlátozott (a gomb mutatja: `Tipp (2)`).

![Tipp ablak](screenshots/hint.png)

### Telefonon és tableten

Álló nézetben a tábla felül van, alatta egy görgethető panel a gombokkal; a tábla **csípő mozdulattal nagyítható**. Fekvő telefonon a betűtartó a tábla mellett áll.

![Telefonos nézet](screenshots/mobile.png)

### Sötét téma

A téma automatikusan követi a rendszerbeállítást (`prefers-color-scheme`), a felső sáv hold / nap gombjával kézzel váltható, és megmarad.

![Sötét téma](screenshots/game-dark.png)

## 4. Játékszabályok

- A játékosok felváltva raknak le betűket a **15×15-ös táblára**; mindenki **7 zsetonnal** kezd.
- Az **első szónak a középső mezőt** kell fednie, és legalább 2 zsetonból áll. Minden további lerakásnak csatlakoznia kell a meglévő betűkhöz.
- A zsetonoknak **egy sorban vagy oszlopban**, hézag nélkül (a már lent lévő betűk átugorhatók) kell állniuk. A keresztben keletkező szavaknak is érvényesnek kell lenniük.
- **Szótár**: a lerakott szavakat a beágyazott magyar szótár ellenőrzi (szótári szavak és ragozott alakjaik). Tulajdonnevek, rövidítések, betűnevek és a nyelvtanilag
  lehetséges, de értelmetlen alakok (FALIM, ÉJÉK…) nem érvényesek.
- **Pontozás**: a zsetonok értéke a betű alján látszik. **Dupla / tripla betű** (DL / TL) a rá rakott betű értékét, **dupla / tripla szó** (DW / TW) a teljes szó értékét szorozza
  — csak az **újonnan lerakott** zsetonokra számít (a középső csillag mező dupla szó). Ha mind a **7 zsetonod** lent van egy lépésben (**bingó**), **+50 pont** jár.
- **Joker**: két üres zseton van, bármelyik betűt helyettesítheti, értéke 0.
- **Csere / passz**: a kör helyett cserélhetsz zsetonokat (ha a zsákban legalább 7 zseton van) vagy passzolhatsz.
- **A játék vége**: ha valaki kirakja az összes zsetonját és a zsák üres, vagy **6 egymást követő pont nélküli kör** után (passz, csere és elutasított lerakás is számít;
  a pontot érő lerakás nullázza a számlálót). A kézben maradt zsetonok értékét levonják; aki kiürítette a kezét, megkapja az ellenfelek maradék zsetonjainak értékét.
  **Egyenlő pontnál döntetlen**: mindenki győztes.

### A magyar zsetonkészlet (100 zseton)

| Érték | Zsetonok (darab) |
|---|---|
| 0 | joker (2) |
| 1 | A (6), E (6), K (6), T (5), Á (4), L (4), N (4), R (4), I (3), M (3), O (3), S (3) |
| 2 | B (3), D (3), G (3), Ó (3) |
| 3 | É (3), H (2), SZ (2), V (2) |
| 4 | F (2), GY (2), J (2), Ö (2), P (2), U (2), Ü (2), Z (2) |
| 5 | C (1), Í (1), NY (1) |
| 7 | CS (1), Ő (1), Ú (1), Ű (1) |
| 8 | LY (1), ZS (1) |
| 10 | TY (1) |

### Kétjegyű betűk (SZ, CS, GY, LY, NY, TY, ZS)

A kétjegyű betű **egyetlen zseton**. Csak a saját zsetonjával rakható ki: két külön zsetonból (**S + Z**, **C + S**, **Z + S**) nem. A szótári szó zsetonokra bontása a
magyar szabályok szerint történik (pl. KÉSZSÉG = K É S **ZS** É G).

### Kihívás mód (megtámadás)

Ha a szobánál bekapcsoltad a **Kihívás módot**, a szavak érvényességét a **játékosok** döntik el, a program **nem ellenőriz szótárral**:

- **2 játékos**: a másik játékos látja a lerakott szavakat, és **Elfogad** vagy **Elutasít**. Elutasításkor a lerakás visszavonódik, és a betűk visszakerülnek a lerakóhoz.
  Ha 30 másodpercen belül nem válaszol, a lerakás elfogadott. (A lerakó addig **visszavonhatja** a lerakását, amíg senki sem szavazott.)
- **3–4 játékos**: 30 másodpercig bárki **Megtámadhat** vagy **Elfogadhat**. Megtámadáskor szavazás indul (újabb 30 mp): a lerakó és a megtámadó nem szavaz, a többiek
  **Elfogad** / **Elutasít**; **legalább 50% elfogadással** a szó marad (döntetlen = elfogadva), különben visszavonódik. A nem szavazó elfogadónak számít. Nincs büntetés a megtámadónak.
- A lerakott szavakra kattintva új lapon keresést indíthatsz a szó jelentésére (a magyar értelmező szótár fókuszával).
- Robot ellen egyedül játszva a kihívás mód hatástalan (nincs ki megtámadja), a szótár dönt; vegyes (emberek + robotok) játékban a robotok nem szavaznak.

## 5. Robot ellenfelek és tippek

Új szoba létrehozásakor **1–3 robot** kérhető. A robot a magyar szótár tőszavait és a gyakori ragozott alakokat használja (összesen kb. 290 000 alakot), és a
jokert is használja. A robot nem „csal”: a táblát és a saját kezét látja, mint egy ember.

| Fokozat | Név | Mért erő (pont / kör) |
|---|---|---|
| 1 – 3 | újonc · kezdő · könnyű | 4,3 · 6,1 · 7,4 |
| 4 – 7 | mérsékelt · alkalmi · közepes · ügyes | 10,1 · 12,7 · 15,7 · 18,4 |
| 8 – 10 | haladó · erős · mester | 21,2 · 24,6 · 27,0 |
| **Igazodik hozzám** | az utolsó ~6 köröd átlagához állítja az erejét | — |

- Az alsó fokozatok célpontszámot sorsolnak, és a hozzá legközelebbi lépést rakják le (az 1. fokozat néha „nem talál” lépést, és cserél vagy passzol); a felsők a pont és a
  kézben maradó zsetonok értéke alapján a legjobb lépést választják, egyre kisebb zajjal.
- **Igazodik hozzám**: ha jól játszol, a robot is erősebb lesz, ha elakadsz, gyengébb.
- A robotos játékok a profil statisztikájában szerepelnek, de **a ranglistán és az értékszámban nem**.
- Emberi játékos vagy néző nélkül a robotok nem játszanak egymás ellen.
- A tippek számát a szoba létrehozásakor állíthatod (0 / 1 / 3 / 5 / 10, alapérték 3).

## 6. Napi feladvány és gyakorlás

**Gyakorlás** fül: a tetején a statisztika-sáv (napi sorozat, mai gyakorlat, kvíz-pontosság) és a kiemelt **Napi feladvány** kártya; alatta a gyakorló módok.

![Gyakorlás](screenshots/practice.png)

### Napi feladvány

Naponta (magyar idő szerint) **mindenkinek ugyanaz** a táblaállás és ugyanaz a hét zseton; a cél a **legtöbb pontot érő lépés** megtalálása.

![Napi feladvány](screenshots/daily-puzzle.png)

- Többször próbálkozhatsz; a legjobb számít (holtversenynél a kevesebb próbálkozás, majd a korábbi idő). Az élő előnézet segít.
- A **Megoldás** gomb megmutatja a legjobb lépést, de utána már nem kerülhetsz a napi ranglistára.
- Vendégként is játszhatsz, de csak regisztrált játékos kerül a ranglistára. A tegnapi megoldás is megnézhető.

### Szókvíz

10 vagy 20 kérdés: **érvényes-e a szó?** (← nem érvényes, → érvényes, `Enter` / szóköz: következő). A szót játékbeli zsetonokkal látod; a válasz után megtudod, miért
(pontérték, javaslatok), és sorozatot is gyűjthetsz. Módok: *vegyes*, *csak két- vagy háromzsetonos szavak*, és a **hosszú–rövid csapdák** (a↔á, o↔ó, ö↔ő, u↔ú…: az érvénytelen szó
egy érvényes szó egyetlen magánhangzójának hosszúságcseréjével készül).

![Szókvíz](screenshots/quiz.png)

### Betűvadász

Hét zsetonból építs minél több érvényes szót (koppintással vagy gépeléssel; a pont a zsetonok értéke, mind a hét zseton +50). Időkorlát nélkül, 60 mp-cel vagy 2 perccel; van
**tipp**, és a listánkon nem szereplő, de érvényes szavak **bónuszként** számítanak. A végén látod a kimaradt szavakat és az eredményed a lehetséges pontokhoz képest.

![Betűvadász](screenshots/hunt.png)

### Bingó-edző

A hét zsetonodból **biztosan kirakható egy hét zsetonos szó** — találd meg! Tipp (a szó kezdőbetűi), feladás után megmutatja a megoldást; a bingók sorozata számolódik.

### Hibáim, Szólisták, Szótár-építő

- **Hibáim** — a kvízben eltévesztett szavak paklija (az eszközödön tárolva). Egy szó akkor kerül ki, ha kétszer egymás után helyesen válaszolsz rá.
- **Szólisták** — az összes érvényes két- és háromzsetonos szó pontértékkel, kereséssel, kezdőbetű-szűrővel és rendezéssel; valamint a 100 zseton értéke és darabszáma.
- **Szótár-építő** — véletlen szavak: **Rendes szó** / **Nem szó** / **Nem tudom**. A „Nem szó” döntéseddel a szó kikerülhet a játék szótárából (alapértelmezésben egyetlen
  szavazat elég), ezért csak bejelentkezve érhető el; a legutóbbi döntés visszavonható.

## 7. Levelezős játék

**Levelezős** fül: barátaiddal (1–3 fő) **órák vagy napok alatt** lépkedhettek — nem kell egyszerre online lennetek.

- **Új játék** (az alsó lap): név, gondolkodási idő lépésenként (**24 óra / 48 óra / 3 nap / 7 nap**) és a barátok kijelölése. A játék azonnal elindul; a barátok a saját Levelezős fülükön
  látják, és értesítést kapnak.
- A listában elöl áll az a játék, ahol te jössz; a kártyán látszik a pontállás, a „Te jössz!” jelvény és a hátralévő idő sávja (sürgős, ha kevés van vissza).
- A játékból bármikor kiléphetsz, a játék megmarad; a szerver minden lépést elment, újraindítás után is folytatható.
- Ha a határidőig nem lépsz, **automatikusan passzolsz**; **három egymást követő lejárt határidő** után a játékot feladottnak tekintjük. A **Játék feladása** gombbal te is feladhatod:
  a játék véget ér, és nem lehetsz a győztes.
- Értesítés: **Web Push** (a profilban kapcsolható) és az alkalmazáson belüli felugró üzenet. Levelezős játékban nincs robot, kihívás mód és tipp.

## 8. Közösség: barátok, megfigyelés, chat

- **Barátok** fül: barátnak jelölés (kérés küldése / elfogadása / elutasítása), online jelző, és a várakozó szobába való közvetlen meghívás.
- **Megfigyelés**: a lobby *Élő játékok* listájából bármelyik nyilvános, folyamatban lévő játékot nézheted; privát játékot a 6 jegyű kóddal (*Megfigyelés* gomb a kódmező mellett, vagy `/?spectate=KÓD` link).
  A megfigyelő látja a táblát, a pontokat, a lépéseket és a chatet, de **a kezeket nem**, nem léphet és nem chatelhet. A játékosok látják a megfigyelők számát; alapértelmezetten legfeljebb 30 néző lehet szobánként.
- **Chat**: játék közben a panel alján; üzenetek legfeljebb 200 karakteresek, és gyors egymásutánban korlátozottak. Telefonon, ha a chat nem látszik, a másik játékos üzenete értesítésként is megjelenik.
- **Bejelentés**: a chat üzenetek és a játékosok bejelenthetők a moderátornak (admin).

## 9. Ranglista, profil, kitüntetések

**Ranglista** fül: értékszám (ELO), győzelmek, nyerési arány, átlagpont és legjobb játék szerint. Csak a **regisztrált**, **robot nélküli**, befejezett játékok számítanak; az értékszám, a nyerési arány és az
átlagpont rangsorához legalább **3 értékelt játék** kell. A saját helyezésed akkor is látszik, ha nem vagy a top 50-ben.

![Ranglista](screenshots/leaderboard.png)

**Értékszám**: mindenki 1200-ról indul; a játék végén minden játékospárra külön számolódik a változás (a nagyobb pontszám nyer, az egyenlő döntetlen), az első 10 értékelt játékban gyorsabban (K = 32),
utána lassabban (K = 20). Csak legalább két regisztrált játékos közti, robot nélküli játék számít; aki feladja a játékot, pontszámától függetlenül veszít.

**Profil** (a felső sáv ikonja): játszott játékok, győzelmek, nyerési arány, átlagpont, értékszám, **kitüntetések**, beállítások (a betűtartó helye, értesítés) és a játékelőzmények értékszám-változással.

![Profil](screenshots/profile.png)

**Kitüntetések**: Első játék · Első győzelem · Bingó · Százas (100+ pontos lépés) · Szóóriás (8+ zsetonos szó) · Jokeres · Pontvadász (300+ pontos játék) · Robotverő (győzelem 8–10. fokozatú robot ellen) ·
Tapasztalt győztes (10 győzelem) · Törzsjátékos (25 játék) · Feladványmester (a napi feladvány legjobb lépése). Új kitüntetésről játék végén értesít a játék.

## 10. Visszajátszás, elemzés, megosztás

A befejezett játékok a profil **előzményeiből** (és a játék végén) lépésről lépésre visszanézhetők (**Előző / Következő**), az utolsó lépés betűi kiemelve.

- **Elemzés indítása**: lépésenként megmutatja a **legjobb lehetséges lépést**, a kint maradt pontokat és a játékosonkénti hatékonyságot (a *Legjobb lépés mutatása* a lépés előtti táblán, halvány zsetonokkal mutatja).
  Az elemzés a háttérben számolódik, és a szerver eltárolja. Csak olyan játékra érhető el, amelynek lépésnaplója a kezeket is őrzi (az elemzés bevezetése óta játszottak).
- **Megosztás**: a *Megosztás* gomb linket készít (`/?replay=TOKEN`), amely **bejelentkezés nélkül** is megnyitja a visszajátszást a végeredménnyel.

![Visszajátszás elemzéssel](screenshots/replay-analysis.png)

## 11. Mentés és újracsatlakozás

- **Mentés**: a szoba tulajdonosa a kilépés menüből *Mentés és kilépés*-t választhat; a mentett játék a **Mentett játékok** fülön folytatható: a tulajdonos várakozó szobát hoz létre, ahová az eredeti
  játékosok csatlakoznak (a mentésben szereplő névvel és fiókkal); aki nincs ott induláskor, menet közben is becsatlakozhat a szoba kódjával.
  Ha a tulajdonos kapcsolata véglegesen megszakad, a szerver automatikusan ment.
- **Újracsatlakozás**: ha megszakad a kapcsolatod (átvált az alkalmazás, kimegy a Wi-Fi, újratöltöd az oldalt), a helyed **2 percig** megmarad, a játék átugorja a köreidet; az oldal újranyitásakor automatikusan visszatérsz.
  Kilépés (menü) után is visszacsatlakozhatsz, amíg a játék tart.

## 12. Beállítások: téma, nyelv, hang, telepítés

- **Nyelv**: magyar és angol (a felső sáv **HU / EN** gombja); az első indításkor a böngésző nyelve dönt. A szótár és a szóellenőrzés mindig magyar.
- **Téma**: világos / sötét, automatikus felismeréssel.
- **Hang**: a hangszóró ikon a hangerőt és a kategóriákat állítja (betűlerakás, szavazás, a szavazás eredménye, „te következel”, chat, játékesemények); a hangok szintetizáltak, külső fájl nélkül.
- **Betűtartó helye**: a profilban vagy a játék *Elrendezés* gombjával (jobb oldalt / alul; asztali gépen és fekvő tableten hat).
- **Értesítés** (Web Push): a profilban kapcsolható; értesít, ha rád kerül a sor, miközben nem nézed a játékot. HTTPS kell hozzá (pl. Cloudflare Tunnel), iPhone-on előbb a Főképernyőhöz kell adni az alkalmazást.
- **Telepítés alkalmazásként (PWA)**: a lobby felső sávjában a **Telepítés** gomb (iOS Safarin: *Megosztás → Főképernyőhöz adás*). Kapcsolat nélkül is elindul a felület, és érthető üzenetet mutat.
- **Szótár-ellenőrző**: a felső sáv könyv ikonja — beírsz egy szót, és megtudod, érvényes-e, hány pontot ér, hogyan bomlik zsetonokra, és javaslatot kapsz.

---

Tovább: [Telepítési útmutató](INSTALL.md) · [Architektúra](ARCHITECTURE.md) · [Főoldal](../README.md)
