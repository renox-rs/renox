use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::Database;

/// Files of a new app: (path, contents). `.stub` files are templated.
const STUBS: &[(&str, &str)] = &[
    ("Cargo.toml", include_str!("../stubs/Cargo.toml.stub")),
    ("AGENTS.md", include_str!("../stubs/AGENTS.md.stub")),
    // Claude Code reads CLAUDE.md; it imports AGENTS.md so there's one text.
    ("CLAUDE.md", include_str!("../stubs/CLAUDE.md.stub")),
    (".env", include_str!("../stubs/env.stub")),
    (".env.example", include_str!("../stubs/env.stub")),
    (".gitignore", include_str!("../stubs/gitignore.stub")),
    (
        ".vscode/settings.json",
        include_str!("../stubs/vscode-settings.json.stub"),
    ),
    ("build.rs", include_str!("../stubs/build.rs")),
    ("migrations/.gitkeep", ""),
    ("src/lib.rs", include_str!("../stubs/src/lib.rs")),
    ("src/main.rs", include_str!("../stubs/src/main.rs")),
    ("tests/home.rs", include_str!("../stubs/tests/home.rs")),
    ("src/app/mod.rs", include_str!("../stubs/src/app/mod.rs")),
    (
        "src/app/home/mod.rs",
        include_str!("../stubs/src/app/home/mod.rs"),
    ),
    (
        "resources/views/layouts/app.html",
        include_str!("../stubs/resources/views/layouts/app.html"),
    ),
    (
        "resources/views/home/index.html",
        include_str!("../stubs/resources/views/home/index.html"),
    ),
    (
        "resources/views/errors/default.html",
        include_str!("../stubs/resources/views/errors/default.html"),
    ),
    ("public/app.css", include_str!("../stubs/public/app.css")),
    (
        "resources/lang/en.json",
        include_str!("../stubs/resources/lang/en.json"),
    ),
];

/// `--starter`: the starter kit, files written over the stubs above (same
/// path) or next to them. Sign-up with email verification, roles, the
/// activity log, a dashboard and the users page in the kit's sidebar layout.
const STARTER: &[(&str, &str)] = &[
    ("src/lib.rs", include_str!("../stubs/starter/src/lib.rs")),
    (
        "src/app/mod.rs",
        include_str!("../stubs/starter/src/app/mod.rs"),
    ),
    (
        "src/app/home/mod.rs",
        include_str!("../stubs/starter/src/app/home/mod.rs"),
    ),
    (
        "src/app/dashboard/mod.rs",
        include_str!("../stubs/starter/src/app/dashboard/mod.rs"),
    ),
    (
        "src/app/users/mod.rs",
        include_str!("../stubs/starter/src/app/users/mod.rs"),
    ),
    (
        "src/app/activity/mod.rs",
        include_str!("../stubs/starter/src/app/activity/mod.rs"),
    ),
    (
        "src/app/roles.rs",
        include_str!("../stubs/starter/src/app/roles.rs"),
    ),
    (
        "src/app/seed.rs",
        include_str!("../stubs/starter/src/app/seed.rs"),
    ),
    (
        "tests/home.rs",
        include_str!("../stubs/starter/tests/home.rs"),
    ),
    (
        "resources/views/layouts/app.html",
        include_str!("../stubs/starter/resources/views/layouts/app.html"),
    ),
    (
        "resources/views/home/index.html",
        include_str!("../stubs/starter/resources/views/home/index.html"),
    ),
    (
        "resources/views/components/figures.html",
        include_str!("../stubs/starter/resources/views/components/figures.html"),
    ),
    (
        "resources/views/components/page_title.html",
        include_str!("../stubs/starter/resources/views/components/page_title.html"),
    ),
    (
        "resources/views/dashboard/show.html",
        include_str!("../stubs/starter/resources/views/dashboard/show.html"),
    ),
    (
        "resources/views/users/index.html",
        include_str!("../stubs/starter/resources/views/users/index.html"),
    ),
    (
        "resources/views/activity/index.html",
        include_str!("../stubs/starter/resources/views/activity/index.html"),
    ),
    (
        "resources/lang/en.json",
        include_str!("../stubs/starter/resources/lang/en.json"),
    ),
    // The page patterns (from examples/bikeshop): the app's own components and CSS.
    (
        "resources/views/components/icon_circle.html",
        include_str!("../stubs/starter/resources/views/components/icon_circle.html"),
    ),
    (
        "resources/views/components/hero.html",
        include_str!("../stubs/starter/resources/views/components/hero.html"),
    ),
    (
        "resources/views/components/art_app.html",
        include_str!("../stubs/starter/resources/views/components/art_app.html"),
    ),
    (
        "resources/views/components/art_icons.html",
        include_str!("../stubs/starter/resources/views/components/art_icons.html"),
    ),
    (
        "resources/views/components/section_head.html",
        include_str!("../stubs/starter/resources/views/components/section_head.html"),
    ),
    (
        "resources/views/components/feature_card.html",
        include_str!("../stubs/starter/resources/views/components/feature_card.html"),
    ),
    (
        "resources/views/components/summary.html",
        include_str!("../stubs/starter/resources/views/components/summary.html"),
    ),
    (
        "resources/views/components/steps.html",
        include_str!("../stubs/starter/resources/views/components/steps.html"),
    ),
    (
        "resources/views/components/empty_state.html",
        include_str!("../stubs/starter/resources/views/components/empty_state.html"),
    ),
    (
        "resources/views/components/stat_card.html",
        include_str!("../stubs/starter/resources/views/components/stat_card.html"),
    ),
    (
        "public/patterns.css",
        include_str!("../stubs/starter/public/patterns.css"),
    ),
    (
        "resources/views/layouts/public.html",
        include_str!("../stubs/starter/resources/views/layouts/public.html"),
    ),
    (
        "resources/views/errors/default.html",
        include_str!("../stubs/starter/resources/views/errors/default.html"),
    ),
    // Renox's sign-in pages in the app's look, with the seeded accounts
    // on the login page in local.
    (
        "resources/views/renox/auth/layout.html",
        include_str!("../stubs/starter/resources/views/renox/auth/layout.html"),
    ),
    (
        "resources/views/renox/auth/login_options.html",
        include_str!("../stubs/starter/resources/views/renox/auth/login_options.html"),
    ),
    (
        "public/demo-logins.js",
        include_str!("../stubs/starter/public/demo-logins.js"),
    ),
    (
        "src/app/demo.rs",
        include_str!("../stubs/starter/src/app/demo.rs"),
    ),
];

