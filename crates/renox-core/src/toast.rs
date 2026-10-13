//! Toasts: short messages that confirm what just happened ("Saved"),
//! shown over the page and announced to screen readers.
//!
//! ```
//! # use renox::prelude::*;
//! use renox::ToastAction;
//!
//! async fn save() -> (Toast, Redirect) {
//!     // … save …
//!     (Toast::success("Product saved"), Redirect::to("/products"))
//! }
//!
//! async fn place() -> (Toast, Redirect) {
//!     let toast = Toast::success("Order #7 placed")
//!         .body("We'll email you when it ships.")
//!         .link("View order", "/orders/7")
//!         .action(ToastAction::event("Undo", "order-undo")) // a DOM event on `document`
//!         .seconds(8); // or .persistent(): stays until dismissed
//!     (toast, Redirect::to("/orders"))
//! }
//!
//! async fn archive() -> Toast {
//!     // … archive …
//!     // A button that sends `DELETE /orders/7/archive` (htmx, with the CSRF
//!     // token); that handler answers with a toast of its own, `HxRefresh`, …
//!     Toast::info("Order #7 archived").action(ToastAction::delete("Undo", "/orders/7/archive"))
//! }
//! ```
//!
//! Errors stay until dismissed; the others leave after a while (longer for
//! longer text), unless `seconds`/`persistent` say otherwise. In the
//! browser, `Renox.toast({kind: "info", message: "…"})` shows one and
//! `Renox.dismissToast(id)` closes the toast given that `id`.
//!
//! After a redirect the toast waits in the session for the next page; an
//! htmx request gets it at once (in `HX-Trigger`), unless it redirects or
//! refreshes (`HxRedirect`, `HxRefresh`), which also wait for the next page.
//! Put `{{ toasts() }}` in the layout, once, and `{{ renox_ui() }}` in its head.
//!
//! A request action ([`ToastAction::post`], `put`, `patch`, `delete`) is
//! sent as an htmx request that swaps nothing: answer it with a [`Toast`]
//! (a `204` carrying it), `HxTrigger`, `HxRedirect` or `HxRefresh`. If it
//! fails without a toast of its own, the page shows an error toast.

use axum::response::{IntoResponse, IntoResponseParts, Response, ResponseParts};
use serde::{Deserialize, Serialize};

/// Session key of the toasts waiting for the next page.
pub(crate) const SESSION_KEY: &str = "_toasts";
/// The htmx event that carries toasts.
pub(crate) const EVENT: &str = "renox:toast";

/// What a toast says about the outcome: its color, icon and whether it
/// stays until dismissed (errors do: they need reading).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ToastKind {
    /// Something worked.
    Success,
    /// Neutral information.
    Info,
    /// Something needs attention.
    Warning,
    /// Something failed.
    Error,
}

/// A toast; return it with the response. See the [module docs](self).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Toast {
    /// Its kind, which sets its colour and icon.
    pub kind: ToastKind,
    /// The text shown (the title, when there is a body).
    pub message: String,
    /// A second, lighter line under the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Links or buttons under the text; each closes the toast.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ToastAction>,
    /// How long it stays, in milliseconds; `Some(0)` until dismissed, `None`
    /// for the default (errors stay, others leave after 4–10 s).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<u64>,
    /// A name for the toast, so the page's script can close it
    /// (`Renox.dismissToast(id)`); a new toast with the same id replaces it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// A link or button in a toast (or a database notification): see
/// [`Toast::action`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToastAction {
    /// The button's text.
    pub label: String,
    /// Where the link goes (http(s), mailto, tel or a relative URL; others
    /// are dropped).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// A DOM event dispatched on `document` instead of a link, e.g. for
    /// `hx-trigger="order-undo from:document"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    /// Sends a request with this method (`POST`, `PUT`, `PATCH` or
    /// `DELETE`) to `url` instead of following it, with the CSRF token.
    /// Only this site's own paths (`/…`) are sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Opens the link in a new tab.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub new_tab: bool,
}

