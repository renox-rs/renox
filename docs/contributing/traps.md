# Problems hit so far, and their fixes

> Every trap hit while building Renox, so nobody has to rediscover it. Read it before touching the area. (Section 6 of the old CLAUDE.md.)
> Start at [CLAUDE.md](../../CLAUDE.md).

## 6.1 Tooling and environment
- **`pkill -f <pattern>` / `pgrep -f` killed my own shell** (exit code 144, twice) because the
  pattern matched the command line of the shell running it. Stale servers then kept port 3000 and
  new ones failed with "Address already in use", so the next test silently hit the *old* binary.
  Fix: start background processes with `& echo $! > pidfile` and `kill $(cat pidfile)`; check
  `ps` for leftovers. Never use `pkill -f`/`pgrep -f`.
- **Piping a node script into `| head -1` killed it with SIGPIPE** before it saved its screenshot.
  Redirect to a file and `head` the file instead.
- **`cargo test` doesn't rebuild example binaries** used for live tests; run `cargo build` first.
- **`gh pr edit` fails** with a GraphQL error about deprecated Projects (classic). Use REST:
  `gh api -X PATCH repos/renox-rs/renox/pulls/N -F body=@file`.
- **`target/` grows to ~100 GB** over a few milestones and fills the disk (link errors, "No space
  left on device"); `cargo clean` before the full two-database run.
- **Browser testing** is tests/browser (headless Chrome over the DevTools protocol, Node 24's
  `node:test`, no npm packages; lib/cdp.mjs launches Chrome, drives pages and collects console
  errors, exceptions and CSP violations). Write a test there rather than a one-off script; it
  found bugs unit tests missed (§6.4). A sandboxed `<iframe>` (mail preview) can't be read from
  the parent: check it via screenshot.

- **Other agent sessions use this checkout too** (2026-10-04: a session working on another
  project ran `git checkout origin/main` here, and a commit landed on a detached HEAD; it
  also builds into the same `target/`, so a test binary looked fresh but lacked the new
  module). Work on an issue in its own worktree with its own target directory:
  `git worktree add -b feat/issue-N-… <scratchpad>/wt-N origin/main`, and
  `CARGO_TARGET_DIR=/home/developer/workspace/renox-target-wt` for its builds. Check
  `git branch --show-current` before committing.

## 6.2 `rnx serve` restart loop
The `notify` watcher reports **OPEN** events (inotify `OPEN` is in notify 8's mask), so `cargo build`
reading `src/` triggered a restart, which triggered a build… Fix in `serve.rs`: events only wake the
loop; a restart happens only if a **fingerprint** (path, mtime, size of watched files) changed.
Also: build first and swap the running app only on success; run `<exe> migrate` before starting.

## 6.3 GitHub PR stacking
PR #4 was based on `m2-database` (stacked on #3). #3 was merged **without deleting the branch**, so
GitHub didn't retarget #4, and merging #4 put the change into `m2-database`, not `main`. Fixed by
opening #5 with the same commit to `main`. Lesson: don't stack; if you must, retarget first.

## 6.4 Bugs found only in a real browser
- 422 validation responses were marked `isError = false` in `htmx:beforeSwap`, so htmx called the
  request "successful" and the form's Alpine `@htmx:after-request` handler **reset the form**,
  wiping the user's input. Keep 422s as errors; only set `shouldSwap = false`.
- A blank text field made serde report "missing field" before any rule ran → one unlabelled error.
  Fix in `validation/extract.rs`: drop empty inputs (so `Option` fields become `None`), and if
  deserialization reports a missing field, put it back as `""` and retry, so rules and labels run.
  A blank number then becomes "required", a wrong type "must be a number".
- Focus went to the alphabetically first field (errors are a `BTreeMap`); now the first
  `[aria-invalid]` in DOM order.
- A translated validation message named the field differently from the translated page label
  → localised labels in auth forms.
- The reset-password mail button reused the page's save-button label → uses the subject.
- A scripted edit meant to add `.image()` to the guestbook's photo rule silently didn't apply, so a
  text file named `.png` was accepted. All framework tests passed; only the headless-Chrome upload
  check showed it. Keep browser checks for UI features.
- renox-grid.js declared two functions named `save` (column preferences, inline-edit cells); the
  second replaced the first, so every column-menu action threw and saved nothing. Found by
  tests/browser/grid.test.mjs (#267); tests/browser/assets.test.mjs now refuses a top-level
  function declared twice in any shipped JS.
- Live validation of a list field (`tags`) answered with no errors while its items failed
  (`each(…)` reports on `tags.0`): the answer looked up the exact key only. Found by
  tests/browser/renoxjs.test.mjs; it includes the items' errors now.
- Writing browser tests (tests/browser): headless Chrome has no hovering mouse (`(hover:
  hover)` is false, so tooltips never show) unless started with `--blink-settings=…HoverType…`;
  each open tab keeps its live-reload stream and Chrome allows six connections per host (close
  every page, `browser.with` does); the browser's own checks (`required`, `type=email`) stop a
  form before the server sees it (give test forms `novalidate`); `hx-confirm`'s native dialog
  blocks the page until `Page.handleJavaScriptDialog` answers it.

## 6.5 Scripted edits go wrong silently
This happened in M6b, M6c, M7 and M9a:
- Python string replacements after `cargo fmt` did nothing (rustfmt had re-wrapped the target).
- A Python heredoc with nested `\"` quoting failed to parse, so *nothing* was applied.
- A multi-line `perl -0pi` regex with a repeated group (`(\s*\.bind(..))*?`) kept only the *last*
  capture and dropped `.bind(key)` from the cache lookup; a test caught it.

What works: prefer the Edit tool on freshly read content; for scripts, write the Python to a file
and `assert old in s` before each replace; rewrite a small file whole rather than patch it in many
places. Afterwards `grep` for the new text, run `cargo fmt --all` (CI failed once on an unformatted
test), and after regex rewrites of call chains audit with `git diff -U0 | grep -E '^[-+].*\.bind\('`
(removed binds must match added ones).

## 6.6 Slow tests (measured, fixed)
Unoptimised Argon2 made every login/registration slow (auth tests 3.9 s) and ten integration-test
binaries each paid a full link. The workspace `Cargo.toml` builds `argon2` with `opt-level = 3` in
the dev profile; new apps get the same plus `blake2` from `stubs/Cargo.toml.stub` (profiles only
apply at a workspace root). The integration tests are one binary. Result: rebuild after a core
change 29 s → 7 s, full run 19 s → 6 s.
The workspace's dev profile has `debug = "line-tables-only"` (as `rnx new` apps do): full
debug info made the `it` binary 415 MB and its link peak at 1.7 GB, and a workspace build
with 8 link jobs ran the 15 GB machine out of memory (systemd-oomd killed the terminal and
the agent in it, twice). Now 150 MB and 0.76 GB. Run heavy builds with `-j 4`, one at a
time, ideally in their own scope (`systemd-run --user --scope -p MemoryHigh=9G …`) so oomd
picks the build, not the terminal.

## 6.7 Library/API traps
- **sqlx 0.9:** dynamic SQL needs `sqlx::AssertSqlSafe(string)`; `SqliteArguments` has no lifetime;
  multi-statement SQL uses `sqlx::raw_sql`. `sqlite::memory:` gives each pooled connection its own
  DB → pool of exactly 1 connection with no idle timeout (see `db::connect`). `PRAGMA foreign_keys`
  is per connection: `migrate:fresh` acquires one connection for the whole drop.
- **axum 0.8:** paths are `/{id}` and `/{*rest}`; `Option<Extractor>` needs
  `OptionalFromRequestParts`; `route_layer` only wraps routes added *before* it (so
  `.require_auth()` must come after the routes it guards); middleware and handler futures must be
  `Send` (§4.2).
- **MiniJinja 2.24:** `eval_to_state` is deprecated → `render_captured_to(ctx, io::sink())` +
  `with_state_mut(|s| s.render_block(..))` for fragments. `merge_maps`: the **last** map wins
  (lookups go in reverse). `tojson`/`urlencode` need the `json`/`urlencode` features (enabled).
  Booleans render as `True`/`False`: use `{% if %}` in templates/tests.
- **argon2 0.6:** `Argon2::default().hash_password(bytes)` (needs default `getrandom` feature),
  verify with `password_hash::phc::PasswordHash`. Hashing runs in `spawn_blocking`. Unknown emails
  are verified against a static dummy hash to equalise timing.
- **Crypto crate versions:** our `sha2 0.11` + `hmac 0.13` share `digest 0.11`; `cookie` pulls the
  older `sha2 0.10/hmac 0.12`. That's fine, but don't mix types across them.
- **Stable clippy** wants let-chains (`if let … && cond {}`) instead of nested ifs.
- **`cargo doc --workspace` output collision:** a binary and a library with the same name
  (`renox`) wrote to the same `target/doc/renox`, failing CI randomly. Resolved by the `rnx` rename.
- **Windows CI:** Git checks out with CRLF, so `include_str!` content ends in `\r\n`; compare
  trimmed text in tests.
- **`Option<T>` has a std `inspect` method,** so call `FieldValue::inspect(&opt)` explicitly in
  tests; generic code in the validator is unaffected.
- **`MultipartError` already carries the right status** (e.g. 413 over the body limit): return
  `err.into_response()`, don't wrap it in `Error::BadRequest`.
- **object_store** needs `with_allow_http` for `http://` endpoints (a local MinIO/SeaweedFS failed
  on every request until the S3 CI job caught it). MinIO's images are gone from Docker Hub/quay;
  use `chrislusf/seaweedfs` (bucket via `weed shell`).

## 6.8 PostgreSQL traps
- sqlx decodes strictly: `i64` needs `BIGINT` (a plain `INTEGER` column is INT4 and fails), and
  `SELECT 1` is INT4 (`/health` returned 503 until it stopped decoding the ping). `SUM(bigint)` is
  `NUMERIC`: write `CAST(SUM(x) AS BIGINT)`.
- sqlx sends parameters in binary with a declared type: a text-typed `NULL` or date string can't
  go into a `TIMESTAMPTZ`, hence the typed `DbValue` variants and the OID-0 untyped `NULL`.
  (Sending text with OID 0 does *not* work for non-text columns: the bytes are binary-format.)
- PostgreSQL keeps microseconds: `db::now()` truncates to them, or a saved model won't equal the
  row read back.
- `citext` doesn't help: `citext_col = $1` with a text parameter compares as text (case-sensitive).
- Every test run leaves its `renox_test_…` schemas behind. After four days of runs (8,355
  schemas) the container's default 64 MB `/dev/shm` filled up ("could not resize shared memory
  segment … No space left on device"), a backend segfaulted and the server went into recovery,
  so 273 tests failed at once with "not yet accepting connections". Recreate the container
  (`docker stop renox-pg`, then the `docker run` above, which sets `--shm-size=512m`) when a
  PostgreSQL run fails everywhere at once.
- The worker arms `Notify::notified()` *before* querying, so a dispatch during the query isn't
  lost (it was, under PostgreSQL's slower round trips).

## 6.9 Pre-1.0 audit and hardening (M13–M16)
- Findings with IDs (W* web, D* data/background, A* Laravel gaps) and the chaos baseline are in
  `docs/audit/2026-09-pre-1.0.md`; ROADMAP M13/M14 use the same IDs.
- Every audit probe is now a passing test on main: `it/web_security.rs`, `it/data_resilience.rs`,
  `it/background_resilience.rs`, and the `tests/chaos` app. A new finding lands the same way: a
  failing test first, then the fix.
- docs/operations.md's failure table is kept in sync with `tests/chaos/run.sh`.

## 6.10 Security rules
- Never store API tokens or secrets in the repo, memory or notes. Publishing to crates.io needs
  Renoxium to run `cargo login` themselves (and a verified email on the account).
- Never send Renoxium's email address to third-party services (e.g. in a User-Agent); use a
  neutral one like `renox-deps-check`.
