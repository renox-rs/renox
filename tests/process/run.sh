#!/usr/bin/env bash
# Process e2e (#270): the app binary and `rnx` as real processes (signals,
# workers, the scheduler, systemd's socket, logs, db:shell, prompts, `rnx
# serve`), and every example binary served and asked for its pages.
#
#   tests/process/run.sh             # everything (rnx serve too)
#   tests/process/run.sh fixture     # the fixture's checks only
#   tests/process/run.sh examples    # the examples only
#   PROCESS_POSTGRES=postgres://user:pw@host:port tests/process/run.sh postgres
#                                    # the engine checks and bikeshop on
#                                    # PostgreSQL (each in a database of its
#                                    # own, dropped afterwards)
#
# RNX_BUILD=1 adds `rnx build` (a release build) to the Tailwind check.
# Builds what it runs first, one package at a time (CARGO_BUILD_JOBS, 2 by
# default), so a small machine isn't overwhelmed. Needs python3; uses
# systemd-socket-activate and script when they're installed.
set -euo pipefail
cd "$(dirname "$0")/../.."

jobs="${CARGO_BUILD_JOBS:-2}"
part="${1:-all}"
build() {
  for package in "$@"; do
    echo "building $package"
    cargo build --quiet -j "$jobs" -p "$package" ${FEATURES:+--features "$FEATURES"}
  done
}

if [ "$part" = all ] || [ "$part" = fixture ]; then
  build browser-fixture renox-cli
  RNX_SERVE="${RNX_SERVE:-1}" python3 tests/process/process.py
fi

if [ "$part" = all ] || [ "$part" = examples ]; then
  build hello bikeshop
  python3 tests/process/examples.py
fi

if [ "$part" = postgres ]; then
  : "${PROCESS_POSTGRES:?set PROCESS_POSTGRES to a PostgreSQL server, e.g. postgres://postgres:postgres@localhost:5432}"
  FEATURES=renox/postgres build browser-fixture bikeshop
  python3 tests/process/process.py
  python3 tests/process/examples.py
fi