impl ToastAction {
    /// A link to `url`.
    pub fn link(label: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            url: Some(url.into()),
            event: None,
            method: None,
            new_tab: false,
        }
    }

    /// A button sending `POST url` (see [`ToastAction::method`](ToastAction#structfield.method)).
    pub fn post(label: impl Into<String>, url: impl Into<String>) -> Self {
        Self::request("POST", label, url)
    }

    /// A button sending `PUT url`.
    pub fn put(label: impl Into<String>, url: impl Into<String>) -> Self {
        Self::request("PUT", label, url)
    }

    /// A button sending `PATCH url`.
    pub fn patch(label: impl Into<String>, url: impl Into<String>) -> Self {
        Self::request("PATCH", label, url)
    }

    /// A button sending `DELETE url`.
    pub fn delete(label: impl Into<String>, url: impl Into<String>) -> Self {
        Self::request("DELETE", label, url)
    }

    fn request(method: &str, label: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            method: Some(method.to_owned()),
            ..Self::link(label, url)
        }
    }

    /// Whether it can be shown: a link to a harmless URL, a request to one
    /// of this site's paths, or an event.
    pub(crate) fn is_safe(&self) -> bool {
        match (&self.method, self.url.as_deref()) {
            (Some(method), Some(url)) => request_method(method).is_some() && local_url(url),
            (Some(_), None) => false,
            (None, Some(url)) => safe_url(url),
            (None, None) => self.event.is_some(),
        }
    }

    /// A button dispatching the DOM event `name` on `document` (its
    /// `detail.toast` is the toast's id).
    pub fn event(label: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            url: None,
            event: Some(name.into()),
            method: None,
            new_tab: false,
        }
    }

    /// Opens the link in a new tab.
    pub fn new_tab(mut self) -> Self {
        self.new_tab = true;
        self
    }
}

impl Toast {
    /// A toast of `kind`.
    pub fn new(kind: ToastKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            body: None,
            actions: Vec::new(),
            duration: None,
            id: None,
        }
    }

    /// Adds a second line under the message.
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Adds a link or button (see [`ToastAction`]).
    pub fn action(mut self, action: ToastAction) -> Self {
        self.actions.push(action);
        self
    }

    /// Adds a link: `.link("View order", "/orders/7")`.
    pub fn link(self, label: impl Into<String>, url: impl Into<String>) -> Self {
        self.action(ToastAction::link(label, url))
    }

    /// Stays until dismissed (errors already do).
    pub fn persistent(mut self) -> Self {
        self.duration = Some(0);
        self
    }

    /// Leaves after `seconds` (also for an error). It still waits while
    /// hovered or focused.
    pub fn seconds(mut self, seconds: u64) -> Self {
        self.duration = Some(seconds.saturating_mul(1000).max(1));
        self
    }

    /// Names the toast (see [`Toast::id`](Toast#structfield.id)).
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// A success toast.
    pub fn success(message: impl Into<String>) -> Self {
        Self::new(ToastKind::Success, message)
    }

    /// An info toast.
    pub fn info(message: impl Into<String>) -> Self {
        Self::new(ToastKind::Info, message)
    }

    /// A warning toast.
    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(ToastKind::Warning, message)
    }

    /// Stays until dismissed.
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(ToastKind::Error, message)
    }
}

/// Toasts attached to a response, delivered by the view middleware.
#[derive(Debug, Clone, Default)]
pub(crate) struct PendingToasts(pub Vec<Toast>);

impl IntoResponseParts for Toast {
    type Error = std::convert::Infallible;

    fn into_response_parts(self, mut res: ResponseParts) -> Result<ResponseParts, Self::Error> {
        let pending = res.extensions_mut().get_mut::<PendingToasts>();
        match pending {
            Some(pending) => pending.0.push(self),
            None => {
                res.extensions_mut().insert(PendingToasts(vec![self]));
            }
        }
        Ok(res)
    }
}

