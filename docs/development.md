# Faster builds while developing

This page helps you wait less while you work on a Renox app. It explains why Rust builds take
time, what `rnx new` already does about it, and a few extra things you can turn on.

Here is the short version. Rust has to turn your code and all the libraries it uses into a
program before the first page appears. That first build is slow. After that, `rnx serve` only
rebuilds **your** code when you change it. Templates, translations and files in `public/` don't
need a build at all: they reload by themselves. The settings below make both kinds of build
quicker.

## Start with rnx doctor

Run `rnx doctor` on a new machine, or when something does not work. It checks what a Renox app
needs and says how to fix what is missing: Rust and its version, the linker, sccache, then (inside
an app) `.env`, `APP_KEY`, the database and whether the migrations ran.

Each line starts with a mark: `✓` is fine, `!` is a warning (the app works, but slower or
less safe), `✗` is a problem that must be fixed. The exit code is 1 when any check shows `✗`, and
0 otherwise, so you can run it in a script or in CI.

`rnx doctor` builds the app to ask it about its database. `rnx doctor --no-build` skips that and
the database checks. Outside an app directory it checks the machine only and tells you so.

### In this guide

- [Start with rnx doctor](#start-with-rnx-doctor): what is missing on this machine or in this app.
- [What `rnx new` already does](#what-rnx-new-already-does): two speed-ups every new app has.
- [A faster linker](#a-faster-linker): speed up the last step of every build.
- [Fewer dependencies](#fewer-dependencies): build less code by turning off parts you don't use.
- [Sharing compiled dependencies](#sharing-compiled-dependencies): reuse work between apps.
- [Docker](#docker): keep Docker builds quick.
- [Editor autocomplete](#editor-autocomplete): tag and attribute suggestions for templates.

### Words you'll meet

| Word | What it means |
|---|---|
| **compile** | Turn Rust source code into machine code the computer can run. |
| **link** | The last step of a build: glue all the compiled pieces into one program file. |
| **crate** | A Rust package. Your app is a crate; Renox and the libraries it uses are crates too. |
| **dependency** | A crate your app uses. Each one has to be compiled at least once. |
| **incremental rebuild** | A rebuild after a small change, where only the changed crate is compiled again. |
| **profile** | A set of build settings. `dev` is used while you develop; `release` for the server. |
| **opt-level** | How hard the compiler works to make code fast. `0` builds quickly but runs slowly; `3` is the fastest code. |
| **debug info** | Extra data in the program that tells tools which line of source each piece came from. |
| **feature** | A switch in `Cargo.toml` that turns an optional part of a crate on or off. |

## What `rnx new` already does

Every app made by `rnx new` comes with two speed-ups in its `Cargo.toml`.

### Fast password hashing in dev builds

**Argon2 and BLAKE2 are optimised in dev builds.** The settings are
`[profile.dev.package.argon2]` and `[profile.dev.package.blake2]`, both with `opt-level = 3`.
(Argon2 uses BLAKE2 inside, so both need it.)

Why? Argon2 turns passwords into hashes, and it is slow on purpose, to make guessing passwords
hard. Without optimisation it becomes *very* slow. Every login in every test would pay for it.
With these two lines, only these two crates are optimised, and the rest of the dev build stays
quick to compile.

### Smaller debug info

**Debug info is only line tables** (`[profile.dev] debug = "line-tables-only"`).

That means the program still knows the file and line number of each piece of code, so a crash
report (a backtrace) still points to the right line. But the program file is much smaller. A
smaller file links faster, and linking is the part of each rebuild you wait for.

> [!TIP]
> Need a debugger that shows the values of your variables? Set `debug = true` for a while. It
> makes builds slower, so switch back when you're done.

> [!TIP]
> `rnx serve` passes any extra arguments on to `cargo build`. `rnx serve --release` runs the
> optimised release build: slower to build, but the app runs as fast as on the server. Useful
> to see how fast a heavy page really is.

## A faster linker

Linking is most of the time of an incremental rebuild. A faster linker helps on every change.

`rnx new` writes `.cargo/config.toml` for you when it finds [mold](https://github.com/rui314/mold)
and clang on the machine (or lld on aarch64 Linux). It keeps the file out of Git and Docker
(`.gitignore` and `.dockerignore`), because it only suits your machine, and it tells you it did it.
Run `rnx doctor` to see which linker is in use.

On x86_64 Linux, Rust's default is already `rust-lld` (since Rust 1.90), so mold is the only
further step. To do it by hand, install mold and clang, then add `.cargo/config.toml` to the app:

```toml
# Use clang to drive the link, and tell it to use mold (Linux, 64-bit Intel/AMD)
[target.x86_64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
```

This tells Rust: "when you build for 64-bit Linux, use `clang` to link, and let it call mold".

On other systems:

- **macOS:** the default linker (ld-prime) is already fast. Nothing to do.
- **Windows:** use `rust-lld`. Set `linker = "rust-lld.exe"` under
  `[target.x86_64-pc-windows-msvc]`.

## Fewer dependencies

Less code to compile means faster builds. Renox has some parts you can switch off with
**features**.

Renox's default features (the ones that are on unless you say otherwise) are:

| Feature | What it gives you |
|---|---|
| `fake` | The `renox::fake` re-export, used by factories to make fake test data. |
| `http` | Real web requests for `state.http`. It brings in the reqwest crate. The test fake works without it. |
| `server-events` | `analytics::ServerEvent`. It needs `http`. |

An app that uses none of them can turn them off:

```toml
renox = { version = "…", default-features = false }
# or keep some: default-features = false, features = ["fake"]
```

Keep the rest of your `renox` line as `rnx new` wrote it: `version = "…"` when `rnx` came from
crates.io, `git = "…", rev = "…"` when it came from Git. Only add the `default-features` and
`features` parts.

`default-features = false` turns all three off. The comment shows how to keep only the ones you
want: list them in `features`.

Some features are **off** unless you turn them on: `postgres`, `s3`, `uuid` and `xlsx` (Excel
exports of data grids).

> [!NOTE]
> `s3` is the heaviest. It turns on object_store's `aws` feature, which brings in reqwest and
> aws-lc-rs (a crypto library written in C). It is not the AWS SDK.

For secure connections (TLS), Renox uses rustls with the `ring` provider. So no C crypto
library (aws-lc) is compiled. The only C code that gets compiled is SQLite's.

## Sharing compiled dependencies

Do you have several Rust apps on one machine? They can share compiled dependencies with
[sccache](https://github.com/mozilla/sccache). Turn it on by setting the environment variable
`RUSTC_WRAPPER=sccache`.

sccache keeps a copy of everything it compiles. When the first build of another app needs the
same crate, it reuses that copy instead of compiling it again.

## Docker

The Dockerfile that `rnx make:deploy` writes builds your dependencies in their own **layer**
(a saved step of a Docker build), using a tool called cargo-chef.

Docker reuses that layer until `Cargo.toml` or `Cargo.lock` change. So after a change to your
code, only your own crate is compiled, not every dependency again.

> [!IMPORTANT]
> Commit `Cargo.lock` to your repository. It records the exact version of every dependency,
> so the dependency layer is built from the same versions each time.

## Editor autocomplete

New apps get suggestions for the kit's `<rx-…>` tags in VS Code. `rnx serve` writes
`.vscode/renox-components.json` after each build (the same as running `rnx view:data`), and
`.vscode/settings.json` points `html.customData` at it. The file is git-ignored. If a build
can't write it, `rnx serve` warns and keeps running. See
[the UI guide](ui.md#editor-autocomplete). JetBrains IDEs aren't covered: they read Web Types,
not this format.
