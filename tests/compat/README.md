# Differenciális összevetés a régi (Python) szerverrel

Az átírás során a Rust szervert a régi Flask + Socket.IO szerverrel is összevetettük: **ugyanazok a forgatókönyvek**
futnak mindkettőn, és a szerkezetesen normalizált válaszokat (HTTP állapotkód, fejlécek, JSON, Socket.IO események)
hasonlítja össze a `harness.py`. Az eltérés-lista üres kell legyen, kivéve a véletlenre épülő részeket (robot lépése,
napi feladvány tartalma, időzítések).

| Fájl | Mit próbál |
|---|---|
| `h1_public.py` | nyilvános HTTP API: auth, ranglista, szótár, gyakorlás, napi feladvány, replay, PWA, hibaesetek |
| `h2_admin.py` | az admin HTTP API (≈ 200 kérés): hozzáférés, napló, felhasználók, szobák, szótár, beállítások, rendszer |
| `s1_lobby.py`, `s2_game.py` | Socket.IO: lobby, szobák, teljes játék lerakásokkal, megtámadás |
| `s3_misc.py` | újracsatlakozás, mentés / visszaállítás, megfigyelők, barátok, bejelentések, robotok, napi, levelezős |
| `s4_timers.py` | körönkénti időlimit, türelmi idők |
| `s5_adminlive.py` | az admin panel élő Socket.IO eseményei |

Futtatás (a Python verzió a `python-final` ágból / címkéből jön, lásd `scripts/python-baseline.sh`):

```bash
tests/compat/run.sh              # minden forgatókönyv
tests/compat/run.sh h1_public    # csak egy
```

A kimenet (átiratok, naplók) a `.compat/` mappába kerül (gitignorált); a `summ.py` / `diffjson.py` egy-egy átirat
vagy eltérés áttekintéséhez van.
