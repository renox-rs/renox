# Renox: guide for agents and contributors

This file is the short version: what you must know before touching anything. The details live
in the docs it links to; read the one for the area you change. Section numbers such as "§4.2"
or "§6.4" (used in older issues and comments) are kept in those files.

## What Renox is

- A batteries-included web framework for Rust, modelled on Laravel: Axum + HTMX + Alpine.js +
  SQLite (PostgreSQL optional). One dependency (`renox = "…"`, `use renox::prelude::*`).
- Apps depend on Renox; framework code is never copied into apps. Upgrading is a version bump.
- Renoxium is the main contributor and maintainer. Open source, `MIT OR Apache-2.0`.
  Repo https://github.com/renox-rs/renox (default branch `main`).
- The framework is *Renox*; the CLI binary is **`rnx`** (crate `renox-cli`). All ten crates share one
  version and are released together (RELEASING.md).
- Everything in the repo is English: code, comments, docs, example content, tests, commit
  messages and PRs. Renox ships English texts only; an example that needs a second language uses
  Spanish. Prices default to US dollars (`APP_CURRENCY=USD`, amounts in cents).

## Where to read next

| You are about to… | Read |
|---|---|
| Find where something lives | [docs/contributing/layout.md](docs/contributing/layout.md) (§2) |
| Understand the request pipeline, key decisions, the database layer | [docs/contributing/architecture.md](docs/contributing/architecture.md) (§3) |
| Write code, tests, templates, config, migrations, CLI, docs, examples | [docs/contributing/conventions.md](docs/contributing/conventions.md) (§4.1–4.10) |
| Branch, commit, open a PR, read CI | [docs/contributing/workflow.md](docs/contributing/workflow.md) (§4.11) and CONTRIBUTING.md |
| Add or change an environment variable | [docs/contributing/config.md](docs/contributing/config.md) (§5) |
| Debug something odd, or work in an area that bit us before | [docs/contributing/traps.md](docs/contributing/traps.md) (§6) |
| Learn what each milestone added, and why | [docs/contributing/status.md](docs/contributing/status.md) (§7), ROADMAP.md ("Decisions"), CHANGELOG.md (read only) |
| Use or document a feature | `docs/*.md` (the guides), CHEATSHEET.md, llms.txt (which example shows what) |
| Release | RELEASING.md. Report a vulnerability: SECURITY.md |

## Rules that always apply

- **Work starts from a GitHub issue** (bug, story or task). Branch `fix/issue-N-…` or `feat/issue-N-…`
  from an up-to-date `origin/main`, in its own worktree with its own `CARGO_TARGET_DIR`; check
  `git branch --show-current` before committing. One PR per milestone or fix, never stacked on
  an unmerged branch. The PR body has one `Closes #n` per issue (a lone `Closes #a, #b` closes
  only the first), follows `.github/pull_request_template.md` and says what was really tested.
  **Renoxium reviews and merges PRs; don't merge unless asked.**
- **Never edit `README.md` or `CHANGELOG.md`** (every PR touched the same lines and conflicted).
  Say in the PR description what would belong there. Example READMEs and crate READMEs are
  yours to keep in step.
- **Use REST for GitHub**: `gh issue edit`/`view` and `gh pr edit` fail on the deprecated
  Projects (classic) field. `gh api repos/renox-rs/renox/issues/N …`, `-X PATCH …/pulls/N`.
- **Never `pkill -f` or `pgrep -f`** (it matched and killed the shell running it). Start background
  processes with `& echo $! > pidfile` and kill by pid.
- **Machine limits** (builds have OOM-killed the terminal and filled the disk): scoped tests
  only (`cargo test -p renox --test it -- module::name`, never the whole workspace; CI runs
  that), `CARGO_BUILD_JOBS=1`, a memory cap (`systemd-run --user --scope -p MemoryHigh=… -p
  MemoryMax=…`), one heavy run at a time, watch `df`, delete regenerable `target/debug`. Details:
  [traps §6.1 and §6.6](docs/contributing/traps.md).
- **Secrets**: never store a token or key in the repo, an issue, a note or a script's output. Never send
  the maintainer's email address to a third-party service (use a neutral User-Agent such as
  `renox-deps-check`). **Ask before every `cargo publish` and before pushing a `v*` tag** (the tag publishes the
  crates through release.yml; publishing is irreversible).
- Commit messages and PR bodies are long and structured (summary, what's included, design notes,
  deviations, testing) and end with the attribution lines the harness gives you.

## Before every commit

```
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo test --workspace
```

(Run them scoped to the crates you touched while working, the full list before the PR; plus the
PostgreSQL run when SQL changed: [conventions §4.7](docs/contributing/conventions.md).) After
pushing, watch CI (`gh run watch <id> -R renox-rs/renox --exit-status`).

## Write the 1.1 way, and teach only that

New pages, guides, examples, stubs and generator output use: `<rx-*>` / `<app-*>` tags (never a kit macro
call when a tag exists; macros without a tag in `contracts.rs` stay macros), own components with
`<rx-props>`, typed columns in queries (`Product::PRICE.lt(..)`; raw fragments stay raw),
`#[model(form)]` instead of a copied form struct, `#[derive(ViewData)]` / `View::typed` for a page's
context, a live component (`#[renox::live_component]`, docs/live.md) for an interactive part whose state would
otherwise need a route and `hx-target` glue per action (htmx stays right for fragments, boosting
and stateless swaps), `make:migration --auto` after a model changes, and the checks
`rnx db:check` / `rnx view:check` with `TestApp::assert_models_match_schema` /
`assert_views_compile` in an app's tests. The old forms stay in the API (semver) but are not shown as
the way: never add a macro-call or string-column example next to the new one.

## The conventions that bite most (full list in conventions and traps)

- Handler futures must be `Send` (§4.2): data APIs that take closures or iterators are plain `fn`s
  returning `impl Future + Send`; add every new data API to `crates/renox/tests/it/send_handlers.rs`.
- A built-in template needs a file **and** a line in `BUILTIN` in `view.rs` (§4.3).
- Tests that use the macros live in `crates/renox/tests/it/` as a module of `main.rs`; don't add
  top-level `tests/*.rs` files (§4.7). Time is read through `clock`, never `SystemTime::now`/`Instant`.
- SQL goes through `db::sql(…)` with `?` placeholders; never `sqlx::query` on `&Db` (§3.3).
- A public API change updates CHEATSHEET.md, llms.txt, the guides and the examples in the same
  PR. Every public item has a doc comment; doc examples are doctests (never ```` ```ignore ````) (§4.1).
- UI changes get a browser test in `tests/browser` (§4.10, §6.4).
- Scripted edits fail silently: prefer the Edit tool, `assert old in s` in scripts, and grep after (§6.5).

## Keeping the docs current

Each milestone PR also updates ROADMAP.md (checkboxes, notes, "Decisions"),
[docs/contributing/status.md](docs/contributing/status.md), `llms.txt`, the example READMEs, the guides in `docs/`,
the stubs for new apps (`crates/renox-cli/stubs/AGENTS.md.stub`, `env.stub`) and the counts in
[conventions §4.5](docs/contributing/conventions.md) when they change. Work you find while doing
something else becomes a GitHub issue, not a note here.
