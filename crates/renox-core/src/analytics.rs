//! Analytics events, with Google Analytics 4 and Tag Manager configured in
//! `.env` (see `AnalyticsConfig`).
//!
//! From a handler, whatever the response is:
//!
//! ```
//! # use renox::prelude::*;
//! # fn demo(session: &Session) -> Result {
//! renox::analytics::event(&session, "sign_up", json!({ "method": "email" }))?;
//! # Ok(()) }
//! ```
//!
//! The event reaches the browser with this response when it's an htmx swap
//! (an `HX-Trigger`) or a page, or with the next page after a redirect;
//! renox.js passes it to `gtag('event', …)` and, with Tag Manager, pushes it
//! to the `dataLayer`. Page views need nothing: GA4 counts the history changes
//! `hx-boost` makes (enhanced measurement), and GTM has a History Change trigger.
//!
//! Events that must not be lost to ad blockers (a purchase) can go from the
//! server instead, through the Measurement Protocol:
//!
//! ```
//! # use renox::prelude::*;
//! use renox::analytics::{GaClientId, ServerEvent};
//!
//! async fn paid(State(state): State<AppState>, GaClientId(client): GaClientId) -> Result<Redirect> {
//!     state.dispatch(ServerEvent::new(client, "purchase").param("value", 49.99).param("currency", "USD")).await?;
//!     Ok(Redirect::to("/thanks"))
//! }
//! ```

use std::convert::Infallible;

use axum::extract::FromRequestParts;
use axum::http::Response;
use axum::http::header::{COOKIE, LOCATION};
use axum::http::request::Parts;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Result, Session};

const SESSION_KEY: &str = "_renox_analytics";
const TRIGGER: &str = "renox:analytics";

/// One analytics event: a name such as `sign_up` and its parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Event {
    /// The event name, e.g. `sign_up`.
    pub name: String,
    /// The event's parameters, a JSON object (empty when missing).
    #[serde(default)]
    pub params: Value,
}

/// Queues an event for the visitor's browser (see the module docs).
pub fn event(session: &Session, name: &str, params: impl Serialize) -> Result {
    let mut events: Vec<Event> = session.get(SESSION_KEY).unwrap_or_default();
    events.push(Event {
        name: name.to_owned(),
        params: serde_json::to_value(params)?,
    });
    session.put(SESSION_KEY, events)
}

/// Takes the events waiting for the browser.
pub(crate) fn take(session: &Session) -> Vec<Event> {
    session.pull(SESSION_KEY).unwrap_or_default()
}

pub(crate) fn has_pending(session: &Session) -> bool {
    session.has(SESSION_KEY)
}

/// The `<meta>` renox.js reads on page load.
pub(crate) fn events_meta(events: &[Event]) -> String {
    if events.is_empty() {
        return String::new();
    }
    let json = serde_json::to_string(events).unwrap_or_default();
    format!(
        "<meta name=\"renox-analytics\" content=\"{}\">\n",
        crate::seo::escape(&json)
    )
}

/// Whether an htmx response can carry the events: a swap, not a redirect.
pub(crate) fn deliverable_by_htmx<B>(res: &Response<B>) -> bool {
    res.status().is_success()
        && !res.headers().contains_key("hx-redirect")
        && !res.headers().contains_key("hx-location")
        && !res.headers().contains_key("hx-refresh")
        && !res.headers().contains_key(LOCATION)
}

/// Adds the events to the response's `HX-Trigger`, keeping any it has.
pub(crate) fn add_trigger<B>(res: &mut Response<B>, events: Vec<Event>) {
    crate::htmx::add_trigger(res, TRIGGER, json!({ "events": events }));
}

/// The GA4 client id from the `_ga` cookie (`GA1.1.123.456` → `123.456`),
/// for server-side events. `None` when the visitor has none (no consent, an
/// ad blocker, or analytics off).
#[derive(Debug, Clone)]
pub struct GaClientId(pub Option<String>);

impl<S: Send + Sync> FromRequestParts<S> for GaClientId {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Infallible> {
        let id = parts
            .headers
            .get_all(COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .filter_map(|pair| pair.trim().strip_prefix("_ga="))
            .find_map(client_id_from_cookie);
        Ok(Self(id))
    }
}

fn client_id_from_cookie(value: &str) -> Option<String> {
    let parts: Vec<&str> = value.split('.').collect();
    (parts.len() >= 4 && parts[0].starts_with("GA"))
        .then(|| format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1]))
}

#[cfg(feature = "server-events")]
pub use server_event::ServerEvent;