impl IntoResponse for Toast {
    fn into_response(self) -> Response {
        (self, axum::http::StatusCode::NO_CONTENT).into_response()
    }
}

/// Where toasts appear: `top` (the default, centred), `top-start`,
/// `top-end`, `bottom`, `bottom-start` or `bottom-end`.
pub(crate) const POSITIONS: &[&str] = &[
    "top",
    "top-start",
    "top-end",
    "bottom",
    "bottom-start",
    "bottom-end",
];

/// The toast region's texts, in the page's language.
pub(crate) struct RegionTexts {
    /// The close button's label.
    pub(crate) dismiss: String,
    /// The error toast of a request action that failed.
    pub(crate) failed: String,
}

/// The toast region's markup, with the toasts waiting in `waiting`. The
/// bundled renox.js adds the ones htmx responses bring, and dismisses.
pub(crate) fn region(waiting: &[Toast], texts: &RegionTexts, position: &str) -> String {
    let position = if POSITIONS.contains(&position) {
        position
    } else {
        "top"
    };
    let dismiss = texts.dismiss.as_str();
    let mut out = format!(
        r#"<div class="rx-toasts rx-toasts--{position}" data-renox-toasts data-dismiss-label="{}" data-failed-label="{}" aria-live="polite" aria-relevant="additions">"#,
        escape(dismiss),
        escape(&texts.failed)
    );
    for toast in waiting {
        out.push_str(&toast_html(toast, dismiss));
    }
    out.push_str("</div>");
    out
}

