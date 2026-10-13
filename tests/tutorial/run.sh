#!/usr/bin/env bash
# Builds the tutorial's app (docs/tutorial.md) step by step, as a reader would, with the
# `rnx` of this checkout, then checks what a reader's CI would: cargo fmt --check, clippy,
# the tests the tutorial writes, migrations and the seeder (twice), and the app answering.
#
#   tests/tutorial/run.sh          # KEEP=1 keeps the app and prints where it is
#
# Run from the repository root. CI runs it (.github/workflows/ci.yml, job `tutorial`).
set -euo pipefail

REPO=$(pwd)
WORK=$(mktemp -d)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$REPO/target/tutorial}
cleanup() {
    if [ -n "${PID:-}" ]; then kill "$PID" 2>/dev/null || true; fi
    if [ -n "${KEEP:-}" ]; then echo "app kept in $WORK"; else rm -rf "$WORK"; fi
}
trap cleanup EXIT
step() { printf '\n== %s\n' "$*"; }

cargo build -q -p renox-cli
RNX="$CARGO_TARGET_DIR/debug/rnx"

step "follow docs/tutorial.md"
python3 "$REPO/tests/tutorial/follow.py" "$WORK" "$RNX" "$REPO"
cd "$WORK/stash"

step "cargo fmt --check (the tutorial's code, as printed)"
cargo fmt --check

step "cargo clippy"
cargo clippy --all-targets -- -D warnings

step "cargo test (the tutorial's tests)"
cargo test

step "migrate, and db:seed twice (it must change nothing the second time)"
cargo run --quiet -- migrate
cargo run --quiet -- db:seed
cargo run --quiet -- db:seed
cargo run --quiet -- schedule:list | grep -q weekly-digest

step "the app answers"
PORT=3197
APP_PORT=$PORT "$CARGO_TARGET_DIR/debug/stash" > "$WORK/server.log" 2>&1 &
PID=$!
for _ in $(seq 1 50); do
    curl -sf -o /dev/null "http://127.0.0.1:$PORT/health" && break
    sleep 0.2
done
curl -sf -o /dev/null "http://127.0.0.1:$PORT/health"
curl -sf -o /dev/null "http://127.0.0.1:$PORT/login"
test "$(curl -s -o /dev/null -w '%{redirect_url}' "http://127.0.0.1:$PORT/bookmarks")" \
    = "http://127.0.0.1:$PORT/login"

echo
echo "tutorial e2e: every step followed; the app is formatted, lint-free, tested and runs"