/// `--notifications` on the plain app (the starter kit has them already):
/// `Auth::new().notifications()`, the bell in the layout's bar, and a test.
fn with_notifications(file: &str, contents: &str) -> String {
    let edits: &[(&str, &str)] = match file {
        "src/lib.rs" => &[(
            "        .module(renox::auth::Auth::new().account()) // login, register, /account\n",
            "        // Login, register, /account, and the notification bell (/notifications).\n        \
             .module(renox::auth::Auth::new().account().notifications())\n",
        )],
        "resources/views/layouts/app.html" => &[(
            "    <template rx-if=\"auth.check\">\n",
            "    <template rx-if=\"auth.check\">\n      \
                 {#- In-app notifications: a badge, a panel, new ones live (docs/mail.md). -#}\n      \
                 <rx-notification-bell :count=\"unread_notifications\"/>\n",
        )],
        "tests/home.rs" => &[("", NOTIFICATIONS_TEST)],
        _ => &[],
    };
    // A checkout with CRLF endings (Git on Windows) compiles the stubs in
    // with them; the edits look for `\n`.
    let mut contents = contents.replace("\r\n", "\n");
    for (old, new) in edits {
        if old.is_empty() {
            contents.push_str(new);
        } else {
            assert!(contents.contains(old), "{file} lost `{old}`");
            contents = contents.replacen(old, new, 1);
        }
    }
    contents
}

/// The test `--notifications` adds to `tests/home.rs`.
const NOTIFICATIONS_TEST: &str = r#"
#[renox::test]
async fn the_bell_shows_notifications() {
    let app = TestApp::new({{crate_name}}::app()).await;
    let user = User::register(app.db(), "Anna", "anna@example.com", "secret123")
        .await
        .unwrap();
    app.acting_as(&user);
    app.get("/").await.assert_ok().assert_see("data-rx-bell");
    app.get("/notifications")
        .await
        .assert_ok()
        .assert_see("No notifications");
}
"#;

/// The starter kit's part of AGENTS.md: where its page patterns live and how
/// to change them, before the traps.
const STARTER_NOTES: &str = include_str!("../stubs/starter/AGENTS.starter.stub");

/// AGENTS.md with [`STARTER_NOTES`] in it.
fn with_starter_notes(contents: &str) -> String {
    let contents = contents.replace("\r\n", "\n");
    let notes = STARTER_NOTES.replace("\r\n", "\n");
    let anchor = "## Things that trip agents up";
    assert!(contents.contains(anchor), "AGENTS.md lost `{anchor}`");
    contents.replacen(anchor, &format!("{notes}{anchor}"), 1)
}

/// The files of a new app: the stubs, with the starter kit's over them.
fn files(starter: bool) -> Vec<(&'static str, &'static str)> {
    let kit: &[(&str, &str)] = if starter { STARTER } else { &[] };
    let mut files: Vec<_> = STUBS
        .iter()
        .map(|&(path, text)| {
            let over = kit.iter().find(|(kit_path, _)| *kit_path == path);
            (path, over.map_or(text, |(_, text)| *text))
        })
        .collect();
    files.extend(
        kit.iter()
            .filter(|(path, _)| !STUBS.iter().any(|(stub, _)| stub == path)),
    );
    files
}