pub(crate) fn toast_html(toast: &Toast, dismiss: &str) -> String {
    let (kind, role, icon) = match toast.kind {
        ToastKind::Success => ("success", "status", ICON_CHECK),
        ToastKind::Info => ("info", "status", ICON_INFO),
        ToastKind::Warning => ("warning", "status", ICON_WARNING),
        ToastKind::Error => ("error", "alert", ICON_ERROR),
    };
    let sticky = match toast.duration {
        Some(0) => " data-sticky".to_owned(),
        Some(ms) => format!(" data-duration=\"{ms}\""),
        None if toast.kind == ToastKind::Error => " data-sticky".to_owned(),
        None => String::new(),
    };
    let id = toast
        .id
        .as_deref()
        .map(|id| format!(" data-toast-id=\"{}\"", escape(id)))
        .unwrap_or_default();
    let body = toast
        .body
        .as_deref()
        .map(|body| format!(r#"<p class="rx-toast__body">{}</p>"#, escape(body)))
        .unwrap_or_default();
    let mut actions = String::new();
    for action in &toast.actions {
        actions.push_str(&action_html(action));
    }
    if !actions.is_empty() {
        actions = format!(r#"<div class="rx-toast__actions">{actions}</div>"#);
    }
    format!(
        r#"<div class="rx-toast rx-toast--{kind}" role="{role}" data-renox-toast{sticky}{id}><span class="rx-toast__icon" aria-hidden="true">{icon}</span><div class="rx-toast__content"><p class="rx-toast__message">{message}</p>{body}{actions}</div><button type="button" class="rx-toast__close" data-renox-dismiss aria-label="{dismiss}">{ICON_CLOSE}</button></div>"#,
        message = escape(&toast.message),
        dismiss = escape(dismiss),
    )
}

fn action_html(action: &ToastAction) -> String {
    let label = escape(&action.label);
    if !action.is_safe() {
        return String::new();
    }
    if let (Some(method), Some(url)) = (action.method.as_deref(), action.url.as_deref()) {
        let method = request_method(method).unwrap_or("POST");
        format!(
            r#"<button type="button" class="rx-toast__action" data-rx-request="{}" data-rx-method="{method}" data-renox-dismiss>{label}</button>"#,
            escape(url)
        )
    } else if let Some(url) = action.url.as_deref() {
        let target = if action.new_tab {
            r#" target="_blank" rel="noopener""#
        } else {
            ""
        };
        format!(
            r#"<a class="rx-toast__action" href="{}"{target} data-renox-dismiss>{label}</a>"#,
            escape(url)
        )
    } else if let Some(event) = &action.event {
        format!(
            r#"<button type="button" class="rx-toast__action" data-rx-toast-event="{}" data-renox-dismiss>{label}</button>"#,
            escape(event)
        )
    } else {
        String::new()
    }
}

/// The method of a request action, upper-cased, if it is one renox-ui.js
/// sends (`GET` is a link).
pub(crate) fn request_method(method: &str) -> Option<&'static str> {
    ["POST", "PUT", "PATCH", "DELETE"]
        .into_iter()
        .find(|known| known.eq_ignore_ascii_case(method))
}

/// Whether `url` is a path on this site (`/orders/7`, not `//other.site`):
/// where a request carrying the CSRF token may go.
pub(crate) fn local_url(url: &str) -> bool {
    let cleaned: String = url
        .chars()
        .filter(|c| !c.is_ascii_control() && !c.is_whitespace())
        .collect();
    cleaned.starts_with('/') && !cleaned.starts_with("//") && !cleaned.starts_with("/\\")
}

/// Whether `url` is http(s), mailto, tel or relative: what a link built
/// from data may point at (not `javascript:`).
pub(crate) fn safe_url(url: &str) -> bool {
    // Browsers ignore control characters and spaces inside a scheme.
    let cleaned: String = url
        .chars()
        .filter(|c| !c.is_ascii_control() && !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    let before_path = cleaned.find(['/', '?', '#']).unwrap_or(usize::MAX);
    match cleaned.find(':') {
        Some(end) if end < before_path => {
            matches!(&cleaned[..end], "http" | "https" | "mailto" | "tel")
        }
        _ => true,
    }
}

pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
    out
}

const ICON_CHECK: &str = r##"<svg viewBox="0 0 20 20" width="20" height="20"><circle cx="10" cy="10" r="9" fill="currentColor"/><path d="M6 10.5l2.5 2.5L14 7.5" stroke="#fff" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;
const ICON_INFO: &str = r##"<svg viewBox="0 0 20 20" width="20" height="20"><circle cx="10" cy="10" r="9" fill="currentColor"/><path d="M10 9v5M10 6h.01" stroke="#fff" stroke-width="2" stroke-linecap="round"/></svg>"##;
const ICON_WARNING: &str = r##"<svg viewBox="0 0 20 20" width="20" height="20"><path d="M10 2l8.5 15h-17z" fill="currentColor" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/><path d="M10 8v4M10 14.5h.01" stroke="#fff" stroke-width="2" stroke-linecap="round"/></svg>"##;
const ICON_ERROR: &str = r##"<svg viewBox="0 0 20 20" width="20" height="20"><circle cx="10" cy="10" r="9" fill="currentColor"/><path d="M7 7l6 6M13 7l-6 6" stroke="#fff" stroke-width="2" stroke-linecap="round"/></svg>"##;
const ICON_CLOSE: &str = r##"<svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true"><path d="M5 5l10 10M15 5L5 15" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toasts_carry_a_body_actions_and_a_duration() {
        let toast = Toast::success("Order <7> placed")
            .body("Thanks & bye")
            .link("View", "/orders/7")
            .action(ToastAction::link("Docs", "https://renox.dev").new_tab())
            .action(ToastAction::link("Bad", "javascript:alert(1)"))
            .action(ToastAction::event("Undo", "order-undo"))
            .seconds(8)
            .id("order-7");
        let html = toast_html(&toast, "Dismiss");
        assert!(
            html.contains(r#"<p class="rx-toast__message">Order &lt;7&gt; placed</p>"#),
            "{html}"
        );
        assert!(html.contains(r#"<p class="rx-toast__body">Thanks &amp; bye</p>"#));
        assert!(html.contains(r#"href="/orders/7" data-renox-dismiss>View</a>"#));
        assert!(html.contains(r#"target="_blank" rel="noopener""#));
        assert!(!html.contains("javascript") && !html.contains(">Bad<"));
        assert!(html.contains(r#"data-rx-toast-event="order-undo""#));
        // An action read back from the session with a method but no URL,
        // or with nothing to do, is never shown.
        let mut nowhere = ToastAction::link("Send", "/x");
        nowhere.url = None;
        nowhere.method = Some("POST".into());
        assert!(!nowhere.is_safe());
        let mut nothing = ToastAction::event("Nothing", "x");
        nothing.event = None;
        assert!(!nothing.is_safe());
        assert_eq!(action_html(&nothing), "");
        assert!(
            html.contains(r#"data-duration="8000""#) && html.contains(r#"data-toast-id="order-7""#)
        );
        let error = toast_html(&Toast::error("No"), "x");
        assert!(error.contains("data-sticky") && !error.contains("rx-toast__actions"));
        assert!(toast_html(&Toast::info("x").persistent(), "x").contains("data-sticky"));
        assert!(toast_html(&Toast::error("x").seconds(3), "x").contains(r#"data-duration="3000""#));
        // Toasts saved in a session before these fields existed still read.
        let old: Toast = serde_json::from_str(r#"{"kind":"info","message":"Hi"}"#).unwrap();
        assert_eq!(old, Toast::info("Hi"));
        assert_eq!(
            serde_json::to_string(&old).unwrap(),
            r#"{"kind":"info","message":"Hi"}"#
        );
        let texts = RegionTexts {
            dismiss: "x".into(),
            failed: "Didn't \"work\"".into(),
        };
        assert!(region(&[], &texts, "bottom-end").contains("rx-toasts--bottom-end"));
        let html = region(&[], &texts, "nowhere");
        assert!(html.contains("rx-toasts--top"), "{html}");
        assert!(
            html.contains(r#"data-failed-label="Didn&#x27;t &quot;work&quot;""#),
            "{html}"
        );
    }

    #[test]
    fn request_actions_go_to_this_site_only() {
        let toast = Toast::info("Archived")
            .action(ToastAction::delete("Undo", "/orders/7/archive"))
            .action(ToastAction::post("Retry", "/orders/7/retry"))
            .action(ToastAction::put("Elsewhere", "https://evil.example/x"))
            .action(ToastAction::patch("Sneaky", "//evil.example/x"))
            .action(ToastAction::post("Backslash", "/\\evil.example/x"));
        let html = toast_html(&toast, "x");
        assert!(
            html.contains(r#"<button type="button" class="rx-toast__action" data-rx-request="/orders/7/archive" data-rx-method="DELETE" data-renox-dismiss>Undo</button>"#),
            "{html}"
        );
        assert!(html.contains(r#"data-rx-request="/orders/7/retry" data-rx-method="POST""#));
        for bad in ["Elsewhere", "Sneaky", "Backslash", "evil"] {
            assert!(!html.contains(bad), "{bad}: {html}");
        }
        // A method that isn't a request's isn't shown, and links stay links.
        let mut odd = ToastAction::post("Odd", "/x");
        odd.method = Some("TRACE".into());
        assert!(!odd.is_safe());
        assert_eq!(request_method("delete"), Some("DELETE"));
        assert!(ToastAction::link("Out", "https://renox.dev").is_safe());
        // In JSON (htmx triggers, stored notifications) it is the method.
        assert_eq!(
            serde_json::to_string(&ToastAction::delete("Undo", "/a")).unwrap(),
            r#"{"label":"Undo","url":"/a","method":"DELETE"}"#
        );
    }

    #[test]
    fn only_harmless_urls_are_links() {
        for ok in [
            "/a",
            "https://x.y",
            "mailto:a@b.c",
            "tel:+62",
            "?q=1",
            "#top",
            "a/b:c",
        ] {
            assert!(safe_url(ok), "{ok}");
        }
        for bad in [
            "javascript:x",
            " JaVa\tscript:x",
            "data:text/html,x",
            "vbscript:x",
        ] {
            assert!(!safe_url(bad), "{bad}");
        }
    }
}
