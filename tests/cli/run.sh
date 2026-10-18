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
# (--starter --tailwind). `--from` (#35) is run with a template made on the spot, from a path and from a
# git+file:// repository, and its error cases.
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

step "make:pwa (#33): offline page, icons and .pwa(..), refused twice, kept by --force"
"$RNX" make:pwa
test -s resources/views/offline.html
for icon in icon-192.png icon-512.png icon-maskable-512.png; do test -s "public/icons/$icon"; done
grep -q '\.pwa(' src/lib.rs
if grep -q '{{title}}' resources/views/offline.html; then echo "FAIL: placeholder left in offline.html"; exit 1; fi
cp src/lib.rs "$WORK/pwa-lib.rs"
if "$RNX" make:pwa 2>/dev/null; then echo "FAIL: make:pwa overwrote existing files without --force"; exit 1; fi
cmp src/lib.rs "$WORK/pwa-lib.rs"
"$RNX" make:pwa --force
test "$(grep -c '\.pwa(' src/lib.rs)" = 1 # forced files, but one call
# The shop's check_app below builds and tests the app with the call in place.

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

step "rnx make:package (#34): a package in a fresh app, with --resource and --with assets,commands"
new_app ledger --database "$DATABASE"
use_database ledger
cp Cargo.toml "$WORK/ledger-Cargo.toml"
cp src/lib.rs "$WORK/ledger-lib.rs"
# Refused with nothing written: --with admin (needs a newer renox), an unknown --with value,
# an invalid name.
if "$RNX" make:package invoicing --with admin 2>"$WORK/err.txt"; then echo "FAIL: --with admin was accepted"; exit 1; fi
grep -q 'needs a newer renox' "$WORK/err.txt"
if "$RNX" make:package invoicing --with nope 2>/dev/null; then echo "FAIL: an unknown --with value was accepted"; exit 1; fi
if "$RNX" make:package "Bad Name" 2>/dev/null; then echo "FAIL: an invalid name was accepted"; exit 1; fi
test ! -e crates
cmp Cargo.toml "$WORK/ledger-Cargo.toml"
cmp src/lib.rs "$WORK/ledger-lib.rs"

"$RNX" make:package invoicing --with assets,commands --resource invoices --fields "number:string total:money"
for f in Cargo.toml build.rs README.md src/lib.rs src/texts.rs views/invoicing/index.html tests/package.rs \
    src/assets.rs src/commands.rs src/invoices/mod.rs views/invoicing/invoices/index.html tests/invoices.rs; do
    test -f "crates/invoicing/$f"
done
# Migrations carry a timestamp and the package's name, never a 0001 prefix.
ls crates/invoicing/migrations | grep -qE '^[0-9]{14}_create_invoicing_entries_table\.up\.sql$'
ls crates/invoicing/migrations | grep -qE '^[0-9]{14}_create_invoicing_invoices_table\.up\.sql$'
if ls crates/invoicing/migrations | grep -q '^0001'; then echo "FAIL: a 0001 migration in the package"; exit 1; fi
grep -q '^members = \[".", "crates/invoicing"\]' Cargo.toml
grep -q '^invoicing = { path = "crates/invoicing" }' Cargo.toml
grep -q '\.module(invoicing::Invoicing)' src/lib.rs
if grep -rnE '\{\{[a-z_]+\}\}' crates/invoicing --exclude-dir=target; then echo "FAIL: placeholders left above"; exit 1; fi
# An existing target is refused without changing anything.
cp Cargo.toml "$WORK/ledger-Cargo.toml"
if "$RNX" make:package invoicing 2>/dev/null; then echo "FAIL: an existing package was overwritten"; exit 1; fi
cmp Cargo.toml "$WORK/ledger-Cargo.toml"

# A second package: the workspace gets it appended, the first stays, each is listed once.
"$RNX" make:package archive
grep -q '^members = \[".", "crates/invoicing", "crates/archive"\]' Cargo.toml
test "$(grep -c '"crates/invoicing"' Cargo.toml)" = 2 # the member and the dependency

