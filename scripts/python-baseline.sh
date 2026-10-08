#!/usr/bin/env bash
# A régi (Python) verzió előkészítése összehasonlításhoz: a `python-final` címke / ág munkamásolata és virtuális
# környezete a `.perf/` mappában (az első futásnál készül, utána újrahasznosul). A Python kód csak a méréshez és az
# összevetéshez kell: a repo fő ágában már nincs benne.
#
#   scripts/python-baseline.sh        # kiírja a két útvonalat: <munkamásolat> <python>
set -euo pipefail

cd "$(dirname "$0")/.."
WORK="${PERF_WORK:-$(pwd)/.perf}"
TAG="${PERF_PYTHON_TAG:-python-final}"

REF=""
for candidate in "refs/tags/$TAG" "refs/remotes/origin/$TAG" "refs/heads/$TAG"; do
    if git rev-parse --verify --quiet "$candidate^{commit}" >/dev/null; then
        REF="$candidate"
        break
    fi
done
if [ -z "$REF" ]; then
    echo "hiányzik a '$TAG' címke / ág: git fetch origin '$TAG'" >&2
    exit 1
fi
if [ ! -d "$WORK/python" ]; then
    echo "== Python munkamásolat a '$TAG' címkéből ==" >&2
    mkdir -p "$WORK"
    git worktree add --detach "$WORK/python" "$REF" >&2
fi
if [ ! -x "$WORK/venv/bin/python" ]; then
    echo "== Python virtuális környezet ==" >&2
    python3 -m venv "$WORK/venv"
    "$WORK/venv/bin/pip" install -q -r "$WORK/python/requirements.txt" requests >&2
fi
echo "$WORK/python $WORK/venv/bin/python"
