#!/usr/bin/env bash
# Injects faults into a running app (tests/chaos) and checks that it answers
# fast, reports them on /health and recovers without a restart.
#
#   tests/chaos/run.sh postgres   # needs docker; starts its own PostgreSQL
#   tests/chaos/run.sh sqlite     # needs python3 (to hold SQLite's lock)
#
# Run from the repository root. CI runs both (.github/workflows/ci.yml).
set -euo pipefail

MODE=${1:?usage: tests/chaos/run.sh postgres|sqlite}
PORT=${CHAOS_PORT:-3077}
PG_PORT=${CHAOS_PG_PORT:-55433}
PG_NAME=renox-chaos-pg
BASE="http://127.0.0.1:$PORT"
WORK=$(mktemp -d)
APP_PID=
FAILURES=0

cleanup() {
    [ -n "$APP_PID" ] && kill "$APP_PID" 2>/dev/null || true
    [ "$MODE" = postgres ] && docker rm -f "$PG_NAME" >/dev/null 2>&1 || true
    if [ "$FAILURES" -gt 0 ]; then
        echo "--- app log (last 60 lines)"
        tail -n 60 "$WORK/app.log" || true
    fi
    rm -rf "$WORK"
}
trap cleanup EXIT

log() { printf '\n== %s\n' "$*"; }
fail() {
    echo "FAIL: $*"
    FAILURES=$((FAILURES + 1))
}

# `request PATH` prints "STATUS SECONDS".
request() {
    curl -s -o /dev/null -m 60 -w '%{http_code} %{time_total}' "$BASE$1" || echo "000 60"
}

# `expect PATH STATUS MAX_SECONDS`: one request, answered with STATUS in time.
expect() {
    local got status secs
    got=$(request "$1")
    status=${got% *}
    secs=${got#* }
    if [ "$status" = "$2" ] && awk "BEGIN { exit !($secs <= $3) }"; then
        echo "ok   GET $1 -> $status in ${secs}s"
    else
        fail "GET $1 -> $status in ${secs}s (wanted $2 within $3s)"
    fi
}

# `until_status PATH STATUS SECONDS`: keeps asking until STATUS.
until_status() {
    local deadline=$((SECONDS + $3)) got
    while [ $SECONDS -lt $deadline ]; do
        got=$(request "$1")
        if [ "${got% *}" = "$2" ]; then
            echo "ok   GET $1 -> $2 again after recovery"
            return 0
        fi
        sleep 0.5
    done
    fail "GET $1 never answered $2 within $3s"
}

stat() {
    curl -s -m 10 "$BASE/stats" | python3 -c "import json, sys; print(json.load(sys.stdin)['$1'])"
}

# `until_stat KEY MIN SECONDS`: waits for a /stats counter to reach MIN.
until_stat() {
    local deadline=$((SECONDS + $3)) value=
    while [ $SECONDS -lt $deadline ]; do
        value=$(stat "$1" 2>/dev/null || true)
        if [ -n "$value" ] && [ "$value" -ge "$2" ]; then
            echo "ok   $1 = $value (>= $2)"
            return 0
        fi
        sleep 0.5
    done
    fail "$1 = ${value:-?}, wanted >= $2 within $3s"
}

# `until_no_pending SECONDS`: waits for the queue to empty (nothing stranded).
until_no_pending() {
    local deadline=$((SECONDS + $1)) value=
    while [ $SECONDS -lt $deadline ]; do
        value=$(stat pending 2>/dev/null || true)
        if [ "$value" = 0 ]; then
            echo "ok   no job left pending"
            return 0
        fi
        sleep 0.5
    done
    fail "${value:-?} job(s) still pending after $1s"
}

pg_ready() {
    local deadline=$((SECONDS + 60))
    until docker exec "$PG_NAME" pg_isready -U postgres -d chaos >/dev/null 2>&1; do
        [ $SECONDS -lt $deadline ] || { echo "PostgreSQL didn't start"; exit 1; }
        sleep 0.5
    done
    sleep 1
}

# ---------------------------------------------------------------- setup

case "$MODE" in
postgres)
    docker rm -f "$PG_NAME" >/dev/null 2>&1 || true
    docker run -d --name "$PG_NAME" -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=chaos \
        -p "$PG_PORT:5432" postgres:17-alpine >/dev/null
    pg_ready
    export DATABASE_URL="postgres://postgres:postgres@127.0.0.1:$PG_PORT/chaos"
    ;;
