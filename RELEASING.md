# Releasing Renox

How a release goes to crates.io. The eleven crates (`renox`, `renox-core`, `renox-macros`,
`renox-cli` and the plugins `renox-2fa`, `renox-editors`, `renox-blocks`, `renox-oauth`,
`renox-admin`, `renox-billing` and `renox-pdf`) share the workspace's version and are released together.
Pushing a `v*` tag publishes them: `.github/workflows/release.yml` does it from this
repository, so pushing a tag is the irreversible step (ask before it, as before a `cargo publish`).

## Who can publish

- The crates are owned on crates.io by `renox-rs`. `cargo owner --list renox` shows the
  owners; an owner adds another with `cargo owner --add <login> <crate>`, which the invitee
  accepts at https://crates.io/me/pending-invites.
- **Trusted Publishing.** The workflow holds no crates.io token: crates.io trusts this
  repository and gives the job a token valid for 30 minutes. Each crate is set up once by an
  owner, at crates.io → the crate → Settings → Trusted Publishing → Add (GitHub): owner
  `renox-rs`, repository `renox`, workflow `release.yml`, environment `release`. All eleven need it;
  a crate without it fails the publish with a 403. A new crate (a new plugin) can't be set up
  before it exists: publish its first version by hand (below), then add it.
- The `release` environment (repository Settings → Environments) is created by the first run.
  Add required reviewers there to make a tag wait for approval before it publishes.
- A release candidate is worth it before a big release: publish `x.y.0-rc.1`, check that
  docs.rs builds the API reference and that `cargo install renox-cli --version x.y.0-rc.1 &&
  rnx new demo` works from crates.io, then publish `x.y.0`. Cargo never picks a pre-release
  unless asked (`renox = "1.0.0-rc.1"`).

## Every release

1. **Main is green.** The last CI run on `main` passed, including PostgreSQL, chaos, the CLI
   jobs and the semver checks.
2. **Choose the version** (see [docs/stability.md](docs/stability.md)): a fix is a patch,
   anything new a minor, a breaking change a major. Set it in the workspace `Cargo.toml` in
   eleven places that must agree: `[workspace.package] version` and the `version` of `renox`,
   `renox-core`, `renox-macros`, `renox-2fa`, `renox-editors`, `renox-blocks`, `renox-oauth`, `renox-admin`, `renox-billing` and `renox-pdf` under `[workspace.dependencies]` (written
   `=1.2.0`: the crates are released in lockstep and pin each other exactly, since the macros
   write code against renox-core's items of the same release).
   Other places name the version too; change all of them:
   - the `git clone --branch v…` line in `README.md` ("Use Renox with Claude Code");
   - the version in `README.md`'s "Status" section;
   - the plugin guides' dependency lines (`renox = "1.0"`, docs/*.md) on a new minor or major;
   - the landing page's version (www/resources/views: the badge and the terminal).
   `grep -rn "1\.0\.0" README.md docs www/resources` finds them.
3. **The changelog.** In `CHANGELOG.md`, rename "Unreleased" to `## 1.2.0 · 2026-11-01` (the
   version and the date) and start a new empty "Unreleased" above it.
4. **Check everything** (as in [CONTRIBUTING.md](CONTRIBUTING.md)), then a dry run of the
   whole release: `cargo publish --workspace --dry-run` builds each crate against the others
   of this version without uploading anything (the workflow runs it too, before publishing).
5. **Commit and merge** the version and changelog as a pull request, as for any change.
6. **Tag the merged commit and push the tag** (from an up-to-date `main`):
   ```bash
   git tag v1.2.0 && git push origin v1.2.0
   ```
   The **Release** workflow then checks that the tag is `v` + the workspace version and is on
   `main`, dry-runs, and runs `cargo publish --workspace`: cargo publishes the eleven crates in
   dependency order (renox-macros, renox-core, renox, renox-cli, then the plugins; renox-pdf,
   which depends on `renox` and so on `renox-core`, is among them) and waits
   for each to be in the index. `renox-core` and `renox-macros` use `renox` only as a path
   dev-dependency, which `cargo publish` leaves out, so the order has no cycle. Watch it with
   `gh run watch <id> -R renox-rs/renox --exit-status`. Apps made by `rnx new` from crates.io
   link their `AGENTS.md` to the docs at the tag.
7. **The GitHub release** is created by the same workflow, with the version's CHANGELOG
   section as its notes (a version with a `-`, such as `1.2.0-rc.1`, is marked as a
   pre-release). A release that already exists for the tag is left as it is.
8. **Check the release:**
   - docs.rs shows `renox` and `renox-core` (built with `postgres`, `uuid` and `xlsx`), and
     `renox-2fa`, `renox-editors`, `renox-blocks`, `renox-oauth`, `renox-admin`, `renox-billing` and
     `renox-pdf`;
   - `cargo install renox-cli` then `rnx new demo`: `demo/Cargo.toml` has
     `renox = { version = "1.2" }` and `cargo test` passes in it.

## Publishing by hand

When the workflow can't (a new crate's first version, crates.io's OIDC down, a publish that
stopped halfway), an owner publishes from a machine:

- A crates.io account with a **verified email address** (crates.io refuses to publish without
  one), and a token from https://crates.io/settings/tokens with the `publish-new` and
  `publish-update` scopes and a short expiry, given to `cargo login`. The token stays in
  `~/.cargo/credentials.toml`: never put it in the repository, an issue, a chat or an
  environment variable that a script prints. Revoke it when done.
- From the tagged commit: `cargo publish --workspace`, or after a publish that stopped halfway,
  `cargo publish -p <crate>` for each crate not yet on crates.io, in the order above (a version
  already on crates.io can't be published again). Then `gh release create v1.2.0 --verify-tag
  --notes-file …` if the release is missing.

## Since the first release (1.0.0, 2026-10-08)

- The README has the crates.io and docs.rs badges, and the quick start is
  `cargo install renox-cli` (with the `--git` line for the latest `main`).
- The `semver` CI job compares with the latest release on crates.io and fails on a breaking
  change (#139).
- `rnx new` keeps pinning git commits when `rnx` itself is installed from git, so the
  development flow doesn't change.

## A broken release

Yank it (`cargo yank --version 1.2.0 renox-core`, and the other ten of that version), fix
it, and publish the next patch version. A yanked version stays for apps that already lock it,
but new apps don't get it. Never reuse a version number.
