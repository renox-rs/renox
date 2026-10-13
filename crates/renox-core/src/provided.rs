//! Values the app provides with `App::provide`.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::{AppState, Error};

pub(crate) type ProvidedMap = Arc<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>;

impl AppState {
    /// A value given to `App::provide`, if one of type `T` was.
    pub fn provided<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.provided
            .get(&TypeId::of::<T>())
            .cloned()
            .and_then(|value| value.downcast::<T>().ok())
    }
}

/// The app's own shared values (an API client, a price list), available in
/// handlers, jobs, listeners, commands and scheduled tasks.
///
/// ```
/// # use renox::prelude::*;
/// use renox::Provided;
///
/// #[derive(Clone)]
/// struct Payments {
///     api_key: String,
/// }
///
/// async fn checkout(payments: Provided<Payments>) -> String {
///     format!("charging with key {}…", &payments.api_key[..3])
/// }
///
/// async fn in_a_job(state: AppState) {
///     let payments = state.provided::<Payments>().expect("provided at boot");
/// #   let _ = payments;
/// }
///
/// # let _ =
/// App::new().provide(Payments { api_key: "sk_test_123".into() })
/// # ;
/// ```
///
/// A handler asking for a type that wasn't provided answers 500, naming the type.
pub struct Provided<T>(pub Arc<T>);

impl<T> Deref for Provided<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: Send + Sync + 'static> FromRequestParts<AppState> for Provided<T> {
    type Rejection = Error;

    async fn from_request_parts(_: &mut Parts, state: &AppState) -> Result<Self, Error> {
        state.provided::<T>().map(Provided).ok_or_else(|| {
            Error::Internal(anyhow::anyhow!(
                "no {} was provided: add `App::provide(…)`",
                std::any::type_name::<T>()
            ))
        })
    }
}
