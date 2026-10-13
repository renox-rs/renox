#!/usr/bin/env bash
# Creates an app with `rnx new`, runs every generator in it, then builds and
# tests the result, so generated code that doesn't compile fails CI.
#
#   tests/cli/run.sh            # sqlite
#   tests/cli/run.sh postgres   # `rnx new --database postgres` (build and lint only)
#   KEEP=1 tests/cli/run.sh     # keep the apps and print where they are
#
#   E2E_POSTGRES=postgres://postgres:postgres@localhost:5432 tests/cli/run.sh postgres
#       With a PostgreSQL server there, the postgres apps also run their tests (in
#       fresh schemas of its `renox_test` database) and their commands, each app in its
#       own database `renox_e2e_<app>` (dropped and created again). The HTTP checks
#       (tests/cli/smoke.py) read the app's SQLite file, so they run with sqlite only.
#
# Apps are made with the `rnx new` options people combine, and names on both sides of
# "renox" (where imports sort: #124): shop (every generator), atlas (plain), pulse
# (--notifications), site (--tailwind), studio (--starter, with the database), desk
# (--starter --tailwind).
# Each must pass cargo fmt --check, clippy (but shop, whose generated items are dead code
# until used) and its tests (#143).
#
#   FROM_GIT=1 DOCKER=1 tests/cli/run.sh
#       The app depends on Renox from GitHub, pinned to this checkout's commit
#       (as for users; the commit must be pushed), and the Dockerfile from
#       `make:deploy` is built and started, and must answer /health.
#
# Run from the repository root. CI runs it (.github/workflows/ci.yml).
set -euo pipefail

DATABASE=${1:-sqlite}
REPO=$(pwd)
WORK=$(mktemp -d)
# Reuse the workspace's build of Renox's dependencies.
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$REPO/target/cli-e2e}
cleanup() {
    if [ -n "${SERVER:-}" ]; then kill "$SERVER" 2>/dev/null || true; fi
    if [ -n "${KEEP:-}" ]; then echo "apps kept in $WORK"; else rm -rf "$WORK"; fi
}
trap cleanup EXIT

step() { printf '\n== %s\n' "$*"; }

# Serves the app in this directory on $1 and drives it with tests/cli/smoke.py $2 (#142).
smoke() {
    local port=$1 scenario=$2 binary
    binary="$CARGO_TARGET_DIR/debug/$(basename "$(pwd)")"
    APP_PORT=$port QUEUE_WORKERS=0 SCHEDULER=false "$binary" > "$WORK/$scenario.log" 2>&1 &
    SERVER=$!
    for _ in $(seq 1 100); do
        curl -sf -o /dev/null "http://127.0.0.1:$port/health" && break
        sleep 0.2
    done
    python3 "$REPO/tests/cli/smoke.py" "$scenario" "http://127.0.0.1:$port" storage/app.db "$binary" \
        || { echo "--- the app's log"; tail -n 40 "$WORK/$scenario.log"; exit 1; }
    kill "$SERVER"
    wait "$SERVER" 2>/dev/null || true
    SERVER=
}

# Whether the apps can run (tests, commands): always on SQLite, on PostgreSQL only with
# a server in E2E_POSTGRES.
runs() { [ "$DATABASE" = sqlite ] || [ -n "${E2E_POSTGRES:-}" ]; }

# `rnx new <name> [options]` in $WORK, against this checkout (or GitHub with FROM_GIT).
new_app() {
    local name=$1
    shift
    cd "$WORK"
    if [ -n "${FROM_GIT:-}" ]; then
        "$RNX" new "$name" "$@"
    else
        "$RNX" new "$name" --renox-path "$REPO" "$@"
    fi
    cd "$name"
}

# On PostgreSQL, points the app at its own fresh database (DATABASE_URL wins over
# .env) and its tests at E2E_POSTGRES's renox_test database.
use_database() {
    if [ "$DATABASE" = postgres ] && [ -n "${E2E_POSTGRES:-}" ]; then
        psql -q "$E2E_POSTGRES/postgres" -c "DROP DATABASE IF EXISTS renox_e2e_$1" \
            -c "CREATE DATABASE renox_e2e_$1"
        export DATABASE_URL="$E2E_POSTGRES/renox_e2e_$1"
        export TEST_DATABASE_URL="$E2E_POSTGRES/renox_test"
    fi
}

# What a new app's own CI would run: formatting, lints and its tests.
check_app() {
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    if runs; then cargo test; else cargo build --all-targets; fi
}

cargo build -q -p renox-cli
RNX="$CARGO_TARGET_DIR/debug/rnx"

step "rnx new shop --database $DATABASE"
new_app shop --database "$DATABASE"
use_database shop
if [ "$(uname -m)" = x86_64 ] && command -v mold >/dev/null && command -v clang >/dev/null; then
    grep -q 'fuse-ld=mold' .cargo/config.toml
