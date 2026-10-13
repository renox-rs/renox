# Git, PRs and CI

> How work is done in this repo: issues, branches, PRs, what CI runs. (Section 4.11 of the old CLAUDE.md.)
> Start at [CLAUDE.md](../../CLAUDE.md).

## 4.11 Git, PRs, CI (how Renoxium works)
- **Work starts from a GitHub issue** (since 1.0.0-rc.3; CONTRIBUTING.md "How work is
  tracked"): a bug, a user story or a task, with labels for type, area and priority, in a
  milestone, on the project board. Found something while working? Open an issue for it
  rather than a note here or a ROADMAP checkbox. Branches are `fix/issue-N-…` (bugs) or
  `feat/issue-N-…`; the PR says `Closes #N` and follows .github/pull_request_template.md.
  ROADMAP.md keeps principles, milestone records and decisions, not open work.
- `gh issue edit`/`view` and `gh pr edit` fail on the deprecated Projects (classic) GraphQL
  field: use REST (`gh api repos/renox-rs/renox/issues/N/labels`, `…/comments`, `-X PATCH
  …/pulls/N`). Projects v2 needs the `project` scope (`gh auth refresh -s project`).
- One branch and one PR per milestone or fix (branch names like `m17b-examples`, `fix-…`).
  Renoxium reviews and **merges PRs**, then says so ("Done"). Don't merge unless asked.
- Before starting, check open PRs (`gh pr list -R renox-rs/renox`) and base new branches on an
  up-to-date `main`. **Don't stack PRs** on unmerged branches (§6.3): push the next branch, but open
  its PR only after the previous one is merged.
- **Never edit `CHANGELOG.md` or the root `README.md`** (Renoxium's rule, 2026-10-05): every
  PR touched the same "Unreleased" lines and the same guide list, so every merge left the other
  open PRs in conflict. Leave both files exactly as they are on `main`, even when a feature or a
  breaking change would belong there; say it in the PR description instead (Renoxium keeps
  those two files). This overrides every other line in this file that says to update them.
  Example READMEs (`examples/*/README.md`, a crate's own README) are not covered and are still
  kept in step.
- Each milestone PR also updates: ROADMAP checkboxes and notes (tick only what is in the
  code, with the API names the code uses), §7 of this file, `llms.txt`, and the example READMEs
  when examples change. Also check, and update when the milestone touches them:
  `docs/operations.md` (commands, tables to prune, what
  `APP_KEY` protects, failure table), the other guides in `docs/`, the counts in §4.5/§7,
  `crates/renox-cli/stubs/AGENTS.md.stub` (traps for app agents) and `env.stub`, and the
  examples: an example that works around something the milestone adds should use the new API,
  and a major new feature should be shown by at least one example. M18–M20 skipped these and
  needed a catch-up PR (#53).
- **Commit messages and PR bodies are long and structured** (Renoxium asked for good
  descriptions and notes): summary paragraph, *What's included*, *Design notes*, *Deviations from
  the roadmap*, *Testing* (what was actually run, including browser checks). End commits and PR
  bodies with the attribution lines the harness gives you.
- Before every commit (the list in CONTRIBUTING.md "What every change needs"):
  ```
  cargo fmt --all
  cargo clippy --workspace --all-targets -- -D warnings
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
  cargo test --workspace
  ```
  plus the PostgreSQL run (§4.7) when SQL changed.
- CI (`.github/workflows/ci.yml`) runs more: **lint** (fmt, clippy default / `--all-features` /
  `-p renox --no-default-features`, the aws-lc guard `! cargo tree -p hello -e normal -i
  aws-lc-rs`, doc), **test** on Ubuntu/macOS/Windows, **test (PostgreSQL)**, **chaos** (sqlite,
  postgres; `tests/chaos/run.sh`), **MSRV (1.94)**, **feature matrix** (`cargo hack check -p
  renox-core -p renox --each-feature --no-dev-deps`), **cli** (sqlite, postgres;
  `tests/cli/run.sh`), **tutorial** (`tests/tutorial/run.sh`), **docker** (`make:deploy` image answers `/health`), **s3** (SeaweedFS; renox's `it/s3.rs` and bikeshop's tests/fields.rs),
  **cargo-deny**, **coverage** (informational); a separate workflow, **Release build (site)**
  (release-site.yml: on pushes to main that touch the docs, builds site/ for Renoxium's server,
  which pulls it),
  **semver checks** (pull requests:
  `cargo semver-checks -p renox-core -p renox --release-type minor` against the latest
  release on crates.io; it fails on a breaking change since 1.0.0 (#139); install it with
  `cargo install --locked cargo-semver-checks` to run it locally).
- MSRV is `rust-version` in the workspace `Cargo.toml` (1.94, set by sqlx 0.9); the `msrv` job
  uses the same number, so raise both together and note it in the PR description.
- After pushing, watch CI (`gh run watch <id> -R renox-rs/renox --exit-status`) and tick the
  "CI green" box in the PR body.
