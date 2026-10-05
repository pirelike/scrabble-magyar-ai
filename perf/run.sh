#!/usr/bin/env bash
# Teljesítményteszt a Python és a Rust verzió között.
#
#   perf/run.sh [--quick] [--out docs/PERFORMANCE.md] [további kapcsolók a `cargo bench --bench compare`-nek]
#
# Teendők: a Rust szerver és az eszközök release fordítása; a Python verzió (a `python-final` git címke) munkamásolata
# és virtuális környezete a `.perf/` mappában (az első futásnál készül, utána újrahasznosul); végül a mérés.
# A Python verzió régi kódja csak a méréshez kell: a repo fő ágában már nincs benne.
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"
WORK="${PERF_WORK:-$ROOT/.perf}"
TAG="${PERF_PYTHON_TAG:-python-final}"

echo "== Rust fordítás (release) =="
cargo build --release --bins

if [ -z "${PERF_SKIP_PYTHON:-}" ]; then
    if ! git rev-parse --verify --quiet "refs/tags/$TAG" >/dev/null; then
        echo "hiányzik a '$TAG' címke: git fetch origin 'refs/tags/$TAG:refs/tags/$TAG'" >&2
        exit 1
    fi
    if [ ! -d "$WORK/python" ]; then
        echo "== Python munkamásolat a '$TAG' címkéből =="
        mkdir -p "$WORK"
        git worktree add --detach "$WORK/python" "$TAG"
    fi
    if [ ! -x "$WORK/venv/bin/python" ]; then
        echo "== Python virtuális környezet =="
        python3 -m venv "$WORK/venv"
        "$WORK/venv/bin/pip" install -q -r "$WORK/python/requirements.txt"
    fi
    PYTHON_ARGS=(--python-dir "$WORK/python" --python-bin "$WORK/venv/bin/python")
else
    PYTHON_ARGS=(--only rust)
fi

echo "== Mérés =="
cargo bench --bench compare -- "${PYTHON_ARGS[@]}" "$@"
