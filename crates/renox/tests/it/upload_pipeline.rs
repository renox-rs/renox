//! The upload pipeline (#43): steps that refuse, change and derive files, the
//! ClamAV step against a fake `clamd`, and encrypted storage.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use renox::prelude::*;
use renox::testing::TestApp;
use renox::upload::{BoxFuture, ClamAv, Pipeline, StepOutcome, UploadContext, UploadStep};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const EICAR: &[u8] = b"X5O!P%@AP[4\\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*";

#[derive(Deserialize)]
struct FileForm {
    name: String,
    file: Option<Upload>,
}

impl Validate for FileForm {
    fn rules(&self, v: &mut Validator) {
        v.field("name", &self.name).required();
    }
}

#[derive(Deserialize)]
struct ListForm {
    files: Vec<Upload>,
}

impl Validate for ListForm {
    fn rules(&self, _: &mut Validator) {}
}

/// Refuses files that contain `BAD`, naming the field in the message.
struct NoBad;

impl UploadStep for NoBad {
    fn process<'a>(
        &'a self,
        file: &'a mut Upload,
        ctx: &'a UploadContext,
    ) -> BoxFuture<'a, Result<StepOutcome>> {
        Box::pin(async move {
            Ok(if file.bytes().windows(3).any(|w| w == b"BAD") {
                StepOutcome::Reject(format!("{} is not allowed here.", ctx.field()))
            } else {
                StepOutcome::Keep
            })
        })
    }
}

/// Upper-cases the content and derives a lower-case copy.
struct Shout {
    runs: Arc<AtomicUsize>,
}

impl UploadStep for Shout {
    fn process<'a>(
        &'a self,
        file: &'a mut Upload,
        _ctx: &'a UploadContext,
    ) -> BoxFuture<'a, Result<StepOutcome>> {
        Box::pin(async move {
            self.runs.fetch_add(1, Ordering::SeqCst);
            let loud = file.bytes().to_ascii_uppercase();
            let quiet = Upload::new(file.file_name(), "text/plain", file.bytes().to_vec());
            file.set_bytes(loud);
            Ok(StepOutcome::Derived("quiet".into(), quiet))
        })
    }
}

struct Site;

impl Module for Site {
    fn name(&self) -> &'static str {
        "site"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/form", || async { view("form.html", ()) })
            .post(
                "/files",
                |State(state): State<AppState>, Valid(form): Valid<FileForm>| async move {
                    let Some(file) = form.file else {
                        return Ok::<_, Error>(format!("{} no file", form.name));
                    };
                    let key = file.store(&state.storage, "docs").await?;
                    let stored = state.storage.get(&key).await?.unwrap();
                    let quiet = state
                        .storage
                        .get(&Upload::derived_key(&key, "quiet"))
                        .await?;
                    Ok(format!(
                        "{} {key} {} {}",
                        form.name,
                        String::from_utf8_lossy(&stored),
                        quiet.map_or("none".into(), |q| String::from_utf8_lossy(&q).into_owned())
                    ))
                },
            )
            .post("/list", |Valid(form): Valid<ListForm>| async move {
                form.files
                    .iter()
                    .map(|f| String::from_utf8_lossy(f.bytes()).into_owned())
                    .collect::<Vec<_>>()
                    .join(",")
            })
    }
}

async fn app(pipeline: Pipeline) -> (TestApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("views")).unwrap();
    std::fs::write(dir.path().join("views/form.html"), "[{{ error('file') }}]").unwrap();
    let views = dir.path().join("views");
    let app = TestApp::with_config(App::new().uploads(pipeline).module(Site), move |c| {
        c.views_path = views;
    })
    .await;
    (app, dir)
}

