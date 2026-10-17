//! Offline forms (offline-forms.test.mjs, #52): a visit form that posts to an
//! `.offline()` route, and a list of what the server applied, so a test can
//! see that each queued post arrived once, in order, with its file intact.
//!
//! A note longer than 20 characters is a 422 with field errors; the note
//! `conflict` is a 409.

use std::sync::Mutex;

use renox::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Validate)]
struct Visit {
    #[validate(required, max = 20)]
    note: String,
    #[serde(default)]
    photo: Option<Upload>,
}

/// What the server did with one post.
#[derive(Clone, Serialize)]
struct Applied {
    note: String,
    file_name: Option<String>,
    file_size: usize,
    /// The sum of the file's bytes, to see that it arrived intact.
    file_sum: u64,
    recorded_at: String,
    client_id: String,
    skew_exceeded: bool,
}

static APPLIED: Mutex<Vec<Applied>> = Mutex::new(Vec::new());

async fn store(offline: Offline, Valid(visit): Valid<Visit>) -> Result<Json<usize>> {
    if visit.note == "conflict" {
        return Err(abort(StatusCode::CONFLICT, "Already recorded elsewhere."));
    }
    let bytes = visit.photo.as_ref().map(|p| p.bytes().to_vec());
    let mut applied = APPLIED.lock().unwrap();
    applied.push(Applied {
        note: visit.note,
        file_name: visit.photo.as_ref().map(|p| p.file_name().to_owned()),
        file_size: bytes.as_ref().map_or(0, Vec::len),
        file_sum: bytes
            .iter()
            .flatten()
            .fold(0u64, |sum, b| sum + u64::from(*b)),
        recorded_at: offline.recorded_at.to_rfc3339(),
        client_id: offline.client_id.unwrap_or_default(),
        skew_exceeded: offline.skew_exceeded,
    });
    Ok(Json(applied.len()))
}

pub struct Visits;

impl Module for Visits {
    fn name(&self) -> &'static str {
        "offline-visits"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/offline-form", || async {
                view("offline_form.html", context! {})
            })
            .get("/offline/visits.json", || async {
                Json(APPLIED.lock().unwrap().clone())
            })
            .post("/offline/visits", store)
            .offline()
    }
}
