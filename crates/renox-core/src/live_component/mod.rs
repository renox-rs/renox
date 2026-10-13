//! Live components: server-rendered pieces of a page whose state travels in a
//! signed snapshot and is changed by actions sent back to the server.

pub(crate) mod snapshot;

pub(crate) mod route;

mod context;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::response::Response;

use serde::Serialize;
use serde::de::DeserializeOwned;

pub use context::LiveContext;

use crate::{Error, Result};

/// A component rendered for a page: give it to the template as `component`
/// and include `renox/live.html`.
#[derive(Serialize)]
#[non_exhaustive]
pub struct Mounted {
    /// The component's name.
    pub name: &'static str,
    /// The element id of its wrapper.
    pub id: String,
    /// The template that renders it.
    pub view: &'static str,
    /// The signed snapshot of its state.
    pub snapshot: String,
    /// The state, for the view.
    pub state: serde_json::Value,
    /// The extra values from [`LiveComponent::data`], for the view.
    pub data: serde_json::Value,
    /// True when an action's answer renders it (no scripts, no wrapper setup).
    pub answer: bool,
}

/// Checks that an action got exactly `n` arguments (a 400 otherwise).
///
/// ```
/// # use renox::prelude::*;
/// let args = vec![serde_json::json!(1)];
/// assert!(renox::live_component::arity(&args, 1).is_ok());
/// assert!(renox::live_component::arity(&args, 2).is_err());
/// ```
pub fn arity(args: &[serde_json::Value], n: usize) -> Result {
    if args.len() == n {
        Ok(())
    } else {
        Err(Error::BadRequest(format!(
            "expected {n} argument(s), got {}",
            args.len()
        )))
    }
}

/// Reads argument `i` of an action as a `T` (a 400 when it is missing or
/// has the wrong type).
///
/// ```
/// # use renox::prelude::*;
/// let args = vec![serde_json::json!(7), serde_json::json!("a")];
/// let n: i64 = renox::live_component::arg(&args, 0)?;
/// assert_eq!(n, 7);
/// assert!(renox::live_component::arg::<i64>(&args, 1).is_err());
/// assert!(renox::live_component::arg::<i64>(&args, 2).is_err());
/// # Ok::<(), Error>(())
/// ```
pub fn arg<T: DeserializeOwned>(args: &[serde_json::Value], i: usize) -> Result<T> {
    let value = args
        .get(i)
        .ok_or_else(|| Error::BadRequest(format!("missing argument {i}")))?;
    serde_json::from_value(value.clone())
        .map_err(|e| Error::BadRequest(format!("argument {i}: {e}")))
}

/// Renders `c` into a [`Mounted`].
pub(crate) async fn render<C: LiveComponent>(
    ctx: &LiveContext,
    c: &C,
    id: String,
    answer: bool,
) -> Result<Mounted> {
    let state = serde_json::to_value(c).map_err(|e| Error::Internal(e.into()))?;
    let data = c.data(ctx).await?;
    let snapshot = snapshot::seal(
        ctx.state.key.signing(),
        ctx.state.config.live_snapshot_max,
        C::NAME,
        &id,
        &state,
    )?;
    Ok(Mounted {
        name: C::NAME,
        id,
        view: C::VIEW,
        snapshot,
        state,
        data,
        answer,
    })
}

/// A registered component's entry point: context, action name and the raw
/// request fields in, a response out.
pub(crate) type LiveFn = Arc<
    dyn Fn(
            LiveContext,
            String,
            Vec<(String, String)>,
        ) -> Pin<Box<dyn Future<Output = Result<Response>> + Send>>
        + Send
        + Sync,
>;

/// Registered components by name.
pub(crate) type LiveMap = Arc<HashMap<&'static str, LiveFn>>;

/// The entry point registered for `C`.
pub(crate) fn handler<C: LiveComponent>() -> LiveFn {
    Arc::new(|ctx, action, fields| Box::pin(route::run::<C>(ctx, action, fields)))
}

/// A live component: a serde struct whose fields are the state, and named
/// actions that change it. The state travels in a signed snapshot.
///
/// `call` is a `match` on the action's name; answer unknown names with
/// `Error::NotFound`. Names starting with `_` are reserved for the framework.
///
/// ```
/// # use renox::prelude::*;
/// # use renox::live_component::LiveContext;
/// #[derive(serde::Serialize, serde::Deserialize)]
/// struct Counter {
///     count: i64,
/// }
///
/// impl LiveComponent for Counter {
///     const NAME: &'static str = "counter";
///     const VIEW: &'static str = "live/counter.html";
///
///     async fn call(
///         &mut self,
///         action: &str,
///         _args: Vec<serde_json::Value>,
///         _ctx: &mut LiveContext,
///     ) -> Result {
///         match action {
///             "increment" => {
///                 self.count += 1;
///                 Ok(())
///             }
///             _ => Err(Error::NotFound),
///         }
///     }
/// }
/// ```
pub trait LiveComponent: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// The component's name, used in its route and its snapshot.
    const NAME: &'static str;

    /// The template that renders the component.
    const VIEW: &'static str;

    /// Extra values for the view, next to the state (as `data`). Empty by default.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # use renox::live_component::LiveContext;
    /// # #[derive(serde::Serialize, serde::Deserialize)]
    /// # struct Items { limit: i64 }
    /// # impl LiveComponent for Items {
    /// #     const NAME: &'static str = "items";
    /// #     const VIEW: &'static str = "live/items.html";
    /// async fn data(&self, ctx: &LiveContext) -> Result<serde_json::Value> {
    ///     let _ = ctx.state();
    ///     Ok(json!({ "shown": self.limit }))
    /// }
    /// #     async fn call(&mut self, _: &str, _: Vec<serde_json::Value>, _: &mut LiveContext) -> Result {
    /// #         Ok(())
    /// #     }
    /// # }
    /// ```
    fn data(&self, ctx: &LiveContext) -> impl Future<Output = Result<serde_json::Value>> + Send {
        let _ = ctx;
        async { Ok(serde_json::Value::Object(Default::default())) }
    }

    /// Runs the action `action` with its `args`, changing `self` and using
    /// `ctx` for the request, toasts, redirects and events.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # use renox::live_component::LiveContext;
    /// # #[derive(serde::Serialize, serde::Deserialize)]
    /// # struct Note { text: String }
    /// # impl LiveComponent for Note {
    /// #     const NAME: &'static str = "note";
    /// #     const VIEW: &'static str = "live/note.html";
    /// async fn call(
    ///     &mut self,
    ///     action: &str,
    ///     args: Vec<serde_json::Value>,
    ///     ctx: &mut LiveContext,
    /// ) -> Result {
    ///     match action {
    ///         "clear" => {
    ///             self.text.clear();
    ///             ctx.toast(Toast::success("Cleared"));
    ///             Ok(())
    ///         }
    ///         _ => Err(Error::NotFound),
    ///     }
    /// }
    /// # }
    /// ```
    fn call(
        &mut self,
        action: &str,
        args: Vec<serde_json::Value>,
        ctx: &mut LiveContext,
    ) -> impl Future<Output = Result> + Send;
}