#[renox::test]
async fn a_rejecting_step_is_an_error_on_the_field() {
    let (app, _dir) = app(Pipeline::new().step(NoBad)).await;
    // HTMX: 422 with the error under the file's field.
    let res = app
        .htmx()
        .post_multipart(
            "/files",
            &[("name", "a")],
            &[("file", "x.txt", b"this is BAD")],
        )
        .await;
    res.assert_invalid("file")
        .assert_see("file is not allowed here.");
    // A plain post: back to the form with the error flashed.
    let res = app
        .request()
        .header("referer", "/form")
        .post_multipart(
            "/files",
            &[("name", "a")],
            &[("file", "x.txt", b"this is BAD")],
        )
        .await;
    res.assert_redirect("/form");
    app.get("/form")
        .await
        .assert_see("[file is not allowed here.]");
    // A clean file goes through.
    app.post_multipart("/files", &[("name", "a")], &[("file", "x.txt", b"fine")])
        .await
        .assert_ok()
        .assert_see("fine");
    // Each file of a list is checked; the error is on the list's field.
    let res = app
        .htmx()
        .post_multipart(
            "/list",
            &[],
            &[("files", "a.txt", b"ok"), ("files", "b.txt", b"BAD")],
        )
        .await;
    res.assert_invalid("files");
}

#[renox::test]
async fn steps_wait_for_the_rules_and_changes_reach_the_handler() {
    let runs = Arc::new(AtomicUsize::new(0));
    let (app, _dir) = app(Pipeline::new().step(Shout { runs: runs.clone() })).await;
    // A rule fails (no name): the steps don't run.
    app.htmx()
        .post_multipart("/files", &[], &[("file", "x.txt", b"quiet words")])
        .await
        .assert_invalid("name");
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    // No file: nothing to process.
    app.post_multipart("/files", &[("name", "n")], &[])
        .await
        .assert_see("n no file");
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    // The handler gets the changed file, and the derived one lands next to it.
    let res = app
        .post_multipart(
            "/files",
            &[("name", "n")],
            &[("file", "x.txt", b"quiet words")],
        )
        .await;
    res.assert_ok().assert_see("QUIET WORDS quiet words");
    assert!(res.text().contains("n docs/") && res.text().contains(".txt QUIET"));
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    // Nothing configured: the file is as it came.
    let (plain, _dir) = app_without_steps().await;
    plain
        .post_multipart(
            "/files",
            &[("name", "n")],
            &[("file", "x.txt", b"quiet words")],
        )
        .await
        .assert_see("quiet words none");
}

async fn app_without_steps() -> (TestApp, tempfile::TempDir) {
    app(Pipeline::new()).await
}

#[renox::test]
async fn process_runs_the_steps_for_a_file_that_isnt_in_a_form() {
    let (app, _dir) = app(Pipeline::new().step(NoBad)).await;
    let state = app.state();
    let mut good = Upload::new("a.txt", "text/plain", b"fine".to_vec());
    good.process(state).await.unwrap();
    let mut bad = Upload::new("a.txt", "text/plain", b"BAD".to_vec());
    let err = bad.process_field(state, "attachment").await.unwrap_err();
    match err {
        Error::Validation(e) => assert_eq!(
            e.errors.first("attachment"),
            Some("attachment is not allowed here.")
        ),
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[test]
fn derived_keys_go_before_the_extension() {
    assert_eq!(Upload::derived_key("a/b.c.jpg", "thumb"), "a/b.c.thumb.jpg");
    assert_eq!(Upload::derived_key("noext", "thumb"), "noext.thumb");
    assert_eq!(Upload::derived_key("x.png", "t"), "x.t.png");
    let mut file = Upload::new("a.png", "image/png", b"x".to_vec());
    file.derive("th umb!", Upload::new("a.png", "image/png", b"1".to_vec()));
    file.derive("thumb", Upload::new("a.png", "image/png", b"2".to_vec()));
    assert_eq!(file.derived().len(), 1, "same name replaces");
    assert_eq!(&file.derived_named("thumb").unwrap().bytes()[..], b"2");
}

// ---- ClamAV against a fake clamd ----

#[derive(Clone, Copy)]
enum Daemon {
    /// Finds EICAR, says OK to the rest.
    Scans,
    /// Takes the file and never answers.
    Silent,
    /// Refuses with an error.
    Errors,
}

/// A clamd on a free port; returns its address and how many files it scanned.
async fn fake_clamd(mode: Daemon) -> (String, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let scanned = Arc::new(AtomicUsize::new(0));
    let count = scanned.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let count = count.clone();
            tokio::spawn(async move {
                let mut command = [0u8; 10];
                socket.read_exact(&mut command).await.unwrap();
                assert_eq!(&command, b"zINSTREAM\0");
                let mut content = Vec::new();
                loop {
                    let mut len = [0u8; 4];
                    socket.read_exact(&mut len).await.unwrap();
                    let len = u32::from_be_bytes(len) as usize;
                    if len == 0 {
                        break;
                    }
                    let mut chunk = vec![0u8; len];
                    socket.read_exact(&mut chunk).await.unwrap();
                    content.extend_from_slice(&chunk);
                }
                count.fetch_add(1, Ordering::SeqCst);
                let reply: &[u8] = match mode {
                    Daemon::Silent => {
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        return;
                    }
                    Daemon::Errors => b"INSTREAM size limit exceeded. ERROR\0",
                    Daemon::Scans if content.windows(EICAR.len()).any(|w| w == EICAR) => {
                        b"stream: Eicar-Test-Signature FOUND\0"
                    }
                    Daemon::Scans => b"stream: OK\0",
                };
                socket.write_all(reply).await.unwrap();
            });
        }
    });
    (address, scanned)
}