const RENOX_GIT: &str = "https://github.com/renox-rs/renox";

/// Renox from GitHub, pinned to the commit this `rnx` was built from, so
/// `cargo update` doesn't move the app to an API it wasn't written for.
fn git_dependency(rev: Option<&str>) -> String {
    match rev {
        Some(rev) => format!("renox = {{ git = \"{RENOX_GIT}\", rev = \"{rev}\""),
        // Built without git: follow main (move to a `rev` when you can).
        None => format!("renox = {{ git = \"{RENOX_GIT}\", branch = \"main\""),
    }
}

/// The Renox repository at `rev` (or `main`), for links in AGENTS.md.
fn docs_url(rev: Option<&str>) -> String {
    format!("{RENOX_GIT}/blob/{}", rev.unwrap_or("main"))
}

/// AGENTS.md's offline line for an app on a local checkout: everything is there.
fn path_offline(checkout: &str) -> String {
    format!(
        "Offline, everything (the source, the cheat sheet, llms.txt, the guides and the \
         examples) is in the local Renox checkout this app uses: `{checkout}`."
    )
}

/// AGENTS.md's offline line for an app pinned to a Git commit: Cargo's checkout
/// of the repository has everything.
fn git_offline(rev: Option<&str>) -> String {
    let dir = rev.map_or("*", |rev| &rev[..7.min(rev.len())]);
    format!(
        "Offline, everything (the source, the cheat sheet, llms.txt, the guides and the \
         examples) is in the checkout Cargo downloaded: `~/.cargo/git/checkouts/renox-*/{dir}/`."
    )
}

/// AGENTS.md's offline line for an app on crates.io: the downloaded crates
/// hold only the source, so it names the clone of this exact version.
fn registry_offline(version: &str) -> String {
    format!(
        "Offline, Renox's source is in the crates Cargo downloaded: \
         `~/.cargo/registry/src/*/renox-core-{version}/`. For the cheat sheet, llms.txt, the \
         guides and the examples of this version: \
         `git clone --depth 1 --branch v{version} {RENOX_GIT}`."
    )
}

/// Renox from crates.io, at this `rnx`'s own version (`cargo install
/// renox-cli` builds from the registry, without git): a caret requirement,
/// since a minor release doesn't break apps. A pre-release (`1.0.0-rc.1`)
/// is written whole: Cargo never picks one for `"1.0"`.
fn registry_dependency(version: &str) -> String {
    if version.contains('-') {
        return format!("renox = {{ version = \"{version}\"");
    }
    let major_minor: Vec<&str> = version.split('.').take(2).collect();
    format!("renox = {{ version = \"{}\"", major_minor.join("."))
}

/// What `rnx new` makes, besides the name.
#[derive(Clone, Copy)]
pub struct Options {
    pub database: Database,
    pub tailwind: bool,
    pub starter: bool,
    /// The notification bell in the plain app (the starter kit has it).
    pub notifications: bool,
}

pub fn run(name: &str, renox_path: Option<&Path>, options: Options) -> Result<()> {
    run_in(Path::new("."), name, renox_path, options)
}

