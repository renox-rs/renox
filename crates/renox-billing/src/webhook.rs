//! The gateways' webhooks, received through `renox::webhook`: one route,
//! `/billing/webhooks/{gateway}` (the default account) and
//! `/billing/webhooks/{gateway}/{account}`, verified by that gateway with
//! that account's secret, stored once per event and applied by a queue
//! worker.

use renox::axum::body::{Body, to_bytes};
use renox::axum::extract::Request;
use renox::axum::http::HeaderValue;
use renox::axum::middleware::Next;
use renox::prelude::*;

use crate::Notice;
use crate::gateway::account_suffix;
use crate::sync::{apply, payment};

/// The gateway the call is for, set from the URL (any sent one is dropped).
const GATEWAY: &str = "x-renox-billing-gateway";
/// The named account the call is for, set from the URL (none: the default).
const ACCOUNT: &str = "x-renox-billing-account";
/// `{gateway}[/{account}]:{event id}`, set from the body by the gateway, so
/// the same id at two accounts is two events.
const EVENT: &str = "x-renox-billing-event";
/// The largest webhook body read (providers send a few kilobytes).
const LIMIT: usize = 2 * 1024 * 1024;

/// Every gateway's webhooks, stored under the provider `billing`.
pub(crate) struct BillingWebhook;

impl Webhook for BillingWebhook {
    const PROVIDER: &'static str = "billing";

    fn verify(request: &WebhookRequest, state: &AppState) -> Result {
        let setup = crate::setup(state)?;
        let gateway = request
            .header(GATEWAY)
            .and_then(|name| setup.gateway(state, name))
            .ok_or(Error::Unauthorized)?;
        gateway.verify_webhook_account(
            state,
            request.header(ACCOUNT),
            &request.headers,
            &request.body,
        )
    }

    fn event_id(request: &WebhookRequest) -> Result<String> {
        request
            .header(EVENT)
            .map(str::to_owned)
            .ok_or_else(|| Error::BadRequest("the webhook has no event id".into()))
    }

    async fn handle(call: WebhookCall, ctx: JobContext) -> Result {
        let state = ctx.state.clone();
        let setup = crate::setup(&state)?;
        let (scope, _) = call
            .event_id
            .split_once(':')
            .ok_or_else(|| renox::anyhow::anyhow!("a billing webhook without its gateway"))?;
        let (name, account) = match scope.split_once('/') {
            Some((name, account)) => (name, Some(account)),
            None => (scope, None),
        };
        let gateway = setup.gateway(&state, name).ok_or_else(|| {
            renox::anyhow::anyhow!("the payment gateway `{name}` isn't set up any more")
        })?;
        for notice in gateway.parse_webhook_account(&state, account, &call.payload)? {
            match notice {
                Notice::Subscription(remote) => {
                    apply(&state, &setup, gateway.name(), remote).await?;
                }
                Notice::Payment(paid) => payment(&state, gateway.name(), paid).await?,
            }
        }
        Ok(())
    }
}

/// Names the gateway and the account (from the URL after
/// `/billing/webhooks/`) and the event (from the body, by the gateway) in
/// headers `BillingWebhook` reads. An unknown or unconfigured gateway, or an
/// account it doesn't have, is a 404.
pub(crate) async fn mark(mut request: Request, next: Next) -> Response {
    request.headers_mut().remove(GATEWAY);
    request.headers_mut().remove(ACCOUNT);
    request.headers_mut().remove(EVENT);
    let Some(state) = request.extensions().get::<AppState>().cloned() else {
        return next.run(request).await;
    };
    let path = request.uri().path().to_owned();
    let tail = path.rsplit_once("/webhooks/").map_or("", |(_, tail)| tail);
    let (name, account) = match tail.split_once('/') {
        Some((name, account)) => (name.to_owned(), Some(account.to_owned())),
        None => (tail.to_owned(), None),
    };
    let Some(gateway) = crate::setup(&state)
        .ok()
        .and_then(|setup| setup.gateway(&state, &name))
    else {
        return Error::NotFound.into_response();
    };
    if let Some(account) = &account
        && (account_suffix(account).is_none() || !gateway.has_account(&state, account))
    {
        return Error::NotFound.into_response();
    }
    let scope = match &account {
        Some(account) => format!("{name}/{account}"),
        None => name.clone(),
    };
    let (mut parts, body) = request.into_parts();
    let Ok(bytes) = to_bytes(body, LIMIT).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    if let Ok(id) = gateway.webhook_event_id(&parts.headers, &bytes) {
        let mut id = format!("{scope}:{id}");
        // Kept short and header-safe; the gateway's name stays in front.
        if id.len() > 150 || HeaderValue::from_str(&id).is_err() {
            id = format!("{scope}:sha256:{}", renox::webhook::sha256_hex(&id));
        }
        if let Ok(value) = HeaderValue::from_str(&id) {
            parts.headers.insert(EVENT, value);
        }
    }
    if let Ok(value) = HeaderValue::from_str(&name) {
        parts.headers.insert(GATEWAY, value);
    }
    if let Some(account) = &account
        && let Ok(value) = HeaderValue::from_str(account)
    {
        parts.headers.insert(ACCOUNT, value);
    }
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}
