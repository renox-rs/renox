use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

/// The result type handlers return; `Error` by default.
pub type Result<T = (), E = Error> = std::result::Result<T, E>;

/// The error type handlers return. Any `anyhow`-compatible error converts into
/// `Error::Internal` with `?`.
///
/// For a status without its own variant (402, 409, 410, …) use
/// `Error::Status`, or [`abort`] / [`abort_if`]:
///
/// ```
/// # use renox::prelude::*;
/// # struct Order { paid: bool }
/// fn download(order: &Order) -> Result<&'static str> {
///     renox::abort_if(!order.paid, StatusCode::PAYMENT_REQUIRED, "Pay for the order first.")?;
///     Ok("the file")
/// }
/// ```
#[non_exhaustive]
pub enum Error {
    /// 400, with a message that is safe to show visitors.
    BadRequest(String),
    /// 401: the request needs a logged-in user.
    Unauthorized,
    /// 403: the user may not do this.
    Forbidden,
    /// 404.
    NotFound,
    /// The CSRF token was missing or wrong, usually because the session expired.
    PageExpired,
    /// A rate limit was hit; see `Routes::throttle`.
    TooManyRequests,
    /// The app is in maintenance mode.
    ServiceUnavailable,
    /// Invalid input; see `ValidationError`.
    Validation(crate::validation::ValidationError),
    /// Any status, with a message that is safe to show visitors (on the
    /// error page, or as `message` in JSON).
    Status(StatusCode, String),
    /// 500 (409 for a unique-constraint violation); details are shown only in debug.
    Internal(anyhow::Error),
}

/// An error with `status` and a message shown to the visitor, for
/// `return Err(abort(…))`. See [`abort_if`] for a condition.
pub fn abort(status: StatusCode, message: impl Into<String>) -> Error {
    Error::Status(status, message.into())
}

/// `Err(abort(status, message))` when `condition` holds, for `?`.
pub fn abort_if(condition: bool, status: StatusCode, message: impl Into<String>) -> Result {
    if condition {
        Err(abort(status, message))
    } else {
        Ok(())
    }
}

/// `Err(abort(status, message))` unless `condition` holds, for `?`.
pub fn abort_unless(condition: bool, status: StatusCode, message: impl Into<String>) -> Result {
    abort_if(!condition, status, message)
}

/// An error that retrying can't fix; see [`Error::permanent`]. It shows as
/// the error it wraps, and stays findable in the chain under more context.
#[derive(Debug)]
struct Permanent(anyhow::Error);

impl std::fmt::Display for Permanent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl std::error::Error for Permanent {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.chain().nth(1)
    }
}

impl Error {
    /// An error that retrying can't fix, e.g. an invalid email address. A job
    /// that returns one goes to `failed_jobs` at once instead of being retried.
    pub fn permanent(err: impl Into<anyhow::Error>) -> Self {
        Self::Internal(anyhow::Error::new(Permanent(err.into())))
    }

    /// [`Error::permanent`] with just a message:
    /// `Err(Error::permanent_message("the card was declined"))`.
    pub fn permanent_message(message: impl std::fmt::Display) -> Self {
        Self::permanent(anyhow::anyhow!("{message}"))
    }

    /// Whether the error was made with [`Error::permanent`].
    pub fn is_permanent(&self) -> bool {
        match self {
            Self::Internal(err) => err.chain().any(|e| e.is::<Permanent>()),
            _ => false,
        }
    }

    /// Whether the error is a database conflict that retrying may clear (see
    /// `DbError::is_retryable`).
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Internal(err) => err.chain().any(|e| {
                e.downcast_ref::<crate::db::DbError>()
                    .is_some_and(crate::db::DbError::is_retryable)
            }),
            _ => false,
        }
    }

    /// Whether the error is a database unique-constraint violation, e.g. a
    /// second sign-up with the same email racing past the `unique` rule.
    pub fn is_unique_violation(&self) -> bool {
        match self {
            Self::Internal(err) => err.chain().any(|e| {
                e.downcast_ref::<crate::db::DbError>()
                    .is_some_and(crate::db::DbError::is_unique_violation)
                    || matches!(
                        e.downcast_ref::<sqlx::Error>(),
                        Some(sqlx::Error::Database(db)) if db.is_unique_violation()
                    )
            }),
            _ => false,
        }
    }

    /// The HTTP status this error responds with.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::PageExpired => StatusCode::from_u16(419).expect("valid status code"),
            Self::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Status(status, _) => *status,
            // A duplicate that got past validation (e.g. two requests at once).
            Self::Internal(_) if self.is_unique_violation() => StatusCode::CONFLICT,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// Readable when `main` returns an error: the message and its causes.
impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Internal(err) => write!(f, "{err:?}"),
            Self::BadRequest(msg) => write!(f, "bad request: {msg}"),
            Self::Status(status, msg) => write!(f, "{}: {msg}", status.as_u16()),
            Self::Validation(err) => write!(f, "validation failed: {:?}", err.errors),
            other => write!(f, "{}", reason(other.status())),
        }
    }
}

/// The message alone, for logs and tests (`{}`): the error's own text
/// without its causes (`{:?}` adds them), the message given to
/// `abort`/`Error::Status`/`Error::BadRequest`, each field's messages for a
/// validation error, else the status's reason ("Not Found").
///
/// ```
/// # use renox::prelude::*;
/// let err = renox::abort(StatusCode::CONFLICT, "The bike is already rented.");
/// assert_eq!(err.to_string(), "The bike is already rented.");
/// assert_eq!(Error::NotFound.to_string(), "Not Found");
/// ```
///
/// `Error` doesn't implement `std::error::Error`: any error converts into it
/// with `?`, and that conversion would then apply to `Error` itself.
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Internal(err) => write!(f, "{err}"),
            Self::BadRequest(msg) | Self::Status(_, msg) => f.write_str(msg),
            Self::Validation(err) => {
                let mut first = true;
                for (field, messages) in err.errors.iter() {
                    for message in messages {
                        if !first {
                            f.write_str("; ")?;
                        }
                        first = false;
                        write!(f, "{field}: {message}")?;
                    }
                }
                if first {
                    f.write_str(reason(self.status()))?;
                }
                Ok(())
            }
            other => f.write_str(reason(other.status())),
        }
    }
}