/// `run`, making the app in `parent` (tests use a temporary directory).
fn run_in(parent: &Path, name: &str, renox_path: Option<&Path>, options: Options) -> Result<()> {
    let Options {
        database,
        tailwind,
        starter,
        notifications,
    } = options;
    validate_name(name)?;
    let root = &parent.join(name);
    if root.exists() {
        bail!("`{name}` already exists");
    }

    let rev = option_env!("RENOX_GIT_REV");
    // Where AGENTS.md sends coding agents: the docs of the Renox this app
    // compiles against, not `main`, which may have APIs it doesn't.
    let (dependency, docs, source) = match renox_path {
        Some(path) => {
            let crate_dir = path.join("crates/renox");
            let crate_dir = crate_dir
                .canonicalize()
                .with_context(|| format!("{} is not a Renox checkout", path.display()))?;
            let checkout = crate_dir
                .parent()
                .and_then(Path::parent)
                .unwrap_or(&crate_dir)
                .display()
                .to_string();
            (
                format!("renox = {{ path = {:?}", crate_dir.display().to_string()),
                docs_url(None),
                path_offline(&checkout),
            )
        }
        None if rev.is_none() && option_env!("RENOX_FROM_CRATES_IO").is_some() => {
            let version = env!("CARGO_PKG_VERSION");
            (
                registry_dependency(version),
                format!("{RENOX_GIT}/blob/v{version}"),
                registry_offline(version),
            )
        }
        None => (git_dependency(rev), docs_url(rev), git_offline(rev)),
    };
    let dependency = match database {
        Database::Sqlite => format!("{dependency} }}"),
        Database::Postgres => format!("{dependency}, features = [\"postgres\"] }}"),
    };
    let crate_name = name.replace('-', "_");
    let postgres = format!("postgres://postgres:postgres@localhost:5432/{crate_name}");
    let (database_url, test_database_url) = match database {
        Database::Sqlite => (
            "sqlite://storage/app.db".to_owned(),
            format!("# TEST_DATABASE_URL={postgres}_test"),
        ),
        Database::Postgres => (
            postgres.clone(),
            format!("TEST_DATABASE_URL={postgres}_test"),
        ),
    };
    let key = crate::generate_key();
    let vars = Vars {
        name,
        title: title(name),
        crate_name: &crate_name,
        renox_dependency: &dependency,
        renox_docs: &docs,
        renox_offline: &source,
        database_url: &database_url,
        test_database_url: &test_database_url,
        app_key: &key,
    };
    let keyless = Vars {
        app_key: "",
        ..vars.clone()
    };

    for (file, contents) in files(starter) {
        let contents = if notifications && !starter {
            with_notifications(file, contents)
        } else if starter && file == "AGENTS.md" {
            with_starter_notes(contents)
        } else {
            contents.to_owned()
        };
        // Only the real .env gets a key; .env.example stays shareable.
        let contents = fill(&contents, if file == ".env" { &vars } else { &keyless });
        let path = root.join(file);
        fs::create_dir_all(path.parent().expect("stub paths have a parent"))?;
        fs::write(&path, contents)
            .with_context(|| format!("could not write {}", path.display()))?;
        crate::format::touched(&path);
    }

    link_fast(root)?;

    if tailwind {
        use_tailwind(root)?;
    }

    print_created(name, &crate_name, database, starter);
    Ok(())
}

/// The values the placeholders of a new app stand for.
#[derive(Clone)]
struct Vars<'a> {
    name: &'a str,
    title: String,
    crate_name: &'a str,
    renox_dependency: &'a str,
    renox_docs: &'a str,
    renox_offline: &'a str,
    database_url: &'a str,
    test_database_url: &'a str,
    app_key: &'a str,
}

/// Fills the known `{{key}}` placeholders (exact keys, no inner spaces) in
/// `contents`; any other `{{ ... }}` stays as it is.
fn fill(contents: &str, vars: &Vars) -> String {
    contents
        .replace("{{name}}", vars.name)
        .replace("{{title}}", &vars.title)
        .replace("{{renox_dependency}}", vars.renox_dependency)
        .replace("{{renox_docs}}", vars.renox_docs)
        .replace("{{renox_offline}}", vars.renox_offline)
        .replace("{{crate_name}}", vars.crate_name)
        .replace("{{database_url}}", vars.database_url)
        .replace("{{test_database_url}}", vars.test_database_url)
        .replace("{{app_key}}", vars.app_key)
}

/// Writes `.cargo/config.toml` in `root` for the fast linker found on this
/// machine, unless there is none or the machine's configuration chooses one.
fn link_fast(root: &Path) -> Result<()> {
    let already = crate::tools::configured_texts(None)
        .iter()
        .any(|t| crate::tools::mentions_linker(t).is_some());
    let found = crate::tools::fast_linker(
        std::env::consts::OS,
        std::env::consts::ARCH,
        cfg!(target_env = "gnu"),
        |p| crate::tools::find(p).is_some(),
    );
    if let Some((linker, text)) = linker_file(found, std::env::consts::ARCH, already) {
        let path = root.join(".cargo/config.toml");
        fs::create_dir_all(path.parent().expect("has a parent"))?;
        fs::write(&path, text).with_context(|| format!("could not write {}", path.display()))?;
        println!(
            "Linking with {} (found on this machine): .cargo/config.toml, kept out of Git.",
            linker.name()
        );
    }
    Ok(())
}

/// Prints the "Created ... Next:" message of a new app.
fn print_created(name: &str, crate_name: &str, database: Database, starter: bool) {
    match database {
        Database::Sqlite => println!("Created {name}. Next:\n\n    cd {name}\n    rnx serve\n"),
        Database::Postgres => println!(
            "Created {name}. Next: create the `{crate_name}` and `{crate_name}_test` databases \
             (or edit DATABASE_URL and TEST_DATABASE_URL in .env), then\n\n    cd {name}\n    rnx serve\n"
        ),
    }
    if starter {
        println!(
            "Then sign up at /register and make yourself an admin:\n\n    \
             rnx users:admin you@example.com\n\n(or `rnx db:seed` for admin@example.com, password123)\n"
        );
    }
}

/// The `.cargo/config.toml` for the fast linker `found` on this machine, unless
/// the machine's configuration `already` chooses one.
fn linker_file(
    found: Option<crate::tools::Linker>,
    arch: &str,
    already: bool,
) -> Option<(crate::tools::Linker, String)> {
    if already {
        return None;
    }
    let linker = found?;
    let text = crate::tools::linker_config(linker, crate::tools::linux_target(arch)?);
    Some((linker, text))
}

