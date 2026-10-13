#!/usr/bin/env bash
# Runs the benchmark: each server in turn, in Docker, on the same two pinned
# CPU cores with the same memory limit, on the host's network (no port
# proxy), loaded by oha from other cores. Writes results/<date>.json and
# prints the table summarize.py makes of it. See README.md.
#
#   benchmarks/run.sh                 everything (about 8 minutes)
#   DURATION=30s CONNECTIONS=128 benchmarks/run.sh
#   ONLY="renox axum" benchmarks/run.sh
#
# Needs: Docker, oha (cargo install oha), python3, cargo (the Rust apps are
# built here in release mode and run in an ubuntu:24.04 container).
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
cd "$HERE"
DURATION=${DURATION:-15s}
WARMUP=${WARMUP:-5s}
CONNECTIONS=${CONNECTIONS:-64}
SERVER_CPUS=${SERVER_CPUS:-0,1}
LOAD_CPUS=${LOAD_CPUS:-4-7}
MEMORY=${MEMORY:-1g}
PORT=${PORT:-8080}
ONLY=${ONLY:-"renox axum laravel-fpm laravel-octane"}
TARGET=${CARGO_TARGET_DIR:-$HERE/target}
STAMP=$(date -u +%Y-%m-%dT%H-%M-%SZ)
OUT="$HERE/results/$STAMP.json"
WORK=$(mktemp -d)
trap 'docker rm -f bench-server >/dev/null 2>&1 || true; rm -rf "$WORK"' EXIT
mkdir -p "$HERE/results"

command -v oha >/dev/null || { echo "run.sh: oha is needed (cargo install oha)" >&2; exit 1; }
if ss -ltn | grep -q ":$PORT "; then echo "run.sh: port $PORT is taken" >&2; exit 1; fi

echo "== preparing"
python3 -I seed.py "$HERE/data/bench.db" >/dev/null
CARGO_TARGET_DIR="$TARGET" cargo build --release --quiet
docker build -q -f laravel/Dockerfile --target fpm -t renox-bench-laravel-fpm . >/dev/null
docker build -q -f laravel/Dockerfile --target octane -t renox-bench-laravel-octane . >/dev/null
docker pull -q ubuntu:24.04 >/dev/null

LIMITS=(--cpuset-cpus "$SERVER_CPUS" --memory "$MEMORY" --memory-swap "$MEMORY" --network host)
KEY="base64:$(head -c 32 /dev/urandom | base64)"

start() {
  case "$1" in
    renox)
      cp data/bench.db "$WORK/bench.db"; mkdir -p "$WORK/storage"; chmod -R a+rwX "$WORK"
      docker run -d --name bench-server "${LIMITS[@]}" -w /app \
        -v "$TARGET/release/bench-renox:/app/server:ro" -v "$HERE/renox-app/resources:/app/resources:ro" -v "$WORK:/work" \
        -e APP_ENV=production -e APP_DEBUG=false -e "APP_KEY=$KEY" -e APP_HOST=127.0.0.1 -e "APP_PORT=$PORT" \
        -e DATABASE_URL=sqlite:///work/bench.db -e STORAGE_PATH=/work/storage -e QUEUE_WORKERS=0 -e SCHEDULER=false \
        -e RUST_LOG=warn ubuntu:24.04 /app/server serve ;;
    axum)
      cp data/bench.db "$WORK/bench.db"; chmod -R a+rwX "$WORK"
      docker run -d --name bench-server "${LIMITS[@]}" -w /app \
        -v "$TARGET/release/bench-axum:/app/server:ro" -v "$HERE/axum-app/views:/app/views:ro" -v "$WORK:/work" \
        -e "APP_PORT=$PORT" -e DATABASE_URL=sqlite:///work/bench.db ubuntu:24.04 /app/server ;;
    laravel-fpm)
      docker run -d --name bench-server "${LIMITS[@]}" -e "NGINX_HTTP_PORT=$PORT" -e NGINX_ACCESS_LOG=/dev/null renox-bench-laravel-fpm ;;
    laravel-octane)
      docker run -d --name bench-server "${LIMITS[@]}" renox-bench-laravel-octane \
        php artisan octane:start --server=frankenphp --host=127.0.0.1 --port="$PORT" --workers=auto --max-requests=100000 ;;
  esac >/dev/null
}

