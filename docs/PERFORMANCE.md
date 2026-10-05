# Teljesítmény: Python és Rust verzió

A mérést a `cargo bench --bench compare` (`perf/run.sh`) állítja elő; mindkét szerver külön folyamat, ugyanarról a mintaadatbázisról indul, és ugyanazt a terhelést kapja. A „arány” oszlop a Rust előnye (nagyobb = a Rust jobb).

- Gép: Intel(R) Xeon(R) Processor @ 2.10GHz, 4 logikai mag
- Python: Python 3.11.15 (Flask + Socket.IO, gevent, egy folyamat)
- Rust: axum + socketioxide (tokio, többszálú), `--release`
- Terhelés: 4 mp végpontonként, 16 párhuzamos kapcsolat; 40 pár egyidejű játék; mintaadatbázis: 400 felhasználó, 4000 befejezett játék
- Dátum: 2026-10-05

## Összefoglaló

| Mérőszám | Python | Rust | arány |
|---|---|---|---|
| Indulás az első kérésig | 7353 ms | 1908 ms | 3.9× |
| Memória üresjáratban (RSS) | 164 MiB | 66 MiB | 2.5× |
| Memória a HTTP terhelés után | 164 MiB | 136 MiB | 1.2× |
| Memória a Socket.IO terhelés után | 165 MiB | 139 MiB | 1.2× |
| Az első szókvíz-kérés (szókincs betöltése) | 8 ms | 3 ms | 3.2× |
| CPU-idő kérésenként (a HTTP végpontok átlaga) | 2.77 ms | 1.14 ms | 2.4× |
| Teljes játékok (szoba + passzok + mentés) | 12.8 játék/mp | 130.0 játék/mp | 10.2× |
| Egy játék ideje (átlag) | 1.89 mp | 0.17 mp | 11.3× |
| Chat körülfordulás p50 | 4.6 ms | 0.6 ms | 7.4× |
| Chat körülfordulás p99 | 42.4 ms | 1.8 ms | 23.0× |
| Szobalista körülfordulás p99 | 18.5 ms | 1.1 ms | 17.4× |
| Robot-aréna (bot–bot játékok, egy szál) | 33.7 mp | 7.0 mp | 4.8× |

## HTTP végpontok

| Végpont | Python kérés/mp | Rust kérés/mp | arány | Python p50 / p99 | Rust p50 / p99 | CPU-mp / 1000 kérés (Py → Rust) |
|---|---|---|---|---|---|---|
| GET / (index sablon) | 1142 | 7142 | 6.3× | 13.3 / 19.8 ms | 1.6 / 8.3 ms | 0.86 → 0.22 |
| GET /static/app.js (statikus fájl) | 480 | 938 | 2.0× | 44.0 / 56.0 ms | 1.8 / 48.7 ms | 1.37 → 0.70 |
| GET /api/leaderboard (ranglista, DB) | 90 | 124 | 1.4× | 172.0 / 252.0 ms | 126.8 / 230.0 ms | 10.80 → 8.21 |
| POST /api/dictionary/check (8 szó) | 129 | 1263 | 9.8× | 123.8 / 205.0 ms | 11.6 / 36.8 ms | 7.51 → 2.46 |
| GET /api/practice/quiz (szókvíz) | 156 | 1554 | 9.9× | 103.3 / 159.9 ms | 9.8 / 22.7 ms | 4.35 → 1.06 |
| GET /api/practice/rack (betűvadász, szókeresés) | 147 | 1627 | 11.1× | 52.0 / 86.9 ms | 4.6 / 11.1 ms | 4.07 → 1.15 |
| GET /api/practice/short-words?length=3 | 199 | 1354 | 6.8× | 76.0 / 148.0 ms | 11.7 / 21.6 ms | 2.97 → 1.75 |
| POST /api/auth/login (PBKDF2, CPU-igényes) | 12 | 102 | 8.6× | 659.9 / 1012.1 ms | 77.9 / 121.7 ms | 76.91 → 36.40 |

## Robot-aréna: a fokozatok ereje (pont/kör)

A két verzió robotja ugyanazt a skálát adja (a kis játékszám miatt kis eltérés természetes).

| Fokozat | Python | Rust |
|---|---|---|
| 1 | 4.2 | 4.1 |
| 2 | 5.8 | 6.0 |
| 3 | 7.4 | 7.4 |
| 4 | 10.1 | 9.9 |
| 5 | 13.3 | 12.6 |
| 6 | 14.0 | 14.8 |
| 7 | 17.5 | 16.4 |
| 8 | 23.9 | 19.1 |
| 9 | 21.5 | 25.9 |
| 10 | 27.3 | 24.7 |

## Megjegyzések

- A Python verzió egyetlen gevent folyamat: a CPU-igényes kérések (szókeresés, jelszó-hash, robot) egyetlen magot használnak, és egymást várakoztatják. A Rust szerver többszálú, ezért a párhuzamos terhelésnél a magok számával is skálázódik.
- Az indulási idő a Pythonnál az importokat és a szótár első betöltését tartalmazza; a Rust szerver a szótárat és a szókincset indításkor, a háttérben tölti be.
- A kérések `X-Forwarded-For` fejlécével különböző kliens-IP-k látszanak, így az IP-alapú forgalomkorlát nem torzítja a méréseket.