/// `--tailwind`: the app's styles move into Tailwind's input, the layout
/// links the built file, and the CSS is built once if the CLI can be had.
fn use_tailwind(root: &Path) -> Result<()> {
    use_tailwind_with(root, |root| crate::tailwind::build(root, false))
}

/// `use_tailwind` with the CSS built by `build` (tests pass one that needs
/// no download).
fn use_tailwind_with(root: &Path, build: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    let own = fs::read_to_string(root.join("public/app.css"))?;
    fs::remove_file(root.join("public/app.css"))?;
    let input = root.join(crate::tailwind::INPUT);
    fs::create_dir_all(input.parent().expect("the input has a parent"))?;
    // The app's own rules, without the stub's header comment.
    let rules = own
        .split_once("*/")
        .map_or(own.as_str(), |(_, rest)| rest.trim_start());
    fs::write(
        &input,
        format!(
            "{}
{rules}",
            crate::tailwind::INPUT_STUB
        ),
    )?;

    // Every layout that links the app's CSS (the starter kit has three).
    for layout in [
        "resources/views/layouts/app.html",
        "resources/views/layouts/public.html",
        "resources/views/renox/auth/layout.html",
    ] {
        let layout = root.join(layout);
        if !layout.is_file() {
            continue;
        }
        let html = fs::read_to_string(&layout)?.replace(
            "<link rel=\"stylesheet\" href=\"{{ asset('app.css') }}\">",
            "{#- Built by Tailwind from resources/css/app.css (`rnx serve`, `rnx build`). -#}\n  \
             <link rel=\"stylesheet\" href=\"{{ asset('css/app.css') }}\">",
        );
        fs::write(&layout, html)?;
    }
    let home = root.join("resources/views/home/index.html");
    let html = fs::read_to_string(&home)?.replace(
        "    <p class=\"rx-subtitle\">{{ t('home.edit') }}</p>",
        "    <p class=\"rx-subtitle\">{{ t('home.edit') }}</p>\n    \
         <p class=\"mt-2 text-sm font-medium text-emerald-700 dark:text-emerald-400\">\
         Tailwind is on: edit resources/css/app.css, or use its classes in any view.</p>",
    );
    fs::write(&home, html)?;

    if let Err(err) = build(root) {
        eprintln!("rnx: Tailwind isn't built yet ({err:#}); `rnx serve` will try again.");
    }
    Ok(())
}

pub(crate) fn validate_name(name: &str) -> Result<()> {
    let valid = name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !valid {
        bail!(
            "`{name}` is not a valid name: use lowercase letters, digits, `-` and `_`, starting with a letter"
        );
    }
    let crate_name = name.replace('-', "_");
    if crate::generate::is_reserved(&crate_name) {
        bail!(
            "`{name}` can't be an app's name: `{crate_name}` is a Rust keyword or a crate the app uses"
        );
    }
    Ok(())
}

