use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use notify_debouncer_mini::notify::RecursiveMode;
use notify_debouncer_mini::{DebounceEventResult, new_debouncer};
use serde_json::Value;

/// Paths whose changes need a rebuild. Views reload inside the running app.
const WATCH: &[&str] = &["src", "migrations", "build.rs", "Cargo.toml", ".env"];

pub fn run(cargo_args: &[String]) -> Result<()> {
    if !Path::new("Cargo.toml").is_file() {
        bail!("no Cargo.toml here; run `rnx serve` from your app's directory");
    }

    children::forward_signals();
    let (tx, rx) = mpsc::channel::<DebounceEventResult>();
    let mut debouncer = new_debouncer(Duration::from_millis(300), tx)?;
    for path in WATCH.iter().map(Path::new).filter(|p| p.exists()) {
        debouncer
            .watcher()
            .watch(path, RecursiveMode::Recursive)
            .with_context(|| format!("could not watch {}", path.display()))?;
    }

    // Tailwind rebuilds public/css/app.css on its own; the app's live reload
    // then refreshes the page. It stops with rnx (Ctrl-C reaches both).
    let _tailwind = if crate::tailwind::enabled(Path::new(".")) {
        match crate::tailwind::watch(Path::new(".")) {
            Ok(child) => {
                children::track(children::Kind::Tailwind, Some(&child));
                Some(KillOnDrop(child))
            }
            Err(err) => {
                eprintln!("rnx: Tailwind didn't start: {err:#}");
                None
            }
        }
    } else {
        None
    };

    let mut snapshot = fingerprint();
    let mut app: Option<Child> = None;
    loop {
        match build(cargo_args)? {
            Some(exe) if view_data(&exe) && migrate(&exe)? => {
                if let Some(mut old) = app.take() {
                    stop(&mut old);
                }
                app = Some(start(&exe)?);
            }
            Some(_) if app.is_some() => {
                eprintln!("\nrnx: migrations failed; the previous version keeps running.")
            }
            Some(_) => eprintln!("\nrnx: migrations failed; waiting for changes…"),
            None if app.is_some() => {
                eprintln!("\nrnx: build failed; the previous version keeps running.")
            }
            None => eprintln!("\nrnx: build failed; waiting for changes…"),
        }

        snapshot = wait_for_change(&rx, snapshot);
        eprintln!("\nrnx: change detected, rebuilding…");
    }
}

/// Runs `cargo build` and returns the path of the app's binary.
pub(crate) fn build(cargo_args: &[String]) -> Result<Option<PathBuf>> {
    let output = Command::new("cargo")
        .args(["build", "--message-format=json-render-diagnostics"])
        .args(cargo_args)
        .stderr(Stdio::inherit())
        .output()
        .context("could not run cargo")?;
    if !output.status.success() {
        return Ok(None);
    }

    let exe = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|msg| msg["reason"] == "compiler-artifact")
        .filter_map(|msg| msg["executable"].as_str().map(PathBuf::from))
        .next_back();
    match exe {
        Some(exe) => Ok(Some(exe)),
        None => bail!("the build produced no binary; is this a Renox app?"),
    }
}