elif [ "$(uname -m)" = x86_64 ]; then test ! -e .cargo/config.toml; fi

step "rnx doctor (no build), and outside an app"
"$RNX" doctor --no-build | tee "$WORK/doctor.txt"
grep -q '✓ APP_KEY' "$WORK/doctor.txt"
if APP_KEY=short "$RNX" doctor --no-build >/dev/null; then echo "FAIL: a short APP_KEY passed"; exit 1; fi
(cd "$WORK" && "$RNX" doctor --no-build | grep -q 'not in an app directory')
grep '^renox' Cargo.toml

step "no template placeholder left in the new app"
if grep -rnE '\{\{[a-z_]+\}\}' . --exclude-dir=target; then
    echo "FAIL: placeholders left above"
    exit 1
fi
if [ -n "${FROM_GIT:-}" ]; then
    rev=$(sed -n 's/.*rev = "\([0-9a-f]*\)".*/\1/p' Cargo.toml)
    grep -q "renox/blob/$rev/CHEATSHEET.md" AGENTS.md # docs of the pinned commit
fi

step "every generator"
"$RNX" make:module catalog
"$RNX" make:model Book --module catalog --migration
"$RNX" make:module stock_movement
"$RNX" make:model StockMovement -m
"$RNX" make:model Invoice --module catalog --key ulid -m
"$RNX" make:model Supplier --module catalog --key string -m
"$RNX" make:job SendReceipt --module catalog
"$RNX" make:command catalog:import --module catalog
"$RNX" make:policy Book --module catalog
"$RNX" make:mail order_shipped
"$RNX" make:migration add_sku_to_products
"$RNX" make:module products --resource $(runs || echo --no-migrate) --fields "name:string price:money notes:text active:bool due_on:date"
"$RNX" make:module tags --resource --no-migrate
if grep -n '{%' resources/views/products/*.html resources/views/tags/*.html; then echo "FAIL: {% %} in generated views"; exit 1; fi
"$RNX" make:factory Book --module catalog
"$RNX" make:seeder DemoData
"$RNX" make:test Checkout
"$RNX" make:notification OrderShipped --module catalog
"$RNX" make:event OrderPlaced --module catalog
"$RNX" make:rule TaxId --module catalog
"$RNX" make:middleware StampRequests
"$RNX" make:component price_tag
"$RNX" make:component legacy_tag --macro
"$RNX" make:deploy

step "make:component --ui forwards to the app's ui:publish"
"$RNX" make:component --ui
test -f resources/views/components/ui.html
test -f public/css/renox-ui.css
# Again: the files are kept without --force (an error), written with it.
if "$RNX" make:component --ui; then
  echo "make:component --ui replaced the kit's files without --force" >&2
  exit 1
fi
"$RNX" make:component --ui --force
# Removed again: the app's own ui:publish runs further down on a clean app.
rm resources/views/components/ui.html public/css/renox-ui.css

step "key:generate on a fresh clone (no .env)"
mv .env "$WORK/env.bak"
"$RNX" key:generate
grep -q '^APP_KEY=base64:' .env
grep -q '^DATABASE_URL=' .env # the rest comes from .env.example
cp "$WORK/env.bak" .env

step "cargo fmt --check (what rnx new and every generator wrote)"
cargo fmt --check

# No clippy here: each generator's output stands alone, unused (dead code until the
# app uses it). The apps below are linted.
step "cargo build and test"
cargo build --all-targets
if runs; then
    cargo test
    step "the app's own commands"
    cargo run -q -- migrate
    "$RNX" doctor | grep -q 'Migrations: all ran'
    cargo run -q -- migrate:status
    cargo run -q -- db:check
    cargo run -q -- migrate:status | grep -q 'ran.*create_products_table'
    cargo run -q -- catalog:import
    # A typed command (clap): its flags, its --help, and a clear error.
    cargo run -q -- catalog:import --dry-run | grep -q 'dry run'
    cargo run -q -- catalog:import --help | grep -q -- '--dry-run'
    if cargo run -q -- catalog:import --bogus 2>"$WORK/err.txt"; then
        echo "FAIL: an unknown flag was accepted"
        exit 1
    fi
    grep -q 'Usage: catalog:import' "$WORK/err.txt"
    cargo run -q -- route:list

    step "make:migration --auto: add a column, then an index, from the models"
    edit_model() {
        python3 - "$1" "$2" <<'PY'
import sys
old, new = sys.argv[1], sys.argv[2]
p = "src/app/catalog/model.rs"
s = open(p).read()
assert old in s, "missing: " + old
open(p, "w").write(s.replace(old, new, 1))
PY
    }
    edit_model '    pub updated_at: Option<DateTime>,
' '    pub updated_at: Option<DateTime>,
    pub sku: Option<String>,
'
    "$RNX" make:migration --auto add_sku
    edit_model '#[model(table = "books")]' '#[model(table = "books", index(sku))]'
    "$RNX" make:migration --auto index_sku
    cargo run -q -- migrate
    cargo run -q -- db:check
    cargo test
    "$RNX" make:migration --auto | grep -q 'Nothing to change'

    if [ "$DATABASE" = sqlite ]; then
        step "the app over HTTP: every page, a --resource module's forms (tests/cli/smoke.py)"
        smoke 3191 resources
    fi

    cargo run -q -- db:seed
    cargo run -q -- ui:publish
    test -f resources/views/components/ui.html
fi

if [ -n "${DOCKER:-}" ]; then
    step "docker build and run"
    IMAGE=renox-cli-e2e
    NAME=renox-cli-e2e
    PORT=${E2E_PORT:-3088}
    docker build -t "$IMAGE" .
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    docker run -d --name "$NAME" -p "$PORT:3000" \
        -e APP_KEY="$(grep '^APP_KEY=' .env | cut -d= -f2-)" -e APP_URL="http://127.0.0.1:$PORT" \
        "$IMAGE" >/dev/null
    trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true; cleanup' EXIT
    status=000
    for _ in $(seq 60); do
        status=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/health" || true)
        [ "$status" = 200 ] && break
        sleep 1
    done
    if [ "$status" != 200 ]; then
        docker logs "$NAME" | tail -40
        echo "FAIL: /health answered $status"
        exit 1
    fi
    curl -s "http://127.0.0.1:$PORT/health"
    echo
    curl -sf -o /dev/null "http://127.0.0.1:$PORT/catalog" && echo "ok   /catalog"
    docker image ls "$IMAGE" --format 'image size: {{.Size}}'
fi

step "rnx new atlas --database $DATABASE (plain, a name before \"renox\": #124)"
new_app atlas --database "$DATABASE"
use_database atlas
# The pages are written with tags (#493): no kit macro import, and no {% %} in the home page.
if grep -rn 'from "renox/' resources/views/home resources/views/errors; then echo "FAIL: kit macros in the plain app's pages"; exit 1; fi
if grep -n '{%' resources/views/home/index.html; then echo "FAIL: {% %} in the plain app's home page"; exit 1; fi
check_app
cargo run --quiet -- view:check

step "rnx new pulse --notifications --database $DATABASE (the bell in the plain app: #151)"
new_app pulse --notifications --database "$DATABASE"
use_database pulse
grep -q '.notifications())' src/lib.rs
grep -q '<rx-notification-bell' resources/views/layouts/app.html
check_app

if [ "$DATABASE" = sqlite ]; then
    step "rnx new site --tailwind (downloads the pinned Tailwind CLI once)"
    new_app site --tailwind
    test -f resources/css/app.css
    test ! -e public/app.css
    grep -q "asset('css/app.css')" resources/views/layouts/app.html
    grep -q 'text-emerald-700' public/css/app.css # built from the views
    "$RNX" tailwind --minify
    grep -q 'text-emerald-700' public/css/app.css
    check_app

    step "rnx new desk --starter --tailwind (a name before \"renox\")"
    new_app desk --starter --tailwind
    test -f resources/css/app.css
    check_app
fi

step "rnx new studio --starter --database $DATABASE (a name after \"renox\": #124)"
new_app studio --starter --database "$DATABASE"
use_database studio
test -f src/app/users/mod.rs
grep -q '.module(Permissions)' src/lib.rs
# The starter's views are written with tags (#516): no `{% %}` anywhere (renox-cli's test
# `no_stub_template_has_a_statement` checks it too), and no page imports the kit's macros.
if grep -rn 'from "renox/' resources/views/home resources/views/dashboard resources/views/users resources/views/activity resources/views/errors resources/views/components; then
    echo "FAIL: kit macros in the starter's pages"
    exit 1
fi
if grep -rn '{%' resources/views; then
    echo "FAIL: a template statement in the starter's views"
    exit 1
fi
test -f resources/views/components/figures.html
check_app
cargo run --quiet -- view:check
if runs; then
    cargo run --quiet -- migrate

    if [ "$DATABASE" = sqlite ]; then
        step "the starter app over HTTP: sign-up, verification, roles (tests/cli/smoke.py)"
        smoke 3192 starter
    fi

    cargo run --quiet -- db:seed
    cargo run --quiet -- users:admin member@example.com
fi

echo
if [ "$DATABASE" = sqlite ]; then
    echo "cli e2e: the generated apps are formatted, lint-free, pass their tests, run their commands and work over HTTP"
elif runs; then
    echo "cli e2e ($DATABASE): the generated apps are formatted, lint-free, pass their tests and run their commands"
else
    echo "cli e2e ($DATABASE): the generated apps are formatted, lint-free and build"
fi
