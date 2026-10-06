| A | B | Párok | Átlagos különbség (A − B) | SE | 95% CI (t) | 95% CI (bootstrap) | p | A nyerési aránya |
|---|---|---:|---:|---:|---|---|---:|---:|
| `bot:10` | `bot:10` | 400 | **+1.5** | 2.19 | [-2.9, +5.8] | [-2.9, +5.8] | 0,506 | 50.0% |
| `bot:10` | `bot:10:full` | 400 | **-20.8** | 2.46 | [-25.6, -15.9] | [-25.7, -15.9] | <0,001 | 40.4% |
| `bot:10` | `bot:9` | 400 | **+24.9** | 2.73 | [+19.5, +30.3] | [+19.5, +30.2] | <0,001 | 62.3% |
| `bot:greedy` | `eng:greedy` | 400 | **+2.5** | 2.07 | [-1.6, +6.5] | [-1.6, +6.5] | 0,233 | 50.5% |
| `eng:greedy` | `bot:10` | 400 | **-12.0** | 2.35 | [-16.6, -7.3] | [-16.6, -7.3] | <0,001 | 45.3% |
| `eng:leaves` | `bot:10` | 400 | **+16.0** | 2.57 | [+11.0, +21.1] | [+11.1, +21.1] | <0,001 | 57.7% |
| `eng:leaves+eg` | `bot:10` | 400 | **+20.1** | 2.62 | [+15.0, +25.2] | [+15.1, +25.2] | <0,001 | 59.6% |
| `eng:leaves:sim+eg` | `bot:10` | 400 | **+44.6** | 2.60 | [+39.5, +49.7] | [+39.5, +49.6] | <0,001 | 71.1% |
| `eng:leaves:sim+eg` | `bot:10:full` | 200 | **+14.2** | 3.68 | [+6.9, +21.4] | [+6.8, +21.4] | <0,001 | 57.2% |
| `eng:leaves:sim-deep+eg` | `bot:10` | 100 | **+53.4** | 4.58 | [+44.4, +62.5] | [+44.5, +62.3] | <0,001 | 80.2% |
| `eng:leaves:sim-fast` | `bot:10` | 400 | **+33.4** | 2.55 | [+28.4, +38.4] | [+28.4, +38.2] | <0,001 | 66.2% |
| `eng:leaves:sim-fast+eg` | `bot:10` | 400 | **+38.2** | 2.52 | [+33.3, +43.2] | [+33.3, +43.1] | <0,001 | 68.4% |
| `eng:stock` | `bot:10` | 400 | **-0.2** | 2.50 | [-5.1, +4.7] | [-5.1, +4.8] | 0,936 | 50.9% |

#### `bot:10` — `bot:10` (400 pár, 800 játék)

| Mutató | A | B |
|---|---:|---:|
| Átlagos pontszám játékonként | 388.9 | 387.5 |
| Pont egy körre (lerakás, csere, passz együtt) | 24.92 | 24.83 |
| Lerakások száma játékonként | 15.1 | 15.1 |
| Bingó játékonként | 0.32 | 0.32 |
| Csere játékonként | 0.02 | 0.02 |
| Processzoridő játékonként (ms) | 124 | 130 |
| Végjáték-kereső hívások (pontos / összes) | 0 / 0 | 0 / 0 |

| Mutató | Érték |
|---|---|
| Átlagos különbség (A − B), páronként | +1.5 ± 2.19 (SE), a pár-átlagok szórása 43.9 |
| A győzelmi aránya (döntetlen = ½) | 50.0% (párszintű SE ±1.4%; játékszintű Wilson-intervallum 46.5–53.5%) |
| Előjelpróba (párok): A nyer / B nyer / döntetlen | 178 / 189 / 33, p = 0.602 |
| A győzelmi pontjai a párban (0 / ½ / 1 / 1½ / 2) | 67 / 0 / 266 / 0 / 67 pár |
| Végjáték-hozam (A különbsége a zsák kiürülésétől a végéig), 800 játékban | +1.3 |
| Kirakással végződő játékok aránya | 86.1% (a többi 6 pont nélküli körrel ért véget); átlagosan 31.3 lépés |

#### `bot:10` — `bot:10:full` (400 pár, 800 játék)

