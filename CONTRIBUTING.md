# Contributing to Renox

Thanks for helping. Bug reports, docs fixes, examples and code are all welcome.

## Before you start

- **A bug:** open an issue with the **Bug** form: the smallest app or test that shows it, what
  you expected and what happened. For a security problem, see [SECURITY.md](SECURITY.md)
  instead.
- **A feature:** open a **User story** first ("As a … I want … so that …", with acceptance
  criteria). Renox keeps a small surface on purpose (see "Not planned" and "Decisions" in
  [ROADMAP.md](ROADMAP.md)), and it's better to agree on the API before you write it.
- **A question or an idea not ready for a story:** [Discussions](https://github.com/renox-rs/renox/discussions).
- **Docs and examples:** send a pull request directly.

## How work is tracked

Everything not done yet lives on GitHub, so there is one place to look:

- **Issues** are the work: bugs, user stories (a large feature is an epic whose parts are
  sub-issues) and tasks. Labels say the type (`bug`, `story`, `task`), the area (`area: cli`,
  `area: db`, …) and the priority (`P1` must, `P2` should, `P3` could). `good first issue`
  marks a small, well-described one.
- **Milestones** are releases (`1.0.0`, `1.1`, …): what ships together.
- **[The project board](https://github.com/orgs/renox-rs/projects)** shows every open issue
  by status: Backlog, Ready, In progress, In review, Done.
- **Pull requests** close their issue (`Closes #N` in the description), on a branch named after
  it: `fix/issue-N-short-name` for a bug, `feat/issue-N-short-name` otherwise.
- **[ROADMAP.md](ROADMAP.md)** keeps the principles, the record of what each milestone built and
  why ("Decisions"); new plans start as issues. **[CHANGELOG.md](CHANGELOG.md)** lists what
  changed in each release.

## Set up

You need Rust 1.94 or later (the MSRV; `rust-version` in `Cargo.toml`). Docker is only needed
for the PostgreSQL, S3 and chaos tests.

```bash
git clone https://github.com/renox-rs/renox && cd renox
cargo test --workspace
```

To try a change to the framework or the generators in a real app, make the app against your
checkout with `rnx new --renox-path DIR`: its `Cargo.toml` then uses the checkout's
`crates/renox` as a path dependency, so it sees your changes without a commit. Make it outside
the checkout (an app inside it would land in the workspace):

```bash
cargo build -p renox-cli                                  # in the checkout
cd .. && renox/target/debug/rnx new demo --renox-path renox
```

[CLAUDE.md](CLAUDE.md) is the short guide (the rules that always apply) and points to
[docs/contributing/](docs/contributing/): the architecture, the conventions and the problems
solved so far. Read the file for the area you change before changing anything large; it's
written for human contributors as much as for coding agents.

## What every change needs

CI runs all of these; running them first saves a round trip:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo test --workspace
```

- **Tests.** New behaviour gets a test, and a fix gets a test that failed before it.
  Integration tests live in `crates/renox/tests/it/` and use `renox::testing::TestApp`.
  Include the negative cases: bad input, a forged request, a failing dependency.
- **Both databases.** Anything that touches SQL must pass on PostgreSQL too:
  ```bash
  docker run -d --rm --name renox-pg --shm-size=512m -e POSTGRES_PASSWORD=postgres \
      -e POSTGRES_DB=renox_test -p 55432:5432 postgres:17-alpine
  TEST_DATABASE_URL=postgres://postgres:postgres@localhost:55432/renox_test \
      cargo test -p renox -p renox-core -p renox-cli -p bikeshop --features renox/postgres
  ```
- **Docs that compile.** Public items have doc comments (`#![warn(missing_docs)]` in `renox`,
  `renox-core` and `renox-macros`, so clippy's `-D warnings` refuses a public item without
  one), and their examples run as doctests.
  A new API also gets a line in [CHEATSHEET.md](CHEATSHEET.md) (compiled as a doctest too).
- **Generators.** A change to `rnx new` or `rnx make:*` must keep `tests/cli/run.sh` passing:
  it builds and tests an app made with every generator, and (with SQLite) serves it and uses
  it over HTTP with `tests/cli/smoke.py`: pages, a `--resource` module's forms, the starter
  kit's sign-up and roles.
  It also makes apps with the combinations of `rnx new` options. With
  `E2E_POSTGRES=postgres://postgres:postgres@localhost:5432 tests/cli/run.sh postgres` the
  PostgreSQL apps run their tests and commands too.
- **The tutorial.** `tests/tutorial/run.sh` follows docs/tutorial.md the way a reader does and
  checks the result (fmt, clippy, tests, seeding, the app running). Run it after changing the
  tutorial, a generator it uses, or anything its app relies on.
- **Docs and examples in step.** When a change adds or changes behaviour, update what
  describes it: [CHEATSHEET.md](CHEATSHEET.md), [README.md](README.md) (feature tour),
  [llms.txt](llms.txt), the guides in `docs/` (operations: new commands, tables that grow,
  config), the new-app agent guide (`crates/renox-cli/stubs/AGENTS.md.stub`), and the examples
  that show that area (use the new API where an example worked around its absence).
- **Stability.** Read [docs/stability.md](docs/stability.md) before changing a public type.
  Breaking changes go in [CHANGELOG.md](CHANGELOG.md) under "Breaking".

Other CI jobs you can run locally when your change touches their area:
`cargo clippy -p renox --no-default-features -- -D warnings`, the guard against C crypto in
default builds (`cargo tree -p hello -e normal -i aws-lc-rs` must print nothing),
`tests/chaos/run.sh sqlite|postgres`, `tests/cli/run.sh sqlite|postgres`, the S3 tests (see
the top of `crates/renox/tests/it/s3.rs`, and `cargo test -p bikeshop --features s3 --test fields`),
`cargo hack check -p renox-core -p renox --each-feature --no-dev-deps`, `cargo deny check`, and
`cargo semver-checks -p renox-core -p renox --baseline-rev origin/main --release-type minor`
(public API changes; `cargo install --locked cargo-semver-checks`). Releases: [RELEASING.md](RELEASING.md).

- **Coverage.** The `coverage` CI job measures every library crate, the CLI and the macros,
  on SQLite (with the `xlsx` feature) and on PostgreSQL, merged; its HTML report is an
  artifact of the run, and it fails when the total drops under 90 % of lines (doctests
  aren't counted on stable Rust). To measure locally (in its own target directory, one run at a time):
  ```bash
  rustup component add llvm-tools-preview && cargo install --locked cargo-llvm-cov
  cargo llvm-cov --no-report -p renox -p renox-core -p renox-cli -p renox-macros \
      -p renox-2fa -p renox-oauth -p renox-admin -p renox-billing -p renox-editors -p renox-blocks -p renox-pdf --features renox/xlsx
  TEST_DATABASE_URL=postgres://postgres:postgres@localhost:55432/renox_test cargo llvm-cov --no-report \
      -p renox -p renox-core -p renox-cli -p renox-2fa -p renox-oauth -p renox-admin \
      -p renox-billing -p renox-editors --features renox/postgres
  cargo llvm-cov report --html
  ```
  New code comes with tests that run it; check the report for the files you touched.

- **UI changes.** Check pages in a real browser (desktop, a phone width, dark mode): several
  bugs only showed there (see [§6.4](docs/contributing/traps.md)). A change to renox.js, the UI kit, the data grid
  or the editors gets a test in `tests/browser/` (headless Chrome over the DevTools protocol,
  Node 24, no npm packages): `tests/browser/run.sh`, or one file with
  `tests/browser/run.sh grid`. CI's `browser` job runs them all, in two halves side by side (`BROWSER_SHARD=rest` and `bikeshop`).
- **Processes.** `tests/process/run.sh` runs the app binary and `rnx` as real processes:
  stopping on signals with a request in flight, `queue:work`, `schedule:work` twice on one
  database, systemd's socket, `LOG_FORMAT`/`LOG_FILE`, `db:shell` and prompts from a pipe and
  a terminal, `rnx serve` restarting (and keeping the old app on a failed build), and both
  example binaries served and asked for its pages, as a guest and logged in. Run it after
  changing `serve`, the commands, logging or an example; `tests/process/run.sh fixture` or
  `examples` runs one half. With `PROCESS_POSTGRES=postgres://postgres:postgres@localhost:55432`,
  `tests/process/run.sh postgres` runs the database checks and the bike shop on
  PostgreSQL (each in a database it makes and drops); `RNX_BUILD=1` adds `rnx build` (a release
  build, slow). CI's `process` job runs all of it.

## Style

- Code reads like the code around it: plain names, short functions, comments that say *why*.
- Everything in the repository is in English: code, comments, docs, example content, tests
  and commit messages. An example that needs a second language uses Spanish.
- Docs and messages are in plain English with short sentences. Error messages say what to do
  next (`there is no module `x`; create it with `rnx make:module x``).
- Commit messages explain what changed and why, with the details a reviewer needs.

## License

Renox is dual-licensed under MIT or Apache-2.0. Unless you say otherwise, any contribution
you submit is licensed the same way, without additional terms.