/// `coffee-shop` -> `Coffee Shop`
fn title(name: &str) -> String {
    name.split(['-', '_'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #516: the templates `rnx new` writes use tags, never template statements.
    #[test]
    fn no_stub_template_has_a_statement() {
        fn walk(dir: &std::path::Path, found: &mut Vec<String>, seen: &mut usize) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, found, seen);
                } else if path.extension().is_some_and(|e| e == "html") {
                    *seen += 1;
                    let text = fs::read_to_string(&path).unwrap();
                    for (n, line) in text.lines().enumerate() {
                        if line.contains("{%") {
                            found.push(format!("{}:{}", path.display(), n + 1));
                        }
                    }
                }
            }
        }
        let stubs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("stubs");
        let (mut found, mut seen) = (Vec::new(), 0);
        walk(&stubs, &mut found, &mut seen);
        assert!(seen > 10, "found only {seen} templates under {stubs:?}");
        assert!(
            found.is_empty(),
            "template statements in the stubs: {found:#?}"
        );
    }

    #[test]
    fn fill_replaces_exact_keys_only() {
        let vars = Vars {
            name: "my-shop",
            title: "My Shop".into(),
            crate_name: "my_shop",
            renox_dependency: "renox = { version = \"1.1\" }",
            renox_docs: "docs",
            renox_offline: "offline",
            database_url: "sqlite://storage/app.db",
            test_database_url: "# test",
            app_key: "KEY",
        };
        let text = "{{name}} {{title}} {{crate_name}} {{app_key}} {{database_url}} \
                    {{ app.locale }} {{ url(\"x\") }} {{unknown}} {{ name }}";
        assert_eq!(
            fill(text, &vars),
            "my-shop My Shop my_shop KEY sqlite://storage/app.db \
             {{ app.locale }} {{ url(\"x\") }} {{unknown}} {{ name }}"
        );
        let keyless = Vars {
            app_key: "",
            ..vars
        };
        assert_eq!(fill("k={{app_key}}", &keyless), "k=");
    }

    #[test]
    fn validates_names() {
        assert!(validate_name("shop").is_ok());
        assert!(validate_name("coffee-shop_2").is_ok());
        assert!(validate_name("Shop").is_err());
        assert!(validate_name("2shop").is_err());
        assert!(validate_name("../shop").is_err());
        for reserved in ["renox", "fn", "self", "type"] {
            assert!(validate_name(reserved).is_err(), "{reserved}");
        }
    }

    #[test]
    fn pins_the_git_dependency() {
        assert_eq!(registry_dependency("1.2.3"), "renox = { version = \"1.2\"");
        assert_eq!(
            registry_dependency("1.0.0-rc.1"),
            "renox = { version = \"1.0.0-rc.1\""
        );
        let rev = "0123456789abcdef0123456789abcdef01234567";
        assert!(git_dependency(Some(rev)).ends_with(&format!("rev = \"{rev}\"")));
        assert!(git_dependency(None).ends_with("branch = \"main\""));
        // AGENTS.md links the docs of that same commit.
        assert_eq!(
            docs_url(Some(rev)),
            format!("https://github.com/renox-rs/renox/blob/{rev}")
        );
        assert!(docs_url(None).ends_with("/blob/main"));
    }

    #[test]
    fn agents_md_says_where_the_docs_are_offline() {
        // crates.io: only the source is downloaded, so the exact clone is named (#221).
        let registry = registry_offline("1.2.3");
        assert!(
            registry.contains(
                "`git clone --depth 1 --branch v1.2.3 https://github.com/renox-rs/renox`"
            ),
            "{registry}"
        );
        assert!(registry.contains("renox-core-1.2.3/"));
        // Git: the checkout of that commit has everything.
        let git = git_offline(Some("0123456789abcdef"));
        assert!(git.contains("checkouts/renox-*/0123456/"), "{git}");
        assert!(!git.contains("git clone"));
        assert!(git_offline(None).contains("checkouts/renox-*/*/"));
        // A local checkout.
        let path = path_offline("/src/renox");
        assert!(
            path.contains("`/src/renox`") && !path.contains("git clone"),
            "{path}"
        );
    }

    const SQLITE: Options = Options {
        database: Database::Sqlite,
        tailwind: false,
        starter: false,
        notifications: false,
    };

    /// A written file with `\n` line ends (Windows checkouts give the stubs `\r\n`).
    fn read_lf(path: std::path::PathBuf) -> String {
        fs::read_to_string(path).unwrap().replace("\r\n", "\n")
    }

    /// `rnx new --tailwind`'s changes to a new app, with a build that needs no
    /// download: one that works and one that fails (the app is still made,
    /// `rnx serve` builds the CSS later).
    #[test]
    fn tailwind_moves_the_apps_css_and_links_the_built_file() {
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for works in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            run_in(dir.path(), "inked", Some(&checkout), SQLITE).unwrap();
            let root = dir.path().join("inked");
            let own = read_lf(root.join("public/app.css"));
            let built = std::cell::Cell::new(false);
            use_tailwind_with(&root, |at| {
                built.set(at == root.as_path());
                if works {
                    Ok(())
                } else {
                    Err(anyhow::anyhow!("no Tailwind here"))
                }
            })
            .unwrap();
            assert!(built.get());
            assert!(!root.join("public/app.css").exists());
            let input = read_lf(root.join(crate::tailwind::INPUT));
            assert!(input.starts_with(crate::tailwind::INPUT_STUB), "{input}");
            let rules = own
                .split_once("*/")
                .map_or(own.as_str(), |(_, r)| r.trim_start());
            assert!(input.ends_with(rules), "the app's own rules kept:\n{input}");
            let layout = read_lf(root.join("resources/views/layouts/app.html"));
            assert!(layout.contains("{{ asset('css/app.css') }}"), "{layout}");
            assert!(!layout.contains("{{ asset('app.css') }}"), "{layout}");
            let home = read_lf(root.join("resources/views/home/index.html"));
            assert!(home.contains("Tailwind is on"), "{home}");
        }
    }

    /// The starter kit with `--tailwind`: its three layouts link the built
    /// CSS, and the patterns keep their own file.
    #[test]
    fn tailwind_reaches_every_layout_of_the_starter_kit() {
        let dir = tempfile::tempdir().unwrap();
        let starter = Options {
            starter: true,
            ..SQLITE
        };
        run_in(dir.path(), "desk", None, starter).unwrap();
        let root = dir.path().join("desk");
        use_tailwind_with(&root, |_| Ok(())).unwrap();
        for layout in [
            "layouts/app.html",
            "layouts/public.html",
            "renox/auth/layout.html",
        ] {
            let html = read_lf(root.join("resources/views").join(layout));
            assert!(html.contains("{{ asset('css/app.css') }}"), "{layout}");
            assert!(!html.contains("{{ asset('app.css') }}"), "{layout}");
            assert!(html.contains("{{ asset('patterns.css') }}"), "{layout}");
        }
        assert!(root.join("public/patterns.css").is_file());
        let home = read_lf(root.join("resources/views/home/index.html"));
        assert!(home.contains("Tailwind is on"), "{home}");
    }

    #[test]
    fn linker_file_follows_what_is_found() {
        use crate::tools::Linker;
        assert!(linker_file(Some(Linker::Mold), "x86_64", true).is_none());
        assert!(linker_file(None, "x86_64", false).is_none());
        let (l, text) = linker_file(Some(Linker::Mold), "x86_64", false).unwrap();
        assert_eq!(l, Linker::Mold);
        assert!(
            text.contains("[target.x86_64-unknown-linux-gnu]") && text.contains("fuse-ld=mold")
        );
        let (l, text) = linker_file(Some(Linker::Lld), "aarch64", false).unwrap();
        assert_eq!(l, Linker::Lld);
        assert!(
            text.contains("[target.aarch64-unknown-linux-gnu]") && text.contains("fuse-ld=lld")
        );
        assert!(linker_file(Some(Linker::Mold), "riscv64", false).is_none());
    }

    #[test]
    fn makes_an_app_with_every_placeholder_filled() {
        let dir = tempfile::tempdir().unwrap();
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        run_in(dir.path(), "coffee-shop", Some(&checkout), SQLITE).unwrap();
        let root = dir.path().join("coffee-shop");
        for (file, _) in files(false) {
            let text = fs::read_to_string(root.join(file)).unwrap();
            // `{{name}}`-style placeholders (templates' `{{ x }}` has spaces).
            let left = text.split("{{").skip(1).any(|rest| {
                rest.split_once("}}").is_some_and(|(inner, _)| {
                    !inner.is_empty() && inner.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                })
            });
            assert!(!left, "{file} still has a placeholder");
        }
        let cargo = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("name = \"coffee-shop\""), "{cargo}");
        assert!(cargo.contains("renox = { path = "), "{cargo}");
        let ignore = fs::read_to_string(root.join(".gitignore")).unwrap();
        assert!(ignore.contains("/.cargo/config.toml"), "{ignore}");
        assert!(
            ignore.contains("/.vscode/renox-components.json"),
            "{ignore}"
        );
        let vscode = fs::read_to_string(root.join(".vscode/settings.json")).unwrap();
        assert!(
            vscode.contains("./.vscode/renox-components.json"),
            "{vscode}"
        );
        // The real .env has a key; the example doesn't.
        let env = read_lf(root.join(".env"));
        assert!(env.contains("APP_KEY=base64:"), "{env}");
        let example = read_lf(root.join(".env.example"));
        assert!(example.contains("APP_KEY=\n"), "{example}");
        assert!(env.contains("DATABASE_URL=sqlite://storage/app.db"));
        // AGENTS.md names the crate in its test example.
        let agents = fs::read_to_string(root.join("AGENTS.md")).unwrap();
        assert!(agents.contains("coffee_shop::app()"));
        // Not twice, and not over a bad name or a checkout that isn't Renox.
        assert!(run_in(dir.path(), "coffee-shop", None, SQLITE).is_err());
        assert!(run_in(dir.path(), "Shop", None, SQLITE).is_err());
        assert!(run_in(dir.path(), "other", Some(dir.path()), SQLITE).is_err());
    }

    #[test]
    fn postgres_apps_point_at_their_databases() {
        let dir = tempfile::tempdir().unwrap();
        run_in(
            dir.path(),
            "kasir",
            None,
            Options {
                database: Database::Postgres,
                ..SQLITE
            },
        )
        .unwrap();
        let root = dir.path().join("kasir");
        let cargo = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("features = [\"postgres\"]"), "{cargo}");
        let env = read_lf(root.join(".env"));
        assert!(env.contains("DATABASE_URL=postgres://postgres:postgres@localhost:5432/kasir\n"));
        assert!(env.contains(
            "\nTEST_DATABASE_URL=postgres://postgres:postgres@localhost:5432/kasir_test"
        ));
    }

    #[test]
    fn the_starter_kit_writes_over_the_stubs() {
        let dir = tempfile::tempdir().unwrap();
        let starter = Options {
            starter: true,
            ..SQLITE
        };
        run_in(dir.path(), "desk", None, starter).unwrap();
        let root = dir.path().join("desk");
        // Every stub once, the kit's version where it has one, and its own files.
        let files = files(true);
        assert_eq!(files.len(), STUBS.len() + 26);
        for (file, _) in &files {
            assert!(root.join(file).is_file(), "{file}");
        }
        for (file, text) in STARTER {
            let written = read_lf(root.join(file));
            let expected = text
                .replace("\r\n", "\n")
                .replace("{{crate_name}}", "desk")
                .replace("{{title}}", "Desk");
            assert_eq!(written, expected, "{file}");
        }
        let lib = fs::read_to_string(root.join("src/lib.rs")).unwrap();
        assert!(lib.contains(".module(Permissions)"), "{lib}");
        assert!(root.join("src/app/users/mod.rs").is_file());
        let tests = fs::read_to_string(root.join("tests/home.rs")).unwrap();
        assert!(tests.contains("use desk::roles"), "{tests}");
        assert!(tests.contains("assert_models_match_schema"), "{tests}");
        assert!(tests.contains("assert_views_compile"), "{tests}");
        // AGENTS.md says where the page patterns live, once, before the traps.
        let agents = read_lf(root.join("AGENTS.md"));
        let notes = agents
            .find("## The starter kit's pages")
            .expect("the starter's notes");
        assert!(notes < agents.find("## Things that trip agents up").unwrap());
        assert_eq!(agents.matches("resources/views/components/").count(), 1);
        // The plain app has none of it.
        run_in(dir.path(), "plain", None, SQLITE).unwrap();
        let plain = read_lf(dir.path().join("plain/AGENTS.md"));
        assert!(!plain.contains("## The starter kit's pages"));
    }

    #[test]
    fn notifications_edit_stubs_checked_out_with_crlf() {
        // Git on Windows checks the stubs out with CRLF endings (#151's
        // edits failed there).
        let lib = "fn app() {\r\n        .module(renox::auth::Auth::new().account()) // login, register, /account\r\n}\r\n";
        let edited = with_notifications("src/lib.rs", lib);
        assert!(
            edited.contains(".module(renox::auth::Auth::new().account().notifications())\n"),
            "{edited}"
        );
    }

    #[test]
    fn notifications_put_the_bell_in_the_plain_app() {
        let dir = tempfile::tempdir().unwrap();
        let bell = Options {
            notifications: true,
            ..SQLITE
        };
        run_in(dir.path(), "relay", None, bell).unwrap();
        let root = dir.path().join("relay");
        let lib = read_lf(root.join("src/lib.rs"));
        assert!(
            lib.contains(".module(renox::auth::Auth::new().account().notifications())\n"),
            "{lib}"
        );
        let layout = read_lf(root.join("resources/views/layouts/app.html"));
        assert!(
            layout.contains("<rx-notification-bell :count=\"unread_notifications\"/>"),
            "{layout}"
        );
        let tests = read_lf(root.join("tests/home.rs"));
        assert!(
            tests.contains("relay::app()")
                && tests.contains("fn the_bell_shows_notifications")
                && tests.contains("assert_models_match_schema"),
            "{tests}"
        );
        // Without the option, none of it; the starter kit has its own bell.
        run_in(dir.path(), "plain", None, SQLITE).unwrap();
        let layout = read_lf(dir.path().join("plain/resources/views/layouts/app.html"));
        assert!(!layout.contains("notification-bell"));
        let kit = Options {
            starter: true,
            ..bell
        };
        run_in(dir.path(), "kit", None, kit).unwrap();
        let layout = read_lf(dir.path().join("kit/resources/views/layouts/app.html"));
        assert_eq!(layout.matches("<rx-notification-bell").count(), 1);
    }

    /// The `.rs` files under `dir`.
    fn rust_files(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut files = Vec::new();
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(rust_files(&path));
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
        files
    }

    /// A new app passes `cargo fmt --check` as it is, whatever its name: rustfmt
    /// sorts `use` lines, so a stub's order must not depend on where the name
    /// sorts next to `renox` (#124: `renoxium` failed, `desk` passed).
    #[test]
    fn new_apps_are_formatted_whatever_their_name() {
        if std::process::Command::new("rustfmt")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("rustfmt isn't installed; skipped");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        for (name, starter, notifications) in [
            ("aardvark", false, false),
            ("aardvark-kit", true, false),
            ("aardvark-bell", false, true),
            ("zebra", false, false),
            ("zebra-kit", true, false),
            ("zebra-bell", false, true),
            ("renoxium", true, false),
        ] {
            let options = Options {
                starter,
                notifications,
                ..SQLITE
            };
            run_in(dir.path(), name, None, options).unwrap();
            let files = rust_files(&dir.path().join(name));
            let output = std::process::Command::new("rustfmt")
                .args(["--check", "--edition", "2024"])
                .args(&files)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{name}: rustfmt would change the new app:\n{}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
    }

    #[test]
    fn titles_names() {
        assert_eq!(title("coffee-shop_downtown"), "Coffee Shop Downtown");
    }
}
