//! Core of the Renox web framework.
//!
//! Most applications should depend on the `renox` crate instead, which
//! re-exports everything here through `renox::prelude`.
#![recursion_limit = "256"]
// the built-in auth texts are one large `json!`
// Every public item is documented; clippy's `-D warnings` in CI keeps it so.
#![warn(missing_docs)]

/// An enum for a setting written as one word in `.env` (`SESSION_DRIVER=database`):
/// `#[non_exhaustive]`, with `as_str`, `Display` and a parser that names the variable.
macro_rules! setting_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident ($variable:literal) {
            $( $(#[$vmeta:meta])* $variant:ident = $text:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum $name {
            $( $(#[$vmeta])* $variant ),+
        }

        impl $name {
            /// The value as written in `.env`.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $( Self::$variant => $text ),+
                }
            }

            /// Reads the value of `
            #[doc = $variable]
            /// `.
            #[allow(dead_code)]
            pub(crate) fn parse(value: &str) -> anyhow::Result<Self> {
                Self::parse_as(value, $variable)
            }

            /// Reads `value`, naming `variable` in the error.
            #[allow(dead_code)]
            pub(crate) fn parse_as(value: &str, variable: &str) -> anyhow::Result<Self> {
                let value = value.trim().to_ascii_lowercase();
                $( if value == $text { return Ok(Self::$variant); } )+
                let expected: &[&str] = &[$($text),+];
                anyhow::bail!("{variable} must be one of {}, got `{value}`", expected.join(", "))
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(&self, s: S) -> ::std::result::Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }
    };
}

pub mod analytics;
mod app;
mod assets;
pub mod audit;
pub mod auth;
pub mod cache;
pub mod calendar;
pub mod chart;
mod client_ip;
mod clock;
pub mod command;
mod components;
mod config;
pub mod context;
mod contributions;
mod cookies;
mod counters;
mod crypto;
mod csrf;
pub mod custom_fields;
pub mod db;
mod domain;
mod download;
mod embedded;
mod error;
pub mod events;
pub mod grid;
mod health;
mod htmx;
pub mod http;
pub mod i18n;
mod icons;
pub mod import;
mod inspector;
mod live;
pub mod live_component;
pub mod mail;
pub mod maintenance;
mod method;
mod module;
pub mod money;
pub mod nav;
mod path;
pub mod prompt;
mod provided;
pub mod pwa;
pub mod quantity;
pub mod queue;
pub mod rate_limit;
mod redirect;
mod registry;
pub mod report;
mod request_id;
mod routing;
pub mod schedule;
pub mod security;
pub mod select;
pub mod seo;
mod session;
pub mod settings;
pub mod shell;
pub mod signed;
mod state;
pub mod state_machine;
pub mod storage;
#[cfg(test)]
mod test_logs;
pub mod testing;
pub mod text;
pub mod theme;
pub mod timezone;
pub mod toast;
pub mod upload;
pub mod validation;
/// Views: MiniJinja templates, the `View` response and template globals.
pub mod view;
mod view_filters;
mod view_stack;
pub mod webhook;

pub use app::{App, Kernel};
pub use assets::{ALPINE_VERSION, CALLY_VERSION, HTMX_VERSION, IDIOMORPH_VERSION};
pub use auth::{AuthUser, Policy};
pub use client_ip::{ClientIp, TrustedProxies};
pub use config::{
    AnalyticsConfig, CacheStore, Config, CspMode, Environment, LogFormat, PermissionsMode,
    SessionDriver,
};
pub use contributions::Contributions;
pub use cookies::{Cookies, SetCookie};
pub use crypto::{generate_key, random_token};
pub use csrf::{CSRF_FIELD, CSRF_HEADER, XSRF_COOKIE, XSRF_HEADER};
pub use domain::DomainParams;
pub use download::Download;
pub use embedded::Embedded;
pub use error::{Error, Result, abort, abort_if, abort_unless};
pub use htmx::{Back, Htmx, HxPushUrl, HxRedirect, HxRefresh, HxReswap, HxRetarget, HxTrigger};
pub use i18n::Lang;
pub use method::METHOD_FIELD;
pub use module::Module;
pub use path::{Found, Path};
pub use provided::Provided;
pub use redirect::RedirectExt;
pub use registry::Registry;
pub use request_id::RequestId;
pub use routing::Resource;
pub(crate) use routing::RouteTable;
pub use routing::{CurrentRoute, RouteInfo, Routes};
pub use session::Session;
pub use state::{AppState, AppStateInner};
pub use state::{SentBroadcast, SentNotification};
pub use toast::{Toast, ToastAction, ToastKind};
pub use upload::Upload;
pub use validation::{Errors, KeyValues, Valid, Validate, ValidationError, Validator};
pub(crate) use view::Views;
pub use view::{View, ViewData, view};
pub use view_filters::{currency_decimals, format_money, format_number};

pub use minijinja::context;

/// `anyhow`, for errors with context: `renox::anyhow::anyhow!("…")`,
/// `.context("…")`, `Error::permanent(err)`.
pub use anyhow;
pub use chrono;
pub use clap;
#[cfg(feature = "fake")]
pub use fake;
/// `minijinja`, for the app's own template filters and functions
/// (`App::templates(|env| …)` gets a `minijinja::Environment`).
pub use minijinja;
pub use serde;
pub use serde_json;
#[doc(hidden)]
pub use sqlx as __sqlx;
/// CORS configuration for `Routes::cors_layer`.
pub use tower_http::cors;
/// `uuid`, for `Uuid` model keys and fields (the `uuid` feature):
/// `renox::uuid::Uuid`.
#[cfg(feature = "uuid")]
pub use uuid;