async fn scan(step: ClamAv, bytes: &[u8]) -> Option<String> {
    let (app, _dir) = app(Pipeline::new().step(step)).await;
    let mut file = Upload::new("f.bin", "application/octet-stream", bytes.to_vec());
    match file.process(app.state()).await {
        Ok(()) => None,
        Err(Error::Validation(e)) => Some(e.errors.first("file").unwrap().to_owned()),
        Err(other) => panic!("{other:?}"),
    }
}

#[renox::test]
async fn clamav_refuses_the_eicar_test_file_and_passes_clean_ones() {
    let (address, scanned) = fake_clamd(Daemon::Scans).await;
    // More than one chunk of clean data.
    let big = vec![b'a'; 200 * 1024];
    assert_eq!(scan(ClamAv::tcp(&address), &big).await, None);
    assert_eq!(
        scan(ClamAv::tcp(&address), EICAR).await.as_deref(),
        Some("The file was refused: it contains malware.")
    );
    assert_eq!(scanned.load(Ordering::SeqCst), 2);

    // Through a form: a 422 on the file's field.
    let (app, _dir) = app(Pipeline::new().step(ClamAv::tcp(&address))).await;
    app.htmx()
        .post_multipart("/files", &[("name", "n")], &[("file", "eicar.com", EICAR)])
        .await
        .assert_invalid("file");
    app.post_multipart("/files", &[("name", "n")], &[("file", "ok.txt", b"hello")])
        .await
        .assert_ok();
}

#[renox::test]
async fn clamav_fails_closed_unless_told_otherwise() {
    let slow = Duration::from_millis(150);
    let message = "The file could not be checked for malware. Try again later.";
    // A daemon that never answers.
    let (silent, _) = fake_clamd(Daemon::Silent).await;
    assert_eq!(
        scan(ClamAv::tcp(&silent).timeout(slow), b"x")
            .await
            .as_deref(),
        Some(message)
    );
    assert_eq!(
        scan(ClamAv::tcp(&silent).timeout(slow).fail_open(), b"x").await,
        None
    );
    // A daemon that answers with an error.
    let (broken, _) = fake_clamd(Daemon::Errors).await;
    assert_eq!(
        scan(ClamAv::tcp(&broken), b"x").await.as_deref(),
        Some(message)
    );
    assert_eq!(scan(ClamAv::tcp(&broken).fail_open(), b"x").await, None);
    // Nobody listens.
    let gone = {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap().to_string()
    };
    assert_eq!(
        scan(ClamAv::tcp(&gone), b"x").await.as_deref(),
        Some(message)
    );
    assert_eq!(scan(ClamAv::tcp(&gone).fail_open(), b"x").await, None);
}

// ---- Encrypted files ----