| Mutató | A | B |
|---|---:|---:|
| Átlagos pontszám játékonként | 388.6 | 409.4 |
| Pont egy körre (lerakás, csere, passz együtt) | 25.04 | 26.48 |
| Lerakások száma játékonként | 15.0 | 15.0 |
| Bingó játékonként | 0.31 | 0.31 |
| Csere játékonként | 0.03 | 0.02 |
| Processzoridő játékonként (ms) | 135 | 118 |
| Végjáték-kereső hívások (pontos / összes) | 0 / 0 | 0 / 0 |

| Mutató | Érték |
|---|---|
| Átlagos különbség (A − B), páronként | -20.8 ± 2.46 (SE), a pár-átlagok szórása 49.3 |
| A győzelmi aránya (döntetlen = ½) | 40.4% (párszintű SE ±1.6%; játékszintű Wilson-intervallum 37.0–43.8%) |
| Előjelpróba (párok): A nyer / B nyer / döntetlen | 144 / 254 / 2, p = 0.000 |
| A győzelmi pontjai a párban (0 / ½ / 1 / 1½ / 2) | 128 / 5 / 213 / 1 / 53 pár |
| Végjáték-hozam (A különbsége a zsák kiürülésétől a végéig), 800 játékban | -0.0 |
| Kirakással végződő játékok aránya | 87.1% (a többi 6 pont nélküli körrel ért véget); átlagosan 31.0 lépés |

#### `bot:10` — `bot:9` (400 pár, 800 játék)

| Mutató | A | B |
|---|---:|---:|
| Átlagos pontszám játékonként | 396.0 | 371.2 |
| Pont egy körre (lerakás, csere, passz együtt) | 24.70 | 23.30 |
| Lerakások száma játékonként | 15.4 | 15.4 |
| Bingó játékonként | 0.29 | 0.28 |
| Csere játékonként | 0.03 | 0.03 |
| Processzoridő játékonként (ms) | 125 | 120 |
| Végjáték-kereső hívások (pontos / összes) | 0 / 0 | 0 / 0 |

| Mutató | Érték |
|---|---|
| Átlagos különbség (A − B), páronként | +24.9 ± 2.73 (SE), a pár-átlagok szórása 54.7 |
| A győzelmi aránya (döntetlen = ½) | 62.3% (párszintű SE ±1.6%; játékszintű Wilson-intervallum 58.9–65.6%) |
| Előjelpróba (párok): A nyer / B nyer / döntetlen | 268 / 131 / 1, p = 0.000 |
| A győzelmi pontjai a párban (0 / ½ / 1 / 1½ / 2) | 49 / 2 / 200 / 1 / 148 pár |
| Végjáték-hozam (A különbsége a zsák kiürülésétől a végéig), 800 játékban | +4.0 |
| Kirakással végződő játékok aránya | 83.4% (a többi 6 pont nélküli körrel ért véget); átlagosan 32.0 lépés |

#### `bot:greedy` — `eng:greedy` (400 pár, 800 játék)

| Mutató | A | B |
|---|---:|---:|
| Átlagos pontszám játékonként | 388.6 | 386.2 |
| Pont egy körre (lerakás, csere, passz együtt) | 23.61 | 23.55 |
| Lerakások száma játékonként | 15.7 | 15.7 |
| Bingó játékonként | 0.24 | 0.24 |
| Csere játékonként | 0.02 | 0.03 |
| Processzoridő játékonként (ms) | 95 | 4 |
| Végjáték-kereső hívások (pontos / összes) | 0 / 0 | 0 / 0 |

| Mutató | Érték |
|---|---|
| Átlagos különbség (A − B), páronként | +2.5 ± 2.07 (SE), a pár-átlagok szórása 41.3 |
| A győzelmi aránya (döntetlen = ½) | 50.5% (párszintű SE ±1.4%; játékszintű Wilson-intervallum 47.0–54.0%) |
| Előjelpróba (párok): A nyer / B nyer / döntetlen | 203 / 182 / 15, p = 0.308 |
| A győzelmi pontjai a párban (0 / ½ / 1 / 1½ / 2) | 65 / 0 / 263 / 6 / 66 pár |
| Végjáték-hozam (A különbsége a zsák kiürülésétől a végéig), 800 játékban | +1.4 |
| Kirakással végződő játékok aránya | 80.8% (a többi 6 pont nélküli körrel ért véget); átlagosan 33.0 lépés |

#### `eng:greedy` — `bot:10` (400 pár, 800 játék)

