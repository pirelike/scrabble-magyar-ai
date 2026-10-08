#!/usr/bin/env bash
# A szerver indítása (szükség esetén előbb lefordítja).
#
#   scripts/run.sh               # Cloudflare tunnellel (publikus URL)
#   scripts/run.sh --no-tunnel   # csak a helyi hálózaton
#
# A gépre jellemző beállítások (PORT, SMTP_*, ADMIN_EMAILS…) a `.env` fájlba kerülhetnek (lásd .env.example).
set -euo pipefail

cd "$(dirname "$0")/.."
cargo build --release --bin scrabble
exec target/release/scrabble "$@"
