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
| A robot szűrése | Élesben a robot a keresztszavait a teljes szótárból is veheti (a generált lépésjelöltjeinek ~7%-a ilyen), amit a motor nem tud kirakni. A párharcban a robot is a szókincsre korlátozott (`bot:10`), így a két oldal ugyanabból a szóhalmazból épít. Az éles működést (`bot:10:full`) külön érzékenységi mérés adja. |
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

A mérés a 6. szakasz protokollja szerint futott (pármagok 100 001-től, a protokoll és a leave-táblázat a mérés előtt
commitolva). Egyetlen játék sem szakadt meg, a futás hibátlanul fejeződött be. A nyers naplók:
`docs/engine_duel/duel-final.jsonl.gz` (8600 játék) és `docs/engine_duel/extra-strict-vs-full.jsonl.gz`; a teljes
jelentés: `docs/engine_duel/report-final.md`. Minden érték **A − B pontkülönbség játékonként**, a mintaegység a
tükrözött pár; SE = a pár-átlagok szórása / √párok.

### 7.1 Az elsődleges mutató

**A tanított értékelésű, szimulációs és végjáték-keresős motor (`eng:leaves:sim+eg`) a 10. fokozatú robotot (`bot:10`)
+44,6 ± 2,6 ponttal (95%-os CI: +39,5 … +49,7; p < 0,001) veri játékonként, 71,1%-os nyerési aránnyal** (400 pár,
324 pár a motornak, 76 a robotnak). Azonos szabályok és azonos szószedet mellett.

### 7.2 A „létra”: ugyanaz a robot, egyre erősebb motor

Mind a `bot:10` ellen, ugyanazokon a magokon (A = a motor oldala).

| Motor (A) | Párok | A − B | SE | 95% CI | A nyerési aránya | A processzoridő / játék |
|---|---:|---:|---:|---|---:|---:|
| `eng:greedy` (csak pont) | 400 | −12,0 | 2,35 | [−16,6; −7,3] | 45,3% | 4 ms |
| `eng:stock` (gyári értékelés) | 400 | −0,2 | 2,50 | [−5,1; +4,7] | 50,9% | 8 ms |
| `eng:leaves` (tanult maradék-érték) | 400 | **+16,0** | 2,57 | [+11,0; +21,1] | 57,7% | 8 ms |
| `eng:leaves+eg` (+ végjáték-kereső) | 400 | +20,1 | 2,62 | [+15,0; +25,2] | 59,6% | 312 ms |
| `eng:leaves:sim-fast` (+ gyors szimuláció) | 400 | +33,4 | 2,55 | [+28,4; +38,4] | 66,2% | 4,4 s |
| `eng:leaves:sim-fast+eg` | 400 | +38,2 | 2,52 | [+33,3; +43,2] | 68,4% | 4,8 s |
| **`eng:leaves:sim+eg`** (alap szimuláció) | 400 | **+44,6** | 2,60 | [+39,5; +49,7] | 71,1% | 22,1 s |
| `eng:leaves:sim-deep+eg` (mély szimuláció) | 100 | +53,4 | 4,58 | [+44,4; +62,5] | 80,2% | 172,6 s |

A `bot:10` lépésenként ~5 ms, játékonként ~130 ms processzoridőt használ. A két legalsó sor tehát a számítási teher
szempontjából is összevethető (a statikus motor gyorsabb a robotnál), a szimulációs sorok **nem**: ott a motor 30–1300×
annyi processzoridőt kap.

### 7.3 Mi mennyit ad hozzá (páros különbségek, ugyanazokon a magokon)

| Hozzájárulás | Különbség | SE |
|---|---:|---:|
| Tanult maradék-érték a gyári értékeléshez képest (`leaves` − `stock`) | +16,2 | 3,4 |
| Tanult maradék-érték a mohó játékhoz képest (`leaves` − `greedy`) | +28,0 | 3,2 |
| Pontos végjáték-kereső, statikus keresés mellett | +4,1 | 0,3 |
| Pontos végjáték-kereső, gyors szimuláció mellett | +4,9 | 0,3 |
| Gyors szimuláció a statikus kereséshez képest | +17,3 | 3,3 |
| Alap szimuláció a gyorshoz képest (végjátékkal) | +6,4 | 3,1 |
| Mély szimuláció az alaphoz képest (végjátékkal, 100 pár) | +3,3 | 4,6 (nem szignifikáns) |