/// Refreshes `.vscode/renox-components.json` (editor autocomplete) with the
/// new binary. A failure only prints a warning; always returns true so the
/// build carries on to the migrations.
fn view_data(exe: &Path) -> bool {
    match Command::new(exe).arg("view:data").output() {
        Ok(out) if out.status.success() => {}
        Ok(out) => eprintln!(
            "rnx: warning: view:data failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(e) => eprintln!("rnx: warning: could not run view:data: {e}"),
    }
    true
}

/// Runs pending migrations with the new binary. Prints its output unless
/// there was nothing to do.
fn migrate(exe: &Path) -> Result<bool> {
    let output = Command::new(exe)
        .arg("migrate")
        .output()
        .with_context(|| format!("could not run {} migrate", exe.display()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim() != "Nothing to do." {
        print!("{stdout}");
    }
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    Ok(output.status.success())
}

fn start(exe: &Path) -> Result<Child> {
    let child = Command::new(exe)
        .spawn()
        .with_context(|| format!("could not start {}", exe.display()))?;
    children::track(children::Kind::App, Some(&child));
    Ok(child)
}

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        stop(&mut self.0);
    }
}

fn stop(child: &mut Child) {
    children::forget(child);
    let _ = child.kill();
    let _ = child.wait();
}

/// The app and the Tailwind watcher `rnx serve` started. A Ctrl-C in a
/// terminal reaches them too (the whole process group gets it), but a signal
/// sent to `rnx` alone (`kill`, an editor's stop button, a closed terminal)
/// would leave the app running and holding its port, so on Unix `rnx`
/// passes SIGTERM on to them before it exits.
mod children {
    use std::process::Child;
    #[cfg(unix)]
    use std::sync::atomic::{AtomicI32, Ordering};

    #[derive(Clone, Copy)]
    pub(super) enum Kind {
        App,
        Tailwind,
    }

    #[cfg(unix)]
    static PIDS: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];

    /// Records the process of `kind` (none: `None`).
    pub(super) fn track(kind: Kind, child: Option<&Child>) {
        #[cfg(unix)]
        {
            let pid = child.and_then(|c| i32::try_from(c.id()).ok()).unwrap_or(0);
            PIDS[kind as usize].store(pid, Ordering::SeqCst);
        }
        #[cfg(not(unix))]
        let _ = (kind, child);
    }

    /// Forgets `child` if it is one of those recorded.
    pub(super) fn forget(child: &Child) {
        #[cfg(unix)]
        if let Ok(pid) = i32::try_from(child.id()) {
            for slot in &PIDS {
                let _ = slot.compare_exchange(pid, 0, Ordering::SeqCst, Ordering::SeqCst);
            }
        }
        #[cfg(not(unix))]
        let _ = child;
    }

    /// Installs the handler for SIGINT, SIGTERM and SIGHUP.
    pub(super) fn forward_signals() {
        #[cfg(unix)]
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            let handler = on_signal as extern "C" fn(libc::c_int);
            // SAFETY: the handler only does async-signal-safe things: atomic
            // loads, kill(2), signal(2) and raise(3).
            unsafe {
                libc::signal(signal, handler as libc::sighandler_t);
            }
        }
    }

    /// Passes SIGTERM on, then ends `rnx` the way the signal would have.
    #[cfg(unix)]
    extern "C" fn on_signal(signal: libc::c_int) {
        for slot in &PIDS {
            let pid = slot.load(Ordering::SeqCst);
            if pid > 0 {
                // SAFETY: kill(2) is async-signal-safe; the pid is our child.
                unsafe {
                    libc::kill(pid, libc::SIGTERM);
                }
            }
        }
        // SAFETY: signal(2) and raise(3) are async-signal-safe.
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
    }
}

/// Blocks until a watched file's contents change and returns the new fingerprint.
///
/// The watcher also reports files being opened (e.g. by `cargo build` reading
/// `src/`), so events only wake us up; the fingerprint decides.
fn wait_for_change(
    rx: &mpsc::Receiver<DebounceEventResult>,
    previous: Vec<(PathBuf, SystemTime, u64)>,
) -> Vec<(PathBuf, SystemTime, u64)> {
    wait_for_change_with(rx, previous, fingerprint)
}

/// `wait_for_change` with the fingerprint taken by `fingerprint`.
fn wait_for_change_with(
    rx: &mpsc::Receiver<DebounceEventResult>,
    previous: Vec<(PathBuf, SystemTime, u64)>,
    mut fingerprint: impl FnMut() -> Vec<(PathBuf, SystemTime, u64)>,
) -> Vec<(PathBuf, SystemTime, u64)> {
    for result in rx {
        if let Err(err) = result {
            eprintln!("rnx: watch error: {err}");
            continue;
        }
        let current = fingerprint();
        if current != previous {
            return current;
        }
    }
    previous
}

