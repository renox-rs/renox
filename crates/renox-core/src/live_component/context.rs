//! What an action can reach: the request, and what it sends back.

use axum::extract::{FromRequestParts, OptionalFromRequestParts};
use axum::http::request::Parts;
use serde::Serialize;

use crate::validation::{ValidationError, Validator};
use crate::{AppState, AuthUser, Error, Result, Session, Toast, Validate};

/// The request behind a live component's action, and the effects it asks for
/// (a toast, a redirect, events).
///
/// ```
/// # use renox::prelude::*;
/// # use renox::live_component::LiveContext;
/// # fn demo(ctx: &mut LiveContext) -> Result {
/// ctx.toast(Toast::success("Saved"));
/// ctx.dispatch("saved", json!({ "id": 3 }));
/// ctx.redirect("/done");
/// # Ok(()) }
/// ```
pub struct LiveContext {
    pub(crate) state: AppState,
    session: Option<Session>,
    user: Option<AuthUser>,
    pub(crate) component: &'static str,
    pub(crate) toast: Option<Toast>,
    pub(crate) redirect: Option<String>,
    pub(crate) events: Vec<(String, serde_json::Value)>,
}

impl LiveContext {
    /// The application state.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # use renox::live_component::LiveContext;
    /// # fn demo(ctx: &LiveContext) { let _db = &ctx.state().db; }
    /// ```
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// Renders `component` for a page: its state, its data and a signed
    /// snapshot. Pass the result to the page's template and
    /// `{% include "renox/live.html" %}` it as `component`.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # use renox::live_component::LiveContext;
    /// # #[derive(serde::Serialize, serde::Deserialize)]
    /// # struct Counter { count: i64 }
    /// # impl LiveComponent for Counter {
    /// #     const NAME: &'static str = "counter";
    /// #     const VIEW: &'static str = "live/counter.html";
    /// #     async fn call(&mut self, _: &str, _: Vec<serde_json::Value>, _: &mut LiveContext) -> Result { Ok(()) }
    /// # }
    /// # async fn demo(ctx: LiveContext) -> Result {
    /// let mounted = ctx.mount(Counter { count: 0 }).await?;
    /// # let _ = mounted; Ok(()) }
    /// ```
    pub async fn mount<C: super::LiveComponent>(&self, c: C) -> Result<super::Mounted> {
        let id = format!("rx-live-{}", &crate::random_token()[..12]);
        super::render(self, &c, id, false).await
    }

    /// The request's session, when there is one.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # use renox::live_component::LiveContext;
    /// # fn demo(ctx: &LiveContext) { let _ = ctx.session(); }
    /// ```
    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    /// The signed-in user; `Error::Unauthorized` when nobody is.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # use renox::live_component::LiveContext;
    /// # fn demo(ctx: &LiveContext) -> Result { let _user = ctx.user()?; Ok(()) }
    /// ```
    pub fn user(&self) -> Result<&AuthUser> {
        self.user.as_ref().ok_or(Error::Unauthorized)
    }

    /// Sends a toast with the answer.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # use renox::live_component::LiveContext;
    /// # fn demo(ctx: &mut LiveContext) { ctx.toast(Toast::success("Saved")); }
    /// ```
    pub fn toast(&mut self, toast: Toast) {
        self.toast = Some(toast);
    }

    /// Sends the browser to `url` (an `HX-Redirect`).
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # use renox::live_component::LiveContext;
    /// # fn demo(ctx: &mut LiveContext) { ctx.redirect("/orders"); }
    /// ```
    pub fn redirect(&mut self, url: impl Into<String>) {
        self.redirect = Some(url.into());
    }

    /// Fires the event `rx:<component>:<name>` on the component, with `detail`.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # use renox::live_component::LiveContext;
    /// # fn demo(ctx: &mut LiveContext) { ctx.dispatch("saved", json!({ "id": 1 })); }
    /// ```
    pub fn dispatch(&mut self, name: &str, detail: serde_json::Value) {
        self.events
            .push((format!("rx:{}:{name}", self.component), detail));
    }

    /// Checks `value` against its rules; a failure is a 422 with the field
    /// errors (and the input, for the form to show again).
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # use renox::live_component::LiveContext;
    /// # #[derive(serde::Serialize)]
    /// # struct Form { name: String }
    /// # impl Validate for Form {
    /// #     fn rules(&self, v: &mut Validator) { v.field("name", &self.name).required(); }
    /// # }
    /// # async fn demo(ctx: &LiveContext, form: &Form) -> Result {
    /// ctx.validate(form).await?;
    /// # Ok(()) }
    /// ```
    pub async fn validate(&self, value: &(impl Validate + Serialize)) -> Result {
        let errors = Validator::rules_of(value)
            .finish_for(&self.state, self.user.as_ref().map(|u| u.user()))
            .await?;
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ValidationError::new(errors).with_input(value).into())
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for LiveContext {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self> {
        let app = parts
            .extensions
            .get::<AppState>()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("the auth middleware is not installed"))?;
        let session = parts.extensions.get::<Session>().cloned();
        let user = match <AuthUser as OptionalFromRequestParts<S>>::from_request_parts(parts, state)
            .await
        {
            Ok(user) => user,
            Err(never) => match never {},
        };
        Ok(Self {
            state: app,
            session,
            user,
            component: "",
            toast: None,
            redirect: None,
            events: Vec::new(),
        })
    }
}
