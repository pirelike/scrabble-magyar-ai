#!/usr/bin/env bash
# Teljesítményteszt a Python és a Rust verzió között (eredmény: docs/PERFORMANCE.md).
#
#   scripts/perf.sh [--quick] [--out docs/PERFORMANCE.md] [további kapcsolók a `cargo bench --bench compare`-nek]
#
# Teendők: a Rust szerver és az eszközök release fordítása; a Python alapverzió előkészítése
# (scripts/python-baseline.sh); végül a mérés (benches/compare.rs). `PERF_SKIP_PYTHON=1`: csak a Rust mérése.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "== Rust fordítás (release) =="
cargo build --release --bins

if [ -z "${PERF_SKIP_PYTHON:-}" ]; then
    read -r PY_DIR PY_BIN < <(scripts/python-baseline.sh)
    PYTHON_ARGS=(--python-dir "$PY_DIR" --python-bin "$PY_BIN")
else
    PYTHON_ARGS=(--only rust)
fi

echo "== Mérés =="
cargo bench --bench compare -- "${PYTHON_ARGS[@]}" "$@"