/// Modification time and size of every watched file, sorted by path.
fn fingerprint() -> Vec<(PathBuf, SystemTime, u64)> {
    let mut files = Vec::new();
    for path in WATCH {
        collect(Path::new(path), &mut files);
    }
    files.sort();
    files
}

fn collect(path: &Path, files: &mut Vec<(PathBuf, SystemTime, u64)>) {
    let Ok(meta) = path.metadata() else { return };
    if meta.is_dir() {
        let Ok(entries) = path.read_dir() else { return };
        for entry in entries.flatten() {
            collect(&entry.path(), files);
        }
    } else if !is_ignored(path) {
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        files.push((path.to_path_buf(), modified, meta.len()));
    }
}

/// Editor swap and backup files.
fn is_ignored(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| (n.starts_with('.') && n != ".env") || n.ends_with('~'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn editor_files_are_ignored_but_env_is_not() {
        for ignored in [".main.rs.swp", "src/.#lib.rs", "main.rs~", ".DS_Store"] {
            assert!(is_ignored(Path::new(ignored)), "{ignored}");
        }
        for watched in ["src/main.rs", ".env", "Cargo.toml", "migrations/x.up.sql"] {
            assert!(!is_ignored(Path::new(watched)), "{watched}");
        }
    }

    #[test]
    fn the_fingerprint_changes_with_content_not_with_ignored_files() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(src.join("app")).unwrap();
        fs::write(src.join("main.rs"), "fn main() {}").unwrap();
        fs::write(src.join("app/mod.rs"), "").unwrap();
        let print = || {
            let mut files = Vec::new();
            collect(&src, &mut files);
            collect(&dir.path().join("missing"), &mut files); // nothing, no error
            files.sort();
            files
        };
        let first = print();
        assert_eq!(first.len(), 2);
        assert_eq!(print(), first, "reading files changes nothing");

        fs::write(src.join(".main.rs.swp"), "swap").unwrap();
        assert_eq!(print(), first, "swap files don't count");

        fs::write(src.join("main.rs"), "fn main() { println!(); }").unwrap();
        assert_ne!(print(), first, "a new size is a change");

        // The same size, a new modification time (an editor saving the same
        // length): a change too.
        let second = print();
        let file = fs::File::options()
            .write(true)
            .open(src.join("app/mod.rs"))
            .unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(86_400))
            .unwrap();
        assert_ne!(print(), second, "a new modification time is a change");
    }

    /// Events only wake the loop: it returns when the fingerprint differs,
    /// logs watch errors, and gives up when the watcher is gone.
    #[test]
    fn events_wake_the_loop_and_the_fingerprint_decides() {
        let at = |secs| {
            (
                PathBuf::from("src/main.rs"),
                SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs),
                1,
            )
        };
        let (tx, rx) = mpsc::channel::<DebounceEventResult>();
        // An open (no change), a watch error, then a real change.
        tx.send(Ok(Vec::new())).unwrap();
        tx.send(Err(notify_debouncer_mini::notify::Error::generic(
            "watch failed",
        )))
        .unwrap();
        tx.send(Ok(Vec::new())).unwrap();
        let mut looks = vec![vec![at(1)], vec![at(2)]].into_iter();
        let changed = wait_for_change_with(&rx, vec![at(1)], || looks.next().unwrap());
        assert_eq!(changed, vec![at(2)]);
        assert!(
            looks.next().is_none(),
            "looked once per event, not for the error"
        );
        // The watcher gone: the previous fingerprint comes back.
        drop(tx);
        assert_eq!(
            wait_for_change_with(&rx, vec![at(3)], Vec::new),
            vec![at(3)]
        );
    }
}