#[renox::test]
async fn encrypted_files_round_trip_and_are_served_decrypted() {
    let (app, _dir) = app(Pipeline::new()).await;
    let state = app.state();
    let storage = &state.storage;
    let mut card = Upload::new("card.txt", "text/plain", b"card 4242".to_vec());
    card.derive(
        "copy",
        Upload::new("card.txt", "text/plain", b"copy 4242".to_vec()),
    );
    let key = card.store_encrypted(storage, "ids").await.unwrap();
    assert!(key.starts_with("ids/") && key.ends_with(".txt"), "{key}");

    // At rest: a header and no plain text.
    let raw = storage.get(&key).await.unwrap().unwrap();
    assert!(raw.starts_with(b"RNXE\x01"));
    assert!(!raw.windows(4).any(|w| w == b"4242"));
    assert_eq!(
        &storage.get_decrypted(&key).await.unwrap().unwrap()[..],
        b"card 4242"
    );
    assert_eq!(
        &storage
            .get_decrypted(&Upload::derived_key(&key, "copy"))
            .await
            .unwrap()
            .unwrap()[..],
        b"copy 4242"
    );
    assert_eq!(
        storage.get_decrypted("ids/missing.txt").await.unwrap(),
        None
    );
    // A file that isn't encrypted is an error, not garbage.
    storage.put("plain.txt", "hi".into()).await.unwrap();
    assert!(storage.get_decrypted("plain.txt").await.is_err());

    // A temporary URL decrypts for the request; the signature is still needed.
    let url = storage
        .temporary_url(state, &key, Duration::from_secs(60))
        .await
        .unwrap();
    let path = url.trim_start_matches("http://127.0.0.1:3000");
    let res = app.get(path).await;
    res.assert_ok().assert_see("card 4242");
    app.get(&format!("/_renox/files/{key}"))
        .await
        .assert_forbidden();
    // Plain files are served as they are.
    let url = storage
        .temporary_url(state, "plain.txt", Duration::from_secs(60))
        .await
        .unwrap();
    app.get(url.trim_start_matches("http://127.0.0.1:3000"))
        .await
        .assert_see("hi");

    // Download::decrypted.
    let download = renox::Download::decrypted(storage, &key, "card.txt")
        .await
        .unwrap();
    let body = axum::response::IntoResponse::into_response(download);
    let bytes = http_body_util_collect(body).await;
    assert_eq!(bytes, b"card 4242");
    assert!(matches!(
        renox::Download::decrypted(storage, "ids/missing.txt", "x").await,
        Err(Error::NotFound)
    ));
}

async fn http_body_util_collect(res: axum::response::Response) -> Vec<u8> {
    axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec()
}

#[renox::test]
async fn a_rotated_app_key_needs_the_files_rewrapped() {
    let old_key = renox::generate_key();
    let old = old_key.clone();
    let (before, _dir) = {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let app = TestApp::with_config(App::new(), move |c| {
            c.key = Some(old);
            c.storage_path = path;
        })
        .await;
        (app, dir)
    };
    let root = _dir.path().to_path_buf();
    before
        .state()
        .storage
        .put_encrypted("ids/a.txt", "secret".into())
        .await
        .unwrap();
    before
        .state()
        .storage
        .put("ids/b.txt", "plain".into())
        .await
        .unwrap();

    // The same files under a new APP_KEY.
    let after = TestApp::with_config(App::new(), {
        let root = root.clone();
        move |c| {
            c.key = Some(renox::generate_key());
            c.storage_path = root;
        }
    })
    .await;
    let storage = &after.state().storage;
    assert!(storage.get_decrypted("ids/a.txt").await.is_err());
    assert_eq!(storage.rewrap_encrypted("", &old_key).await.unwrap(), 1);
    assert_eq!(
        &storage.get_decrypted("ids/a.txt").await.unwrap().unwrap()[..],
        b"secret"
    );
    // Running it again changes nothing.
    assert_eq!(storage.rewrap_encrypted("", &old_key).await.unwrap(), 0);
    assert_eq!(
        &storage.get("ids/b.txt").await.unwrap().unwrap()[..],
        b"plain"
    );
}