| Mutató | A | B |
|---|---:|---:|
| Átlagos pontszám játékonként | 381.6 | 393.5 |
| Pont egy körre (lerakás, csere, passz együtt) | 23.96 | 24.45 |
| Lerakások száma játékonként | 15.3 | 15.4 |
| Bingó játékonként | 0.24 | 0.33 |
| Csere játékonként | 0.02 | 0.02 |
| Processzoridő játékonként (ms) | 4 | 129 |
| Végjáték-kereső hívások (pontos / összes) | 0 / 0 | 0 / 0 |

| Mutató | Érték |
|---|---|
| Átlagos különbség (A − B), páronként | -12.0 ± 2.35 (SE), a pár-átlagok szórása 47.0 |
| A győzelmi aránya (döntetlen = ½) | 45.3% (párszintű SE ±1.5%; játékszintű Wilson-intervallum 41.9–48.8%) |
| Előjelpróba (párok): A nyer / B nyer / döntetlen | 163 / 236 / 1, p = 0.000 |
| A győzelmi pontjai a párban (0 / ½ / 1 / 1½ / 2) | 96 / 1 / 243 / 2 / 58 pár |
| Végjáték-hozam (A különbsége a zsák kiürülésétől a végéig), 799 játékban | -2.1 |
| Kirakással végződő játékok aránya | 83.5% (a többi 6 pont nélküli körrel ért véget); átlagosan 32.1 lépés |

#### `eng:leaves` — `bot:10` (400 pár, 800 játék)

| Mutató | A | B |
|---|---:|---:|
| Átlagos pontszám játékonként | 399.0 | 382.9 |
| Pont egy körre (lerakás, csere, passz együtt) | 25.87 | 25.28 |
| Lerakások száma játékonként | 15.0 | 14.8 |
| Bingó játékonként | 0.52 | 0.31 |
| Csere játékonként | 0.02 | 0.02 |
| Processzoridő játékonként (ms) | 8 | 130 |
| Végjáték-kereső hívások (pontos / összes) | 0 / 0 | 0 / 0 |

| Mutató | Érték |
|---|---|
| Átlagos különbség (A − B), páronként | +16.0 ± 2.57 (SE), a pár-átlagok szórása 51.4 |
| A győzelmi aránya (döntetlen = ½) | 57.7% (párszintű SE ±1.7%; játékszintű Wilson-intervallum 54.2–61.1%) |
| Előjelpróba (párok): A nyer / B nyer / döntetlen | 237 / 163 / 0, p = 0.000 |
| A győzelmi pontjai a párban (0 / ½ / 1 / 1½ / 2) | 64 / 1 / 208 / 2 / 125 pár |
| Végjáték-hozam (A különbsége a zsák kiürülésétől a végéig), 800 játékban | +8.8 |
| Kirakással végződő játékok aránya | 89.9% (a többi 6 pont nélküli körrel ért véget); átlagosan 30.6 lépés |

#### `eng:leaves+eg` — `bot:10` (400 pár, 800 játék)

| Mutató | A | B |
|---|---:|---:|
| Átlagos pontszám játékonként | 401.3 | 381.2 |
| Pont egy körre (lerakás, csere, passz együtt) | 25.40 | 24.85 |
| Lerakások száma játékonként | 15.3 | 14.8 |
| Bingó játékonként | 0.52 | 0.31 |
| Csere játékonként | 0.02 | 0.02 |
| Processzoridő játékonként (ms) | 312 | 129 |
| Végjáték-kereső hívások (pontos / összes) | 1877 / 1916 | 0 / 0 |

| Mutató | Érték |
|---|---|
| Átlagos különbség (A − B), páronként | +20.1 ± 2.62 (SE), a pár-átlagok szórása 52.4 |
| A győzelmi aránya (döntetlen = ½) | 59.6% (párszintű SE ±1.7%; játékszintű Wilson-intervallum 56.1–62.9%) |
| Előjelpróba (párok): A nyer / B nyer / döntetlen | 246 / 154 / 0, p = 0.000 |
| A győzelmi pontjai a párban (0 / ½ / 1 / 1½ / 2) | 60 / 1 / 202 / 0 / 137 pár |
| Végjáték-hozam (A különbsége a zsák kiürülésétől a végéig), 800 játékban | +12.8 |
| Kirakással végződő játékok aránya | 89.9% (a többi 6 pont nélküli körrel ért véget); átlagosan 31.2 lépés |

#### `eng:leaves:sim+eg` — `bot:10` (400 pár, 800 játék)