/// `ServerEvent` needs an HTTP client, so it comes with the
/// `server-events` feature (on by default).
#[cfg(feature = "server-events")]
mod server_event {

    use anyhow::anyhow;

    use super::*;
    use crate::config::Environment;
    use crate::queue::{Job, JobContext};
    use serde_json::Map;

    /// An event sent from the server to GA4 (Measurement Protocol), as a queue
    /// job so a slow or failing request never holds up the page. Needs
    /// `GA4_MEASUREMENT_ID` and `GA4_API_SECRET`; outside production it's only logged.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ServerEvent {
        client_id: String,
        user_id: Option<String>,
        name: String,
        params: Map<String, Value>,
    }

    impl ServerEvent {
        /// `client_id` from [`GaClientId`]; without one a random id is used, so
        /// the event still counts but isn't linked to the visitor's session.
        pub fn new(client_id: Option<String>, name: &str) -> Self {
            let client_id = client_id.unwrap_or_else(|| {
                let [a, b] = [rand::random::<u32>(), rand::random::<u32>()];
                format!("{a}.{b}")
            });
            Self {
                client_id,
                user_id: None,
                name: name.to_owned(),
                params: Map::new(),
            }
        }

        /// Adds a parameter; a value that can't be serialized becomes `null`.
        pub fn param(mut self, key: &str, value: impl Serialize) -> Self {
            self.params.insert(
                key.to_owned(),
                serde_json::to_value(value).unwrap_or(Value::Null),
            );
            self
        }

        /// Links the event to your own user id (GA4 User-ID).
        pub fn user_id(mut self, id: impl ToString) -> Self {
            self.user_id = Some(id.to_string());
            self
        }

        /// The JSON body sent to `/mp/collect`.
        pub fn payload(&self) -> Value {
            let mut body = json!({
                "client_id": self.client_id,
                "events": [{ "name": self.name, "params": self.params }],
            });
            if let Some(user_id) = &self.user_id {
                body["user_id"] = json!(user_id);
            }
            body
        }
    }

    impl Job for ServerEvent {
        const NAME: &'static str = "renox:analytics";

        async fn handle(self, ctx: JobContext) -> Result {
            let config = &ctx.state.config;
            let analytics = &config.analytics;
            let (Some(id), Some(secret)) =
                (&analytics.ga4_measurement_id, &analytics.ga4_api_secret)
            else {
                tracing::debug!(event = %self.name, "analytics event not sent: GA4_MEASUREMENT_ID or GA4_API_SECRET is not set");
                return Ok(());
            };
            if config.env != Environment::Production {
                tracing::info!(event = %self.name, payload = %self.payload(), "analytics event (not sent outside production)");
                return Ok(());
            }
            let res = ctx
                .state
                .http
                .post("https://www.google-analytics.com/mp/collect")
                .query(&[("measurement_id", id), ("api_secret", secret)])
                .json(&self.payload())
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await?;
            if !res.ok() {
                return Err(anyhow!("GA4 answered {}", res.status()).into());
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn reads_the_client_id_from_the_ga_cookie() {
        assert_eq!(
            client_id_from_cookie("GA1.1.1234567890.1700000000").as_deref(),
            Some("1234567890.1700000000")
        );
        assert_eq!(client_id_from_cookie("junk"), None);
    }

    #[test]
    #[cfg(feature = "server-events")]
    fn builds_measurement_protocol_payloads() {
        let event = ServerEvent::new(Some("1.2".into()), "purchase")
            .param("value", 49.99)
            .param("currency", "USD")
            .user_id(42);
        assert_eq!(
            event.payload(),
            json!({
                "client_id": "1.2",
                "user_id": "42",
                "events": [{ "name": "purchase", "params": { "value": 49.99, "currency": "USD" } }],
            })
        );
        assert!(
            ServerEvent::new(None, "x").payload()["client_id"]
                .as_str()
                .unwrap()
                .contains('.')
        );
    }

    #[test]
    fn merges_with_existing_triggers() {
        let events = vec![Event {
            name: "sign_up".into(),
            params: json!({"method": "email"}),
        }];
        let mut res = Response::new(());
        res.headers_mut()
            .insert("hx-trigger", HeaderValue::from_static("saved, closed"));
        add_trigger(&mut res, events);
        let header: Value =
            serde_json::from_str(res.headers()["hx-trigger"].to_str().unwrap()).unwrap();
        assert_eq!(header["saved"], Value::Null);
        assert_eq!(header["closed"], Value::Null);
        assert_eq!(header["renox:analytics"]["events"][0]["name"], "sign_up");
    }
}
