#!/usr/bin/env bash
# Differenciális összevetés: ugyanazok a forgatókönyvek (HTTP API, Socket.IO, admin panel) a régi Python szerveren
# (`python-final`) és a Rust szerveren, a válaszok szerkezetének összevetésével. Lásd tests/compat/README.md.
#
#   tests/compat/run.sh [forgatókönyv ...]     # alapból mind: h1_public h2_admin s1_lobby s2_game s3_misc s4_timers s5_adminlive
set -euo pipefail

cd "$(dirname "$0")/../.."
ROOT="$(pwd)"
OUT="${COMPAT_OUT:-$ROOT/.compat}"
PY_PORT=5056
RS_PORT=5055
mkdir -p "$OUT"

cargo build --release --bin scrabble
read -r PY_DIR PY_BIN < <(scripts/python-baseline.sh)
export PYTHON_FINAL_DIR="$PY_DIR" COMPAT_OUT="$OUT" COMPAT_PY_PORT="$PY_PORT" COMPAT_RS_PORT="$RS_PORT"

# közös kiinduló adatbázis és a Python robot szókincse
"$PY_BIN" tests/compat/prep_db.py "$OUT/base.db" >/dev/null
(cd "$PY_DIR" && "$PY_BIN" -c "import ai_player, dictionary; dictionary.warm_up(); print('\n'.join(ai_player.get_vocabulary().words))") >"$OUT/py_vocab.txt" 2>/dev/null
for n in py rs; do
    rm -f "$OUT/$n.db"*
    cp "$OUT/base.db" "$OUT/$n.db"
    cp dict/hu_rejected.txt "$OUT/$n.rejected"
done

cleanup() {
    kill "$(cat "$OUT/py.pid" 2>/dev/null)" "$(cat "$OUT/rs.pid" 2>/dev/null)" 2>/dev/null || true
}
trap cleanup EXIT

(cd "$PY_DIR" && ADMIN_EMAILS=admin@example.com SCRABBLE_DB_PATH="$OUT/py.db" PORT="$PY_PORT" SCRABBLE_BACKUP_DIR="$OUT/py-bk" \
    "$PY_BIN" server.py --no-tunnel >"$OUT/py.log" 2>&1 & echo $! >"$OUT/py.pid")
ADMIN_EMAILS=admin@example.com SCRABBLE_DB_PATH="$OUT/rs.db" SCRABBLE_REJECTED_FILE="$OUT/rs.rejected" PORT="$RS_PORT" \
    SCRABBLE_BACKUP_DIR="$OUT/rs-bk" target/release/scrabble --no-tunnel >"$OUT/rs.log" 2>&1 &
echo $! >"$OUT/rs.pid"

for port in "$RS_PORT" "$PY_PORT"; do
    for _ in $(seq 1 120); do
        curl -s -o /dev/null "http://localhost:$port/" && break
        sleep 1
    done
done
echo "szerverek: Python :$PY_PORT, Rust :$RS_PORT (kimenet: $OUT)"

SCENARIOS=("$@")
[ ${#SCENARIOS[@]} -eq 0 ] && SCENARIOS=(h1_public h2_admin s1_lobby s2_game s3_misc s4_timers s5_adminlive)
for scenario in "${SCENARIOS[@]}"; do
    "$PY_BIN" "tests/compat/$scenario.py"
done