A végjáték-kereső hozama azért ilyen pontosan mérhető, mert csak a zsák kiürülése után különbözik a két játék.

### 7.4 Kontrollok (ugyanazokon a magokon)

| Páros | A − B | SE | p | Értelmezés |
|---|---:|---:|---:|---|
| `bot:10` – `bot:10` | +1,5 | 2,19 | 0,51 | nulla, a keret torzítatlan |
| `bot:greedy` – `eng:greedy` | +2,5 | 2,07 | 0,23 | nulla, a két generátor és a pontozás azonos |
| `bot:10` – `bot:9` | +24,9 | 2,73 | <0,001 | a 9. és 10. fokozat távolsága (a fejlesztői magokon +25,9) |

### 7.5 Érzékenység: a robot éles működése (utólagos, nem előre rögzített elemzés)

Élesben a robot a keresztszavait a **teljes szótárból** is veheti, a párharcban viszont a szókincsére korlátozott. A
motor szótára a szókincs, így az éles robot olyan szavakat is rakhat, amelyeket a motor nem.

| Páros | Párok | A − B | SE |
|---|---:|---:|---:|
| `eng:leaves:sim+eg` – **`bot:10:full`** (éles robot) | 200 | **+14,2** | 3,68 |
| `bot:10` (szűrt) – `bot:10:full` (éles) | 400 | −20,8 | 2,46 |

Az éles robot **~21 ponttal erősebb** a szűrtnél (a generált lépésjelöltek kb. 7%-a tartalmaz szókincsen kívüli keresztszót), és
ezzel a motor előnye +44,6-ról +14,2-re olvad (a két hatás nem pontosan additív: a +44,6 és a −20,8 különbsége +23,8 volna; a +14,2 SE-je 3,7, és csak az első 200 pár adja). Ez nem azt jelenti, hogy a robot algoritmusa jobb: a szókincs bővebb
szóhalmazt ad a robotnak. A motor azonos szótárral (a teljes szótár minden alakja nem sorolható fel) valószínűleg még
erősebb volna; ezt nem mértük. A 7.2. szakasz számai a **közös szószedetű**, egyenlő feltételű összevetést adják, a
fenti táblázat a robotot úgy, ahogy élesben fut.

### 7.6 Hol veszít a robot? (másodlagos mutatók)

| Mutató | `bot:10` | `eng:leaves` | `eng:leaves:sim+eg` |
|---|---:|---:|---:|
| Átlagos pontszám játékonként | 382,9 / 369,7 | 399,0 | 414,3 |
| Pont egy körre | 25,3 / 23,7 | 25,9 | 25,9 |
| Bingó játékonként | 0,31 / 0,25 | 0,52 | 0,60 |
| Végjáték-hozam (a különbség változása a zsák kiürülésétől) | | +8,8 | +22,6 |

(A `bot:10` oszlop az adott párharc ellenfeleként mért értékeket mutatja: `eng:leaves` / `eng:leaves:sim+eg` ellen.) A
motor fő előnye a **bingók**: a tanult maradék-érték a bingó-képes kezeket tartja meg (a motor ~1,7–2,4× annyi bingót
rak ki), a szimuláció ezt tovább erősíti, és a zsák kiürülése után a végjáték-keresés +9 … +23 pontot hoz.

### 7.7 Következtetések a robotra

* A statikus motor (`eng:leaves`) ugyanazt a lépésszóhalmazt és pontozást használja, mint a robot, csak az értékelés más: a
  **+16 pont tehát az értékelésből jön** (a maradék-értékből és a zsák kiürülése utáni képletből). Ebből az következik, hogy
  a robot kézzel írt `leave_value`-jának a tanult táblára cserélése (és egy pontos végjáték-keresés, +4 pont) a robot erejét
  valószínűleg nagyjából ennyivel növelné, szimuláció nélkül, a mostani sebességnél; ezt a robotban nem mértük.