# What the package's own CI runs, then the app's.
(cd crates/invoicing && cargo fmt --check && cargo clippy --all-targets -- -D warnings)
(cd crates/archive && cargo fmt --check && cargo clippy --all-targets -- -D warnings)
check_app
cargo clippy --workspace --all-targets -- -D warnings
cargo run --quiet -- view:check
if runs; then
    cargo test -p invoicing
    cargo test -p archive
    cargo run --quiet -- migrate
    cargo run --quiet -- migrate:status | grep -q 'ran.*create_invoicing_invoices_table'
    cargo run --quiet -- db:check
    cargo run --quiet -- invoicing:status
    if [ "$DATABASE" = sqlite ]; then
        step "the package over HTTP: its page and its resource (tests/cli/smoke.py)"
        smoke 3193 package
    fi
fi

step "rnx make:package in a virtual workspace: --register, renox.workspace = true"
cd "$WORK"
mkdir vw
cd vw
"$RNX" new platform --renox-path "$REPO" --database "$DATABASE" >/dev/null
renox_line=$(grep '^renox' platform/Cargo.toml)
printf '[workspace]\nmembers = ["platform"]\nresolver = "2"\n\n[workspace.dependencies]\n%s\n' "$renox_line" > Cargo.toml
cp platform/Cargo.toml "$WORK/platform-Cargo.toml"
cp platform/src/lib.rs "$WORK/platform-lib.rs"
"$RNX" make:package reports
grep -q '"crates/reports"' Cargo.toml
grep -q '^renox.workspace = true' crates/reports/Cargo.toml
# The app is left alone without --register.
cmp platform/Cargo.toml "$WORK/platform-Cargo.toml"
cmp platform/src/lib.rs "$WORK/platform-lib.rs"
if "$RNX" make:package ledgers --register missing 2>/dev/null; then echo "FAIL: --register named a crate that isn't there"; exit 1; fi
test ! -e crates/ledgers
"$RNX" make:package audit --register platform
grep -q '^members = \["platform", "crates/reports", "crates/audit"\]' Cargo.toml
grep -q '^audit = { path = "../crates/audit" }' platform/Cargo.toml
grep -q '\.module(audit::Audit)' platform/src/lib.rs
if grep -q 'reports' platform/Cargo.toml; then echo "FAIL: reports was registered without --register"; exit 1; fi
cargo build -p reports -p audit

step "rnx new --from (#35): a template from a path and from a repository, built and tested"
cd "$WORK"
TPL="$WORK/acme-template"
REPO_TPL="$WORK/acme-template-repo"
MARKER="$WORK/template-code-ran"
mkdir -p "$TPL/src" "$TPL/tests"
cat > "$TPL/Cargo.toml" <<'EOF'
[package]
name = "{{crate_name}}"
version = "0.1.0"
edition = "2021"

[dependencies]
{{renox_dependency}}
EOF
cat > "$TPL/src/lib.rs" <<'EOF'
//! Other double-brace text, like {{ app.locale }} and {{unknown}}, stays as written.
pub const NAME: &str = "{{name}}";
pub const COMPANY: &str = "{{company}}";
pub const KEPT: &str = "{{unknown}}";
EOF
cat > "$TPL/tests/template.rs" <<'EOF'
#[test]
fn placeholders_were_filled() {
    assert_eq!({{crate_name}}::COMPANY, "Acme Inc");
    assert_eq!({{crate_name}}::KEPT, "{{unknown}}");
    assert!(!{{crate_name}}::NAME.contains("{{"));
}
EOF
printf 'APP_KEY={{app_key}}\nAPP_NAME={{title}}\nDATABASE_URL={{database_url}}\n' > "$TPL/.env"
printf 'APP_KEY={{app_key}}\n' > "$TPL/.env.example"
printf '\211PNG\0{{name}}' > "$TPL/logo.png"
printf 'target/\n' > "$TPL/.gitignore"
printf 'scratch\n' > "$TPL/notes.bak"
# Code that would leave a marker file if anything ran it.
printf 'fn main() { std::fs::write("%s", "x").unwrap(); }\n' "$MARKER" > "$TPL/build.rs"
printf '#!/bin/sh\ntouch "%s"\n' "$MARKER" > "$TPL/setup.sh"
chmod +x "$TPL/setup.sh"
cat > "$TPL/renox-template.toml" <<'EOF'
drop = ["*.bak"]

[[ask]]
key = "company"
question = "Company name?"
EOF
# The same template as a repository: tag v1, then one more commit.
cp -r "$TPL" "$REPO_TPL"
git -C "$REPO_TPL" init --quiet
git -C "$REPO_TPL" add -A
git -C "$REPO_TPL" -c user.name=t -c user.email=t@example.com commit --quiet -m one
git -C "$REPO_TPL" tag v1
git -C "$REPO_TPL" -c user.name=t -c user.email=t@example.com commit --quiet --allow-empty -m two