| Mutató | A | B |
|---|---:|---:|
| Átlagos pontszám játékonként | 414.3 | 369.7 |
| Pont egy körre (lerakás, csere, passz együtt) | 25.89 | 23.73 |
| Lerakások száma játékonként | 15.6 | 15.0 |
| Bingó játékonként | 0.60 | 0.25 |
| Csere játékonként | 0.02 | 0.03 |
| Processzoridő játékonként (ms) | 22123 | 137 |
| Végjáték-kereső hívások (pontos / összes) | 1864 / 1929 | 0 / 0 |

| Mutató | Érték |
|---|---|
| Átlagos különbség (A − B), páronként | +44.6 ± 2.60 (SE), a pár-átlagok szórása 51.9 |
| A győzelmi aránya (döntetlen = ½) | 71.1% (párszintű SE ±1.5%; játékszintű Wilson-intervallum 67.8–74.1%) |
| Előjelpróba (párok): A nyer / B nyer / döntetlen | 324 / 76 / 0, p = 0.000 |
| A győzelmi pontjai a párban (0 / ½ / 1 / 1½ / 2) | 23 / 1 / 182 / 4 / 190 pár |
| Végjáték-hozam (A különbsége a zsák kiürülésétől a végéig), 800 játékban | +22.6 |
| Kirakással végződő játékok aránya | 90.1% (a többi 6 pont nélküli körrel ért véget); átlagosan 31.6 lépés |

#### `eng:leaves:sim+eg` — `bot:10:full` (200 pár, 400 játék)

| Mutató | A | B |
|---|---:|---:|
| Átlagos pontszám játékonként | 410.9 | 396.7 |
| Pont egy körre (lerakás, csere, passz együtt) | 25.60 | 25.52 |
| Lerakások száma játékonként | 15.6 | 14.9 |
| Bingó játékonként | 0.56 | 0.29 |
| Csere játékonként | 0.02 | 0.02 |
| Processzoridő játékonként (ms) | 22503 | 138 |
| Végjáték-kereső hívások (pontos / összes) | 969 / 988 | 0 / 0 |

| Mutató | Érték |
|---|---|
| Átlagos különbség (A − B), páronként | +14.2 ± 3.68 (SE), a pár-átlagok szórása 52.1 |
| A győzelmi aránya (döntetlen = ½) | 57.2% (párszintű SE ±2.2%; játékszintű Wilson-intervallum 52.4–62.0%) |
| Előjelpróba (párok): A nyer / B nyer / döntetlen | 124 / 73 / 3, p = 0.000 |
| A győzelmi pontjai a párban (0 / ½ / 1 / 1½ / 2) | 27 / 1 / 115 / 1 / 56 pár |
| Végjáték-hozam (A különbsége a zsák kiürülésétől a végéig), 400 játékban | +22.4 |
| Kirakással végződő játékok aránya | 91.2% (a többi 6 pont nélküli körrel ért véget); átlagosan 31.6 lépés |

#### `eng:leaves:sim-deep+eg` — `bot:10` (100 pár, 200 játék)

| Mutató | A | B |
|---|---:|---:|
| Átlagos pontszám játékonként | 414.8 | 361.3 |
| Pont egy körre (lerakás, csere, passz együtt) | 25.57 | 22.93 |
| Lerakások száma játékonként | 15.8 | 15.0 |
| Bingó játékonként | 0.61 | 0.28 |
| Csere játékonként | 0.04 | 0.04 |
| Processzoridő játékonként (ms) | 172562 | 134 |
| Végjáték-kereső hívások (pontos / összes) | 468 / 484 | 0 / 0 |

| Mutató | Érték |
|---|---|
| Átlagos különbség (A − B), páronként | +53.4 ± 4.58 (SE), a pár-átlagok szórása 45.8 |
| A győzelmi aránya (döntetlen = ½) | 80.2% (párszintű SE ±2.7%; játékszintű Wilson-intervallum 74.2–85.2%) |
| Előjelpróba (párok): A nyer / B nyer / döntetlen | 90 / 10 / 0, p = 0.000 |
| A győzelmi pontjai a párban (0 / ½ / 1 / 1½ / 2) | 3 / 0 / 33 / 1 / 63 pár |
| Végjáték-hozam (A különbsége a zsák kiürülésétől a végéig), 199 játékban | +26.0 |
| Kirakással végződő játékok aránya | 89.5% (a többi 6 pont nélküli körrel ért véget); átlagosan 32.0 lépés |

#### `eng:leaves:sim-fast` — `bot:10` (400 pár, 800 játék)

