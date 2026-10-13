//! Values that belong to the current request or job, such as the current
//! team of a multi-tenant app, readable anywhere down the call stack
//! without passing them along (a model's `default_scope`, a listener, a
//! helper).
//!
//! Every request, job, scheduled task and app command runs in a fresh,
//! empty context. Set a value early (e.g. in an `App::layer` middleware
//! after the user is known) and read it later:
//!
//! ```
//! use renox::prelude::*;
//!
//! #[derive(Clone)]
//! struct CurrentTeam(i64);
//!
//! async fn pick_team(user: Option<AuthUser>, req: Request, next: Next) -> Response {
//!     if let Some(team) = user.as_ref().and_then(|u| u.get::<i64>("team_id")) {
//!         renox::context::set(CurrentTeam(team));
//!     }
//!     next.run(req).await
//! }
//!
//! # async fn demo() {
//! // In a job or a command, the value comes from the payload instead:
//! renox::context::set(CurrentTeam(7));
//! let team = renox::context::get::<CurrentTeam>().map(|t| t.0); // Some(7)
//! # assert_eq!(team, Some(7));
//! # }
//! # use renox::axum::{extract::Request, middleware::Next};
//! # let _ = pick_team;
//! ```
//!
//! A context lives in its task: `tokio::spawn` starts without one (wrap the
//! future in [`scope`] to give it one).

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;

type Values = HashMap<TypeId, Box<dyn Any + Send + Sync>>;

tokio::task_local! {
    static CONTEXT: RefCell<Values>;
}

/// Stores `value` in the current context, replacing one of the same type.
/// Outside a context (a bare `tokio::spawn`), it does nothing and returns
/// false.
pub fn set<T: Clone + Send + Sync + 'static>(value: T) -> bool {
    CONTEXT
        .try_with(|values| {
            values
                .borrow_mut()
                .insert(TypeId::of::<T>(), Box::new(value));
        })
        .is_ok()
}

/// The value of type `T` in the current context, if one was set.
pub fn get<T: Clone + Send + Sync + 'static>() -> Option<T> {
    CONTEXT
        .try_with(|values| {
            values
                .borrow()
                .get(&TypeId::of::<T>())
                .and_then(|value| value.downcast_ref::<T>())
                .cloned()
        })
        .ok()
        .flatten()
}

/// Removes the value of type `T` from the current context.
pub fn remove<T: Send + Sync + 'static>() {
    let _ = CONTEXT.try_with(|values| values.borrow_mut().remove(&TypeId::of::<T>()));
}

/// Runs `fut` in a fresh, empty context.
pub async fn scope<F: Future>(fut: F) -> F::Output {
    CONTEXT.scope(RefCell::new(Values::new()), fut).await
}

/// A value from the current context as a handler argument, e.g. the team a
/// middleware picked: `Current(team): Current<CurrentTeam>`. A request
/// without one is a 500 (a middleware should have set it); take
/// `Option<Current<T>>` when it's optional.
///
/// ```
/// # use renox::prelude::*;
/// use renox::context::Current;
///
/// #[derive(Clone)]
/// struct CurrentTeam(i64);
///
/// async fn dashboard(Current(team): Current<CurrentTeam>) -> String {
///     format!("team {}", team.0)
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Current<T>(pub T);

impl<T, S> axum::extract::FromRequestParts<S> for Current<T>
where
    T: Clone + Send + Sync + 'static,
    S: Send + Sync,
{
    type Rejection = crate::Error;

    async fn from_request_parts(
        _: &mut axum::http::request::Parts,
        _: &S,
    ) -> Result<Self, crate::Error> {
        get::<T>().map(Current).ok_or_else(|| {
            anyhow::anyhow!(
                "no `{}` in the request's context: set it in a middleware with renox::context::set",
                std::any::type_name::<T>()
            )
            .into()
        })
    }
}

impl<T, S> axum::extract::OptionalFromRequestParts<S> for Current<T>
where
    T: Clone + Send + Sync + 'static,
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        _: &mut axum::http::request::Parts,
        _: &S,
    ) -> Result<Option<Self>, std::convert::Infallible> {
        Ok(get::<T>().map(Current))
    }
}

/// The app of the current request, job, task or command, e.g. for a model
/// hook that emits an event or forgets a cache key. `None` outside one
/// (a bare `tokio::spawn`, or a test calling models directly).
pub fn app() -> Option<crate::AppState> {
    get::<crate::AppState>()
}

/// Runs `fut` in a fresh context that knows the app.
pub(crate) async fn scope_app<F: Future>(state: crate::AppState, fut: F) -> F::Output {
    scope(async move {
        set(state);
        fut.await
    })
    .await
}

/// Runs each request in its own context.
pub(crate) async fn middleware(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let info = RequestInfo {
        method: req.method().to_string(),
        path: req.uri().path().to_owned(),
        id: req
            .extensions()
            .get::<crate::RequestId>()
            .map(|id| id.0.clone())
            .unwrap_or_default(),
        ip: crate::ClientIp::of(&req).map(|ip| ip.to_string()),
    };
    scope_app(state, async move {
        set(info);
        next.run(req).await
    })
    .await
}

/// What error reports say about the request they come from.
#[derive(Debug, Clone)]
pub(crate) struct RequestInfo {
    pub method: String,
    pub path: String,
    pub id: String,
    pub ip: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct Team(i64);

    #[tokio::test]
    async fn values_live_in_their_scope() {
        assert!(!set(Team(1)), "no context outside a scope");
        assert_eq!(get::<Team>(), None);
        scope(async {
            assert!(set(Team(1)));
            set(Team(2));
            assert_eq!(get::<Team>(), Some(Team(2)));
            assert_eq!(get::<String>(), None);
            scope(async { assert_eq!(get::<Team>(), None, "a new scope starts empty") }).await;
            remove::<Team>();
            assert_eq!(get::<Team>(), None);
        })
        .await;
    }
}
