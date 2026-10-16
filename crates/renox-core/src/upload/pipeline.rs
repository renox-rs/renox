//! The upload pipeline: steps every uploaded file passes before the handler
//! sees it (a malware scan, image processing, a format conversion).

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use futures_util::future::BoxFuture;

use super::Upload;
use crate::validation::Errors;
use crate::{AppState, Result};

/// What a step decided about a file.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum StepOutcome {
    /// The file goes on (possibly changed by the step) to the next step.
    Keep,
    /// The file is refused: the message becomes a validation error on the
    /// form field the file came from, and no later step runs for it.
    Reject(String),
    /// The file goes on, and this extra file is stored next to it under the
    /// name (a thumbnail): see [`Upload::derived`] and [`Upload::derived_key`].
    Derived(String, Upload),
}

/// What a step knows about the file it processes.
#[derive(Clone)]
#[non_exhaustive]
pub struct UploadContext {
    field: String,
    state: AppState,
}

impl UploadContext {
    pub(crate) fn new(field: impl Into<String>, state: &AppState) -> Self {
        Self {
            field: field.into(),
            state: state.clone(),
        }
    }

    /// The form field the file came from, e.g. `photo` or `documents.0`
    /// (`file` for [`Upload::process`]).
    pub fn field(&self) -> &str {
        &self.field
    }

    /// The app's state: storage, database, HTTP client, config.
    pub fn state(&self) -> &AppState {
        &self.state
    }
}

impl fmt::Debug for UploadContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UploadContext")
            .field("field", &self.field)
            .finish_non_exhaustive()
    }
}

/// One step of the [`Pipeline`]. Implement it for your own processing, e.g.
/// converting a phone's HEIC photo to JPEG with an external tool:
///
/// ```
/// # use renox::prelude::*;
/// use renox::upload::{StepOutcome, UploadContext, UploadStep};
/// use renox::upload::BoxFuture;
///
/// struct NoExecutables;
///
/// impl UploadStep for NoExecutables {
///     fn process<'a>(
///         &'a self,
///         file: &'a mut Upload,
///         _ctx: &'a UploadContext,
///     ) -> BoxFuture<'a, Result<StepOutcome>> {
///         Box::pin(async move {
///             Ok(if file.bytes().starts_with(b"MZ") {
///                 StepOutcome::Reject("Programs can't be uploaded.".into())
///             } else {
///                 StepOutcome::Keep
///             })
///         })
///     }
/// }
/// # let _ = App::new().uploads(renox::upload::Pipeline::new().step(NoExecutables));
/// ```
///
/// A step changes the file in place (`file.set_bytes(..)`), refuses it, or
/// derives another file from it. An `Err` is a failure of the step itself
/// (a 500), not of the file: a step that can't do its job should refuse the
/// file instead when it must fail closed.
pub trait UploadStep: Send + Sync + 'static {
    /// Processes one file.
    fn process<'a>(
        &'a self,
        file: &'a mut Upload,
        ctx: &'a UploadContext,
    ) -> BoxFuture<'a, Result<StepOutcome>>;
}

/// The steps uploads pass through, in order: [`App::uploads`](crate::App::uploads).
///
/// ```
/// # use renox::prelude::*;
/// use renox::upload::{ClamAv, Pipeline};
///
/// # let _ =
/// App::new().uploads(Pipeline::new().step(ClamAv::tcp("127.0.0.1:3310")))
/// # ;
/// ```
///
/// Inside [`Valid`](crate::Valid) the steps run after the form's rules, for
/// every file of a multipart form; a refusal is an error on that file's field
/// (a redirect back with the errors, or `422` for HTMX and JSON clients).
/// Elsewhere call [`Upload::process`].
#[derive(Clone, Default)]
pub struct Pipeline {
    steps: Vec<Arc<dyn UploadStep>>,
}

impl Pipeline {
    /// A pipeline with no steps.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a step after the ones already there.
    pub fn step(mut self, step: impl UploadStep) -> Self {
        self.steps.push(Arc::new(step));
        self
    }

    /// Whether there are no steps.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Runs the steps on `file`. `Ok(Some(message))` is a refusal.
    pub(crate) async fn run(
        &self,
        file: &mut Upload,
        ctx: &UploadContext,
    ) -> Result<Option<String>> {
        for step in &self.steps {
            match step.process(file, ctx).await? {
                StepOutcome::Keep => {}
                StepOutcome::Reject(message) => return Ok(Some(message)),
                StepOutcome::Derived(name, derived) => file.derive(&name, derived),
            }
        }
        Ok(None)
    }

    /// Runs the steps on the files of a form: `fields` pairs each form field
    /// with its file's token in `uploads`. Refusals are collected as errors
    /// on the fields; returns whether any file changed.
    pub(crate) async fn run_form(
        &self,
        state: &AppState,
        fields: &[(String, String)],
        uploads: &mut HashMap<String, Upload>,
        errors: &mut Errors,
    ) -> Result<bool> {
        let mut changed = false;
        for (field, token) in fields {
            let Some(file) = uploads.get_mut(token) else {
                continue;
            };
            let field = field_name(field);
            let ctx = UploadContext::new(field.clone(), state);
            let before = file.fingerprint();
            if let Some(message) = self.run(file, &ctx).await? {
                errors.add(&field, message);
            }
            changed |= before != file.fingerprint();
        }
        Ok(changed)
    }
}

impl fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pipeline")
            .field("steps", &self.steps.len())
            .finish()
    }
}

/// The error key of a form field: `items[0][file]` -> `items.0.file`,
/// `photos[]` -> `photos`.
fn field_name(field: &str) -> String {
    let field = field.strip_suffix("[]").unwrap_or(field);
    crate::validation::nested::normalize(field)
}
