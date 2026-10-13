//! Browser live reload while developing (`APP_ENV=local` with `APP_DEBUG` on).
//!
//! The page subscribes to `/_renox/live` (Server-Sent Events). When a file
//! under the views, public or lang directories changes, the server sends
//! `reload`. When `rnx serve` restarts the app, the browser reconnects, sees a
//! new boot id and reloads too. Polling file times keeps it portable.

use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use axum::Router;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use futures_util::stream::{self, Stream, StreamExt};
use tokio::sync::broadcast;

use crate::crypto::random_token;
use crate::{AppState, Error, Result};

const POLL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy)]
enum Message {
    Reload,
    Stop,
}

pub(crate) struct Live {
    boot: String,
    tx: broadcast::Sender<Message>,
}

fn fingerprint(paths: &[PathBuf]) -> Vec<(PathBuf, SystemTime, u64)> {
    fn walk(path: &Path, out: &mut Vec<(PathBuf, SystemTime, u64)>) {
        let Ok(meta) = path.metadata() else { return };
        if meta.is_dir() {
            for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
                walk(&entry.path(), out);
            }
        } else {
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            out.push((path.to_path_buf(), modified, meta.len()));
        }
    }
    let mut files = Vec::new();
    for path in paths {
        walk(path, &mut files);
    }
    files.sort();
    files
}

impl Live {
    /// Starts watching `paths`; must run inside the Tokio runtime.
    pub(crate) fn start(paths: Vec<PathBuf>) -> Arc<Self> {
        let (tx, _) = broadcast::channel(16);
        let live = Arc::new(Self {
            boot: random_token()[..12].to_owned(),
            tx,
        });
        let watcher = Arc::downgrade(&live);
        tokio::spawn(async move {
            let mut last = fingerprint(&paths);
            loop {
                tokio::time::sleep(POLL).await;
                let Some(live) = watcher.upgrade() else { break };
                let now = fingerprint(&paths);
                if now != last {
                    last = now;
                    let _ = live.tx.send(Message::Reload);
                }
            }
        });
        live
    }

    /// Ends open event streams, so a graceful shutdown doesn't wait for them.
    pub(crate) fn stop(&self) {
        let _ = self.tx.send(Message::Stop);
    }
}

pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/_renox/live", get(events))
}

async fn events(
    State(state): State<AppState>,
) -> Result<Sse<impl Stream<Item = std::result::Result<Event, Infallible>>>> {
    let live = state.live.clone().ok_or(Error::NotFound)?;
    let hello = Event::default()
        .event("boot")
        .data(live.boot.clone())
        .retry(Duration::from_millis(500));
    let updates = stream::unfold(live.tx.subscribe(), |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(Message::Reload) => {
                    return Some((Event::default().event("reload").data("changed"), rx));
                }
                Ok(Message::Stop) | Err(broadcast::error::RecvError::Closed) => return None,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    });
    let events = stream::once(async move { hello }).chain(updates).map(Ok);
    Ok(Sse::new(events).keep_alive(KeepAlive::default()))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;

    /// A changed file sends `reload`; stopping ends every open stream, even
    /// one that fell behind.
    #[tokio::test]
    async fn the_stream_sends_reloads_and_ends_when_stopped() {
        let app = crate::testing::TestApp::with_config(crate::App::new(), |c| {
            c.env = crate::Environment::Local;
            c.debug = true;
        })
        .await;
        let live = app
            .state()
            .live
            .clone()
            .expect("live reload while developing");
        let req = axum::http::Request::builder()
            .uri("/_renox/live")
            .body(Body::empty())
            .unwrap();
        let res = app.kernel().router().oneshot(req).await.unwrap();
        let mut body = res.into_body();
        let first = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert!(String::from_utf8_lossy(&first).contains("event: boot"));
        // More than the channel holds: the stream skips what it missed.
        for _ in 0..40 {
            let _ = live.tx.send(Message::Reload);
        }
        live.stop();
        let rest = tokio::time::timeout(Duration::from_secs(5), body.collect())
            .await
            .expect("the stream ends")
            .unwrap()
            .to_bytes();
        let rest = String::from_utf8_lossy(&rest);
        assert!(rest.contains("event: reload"), "{rest}");
    }

    #[tokio::test]
    async fn a_changed_file_is_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let live = Live::start(vec![dir.path().to_path_buf()]);
        let mut rx = live.tx.subscribe();
        // The watcher's first look, before the change.
        tokio::time::sleep(POLL / 5).await;
        std::fs::write(dir.path().join("page.html"), "new").unwrap();
        let message = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("a message")
            .unwrap();
        assert!(matches!(message, Message::Reload));
        // Dropped: the watcher stops at its next look.
        drop(live);
        tokio::time::sleep(POLL * 2).await;
    }
}
