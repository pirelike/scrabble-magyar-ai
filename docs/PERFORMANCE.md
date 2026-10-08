# Teljesítmény: Python és Rust verzió

A mérést a `cargo bench --bench compare` (`scripts/perf.sh`) állítja elő; mindkét szerver külön folyamat, ugyanarról a mintaadatbázisról indul, és ugyanazt a terhelést kapja. Az „arány” oszlop a Rust előnye (nagyobb = a Rust jobb).

- Gép: Intel(R) Xeon(R) Processor @ 2.10GHz, 4 logikai mag
- Python: Python 3.11.15 (Flask + Socket.IO, gevent, egy folyamat)
- Rust: axum + socketioxide (tokio, többszálú), `--release`
- Terhelés: 4 mp végpontonként, 16 párhuzamos kapcsolat; 40 pár egyidejű játék; mintaadatbázis: 400 felhasználó, 4000 befejezett játék
- Dátum: 2026-10-05

## Összefoglaló

| Mérőszám | Python | Rust | arány |
|---|---|---|---|
| Indulás az első kérésig | 7284 ms | 1868 ms | 3.9× |
| Memória üresjáratban (RSS) | 164 MiB | 66 MiB | 2.5× |
| Memória a HTTP terhelés után | 164 MiB | 141 MiB | 1.2× |
| Memória a Socket.IO terhelés után | 166 MiB | 144 MiB | 1.1× |
| Az első szókvíz-kérés (szókincs betöltése) | 8 ms | 5 ms | 1.6× |
| CPU-idő kérésenként (a HTTP végpontok átlaga) | 2.63 ms | 1.19 ms | 2.2× |
| Teljes játékok (szoba + passzok + mentés) | 17.5 játék/mp | 101.5 játék/mp | 5.8× |
| Egy játék ideje (átlag) | 1.43 mp | 0.21 mp | 6.8× |
| Chat körülfordulás p50 | 3.9 ms | 0.7 ms | 5.6× |
| Chat körülfordulás p99 | 42.9 ms | 3.4 ms | 12.8× |
| Szobalista körülfordulás p99 | 6.9 ms | 2.6 ms | 2.7× |
| Robot-aréna (bot–bot játékok, egy szál) | 32.1 mp | 7.7 mp | 4.2× |

## HTTP végpontok

| Végpont | Python kérés/mp | Rust kérés/mp | arány | Python p50 / p99 | Rust p50 / p99 | CPU-mp / 1000 kérés (Py → Rust) |
|---|---|---|---|---|---|---|
| GET / (index sablon) | 1272 | 6391 | 5.0× | 12.0 / 19.7 ms | 1.8 / 10.1 ms | 0.78 → 0.23 |
| GET /static/app.js (statikus fájl) | 486 | 923 | 1.9× | 44.1 / 55.8 ms | 2.1 / 49.3 ms | 1.43 → 0.73 |
| GET /api/leaderboard (ranglista, DB) | 97 | 122 | 1.3× | 164.0 / 255.2 ms | 132.8 / 239.0 ms | 10.07 → 8.37 |
| POST /api/dictionary/check (8 szó) | 130 | 1432 | 11.0× | 120.0 / 212.0 ms | 10.5 / 26.8 ms | 7.46 → 2.20 |
| GET /api/practice/quiz (szókvíz) | 158 | 1493 | 9.4× | 100.0 / 169.9 ms | 9.7 / 31.9 ms | 4.23 → 1.06 |
| GET /api/practice/rack (betűvadász, szókeresés) | 148 | 1579 | 10.7× | 52.0 / 84.0 ms | 4.8 / 12.0 ms | 3.96 → 1.16 |
| GET /api/practice/short-words?length=3 | 203 | 1248 | 6.1× | 76.0 / 132.0 ms | 12.6 / 28.7 ms | 2.98 → 1.85 |
| POST /api/auth/login (PBKDF2, CPU-igényes) | 12 | 105 | 8.8× | 640.0 / 1036.0 ms | 76.1 / 116.7 ms | 77.09 → 35.62 |

## Robot-aréna: a fokozatok ereje (pont/kör)

A két verzió robotja ugyanazt a skálát adja (a kis játékszám miatt kis eltérés természetes).

| Fokozat | Python | Rust |
|---|---|---|
| 1 | 4.2 | 4.3 |
| 2 | 5.8 | 5.8 |
| 3 | 7.4 | 7.4 |
| 4 | 10.1 | 10.1 |
| 5 | 13.3 | 12.8 |
| 6 | 14.0 | 13.8 |
| 7 | 17.5 | 17.7 |
| 8 | 23.9 | 24.9 |
| 9 | 21.5 | 25.2 |
| 10 | 27.3 | 26.4 |

## Megjegyzések

- A Python verzió egyetlen gevent folyamat: a CPU-igényes kérések (szókeresés, jelszó-hash, robot) egyetlen magot használnak, és egymást várakoztatják. A Rust szerver többszálú, ezért a párhuzamos terhelésnél a magok számával is skálázódik.
- Mindkét szerver az első kérés előtt betölti a szótárat, a robot szókincsét (a ragozott alakokkal) és a napi feladványt; az indulási idő ezt is tartalmazza.
- A ranglista végpont ideje főleg az SQLite összesítő lekérdezésére megy el (mindkét verzió ugyanazt a sémát és lekérdezést használja), ezért ott kisebb az eltérés.
- A kérések `X-Forwarded-For` fejlécével különböző kliens-IP-k látszanak, így az IP-alapú forgalomkorlát nem torzítja a méréseket.
- A Python verzió a `python-final` git címkén van; a mintaadatbázist a Rust kód állítja elő, és mindkét szerver ugyanazt a fájlt használja (a két verzió adatbázis-sémája azonos).
- Újrafuttatás: `scripts/perf.sh --out docs/PERFORMANCE.md` (gyors próba: `--quick`).