| Mutató | A | B |
|---|---:|---:|
| Átlagos pontszám játékonként | 410.0 | 376.7 |
| Pont egy körre (lerakás, csere, passz együtt) | 25.59 | 23.87 |
| Lerakások száma játékonként | 15.6 | 15.3 |
| Bingó játékonként | 0.58 | 0.27 |
| Csere játékonként | 0.02 | 0.03 |
| Processzoridő játékonként (ms) | 4396 | 138 |
| Végjáték-kereső hívások (pontos / összes) | 0 / 0 | 0 / 0 |

| Mutató | Érték |
|---|---|
| Átlagos különbség (A − B), páronként | +33.4 ± 2.55 (SE), a pár-átlagok szórása 50.9 |
| A győzelmi aránya (döntetlen = ½) | 66.2% (párszintű SE ±1.6%; játékszintű Wilson-intervallum 62.9–69.4%) |
| Előjelpróba (párok): A nyer / B nyer / döntetlen | 308 / 91 / 1, p = 0.000 |
| A győzelmi pontjai a párban (0 / ½ / 1 / 1½ / 2) | 34 / 1 / 200 / 1 / 164 pár |
| Végjáték-hozam (A különbsége a zsák kiürülésétől a végéig), 800 játékban | +17.7 |
| Kirakással végződő játékok aránya | 90.1% (a többi 6 pont nélküli körrel ért véget); átlagosan 31.9 lépés |

#### `eng:leaves:sim-fast+eg` — `bot:10` (400 pár, 800 játék)

| Mutató | A | B |
|---|---:|---:|
| Átlagos pontszám játékonként | 411.9 | 373.6 |
| Pont egy körre (lerakás, csere, passz együtt) | 25.47 | 23.81 |
| Lerakások száma játékonként | 15.7 | 15.1 |
| Bingó játékonként | 0.58 | 0.27 |
| Csere játékonként | 0.02 | 0.03 |
| Processzoridő játékonként (ms) | 4817 | 138 |
| Végjáték-kereső hívások (pontos / összes) | 1970 / 2026 | 0 / 0 |

| Mutató | Érték |
|---|---|
| Átlagos különbség (A − B), páronként | +38.2 ± 2.52 (SE), a pár-átlagok szórása 50.4 |
| A győzelmi aránya (döntetlen = ½) | 68.4% (párszintű SE ±1.5%; játékszintű Wilson-intervallum 65.1–71.5%) |
| Előjelpróba (párok): A nyer / B nyer / döntetlen | 314 / 83 / 3, p = 0.000 |
| A győzelmi pontjai a párban (0 / ½ / 1 / 1½ / 2) | 29 / 1 / 192 / 3 / 175 pár |
| Végjáték-hozam (A különbsége a zsák kiürülésétől a végéig), 800 játékban | +22.6 |
| Kirakással végződő játékok aránya | 90.9% (a többi 6 pont nélküli körrel ért véget); átlagosan 31.9 lépés |

#### `eng:stock` — `bot:10` (400 pár, 800 játék)

| Mutató | A | B |
|---|---:|---:|
| Átlagos pontszám játékonként | 391.1 | 391.3 |
| Pont egy körre (lerakás, csere, passz együtt) | 24.89 | 25.10 |
| Lerakások száma játékonként | 15.2 | 15.1 |
| Bingó játékonként | 0.45 | 0.32 |
| Csere játékonként | 0.02 | 0.02 |
| Processzoridő játékonként (ms) | 8 | 129 |
| Végjáték-kereső hívások (pontos / összes) | 0 / 0 | 0 / 0 |

| Mutató | Érték |
|---|---|
| Átlagos különbség (A − B), páronként | -0.2 ± 2.50 (SE), a pár-átlagok szórása 50.0 |
| A győzelmi aránya (döntetlen = ½) | 50.9% (párszintű SE ±1.6%; játékszintű Wilson-intervallum 47.5–54.4%) |
| Előjelpróba (párok): A nyer / B nyer / döntetlen | 191 / 207 / 2, p = 0.452 |
| A győzelmi pontjai a párban (0 / ½ / 1 / 1½ / 2) | 73 / 2 / 242 / 3 / 80 pár |
| Végjáték-hozam (A különbsége a zsák kiürülésétől a végéig), 800 játékban | +3.4 |
| Kirakással végződő játékok aránya | 86.6% (a többi 6 pont nélküli körrel ért véget); átlagosan 31.4 lépés |
