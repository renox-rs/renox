//! What a test logs, for tests of warnings: `let (logs, _guard) =
//! logs::capture();` then `logs.text()`. The subscriber is this thread's
//! (`#[renox::test]` runs on one), so tests running in parallel don't mix.

use std::sync::{Arc, Mutex};

/// The captured lines.
#[derive(Clone, Default)]
pub struct Logs(Arc<Mutex<Vec<u8>>>);

impl Logs {
    /// Everything logged so far.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }

    /// Whether a line holds every one of `parts`.
    pub fn has(&self, parts: &[&str]) -> bool {
        self.text()
            .lines()
            .any(|line| parts.iter().all(|part| line.contains(part)))
    }
}

impl std::io::Write for Logs {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Logs {
    type Writer = Logs;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Starts capturing this thread's logs (debug and up) until the guard drops.
pub fn capture() -> (Logs, tracing::subscriber::DefaultGuard) {
    let logs = Logs::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(logs.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::DEBUG)
        .finish();
    (logs, tracing::subscriber::set_default(subscriber))
}
