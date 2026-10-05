# Aranyfájlok: a Rust és a régi Python implementáció egyezése

Ezeket a fájlokat a régi (Python) verzió állította elő; a Rust tesztek (`tests/*_golden.rs`, `tests/python_compat.rs`)
ezekkel vetik össze az eredményeket, így a szótár, a robot, a gyakorló módok, a játékállapot és az SQLite fájl
formátuma bájtra / szóra egyezik a Python verzióval.

| Fájl | Mit rögzít | Ki használja |
|---|---|---|
| `dictionary.json`, `explain.json`, `suggest.json` | szóellenőrzés (tőszavak, ragozott alakok, mutációk), levezetések magyarázata, javaslatok | `dictionary_golden.rs` |
| `moves.json` | a robot lépésgenerátora (az összes lehetséges lépés összefoglalója táblánként és kézenként) | `ai_golden.rs` |
| `practice.json` | rövid szavak, szótövek, zsetonokra bontás, kvíz-válaszok, betűvadász szólisták | `practice_golden.rs` |
| `vocabulary.json` | a robot szókincse (darabszám, ellenőrző összeg, minta) | `vocabulary_golden.rs` |
| `python_fixture.json`, `python_fixture.sqlite` | Python által írt játékállapotok és adatbázis (jelszó-hash, munkamenetek, mentett játékok) | `python_compat.rs` |

## Újragenerálás

A generátorok a `generators/` mappában vannak, és a régi verzió forrását kérik (a `python-final` ág / címke):

```bash
read -r PY_DIR PY_BIN < <(scripts/python-baseline.sh)
export PYTHON_FINAL_DIR="$PY_DIR"
"$PY_BIN" tests/golden/generators/gen_dict_golden.py
"$PY_BIN" tests/golden/generators/gen_moves_golden.py
"$PY_BIN" tests/golden/generators/gen_practice_golden.py
"$PY_BIN" tests/golden/generators/gen_vocab_golden.py tests/golden/vocabulary.json
"$PY_BIN" tests/golden/generators/gen_pydb.py tests/golden/python_fixture.sqlite
```

(A `GOLDEN_OUT` környezeti változó a kimeneti mappát állítja, alapból `tests/golden`.) Az aranyfájlokat csak akkor kell
újragenerálni, ha a szándékos viselkedés változik (pl. a szótár szabályai); egyébként a tesztek épp azt őrzik,
hogy a Rust viselkedése nem csúszik el.