sqlite)
    export DATABASE_URL="sqlite://$WORK/chaos.db"
    ;;
*)
    echo "unknown mode $MODE" >&2
    exit 2
    ;;
esac

export APP_ENV=local APP_DEBUG=false APP_PORT=$PORT QUEUE_WORKERS=2 SCHEDULER=true \
    STORAGE_PATH="$WORK/storage" REQUEST_TIMEOUT=10 DATABASE_ACQUIRE_TIMEOUT=5 \
    DATABASE_STATEMENT_TIMEOUT=30 RUST_LOG=${RUST_LOG:-warn}

cargo build -q -p chaos
APP=target/debug/chaos
"$APP" migrate >/dev/null
"$APP" serve >"$WORK/app.log" 2>&1 &
APP_PID=$!
until_status /health 200 30

# ---------------------------------------------------------------- panics

log "a handler panics"
expect /panic 500 3
expect /read 200 2

log "a listener panics; the next listener still runs"
expect /emit 500 3
until_stat listeners 1 5

log "a job panics; the worker keeps going"
expect /dispatch-panicky 200 3
until_stat failed 1 15
expect "/dispatch?secs=0" 200 3
until_stat jobs_done 1 10

log "a scheduled task panics every second and keeps its schedule"
until_stat task_panics 3 10
until_stat ticks 3 10

# ---------------------------------------------------------------- faults

if [ "$MODE" = postgres ]; then
    log "PostgreSQL paused (hangs) during a request and before others"
    request /sleep >"$WORK/in-flight" &
    IN_FLIGHT=$!
    sleep 0.5
    docker pause "$PG_NAME" >/dev/null
    expect /read 500 8
    expect /health 503 4
    wait "$IN_FLIGHT"
    got=$(cat "$WORK/in-flight")
    if [ "${got% *}" = 500 ] && awk "BEGIN { exit !(${got#* } <= 12) }"; then
        echo "ok   the request in flight -> 500 in ${got#* }s (REQUEST_TIMEOUT=10)"
    else
        fail "the request in flight -> $got (wanted 500 within 12s)"
    fi
    docker unpause "$PG_NAME" >/dev/null
    until_status /read 200 15

    log "PostgreSQL stopped"
    docker stop "$PG_NAME" >/dev/null
    expect /read 500 8
    expect /health 503 4
    expect "/dispatch?secs=0" 500 8

    log "PostgreSQL started again: the app recovers without a restart"
    docker start "$PG_NAME" >/dev/null
    pg_ready
    until_status /read 200 30
    until_status /health 200 10
    done_before=$(stat jobs_done)
    expect "/dispatch?secs=0" 200 3
    until_stat jobs_done $((done_before + 1)) 20

    log "PostgreSQL restarted while a job runs"
    done_before=$(stat jobs_done)
    expect "/dispatch?secs=2" 200 3
    sleep 0.5
    docker restart -t 1 "$PG_NAME" >/dev/null
    pg_ready
    until_stat jobs_done $((done_before + 1)) 40
    until_no_pending 20
    ticks_before=$(stat ticks)
    until_stat ticks $((ticks_before + 2)) 10
else
    hold_lock() {
        python3 - "$WORK/chaos.db" "$1" <<'PY' &
import sqlite3, sys, time
db = sqlite3.connect(sys.argv[1], isolation_level=None, timeout=10)
db.execute("BEGIN IMMEDIATE")
time.sleep(float(sys.argv[2]))
db.execute("ROLLBACK")
PY
        LOCK_PID=$!
        sleep 0.5
    }

    log "SQLite held locked by another process"
    hold_lock 8
    expect /read 200 2
    expect /health 200 3
    expect /write 500 9
    wait "$LOCK_PID"
    expect /write 200 3

    log "SQLite locked while a job finishes: its outcome is written after the lock"
    done_before=$(stat jobs_done)
    expect "/dispatch?secs=1" 200 3
    hold_lock 7
    wait "$LOCK_PID"
    until_stat jobs_done $((done_before + 1)) 40
    until_no_pending 20
fi

log "still serving"
expect /read 200 2
expect /health 200 3

if [ "$FAILURES" -gt 0 ]; then
    echo
    echo "$FAILURES check(s) failed"
    exit 1
fi
echo
echo "chaos ($MODE): all checks passed"