impl<E: Into<anyhow::Error>> From<E> for Error {
    fn from(err: E) -> Self {
        Self::Internal(err.into())
    }
}

/// Marks an error response so the view middleware can render it with
/// `errors/{status}.html` or the built-in `renox/error.html`.
#[derive(Debug, Clone)]
pub(crate) struct ErrorPage {
    pub status: StatusCode,
    /// Safe to show anyone, e.g. a bad request's message.
    pub detail: Option<String>,
    /// The internal error's chain, shown only when the app's `APP_DEBUG` is on.
    pub debug_detail: Option<String>,
    /// For a template error, where it happened with the lines around it;
    /// shown only with `APP_DEBUG`.
    pub template: Option<String>,
}

impl ErrorPage {
    /// What to show on the page for an app with this debug setting.
    pub fn shown_detail(&self, debug: bool) -> Option<&str> {
        self.detail.as_deref().or(if debug {
            self.debug_detail.as_deref()
        } else {
            None
        })
    }
}

impl ErrorPage {
    /// The error as `{"message": …}` for API clients, with the same status.
    pub fn json(&self, debug: bool) -> Response {
        let message = self
            .shown_detail(debug)
            .unwrap_or_else(|| reason(self.status));
        (
            self.status,
            axum::Json(serde_json::json!({ "message": message })),
        )
            .into_response()
    }
}

/// Whether the client asked for JSON: `Accept: application/json`, or a JSON body.
pub(crate) fn wants_json(headers: &axum::http::HeaderMap) -> bool {
    let header = |name| headers.get(name).and_then(|v| v.to_str().ok());
    header(axum::http::header::ACCEPT).is_some_and(|v| v.contains("application/json"))
        || header(axum::http::header::CONTENT_TYPE)
            .is_some_and(|v| v.starts_with("application/json"))
}

/// The text of a caught panic.
pub(crate) fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "(no message)".into())
}

pub(crate) fn reason(status: StatusCode) -> &'static str {
    match status.as_u16() {
        419 => "Page Expired",
        _ => status.canonical_reason().unwrap_or("Error"),
    }
}

impl From<crate::validation::ValidationError> for Error {
    fn from(err: crate::validation::ValidationError) -> Self {
        Self::Validation(err)
    }
}

impl From<crate::validation::Errors> for Error {
    fn from(errors: crate::validation::Errors) -> Self {
        Self::Validation(errors.into())
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        if let Self::Validation(err) = self {
            return err.into_response();
        }
        let status = self.status();
        let mut template = None;
        let (detail, debug_detail) = match &self {
            Self::Internal(err) => {
                tracing::error!(error = ?err, "internal server error");
                crate::report::request_error(err);
                template = err
                    .chain()
                    .find_map(|e| e.downcast_ref::<minijinja::Error>())
                    .map(|e| e.display_debug_info().to_string())
                    .filter(|info| !info.trim().is_empty());
                (None, Some(format!("{err:?}")))
            }
            Self::BadRequest(msg) => (Some(msg.clone()), None),
            Self::Status(_, msg) if !msg.is_empty() => (Some(msg.clone()), None),
            _ => (None, None),
        };
        // The view middleware renders the page, adding the internal detail
        // only for apps in debug mode; this plain body never includes it.
        let mut res = (status, Html(error_page(status, detail.as_deref()))).into_response();
        res.extensions_mut().insert(ErrorPage {
            status,
            detail,
            debug_detail,
            template,
        });
        res
    }
}

/// Plain page used without the view middleware or when the error template fails.
pub(crate) fn error_page(status: StatusCode, detail: Option<&str>) -> String {
    let code = status.as_u16();
    let reason = reason(status);
    let detail = detail
        .map(|d| format!("<pre>{}</pre>", escape(d)))
        .unwrap_or_default();
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{code} {reason}</title></head>\
         <body><h1>{code} · {reason}</h1>{detail}</body></html>"
    )
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_errors_answer_422_and_other_errors_500() {
        let mut errors = crate::validation::Errors::new();
        errors.add("name", "The name field is required.");
        let err = Error::Validation(crate::validation::ValidationError::new(errors));
        assert_eq!(err.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let plain = Error::from(anyhow::anyhow!("no database involved"));
        assert!(!plain.is_unique_violation());
        assert_eq!(plain.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// A unique violation from sqlx itself (an app querying its pool
    /// directly, then `?`) is a 409 like one from `renox::db`.
    #[tokio::test]
    async fn a_raw_sqlx_unique_violation_is_a_conflict() {
        let db = crate::db::connect(&crate::Config::default()).await.unwrap();
        let Some(pool) = db.sqlite() else {
            return; // the PostgreSQL run: the same check, another driver
        };
        sqlx::raw_sql("CREATE TABLE tags (name TEXT UNIQUE); INSERT INTO tags VALUES ('a')")
            .execute(pool)
            .await
            .unwrap();
        let raw = sqlx::raw_sql("INSERT INTO tags VALUES ('a')")
            .execute(pool)
            .await
            .unwrap_err();
        let err = Error::from(anyhow::Error::new(raw));
        assert!(err.is_unique_violation());
        assert_eq!(err.status(), StatusCode::CONFLICT);
    }
}