* A szókincs bővítése (hosszabb és többszörösen ragozott alakok) a mérés szerint nagyon sokat ér (7.5: ~21 pont a
  keresztszavakon keresztül is), de memóriaigényes (lásd `INFLECT_MAX_STEM`, `INFLECT_MAX_FORM`).
* A szimuláció a legnagyobb, de számítási szempontból nagyon drága nyereség (+17 … +25 pont, lépésenként tizedmásodperctől
  másodpercekig): szerveren, sok szobánál nem reális, egyjátékos tipp / elemző funkcióhoz igen.

### 7.8 Korlátok

* **A motor ereje részben a mi munkánk:** a magyar maradék-értékeket mi tanítottuk (lineáris + páros modell, 1,0 millió
  önjáték-játékból); egy sokmilliós játékból tanult, háromtagú szinergiákat is tartalmazó táblázat (a
  hivatásos motorok „superleave”-je) valószínűleg erősebb. A motor ereje tehát alsó becslés. A `stock` sor mutatja, mit
  tud a motor kész magyar értékelés nélkül: a robottal egyenlő.
* **A processzoridő nem egyenlő**, és nem is törekedtünk rá (lásd a 7.2. szakaszt).
* **A szimuláció egyszerűsítéseket tartalmaz** (az ellenfél mohó modellje, cserék és passzok nélkül), a pontos
  végjáték-kereső csak ≤ 9 zsetonnál és csak pontos eredmény esetén dönt (az eseteknek ~97%-a pontos).
* **A mély szimuláció csak 100 pár** (SE 4,6), a hozzá tartozó többlet (+3,3 ± 4,6) nem szignifikáns.
* **Ellenőrzések:** a híd, a szabályazonosság, a két független lépésgenerátor egyezése, a játékvezető hibakezelése, az
  ismételhetőség és a tanítás tesztelt (`tests/engine_duel.rs`, 30 teszt); a jelentés számait Python-ban újraszámoltuk;
  az automatikus, független (ügynökös) átvizsgálás a heti keret elfogyása miatt nem futott le, ezért a keret
  helyességét a tesztek és a saját ellenőrzések támasztják alá.
* **A mérés egyetlen játékvezetőn és egyetlen szószedeten fut** (a robot szókincse); a robot éles működésére a 7.5.
  szakasz ad becslést.

## 8. Reprodukálás

```bash
cargo build --release --features engine-duel --bin engine_duel
target/release/engine_duel info                     # szókincs, zsetonsorok
target/release/engine_duel crosscheck --games 6     # a két lépésgenerátor egyezése (0 eltérés)
# a leave-értékek tanítása (≈ 1–1,5 óra 4 magon; a végleges tábla: docs/engine_duel/leaves-hu.json)
target/release/engine_duel train --games 400000 --out .perf/duel/leaves-r0.json
target/release/engine_duel train --from .perf/duel/leaves-r0.json --games 400000 --holdout 10 --save-normal .perf/duel/n-r1 --seed-start 1500000000 --out .perf/duel/leaves-r1-raw.json
target/release/engine_duel solve --normal .perf/duel/n-r1 --out .perf/duel/leaves-r1.json
target/release/engine_duel train --from .perf/duel/leaves-r1.json --games 600000 --holdout 10 --save-normal .perf/duel/n-r2 --seed-start 2000000000 --out .perf/duel/leaves-r2-raw.json
target/release/engine_duel solve --normal .perf/duel/n-r1,.perf/duel/n-r2 --out .perf/duel/leaves-final.json
# az elsődleges mérés (≈ 80 perc 4 magon)
target/release/engine_duel duel eng:leaves:sim+eg bot:10 --leaves docs/engine_duel/leaves-hu.json --pairs 400 --first-pair 100001 -j 4 --out .perf/duel/duel.jsonl
target/release/engine_duel report .perf/duel/duel.jsonl
```

A futás megismételhető és `-j`-től független (a naplóban a processzoridőn kívül minden bitre azonos). A tanítás a
szálak számától szintén független, de a lebegőpontos összegzés sorrendje miatt a súlyok csak ~10⁻⁶ pontossággal
egyeznek.
