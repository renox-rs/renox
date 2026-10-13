//! Events and listeners, for decoupling modules: the order module emits
//! `OrderPlaced`, and the stock and mail modules react to it.
//!
//! ```
//! # use renox::prelude::*;
//! # use serde::{Deserialize, Serialize};
//! # #[derive(Serialize, Deserialize)] struct SendReceipt { order_id: i64 }
//! # impl Job for SendReceipt { const NAME: &'static str = "send-receipt"; async fn handle(self, _: JobContext) -> Result { Ok(()) } }
//! #[derive(Clone)]
//! struct OrderPlaced { order_id: i64 }
//! impl Event for OrderPlaced {}
//!
//! # let _ =
//! App::new().listen(|event: OrderPlaced, state| async move {
//!     state.dispatch(SendReceipt { order_id: event.order_id }).await?; // slow work: queue it
//!     Ok(())
//! })
//! # ;
//!
//! # async fn demo(state: AppState, order_id: i64) -> Result {
//! state.emit(OrderPlaced { order_id }).await?;
//! # Ok(()) }
//! ```

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::{AppState, Error, Result};

/// Something that happened. Listeners get a clone each.
pub trait Event: Clone + Send + Sync + 'static {}

pub(crate) type ListenerFn = Arc<
    dyn Fn(Box<dyn Any + Send>, AppState) -> Pin<Box<dyn Future<Output = Result> + Send>>
        + Send
        + Sync,
>;

pub(crate) type Listeners = Arc<HashMap<TypeId, Vec<ListenerFn>>>;

pub(crate) fn listener<E, F, Fut>(listener: F) -> (TypeId, ListenerFn)
where
    E: Event,
    F: Fn(E, AppState) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result> + Send + 'static,
{
    let run: ListenerFn = Arc::new(move |event, state| match event.downcast::<E>() {
        Ok(event) => Box::pin(listener(*event, state)),
        Err(_) => Box::pin(async { Ok(()) }),
    });
    (TypeId::of::<E>(), run)
}

impl AppState {
    /// Runs every listener of `E` in registration order: those added with
    /// `App::listen` first, then modules' (registered at boot). All of them
    /// run even if one fails; the first error is returned.
    pub async fn emit<E: Event>(&self, event: E) -> Result {
        // `TestApp::fake_events`: record it, run nothing.
        if self.fakes.record_event(event.clone()) {
            return Ok(());
        }
        let Some(listeners) = self.listeners.get(&TypeId::of::<E>()) else {
            return Ok(());
        };
        let mut first_error: Option<Error> = None;
        for listener in listeners {
            // A panicking listener is a failed one; the others still run.
            let run = std::panic::AssertUnwindSafe(listener(Box::new(event.clone()), self.clone()));
            let outcome = futures_util::FutureExt::catch_unwind(run)
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("the listener panicked").into()));
            if let Err(err) = outcome {
                tracing::error!(event = std::any::type_name::<E>(), error = ?err, "listener failed");
                first_error.get_or_insert(err);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct Placed;
    impl Event for Placed {}

    #[derive(Clone)]
    struct Cancelled;
    impl Event for Cancelled {}

    /// Listeners are kept per event type, so one never gets another type;
    /// if it did, it would do nothing rather than fail.
    #[tokio::test]
    async fn a_listener_ignores_an_event_of_another_type() {
        let app = crate::testing::TestApp::new(crate::App::new()).await;
        let (kind, run) = listener(|_: Placed, _state| async {
            Err(Error::Internal(anyhow::anyhow!("only for Placed")))
        });
        assert_eq!(kind, TypeId::of::<Placed>());
        assert!(run(Box::new(Cancelled), app.state().clone()).await.is_ok());
        assert!(run(Box::new(Placed), app.state().clone()).await.is_err());
    }
}