# What every app made from the template must look like.
check_template_app() {
    local app=$1
    test -d "$app"
    test ! -e "$app/.git"
    test ! -e "$app/renox-template.toml"
    test ! -e "$app/notes.bak"
    test ! -e "$MARKER"
    grep -q 'pub const COMPANY: &str = "Acme Inc";' "$app/src/lib.rs"
    grep -q 'pub const KEPT: &str = "{{unknown}}";' "$app/src/lib.rs"
    grep -q '{{ app.locale }}' "$app/src/lib.rs"
    grep -Eq '^APP_KEY=.{20,}' "$app/.env"
    grep -q '^APP_KEY=$' "$app/.env.example"
    cmp "$app/logo.png" "$TPL/logo.png"
    if grep -rnE '\{\{(name|title|crate_name|renox_dependency|database_url|app_key|company)\}\}' "$app" \
        --exclude=logo.png --exclude-dir=target; then
        echo "FAIL: placeholders left above"
        exit 1
    fi
}

# --set answers the prompt (there is no terminal here).
"$RNX" new acme-path --renox-path "$REPO" --database "$DATABASE" --from "$TPL" --set company="Acme Inc"
check_template_app acme-path
"$RNX" new acme-tag --renox-path "$REPO" --database "$DATABASE" --from "git+file://$REPO_TPL#v1" --set company="Acme Inc"
check_template_app acme-tag
# Without #ref: the default branch.
"$RNX" new acme-head --renox-path "$REPO" --database "$DATABASE" --from "git+file://$REPO_TPL" --set company="Acme Inc"
check_template_app acme-head
# The generated app builds and its tests pass.
for app in acme-path acme-tag; do
    cd "$WORK/$app"
    use_database "${app//-/_}"
    if runs; then cargo test; else cargo build --all-targets; fi
done
cd "$WORK"

step "rnx new --from: clear errors, nothing left behind"
# Building the apps above ran the template's build.rs (it is app code now), which
# wrote the marker. From here on, only `rnx new` runs: it must write nothing.
rm -f "$MARKER"
# fail_new <expected text> <rnx new args...>: must fail, say the text, make no directory.
fail_new() {
    local want=$1 out
    shift
    if out=$("$RNX" new "$@" 2>&1); then echo "FAIL: rnx new $* succeeded"; exit 1; fi
    printf '%s\n' "$out" | grep -qF -- "$want" || { echo "FAIL: '$want' not in: $out"; exit 1; }
}
fail_new "--set" broken --renox-path "$REPO" --from "$TPL"
fail_new "nope" broken --renox-path "$REPO" --from "git+file://$REPO_TPL#nope" --set company=x
sha=$(git -C "$REPO_TPL" rev-parse HEAD)
fail_new "SHA" broken --renox-path "$REPO" --from "git+file://$REPO_TPL#$sha" --set company=x
fail_new "not a directory" broken --renox-path "$REPO" --from "$WORK/no-such-template"
fail_new "nothing" broken --renox-path "$REPO" --from "$TPL" --set company=x --set nothing=1
test ! -e broken
fail_new "already exists" acme-path --renox-path "$REPO" --from "$TPL" --set company=x
# No git on PATH: a git+ source is a clear error.
if out=$(PATH=/nonexistent "$RNX" new broken --renox-path "$REPO" \
    --from "git+file://$REPO_TPL#v1" --set company=x 2>&1); then
    echo "FAIL: succeeded without git"
    exit 1
fi
printf '%s\n' "$out" | grep -q 'was not found on PATH' || { echo "FAIL: no clear error without git: $out"; exit 1; }
test ! -e broken
# --from clashes with the built-in starters.
for clash in --starter --notifications --tailwind; do
    if "$RNX" new broken --from "$TPL" "$clash" >/dev/null 2>&1; then echo "FAIL: --from with $clash"; exit 1; fi
done
test ! -e broken
# Nothing in the template ran, in any of the above.
test ! -e "$MARKER"

echo
if [ "$DATABASE" = sqlite ]; then
    echo "cli e2e: the generated apps are formatted, lint-free, pass their tests, run their commands and work over HTTP"
elif runs; then
    echo "cli e2e ($DATABASE): the generated apps are formatted, lint-free, pass their tests and run their commands"
else
    echo "cli e2e ($DATABASE): the generated apps are formatted, lint-free and build"
fi
