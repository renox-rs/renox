//! `STORAGE_DISK=s3` against a real S3-compatible server. CI runs SeaweedFS
//! (MinIO no longer publishes images):
//!
//! ```text
//! docker run -d --rm --name renox-s3 -p 8333:8333 -e AWS_ACCESS_KEY_ID=renox \
//!     -e AWS_SECRET_ACCESS_KEY=renox-secret chrislusf/seaweedfs:4.47 server -s3
//! docker exec renox-s3 sh -c 'echo "s3.bucket.create -name renox-test" | weed shell'
//! TEST_S3_ENDPOINT=http://127.0.0.1:8333 TEST_S3_BUCKET=renox-test \
//!     TEST_S3_ACCESS_KEY_ID=renox TEST_S3_SECRET_ACCESS_KEY=renox-secret \
//!     cargo test -p renox --features s3 --test it s3
//! ```
//!
//! Without `TEST_S3_ENDPOINT` these tests pass without doing anything.

use std::time::Duration;

use renox::prelude::*;
use renox::testing::TestApp;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A test app on the S3 disk, or `None` when no server is configured.
async fn s3_app(app: App) -> Option<TestApp> {
    let endpoint = std::env::var("TEST_S3_ENDPOINT").ok()?;
    let var = |name: &str| {
        std::env::var(name).unwrap_or_else(|_| panic!("{name} is required with TEST_S3_ENDPOINT"))
    };
    let (bucket, id, secret) = (
        var("TEST_S3_BUCKET"),
        var("TEST_S3_ACCESS_KEY_ID"),
        var("TEST_S3_SECRET_ACCESS_KEY"),
    );
    Some(
        TestApp::with_config(app, |c| {
            c.storage.disk = renox::storage::DiskDriver::S3;
            c.storage.endpoint = Some(endpoint.clone());
            c.storage.bucket = Some(bucket);
            c.storage.region = Some("us-east-1".into());
            c.storage.access_key_id = Some(id);
            c.storage.secret_access_key = Some(secret);
            c.storage.url = Some(format!("{endpoint}/public-cdn"));
        })
        .await,
    )
}

/// A plain HTTP GET (MinIO in CI speaks http): the status and the body.
async fn http_get(url: &str) -> (u16, Vec<u8>) {
    let rest = url.strip_prefix("http://").expect("an http:// URL");
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let mut stream = tokio::net::TcpStream::connect(host).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let split = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&response[..split]);
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    // Small bodies without chunking are enough here.
    (status, response[split + 4..].to_vec())
}

#[renox::test]
async fn files_round_trip_through_s3() {
    let Some(app) = s3_app(App::new()).await else {
        return;
    };
    let storage = &app.state().storage;
    let key = format!(
        "private/{}.txt",
        renox::generate_key().replace(['/', '+', '='], "")
    );

    assert!(!storage.exists(&key).await.unwrap());
    assert_eq!(storage.get(&key).await.unwrap(), None);
    storage.put(&key, "hello world".into()).await.unwrap();
    assert!(storage.exists(&key).await.unwrap());
    assert_eq!(
        storage.get(&key).await.unwrap().as_deref(),
        Some(&b"hello world"[..])
    );

    // Presigned by S3, readable without credentials until it expires.
    let link = storage
        .temporary_url(app.state(), &key, Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(http_get(&link).await, (200, b"hello world".to_vec()));
    let (status, _) = http_get(&link.replace("X-Amz-Signature=", "X-Amz-Signature=0")).await;
    assert_eq!(status, 403, "a changed signature is refused");
    let (status, _) = http_get(link.split('?').next().unwrap()).await;
    assert_eq!(status, 403, "private files need the signature");

    let short = storage
        .temporary_url(app.state(), &key, Duration::from_secs(1))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(2100)).await;
    assert_eq!(http_get(&short).await.0, 403, "an expired link is refused");

    assert_eq!(
        storage.url("public/logo.png"),
        format!(
            "{}/public-cdn/public/logo.png",
            std::env::var("TEST_S3_ENDPOINT").unwrap()
        )
    );

    storage.delete(&key).await.unwrap();
    storage.delete(&key).await.unwrap(); // deleting twice is fine
    assert!(!storage.exists(&key).await.unwrap());
    assert!(storage.put("../escape.txt", "x".into()).await.is_err());

    // Listing, copying and moving, under a folder of this run only.
    let dir = format!("{}-tree", key.trim_end_matches(".txt"));
    storage
        .put(&format!("{dir}/a.txt"), "a".into())
        .await
        .unwrap();
    storage
        .put(&format!("{dir}/sub/b.txt"), "bb".into())
        .await
        .unwrap();
    storage
        .copy(&format!("{dir}/a.txt"), &format!("{dir}/c.txt"))
        .await
        .unwrap();
    storage
        .rename(&format!("{dir}/sub/b.txt"), &format!("{dir}/d.txt"))
        .await
        .unwrap();
    let listed: Vec<(String, u64)> = storage
        .list(&dir)
        .await
        .unwrap()
        .into_iter()
        .map(|f| (f.key, f.size))
        .collect();
    assert_eq!(
        listed,
        [
            (format!("{dir}/a.txt"), 1),
            (format!("{dir}/c.txt"), 1),
            (format!("{dir}/d.txt"), 2)
        ]
    );
    assert_eq!(
        storage.size(&format!("{dir}/d.txt")).await.unwrap(),
        Some(2)
    );
    assert!(
        storage
            .copy(&format!("{dir}/nope"), &format!("{dir}/x"))
            .await
            .is_err()
    );
    assert_eq!(storage.delete_all(&dir).await.unwrap(), 3);
    assert!(storage.list(&dir).await.unwrap().is_empty());
}

#[derive(serde::Deserialize)]
struct AvatarForm {
    avatar: Upload,
}

impl Validate for AvatarForm {
    fn rules(&self, v: &mut Validator) {
        v.field("avatar", &self.avatar).required().image();
    }
}

struct Uploads;

impl Module for Uploads {
    fn name(&self) -> &'static str {
        "uploads"
    }

    fn routes(&self) -> Routes {
        Routes::new().post(
            "/avatar",
            |State(state): State<AppState>, Valid(form): Valid<AvatarForm>| async move {
                let key = form.avatar.store(&state.storage, "avatars").await?;
                let bytes = state.storage.get(&key).await?.unwrap_or_default();
                Ok::<_, Error>(format!("{key} {}", bytes.len()))
            },
        )
    }
}

#[renox::test]
async fn uploads_are_stored_on_s3() {
    let Some(app) = s3_app(App::new().module(Uploads)).await else {
        return;
    };
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
    let res = app
        .post_multipart("/avatar", &[], &[("avatar", "me.png", png)])
        .await;
    res.assert_ok();
    let body = res.text();
    let (key, len) = body.split_once(' ').unwrap();
    assert!(
        key.starts_with("avatars/") && key.ends_with(".png"),
        "{key}"
    );
    assert_eq!(len, png.len().to_string());
    app.state().storage.delete(key).await.unwrap();
}