# Milliseconds from `docker run` to the first 200 from /plaintext.
cold_start() {
  local t0 t1
  t0=$(date +%s%N)
  start "$1"
  until curl -sf -o /dev/null "http://127.0.0.1:$PORT/plaintext"; do
    sleep 0.01
    if (( ($(date +%s%N) - t0) / 1000000 > 60000 )); then echo "run.sh: $1 didn't start" >&2; docker logs bench-server >&2; exit 1; fi
  done
  t1=$(date +%s%N)
  echo $(( (t1 - t0) / 1000000 ))
}

# The container's memory now, in MiB.
memory() {
  docker stats --no-stream --format '{{.MemUsage}}' bench-server | awk '{print $1}' | python3 -I -c '
import sys, re
v = sys.stdin.read().strip(); n, u = re.match(r"([\d.]+)\s*([A-Za-z]+)", v).groups()
print(round(float(n) * {"B": 1/1048576, "KiB": 1/1024, "MiB": 1, "GiB": 1024}[u], 1))'
}

{
  echo "{"
  echo "  \"date\": \"$STAMP\","
  echo "  \"machine\": {\"cpu\": \"$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2 | sed 's/^ //')\", \"cores\": $(nproc), \"kernel\": \"$(uname -r)\"},"
  echo "  \"settings\": {\"duration\": \"$DURATION\", \"warmup\": \"$WARMUP\", \"connections\": $CONNECTIONS, \"server_cpus\": \"$SERVER_CPUS\", \"memory\": \"$MEMORY\", \"oha\": \"$(oha --version | awk '{print $2}')\"},"
  echo "  \"servers\": {"
} > "$OUT"

first=1
for server in $ONLY; do
  echo "== $server"
  docker rm -f bench-server >/dev/null 2>&1 || true
  cold=$(cold_start "$server")
  sleep 2
  idle=$(memory)
  case "$server" in
    renox) size=$(stat -c %s "$TARGET/release/bench-renox") ;;
    axum) size=$(stat -c %s "$TARGET/release/bench-axum") ;;
    *) size=$(docker exec bench-server du -sb /var/www/html | cut -f1) ;;
  esac
  [ $first = 1 ] || echo "," >> "$OUT"; first=0
  printf '    "%s": {"cold_start_ms": %s, "idle_mib": %s, "artifact_bytes": %s, "endpoints": {' "$server" "$cold" "$idle" "$size" >> "$OUT"
  efirst=1
  for endpoint in plaintext json "db?id=500" "page?id=500"; do
    url="http://127.0.0.1:$PORT/$endpoint"
    taskset -c "$LOAD_CPUS" oha -z "$WARMUP" -c "$CONNECTIONS" --no-tui "$url" >/dev/null
    peak_file="$WORK/peak"; echo 0 > "$peak_file"
    ( while true; do m=$(memory 2>/dev/null || echo 0); python3 -I -c "import sys; a,b=float(open(sys.argv[1]).read()),float(sys.argv[2]); open(sys.argv[1],'w').write(str(max(a,b)))" "$peak_file" "$m"; sleep 1; done ) &
    sampler=$!
    taskset -c "$LOAD_CPUS" oha -z "$DURATION" -c "$CONNECTIONS" --no-tui --output-format json "$url" > "$WORK/oha.json"
    kill $sampler 2>/dev/null; wait $sampler 2>/dev/null || true
    name=${endpoint%%\?*}
    [ $efirst = 1 ] || printf ',' >> "$OUT"; efirst=0
    python3 -I - "$WORK/oha.json" "$name" "$(cat "$peak_file")" >> "$OUT" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
s, p = d["summary"], d["latencyPercentiles"]
codes = d.get("statusCodeDistribution", {})
print(json.dumps({sys.argv[2]: {
    "rps": round(s["requestsPerSec"], 1),
    "p50_ms": round(p["p50"] * 1000, 3), "p99_ms": round(p["p99"] * 1000, 3),
    # Requests still in flight when the time is up are cut off by oha
    # ("aborted due to deadline", one per connection): not errors.
    "ok": codes.get("200", 0), "errors": sum(v for k, v in codes.items() if k != "200") + sum(v for k, v in d.get("errorDistribution", {}).items() if "deadline" not in k),
    "peak_mib": float(sys.argv[3]),
}})[1:-1], end="")
PY
    echo "   $name done"
  done
  echo "}}" >> "$OUT"
  docker rm -f bench-server >/dev/null
done
echo "  }" >> "$OUT"
echo "}" >> "$OUT"

python3 -I summarize.py "$OUT"
echo "results: $OUT"
