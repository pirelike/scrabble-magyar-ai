# A robot és egy külső Scrabble motor párharca

Ez a dokumentum azt írja le, **hogyan** mérjük meg a legerősebb (10. fokozatú) robotunk erejét egy tőle független
Scrabble motorhoz képest, és — a mérések után — mit kaptunk. A protokoll a mérés **előtt** lett rögzítve és commitolva
(lásd „Protokoll”); az eredmények külön szakaszban, a protokoll módosítása nélkül követik.

## 1. A kérdés

*Mennyivel erősebb (vagy gyengébb) a 10. fokozatú robot egy valódi, független motornál, ugyanazokkal a szabályokkal és
ugyanazzal a szótárral, és a motor erejéből mennyi az értékelés, a szimuláció és a végjáték-kereső érdeme?*

## 2. A külső motor

* **Motor:** a crates.io `scrabble` 0.1.0 csomag (szerző: Pranav Gundu, MIT licenc, <https://github.com/pranavgundu/scrabble>):
  GADDAG-os lépésgenerátor, maradék-értékelés, Monte-Carlo szimuláció (közös véletlen számokkal, szekvenciális
  felezéssel) és pontos (alfa-béta) végjáték-kereső. Független megvalósítás: semmi közös kódja nincs a robotunkkal.
* **Módosítás:** az eredeti legfeljebb 30 betűs ábécét bír (u32 maszkok), a magyar 38 betűs (SZ, CS, GY, LY, NY, TY, ZS
  egy-egy zseton). A vendorolt másolat (`third_party/pg-scrabble`, csomagnév `pg-scrabble`) minden betűmaszkot 64 bitesre
  szélesít (legfeljebb 62 betű). A változtatások teljes listája: `third_party/pg-scrabble/PATCHES.md`. A 30 betűs
  ábécékre a viselkedés **bitre azonos** az eredetivel (differenciális ellenőrzés: ~5,1 millió lépés, végjátékok,
  szimulátor), az eredeti 167 tesztje és 29 új teszt (köztük a magyar, 38 betűs ábécén futó gyors-lassú generátor
  összevetés) zöld: `cargo test -p pg-scrabble --all-features`.
* **Nem része az alap fordításnak:** `cargo build` a szervert fordítja; a párharc a `--features engine-duel` kapcsolóval.

## 3. Egyenlő feltételek

| Terület | Megoldás |
|---|---|
| Játékvezető | A **mi** `Game`-ünk (szótár, kétjegyű betűk szabálya, pontozás, játék vége). A motor csak javasol; ha bármely lépését elutasítja a játékvezető, a futás **megáll** (nincs csendes passz, hiba sosem győzelem). |
| Szabályok | Azonos tábla (225 mező egyenként ellenőrizve), zsetonértékek és -darabszámok, 2 joker, 7 zsetonos kéz, 50 pontos bingó, csere legalább 7 zsetonos zsáknál, 6 pont nélküli kör a vége, a kézben maradt zsetonok levonása, a kiürítő megkapja az ellenfél zsetonjainak értékét. A `tests/engine_duel.rs` ezeket teszteli. |
| Szótár | A robot szókincse (290 514 szó) **minden jogos zsetonbontásban** (290 640 zsetonsor). Hasított kétjegyű betű (S+Z, C+S, Z+S külön zsetonnal) a szótárban nincs, így a motor a szabályt külön kód nélkül betartja. |
| A robot szűrése | Élesben a robot a keresztszavait a teljes szótárból is veheti (a lépései ~7%-a ilyen), amit a motor nem tud kirakni. A párharcban a robot is a szókincsre korlátozott (`bot:10`), így a két oldal ugyanabból a szóhalmazból épít. Az éles működést (`bot:10:full`) külön érzékenységi mérés adja. |
| Lépéshalmaz | A robot és a motor lépésgenerátora **független**; a `crosscheck` önjáték-állásokon összeveti őket: azonos lépéshalmaz, azonos pontszám, és a játékvezető minden motorlépést érvényesnek talál (0 eltérés). |
| Információ | Mindkét oldal csak a `View`-t látja: tábla, saját kéz, pontok, a zsák és az ellenfél kezének **mérete**. A nem látott zsetonok készlete a táblából és a saját kézből számolt; az ellenfél kezét a motor sosem olvassa (teszt: a számolt készlet = zsák + ellenfél keze). |
| Véletlen | Minden véletlen magból (pár, fél, lépés) származik; a futás megismételhető, és `-j 1` / `-j 4` mellett bitre azonos (ellenőrizve szimulációs és végjátékos motorral is). A cserénél a zsák újrakeverése is magból történik. |
| Ha nincs lépés | Mindkét oldalon ugyanaz: csere, ha a zsákban legalább 7 zseton van, különben passz. |
| Tükrözött párok | Egy pár két játék **ugyanabból a zsákból**, felcserélt kezdéssel (a kezdő ülés mindkét félben ugyanazt a kezet kapja). A mintaegység a pár; a statisztika a pár-átlagokon fut. |
| Számítási teher | **Nem egyenlő**, és nem is állítjuk annak: a robot lépése ~5 ms, a motor szimulációja tizedmásodpercektől másodpercekig tart. A jelentés a játékonkénti processzoridőt oldalanként kiírja. |

## 4. Az oldalak

`engine_duel duel A B` — az eredmény mindig **A − B** pontkülönbség (pozitív: az A jobb).

| Leírás | Jelentés |
|---|---|
| `bot:10` | a robot 10. fokozata (a legnagyobb értékelésű lépés, zaj nélkül), a szókincsre korlátozva |
| `bot:10:full` | a robot az éles működése szerint |
| `bot:9`, `bot:8` | alacsonyabb fokozatok (a pontok „fokozatra váltásához”) |
| `bot:greedy` | csak a pontszám számít (kontroll) |
| `eng:greedy` | a motor, csak a pontszám |
| `eng:stock` | a motor gyári maradék-értékelése (magyar ábécén gyakorlatilag csak a joker és az ismétlés számít) |
| `eng:leaves` | a motor az **általunk tanított** magyar maradék-értékekkel, egy lépéses (statikus) kereséssel |
| `…+eg` | + pontos végjáték-kereső, ha a zsák üres és legfeljebb 9 zseton van játékban (300 000 csomópontos keret; csak a **pontos** eredményt használja) |
| `…:sim-fast` / `:sim` / `:sim-deep` | + Monte-Carlo szimuláció a legjobb jelöltekre (12×20×2 / 20×40×2–3 / 30×120×3 jelölt×iteráció×lépés; lásd `SimOptions`) |

## 5. A motor magyar maradék-értékelése

A motorhoz nem jár magyar értékelés, és a gyári a magyar ábécén gyakorlatilag mohó. Ezért **önjátékból tanítottuk**:

* **Modell:** a kézben maradó zsetonok (a „maradék”) értéke = Σ zsetononkénti érték + Σ párok szinergiája (azonos
  betűk ismétlődése is); 820 jellemző.
* **Adat:** a motor önjátéka (a robot kódját és adatát nem használja), középjáték (a zsákban legalább 7 zseton),
  ε = 0,1 felfedezés (a lépés 10%-ban az öt legjobb közül véletlen). A kimenet: a lerakás utáni állástól a játék
  végéig elért pontkülönbség.
* **Körök:** 0. kör: 400 ezer játék mohó szabállyal; 1. kör: 400 ezer játék a 0. kör értékeivel; 2. kör: 600 ezer
  játék az 1. kör értékeivel. A végleges modell az 1. és 2. kör egyesített normálegyenlete (21,3 millió tanító és
  2,4 millió kivárt minta), a gerinc-büntetés a **kivárt halmazon** választott (λ = 1000; a hiba λ-tól gyakorlatilag
  független). A kivárt halmaz a játékok 10%-a (játék szerint elkülönítve); a modell a kimenet szórásnégyzetének 3,4%-át
  magyarázza (a játék kimenetele túlnyomórészt a lerakások pontja, a maradék kis rész).
* **Szétválasztott magok:** a tanító játékok magjai ≥ 10⁹, a kiértékelőké < 10⁶ (a kód ellenőrzi). A modell fájlja:
  `docs/engine_duel/leaves-hu.json`.
* **Fejlesztői és mérési magok:** a hangolás és a próbák a 90 000 alatti, illetve 90 001–99 999 pármagokon futottak; a
  **végleges mérés** a 100 001-től induló magokat használja, amelyeken addig semmi sem futott.

## 6. Protokoll (a mérés előtt rögzítve)

* **Elsődleges mutató:** a `eng:leaves:sim+eg` és a `bot:10` párharcában az átlagos pár-pontkülönbség (A − B), 400 pár
  (800 játék), pármagok 100 001–100 400.
* **Másodlagos mutatók:** a győzelmi arány (döntetlen = ½), a pontszám egy körre, a bingók, a cserék, a végjáték-hozam
  (a különbség változása a zsák kiürülésétől), a processzoridő.
* **A „létra”** (ugyanazokon a magokon, mind a `bot:10` ellen, 400 pár): `eng:greedy`, `eng:stock`, `eng:leaves`,
  `eng:leaves+eg`, `eng:leaves:sim-fast`, `eng:leaves:sim-fast+eg`, `eng:leaves:sim+eg`; `eng:leaves:sim-deep+eg` a
  számítási teher miatt 100 páron (100 001–100 100). A különbségek a hozzájárulásokat adják: tanult maradék-érték,
  végjáték-kereső, szimuláció.
* **Kontrollok** (ugyanazokon a magokon): `bot:10` — `bot:10` és `bot:greedy` — `eng:greedy` (a várt érték 0, a
  keret torzítatlanságát igazolja), `bot:10` — `bot:9` (a pontok „fokozatra váltása”).
* **Érzékenység:** `eng:leaves:sim+eg` — `bot:10:full` (a robot éles működésével).
* **Statisztika:** pár-átlagok, SE = sd/√N, 95%-os Student-féle és bootstrap (10 000 újramintavétel) intervallum,
  kétoldali p-érték, győzelmi arány, előjelpróba. Csak az elsődleges mutató és a nullkontrollok kapnak formális
  tesztet; a létra leíró jellegű. A mérést **nem** bővítjük utólag (nincs „kukucskálás”).
* **Hibák:** egyetlen megszakított vagy hibás játék sem engedhető meg; ilyenkor a futást javítás után elölről kell
  indítani.

## 7. Eredmények

*(a mérés után kerül ide)*
