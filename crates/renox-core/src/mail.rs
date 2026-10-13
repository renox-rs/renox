//! Sending email: SMTP in production, the log or memory in development and
//! tests, HTML templates with a text version, and a preview page.
//!
//! ```
//! # use renox::prelude::*;
//! # async fn demo(state: AppState, order: renox::serde_json::Value) -> Result {
//! let mail = state.mail_view("ben@example.com", "Your receipt", "mail/receipt", context! { order })?;
//! state.mailer.send(mail.clone()).await?;   // now
//! state.queue_mail(mail).await?;            // through the queue, with retries
//! # Ok(()) }
//! ```
//!
//! `MAIL_MAILER` picks the driver: `smtp` (`MAIL_HOST`, `MAIL_PORT`,
//! `MAIL_USERNAME`, `MAIL_PASSWORD`, `MAIL_ENCRYPTION` = `tls`, `starttls` or
//! `none`), `log` (the default: messages go to the server log) or `memory`
//! (for tests: `kernel.mailer().sent()`). With `APP_DEBUG` on, the last 50
//! messages are listed at `/_renox/mail`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use axum::Router;
use axum::extract::{Path, State};
use axum::response::Html;
use axum::routing::get;
use lettre::message::header::ContentType;
use lettre::message::{Mailbox, MultiPart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use serde::{Deserialize, Serialize};

use crate::db::{DateTime, now};
use crate::queue::{Job, JobContext};
use crate::{AppState, Config, Error, Result, context};

/// How many sent messages the preview page keeps.
const OUTBOX: usize = 50;

/// One email message. `html` is optional; `text` is always sent.
///
/// ```
/// # use renox::mail::Mail;
/// let pdf: Vec<u8> = b"%PDF-1.7 ...".to_vec();
/// let mail = Mail::new("ben@example.com", "Invoice INV-001", "Your invoice is attached.")
///     .also_to("finance@example.com")
///     .cc("sales@example.com")
///     .bcc("archive@example.com")
///     .reply_to("Coffee Shop <hello@shop.example>")
///     .from("Coffee Shop Billing <billing@shop.example>") // instead of MAIL_FROM_*
///     .attach("INV-001.pdf", "application/pdf", pdf);
/// ```
///
/// Addresses are `a@b.c` or `Name <a@b.c>`; an invalid one fails the send
/// (and a queued mail isn't retried for it).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct Mail {
    /// Recipients.
    pub to: Vec<String>,
    /// The subject line.
    pub subject: String,
    /// The plain-text body.
    pub text: String,
    /// An HTML body, sent next to `text` when set.
    #[serde(default)]
    pub html: Option<String>,
    /// Carbon-copy recipients.
    #[serde(default)]
    pub cc: Vec<String>,
    /// Blind-carbon-copy recipients.
    #[serde(default)]
    pub bcc: Vec<String>,
    /// The address replies go to.
    #[serde(default)]
    pub reply_to: Option<String>,
    /// Sender instead of `MAIL_FROM_ADDRESS` / `MAIL_FROM_NAME`.
    #[serde(default)]
    pub from: Option<String>,
    /// Files sent with the mail.
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

/// A file sent with a mail.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct Attachment {
    /// The file name the recipient sees, e.g. `INV-001.pdf`.
    pub filename: String,
    /// e.g. `application/pdf`.
    pub content_type: String,
    /// Stored as base64 when the mail is queued.
    #[serde(with = "base64_bytes")]
    pub data: Vec<u8>,
}

mod base64_bytes {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(data: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(data))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        STANDARD.decode(text).map_err(serde::de::Error::custom)
    }
}

impl Mail {
    /// A mail to `to` with this subject and plain-text body.
    pub fn new(to: impl Into<String>, subject: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            to: vec![to.into()],
            subject: subject.into(),
            text: text.into(),
            html: None,
            cc: Vec::new(),
            bcc: Vec::new(),
            reply_to: None,
            from: None,
            attachments: Vec::new(),
        }
    }

    /// Adds an HTML body; `text` is still sent as the plain alternative.
    pub fn html(mut self, html: impl Into<String>) -> Self {
        self.html = Some(html.into());
        self
    }

    /// Another recipient.
    pub fn also_to(mut self, address: impl Into<String>) -> Self {
        self.to.push(address.into());
        self
    }

    /// Adds a carbon-copy recipient.
    pub fn cc(mut self, address: impl Into<String>) -> Self {
        self.cc.push(address.into());
        self
    }

    /// Adds a blind-carbon-copy recipient.
    pub fn bcc(mut self, address: impl Into<String>) -> Self {
        self.bcc.push(address.into());
        self
    }

    /// Where replies go.
    pub fn reply_to(mut self, address: impl Into<String>) -> Self {
        self.reply_to = Some(address.into());
        self
    }

    /// Sends as `address` instead of `MAIL_FROM_ADDRESS` / `MAIL_FROM_NAME`.
    pub fn from(mut self, address: impl Into<String>) -> Self {
        self.from = Some(address.into());
        self
    }

    /// Attaches `data` as a file named `filename` of type `content_type`.
    pub fn attach(
        mut self,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        data: impl Into<Vec<u8>>,
    ) -> Self {
        self.attachments.push(Attachment {
            filename: filename.into(),
            content_type: content_type.into(),
            data: data.into(),
        });
        self
    }

    /// Whether `address` is among `to`, `cc` or `bcc`.
    pub fn is_for(&self, address: &str) -> bool {
        self.to
            .iter()
            .chain(&self.cc)
            .chain(&self.bcc)
            .any(|a| a == address || a.ends_with(&format!("<{address}>")))
    }
}

setting_enum! {
    /// How mail is sent, from `MAIL_MAILER`.
    pub enum MailDriver ("MAIL_MAILER") {
        /// Through an SMTP server (`MAIL_HOST`); use it in production.
        Smtp = "smtp",
        /// Written to the server log; nothing leaves the machine (the default).
        Log = "log",
        /// Kept in memory, for tests (`app.sent_mail()`).
        Memory = "memory",
    }
}

setting_enum! {
    /// How the SMTP connection is secured, from `MAIL_ENCRYPTION`.
    pub enum MailEncryption ("MAIL_ENCRYPTION") {
        /// TLS from the start (usually port 465).
        Tls = "tls",
        /// Plain, upgraded with STARTTLS (usually port 587; the default).
        StartTls = "starttls",
        /// Not encrypted: a local test server such as Mailpit only.
        None = "none",
    }
}

/// Mail settings, from `MAIL_*`.
#[derive(Clone)]
#[non_exhaustive]
pub struct MailConfig {
    /// How mail is sent: SMTP, the log or memory.
    pub mailer: MailDriver,
    /// SMTP server, from `MAIL_HOST` (default `localhost`).
    pub host: String,
    /// SMTP port, from `MAIL_PORT`; `None` uses the encryption's usual port.
    pub port: Option<u16>,
    /// SMTP user name, from `MAIL_USERNAME`.
    pub username: Option<String>,
    /// SMTP password, from `MAIL_PASSWORD`.
    pub password: Option<String>,
    /// `tls` (usually port 465), `starttls` (587) or `none` (e.g. Mailpit on 1025).
    pub encryption: MailEncryption,
    /// Sender address, from `MAIL_FROM_ADDRESS` (default `hello@example.com`).
    pub from_address: String,
    /// Sender name, from `MAIL_FROM_NAME`.
    pub from_name: Option<String>,
    /// How long sending one mail over SMTP may take, from `MAIL_TIMEOUT` in
    /// seconds (default 10).
    pub timeout: std::time::Duration,
    /// Mailers (`App::mailer` names) to try in order when this one fails,
    /// from `MAIL_FAILOVER` (comma-separated), e.g. `backup` for a second
    /// SMTP provider.
    pub failover: Vec<String>,
}

impl std::fmt::Debug for MailConfig {
    // Secrets show as `[hidden]`, so a logged config doesn't leak them.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MailConfig")
            .field("mailer", &self.mailer)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "[hidden]"))
            .field("encryption", &self.encryption)
            .field("from_address", &self.from_address)
            .field("from_name", &self.from_name)
            .field("timeout", &self.timeout)
            .field("failover", &self.failover)
            .finish()
    }
}

impl Default for MailConfig {
    fn default() -> Self {
        Self {
            mailer: MailDriver::Log,
            host: "localhost".into(),
            port: None,
            username: None,
            password: None,
            encryption: MailEncryption::StartTls,
            from_address: "hello@example.com".into(),
            timeout: std::time::Duration::from_secs(10),
            from_name: None,
            failover: Vec::new(),
        }
    }
}

impl MailConfig {
    /// A mailer's settings from variables starting with `prefix`, for
    /// [`App::mailer`](crate::App::mailer): `<PREFIX>_MAILER` (`smtp`,
    /// `log` or `memory`; default: the app's `MAIL_MAILER`, so it logs
    /// while developing and keeps mail in memory in tests), `<PREFIX>_HOST`, `_PORT`,
    /// `_USERNAME`, `_PASSWORD`, `_ENCRYPTION`, `_TIMEOUT` (seconds), and
    /// `<PREFIX>_FROM_ADDRESS` / `_FROM_NAME`, which fall back to the
    /// app's `MAIL_FROM_*`. A value that doesn't fit (an unknown driver, a
    /// port that isn't a number) is an error naming the variable, and the
    /// app doesn't boot.
    pub fn from_env(config: &Config, prefix: &str) -> crate::Result<Self> {
        let prefix = prefix.trim_end_matches('_').to_ascii_uppercase();
        let name = |name: &str| format!("{prefix}_{name}");
        let var = |key: &str| config.var(&name(key));
        let number = |key: &str| -> anyhow::Result<Option<u64>> {
            var(key)
                .map(|value| {
                    value.trim().parse::<u64>().map_err(|_| {
                        anyhow::anyhow!("{} must be a number, got `{value}`", name(key))
                    })
                })
                .transpose()
        };
        let defaults = Self::default();
        let port = number("PORT")?
            .map(|p| {
                u16::try_from(p).map_err(|_| anyhow::anyhow!("{} is not a port", name("PORT")))
            })
            .transpose()?;
        Ok(Self {
            mailer: var("MAILER")
                .map(|v| MailDriver::parse_as(&v, &name("MAILER")))
                .transpose()?
                .unwrap_or(config.mail.mailer),
            host: var("HOST").unwrap_or(defaults.host),
            port,
            username: var("USERNAME"),
            password: var("PASSWORD"),
            encryption: var("ENCRYPTION")
                .map(|v| MailEncryption::parse_as(&v, &name("ENCRYPTION")))
                .transpose()?
                .unwrap_or(defaults.encryption),
            from_address: var("FROM_ADDRESS").unwrap_or_else(|| config.mail.from_address.clone()),
            from_name: var("FROM_NAME").or_else(|| config.mail.from_name.clone()),
            timeout: number("TIMEOUT")?.map_or(defaults.timeout, std::time::Duration::from_secs),
            failover: Vec::new(),
        })
    }
}

#[derive(Clone)]
enum Driver {
    Log,
    Memory,
    Smtp {
        transport: AsyncSmtpTransport<Tokio1Executor>,
        from: Mailbox,
        timeout: std::time::Duration,
    },
}

#[derive(Debug, Clone, Serialize)]
struct Sent {
    id: u64,
    at: DateTime,
    mail: Mail,
}

/// Sends mail with the configured driver and remembers recent messages for
/// the preview page and tests.
#[derive(Clone)]
pub struct Mailer {
    driver: Driver,
    /// Mailers tried in order when `driver` fails (`MAIL_FAILOVER`).
    failover: Vec<(String, Driver)>,
    outbox: Arc<Mutex<(u64, VecDeque<Sent>)>>,
    /// The memory driver keeps everything; others keep the last `OUTBOX` in debug.
    keep: Option<usize>,
}

impl Mailer {
    pub(crate) fn from_config(config: &Config) -> anyhow::Result<Self> {
        Self::open(&config.mail, config)
    }

    /// A mailer from `mail`.
    pub(crate) fn open(mail: &MailConfig, config: &Config) -> anyhow::Result<Self> {
        let (driver, keep) = match mail.mailer {
            MailDriver::Log => (Driver::Log, config.debug.then_some(OUTBOX)),
            MailDriver::Memory => (Driver::Memory, Some(usize::MAX)),
            MailDriver::Smtp => (smtp(mail, &config.name)?, config.debug.then_some(OUTBOX)),
        };
        Ok(Self {
            driver,
            failover: Vec::new(),
            outbox: Arc::default(),
            keep,
        })
    }

    /// Tries `others` in order when this mailer fails.
    pub(crate) fn with_failover(mut self, others: Vec<(String, Mailer)>) -> Self {
        self.failover = others
            .into_iter()
            .map(|(name, mailer)| (name, mailer.driver))
            .collect();
        self
    }

    /// Sends `mail` through the configured driver (the `log` and `memory`
    /// drivers only record it). When it fails and `MAIL_FAILOVER` names
    /// other mailers, they're tried in order; a mail that can't be sent at
    /// all (a bad address) isn't handed on.
    pub async fn send(&self, mail: Mail) -> Result {
        if let Err(mut last) = deliver(&self.driver, &mail).await {
            if last.is_permanent() || self.failover.is_empty() {
                return Err(last);
            }
            let mut sent = false;
            for (name, driver) in &self.failover {
                tracing::warn!(error = ?last, mailer = %name, "sending mail failed; trying the next mailer");
                match deliver(driver, &mail).await {
                    Ok(()) => {
                        sent = true;
                        break;
                    }
                    Err(err) => last = err,
                }
            }
            if !sent {
                return Err(last);
            }
        }
        self.remember(mail);
        Ok(())
    }

    fn remember(&self, mail: Mail) {
        let Some(keep) = self.keep else { return };
        let mut outbox = self.outbox.lock().unwrap_or_else(|e| e.into_inner());
        outbox.0 += 1;
        let id = outbox.0;
        outbox.1.push_back(Sent {
            id,
            at: now(),
            mail,
        });
        while outbox.1.len() > keep {
            outbox.1.pop_front();
        }
    }

    /// Messages sent so far that the mailer kept: all of them with the
    /// `memory` driver, the last 50 with `APP_DEBUG` on.
    pub fn sent(&self) -> Vec<Mail> {
        let outbox = self.outbox.lock().unwrap_or_else(|e| e.into_inner());
        outbox.1.iter().map(|s| s.mail.clone()).collect()
    }

    fn kept(&self) -> Vec<Sent> {
        self.outbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .iter()
            .cloned()
            .collect()
    }
}

/// Hands `mail` to one driver.
async fn deliver(driver: &Driver, mail: &Mail) -> Result {
    match driver {
        Driver::Log => tracing::info!(
            "mail (log driver)\nTo: {}\nSubject: {}\n\n{}\n",
            mail.to.join(", "),
            mail.subject,
            mail.text
        ),
        Driver::Memory => {}
        Driver::Smtp {
            transport,
            from,
            timeout,
        } => {
            let message = message(from, mail)?;
            // lettre's own timeout doesn't cover a server that accepts the
            // connection and then says nothing.
            tokio::time::timeout(*timeout, transport.send(message))
                .await
                .map_err(|_| anyhow::anyhow!("no answer from the SMTP server in {timeout:?}"))
                .and_then(|sent| sent.map_err(anyhow::Error::from))
                .with_context(|| format!("sending mail to {}", mail.to.join(", ")))?;
        }
    }
    Ok(())
}

fn smtp(mail: &MailConfig, app_name: &str) -> anyhow::Result<Driver> {
    let mut builder = match mail.encryption {
        MailEncryption::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&mail.host)?,
        MailEncryption::StartTls => {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&mail.host)?
        }
        MailEncryption::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&mail.host),
    };
    if let Some(port) = mail.port {
        builder = builder.port(port);
    }
    builder = builder.timeout(Some(mail.timeout));
    if let (Some(user), Some(password)) = (&mail.username, &mail.password) {
        builder = builder.credentials(Credentials::new(user.clone(), password.clone()));
    }
    let name = mail
        .from_name
        .clone()
        .unwrap_or_else(|| app_name.to_owned());
    let address = mail.from_address.parse().with_context(|| {
        format!(
            "MAIL_FROM_ADDRESS `{}` is not an email address",
            mail.from_address
        )
    })?;
    Ok(Driver::Smtp {
        transport: builder.build(),
        from: Mailbox::new(Some(name), address),
        timeout: mail.timeout,
    })
}

fn mailbox(address: &str) -> Result<Mailbox> {
    address.trim().parse().map_err(|err| {
        Error::permanent(
            anyhow::Error::new(err).context(format!("`{address}` is not an email address")),
        )
    })
}

fn message(from: &Mailbox, mail: &Mail) -> Result<Message> {
    use lettre::message::{Attachment as Part, SinglePart};

    if mail.to.is_empty() {
        return Err(Error::permanent(anyhow::anyhow!(
            "the mail has no recipient"
        )));
    }
    let from = match &mail.from {
        Some(address) => mailbox(address)?,
        None => from.clone(),
    };
    let mut builder = Message::builder().from(from).subject(mail.subject.clone());
    for address in &mail.to {
        builder = builder.to(mailbox(address)?);
    }
    for address in &mail.cc {
        builder = builder.cc(mailbox(address)?);
    }
    for address in &mail.bcc {
        builder = builder.bcc(mailbox(address)?);
    }
    if let Some(address) = &mail.reply_to {
        builder = builder.reply_to(mailbox(address)?);
    }
    let body = match &mail.html {
        Some(html) => MultiPart::alternative_plain_html(mail.text.clone(), html.clone()),
        None => MultiPart::mixed().singlepart(
            SinglePart::builder()
                .header(ContentType::TEXT_PLAIN)
                .body(mail.text.clone()),
        ),
    };
    let message = if mail.attachments.is_empty() {
        match &mail.html {
            Some(_) => builder.multipart(body),
            None => builder
                .header(ContentType::TEXT_PLAIN)
                .body(mail.text.clone()),
        }
    } else {
        let mut mixed = MultiPart::mixed().multipart(body);
        for file in &mail.attachments {
            let content_type = ContentType::parse(&file.content_type).map_err(|err| {
                Error::permanent(anyhow::anyhow!(
                    "attachment `{}`: `{}` is not a content type ({err})",
                    file.filename,
                    file.content_type
                ))
            })?;
            mixed = mixed
                .singlepart(Part::new(file.filename.clone()).body(file.data.clone(), content_type));
        }
        builder.multipart(mixed)
    };
    Ok(message.map_err(anyhow::Error::from)?)
}

/// Sends a mail from the queue; see `AppState::queue_mail`.
#[derive(Serialize, Deserialize)]
pub(crate) struct SendMail(pub Mail);

impl Job for SendMail {
    const NAME: &'static str = "renox.send-mail";
    const MAX_ATTEMPTS: u32 = 5;

    async fn handle(self, ctx: JobContext) -> Result {
        ctx.state.mailer.send(self.0).await
    }
}

/// Sends a mail from the queue through a named mailer; see
/// `AppState::queue_mail_via`.
#[derive(Serialize, Deserialize)]
pub(crate) struct SendMailVia {
    pub mailer: String,
    pub mail: Mail,
}

impl Job for SendMailVia {
    const NAME: &'static str = "renox.send-mail-via";
    const MAX_ATTEMPTS: u32 = 5;

    async fn handle(self, ctx: JobContext) -> Result {
        // A mailer removed since the mail was queued won't come back.
        let mailer = ctx
            .state
            .mailer_named(&self.mailer)
            .map_err(|err| Error::permanent(anyhow::anyhow!("{err:?}")))?;
        mailer.send(self.mail).await
    }
}

impl AppState {
    /// Renders `{view}.html` into a mail, with `{view}.txt` as the text
    /// version when it exists (otherwise text made from the HTML).
    /// Templates see `app` (`app.locale` too) and `t(key, …)` in the
    /// [`current_locale`](crate::i18n::current_locale) besides `ctx`, can
    /// extend `renox/mail/layout.html` and import
    /// `renox/mail/components.html` (`button`, `panel`, `table`).
    pub fn mail_view(
        &self,
        to: impl Into<String>,
        subject: impl Into<String>,
        view: &str,
        ctx: impl Serialize,
    ) -> Result<Mail> {
        let locale = crate::i18n::current_locale(self);
        let ctx = minijinja::value::merge_maps([
            minijinja::Value::from_serialize(&ctx),
            context! {
                app => context! { name => self.config.name, url => self.config.url, locale => locale },
                t => crate::view::translate_function(self, &locale),
            },
        ]);
        let html = self.views.render(&format!("{view}.html"), &ctx)?;
        let text = match self.views.render(&format!("{view}.txt"), &ctx) {
            Ok(text) => text,
            Err(err) if is_not_found(&err) => html_to_text(&html),
            Err(err) => return Err(err.into()),
        };
        Ok(Mail::new(to, subject, text.trim().to_owned()).html(html))
    }

    /// `mail_view` in `locale` (the recipient's language).
    pub fn mail_view_in(
        &self,
        locale: &str,
        to: impl Into<String>,
        subject: impl Into<String>,
        view: &str,
        ctx: impl Serialize,
    ) -> Result<Mail> {
        crate::i18n::with_locale(Some(locale), || self.mail_view(to, subject, view, ctx))
    }

    /// Sends `mail` from a queue worker, retrying up to five times.
    pub async fn queue_mail(&self, mail: Mail) -> Result<i64> {
        self.dispatch(SendMail(mail)).await
    }

    /// A mailer the app added with [`App::mailer`](crate::App::mailer), e.g.
    /// `state.mailer_named("newsletter")?.send(mail)`; an unknown name is an
    /// error (500).
    pub fn mailer_named(&self, name: &str) -> Result<&Mailer> {
        self.mailers.get(name).ok_or_else(|| {
            anyhow::anyhow!("no mailer named `{name}`: add it with `App::mailer(\"{name}\", …)`")
                .into()
        })
    }

    /// Like `queue_mail`, through the mailer named `mailer`.
    pub async fn queue_mail_via(&self, mailer: &str, mail: Mail) -> Result<i64> {
        self.mailer_named(mailer)?;
        self.dispatch(SendMailVia {
            mailer: mailer.to_owned(),
            mail,
        })
        .await
    }
}

fn is_not_found(err: &anyhow::Error) -> bool {
    err.downcast_ref::<minijinja::Error>()
        .is_some_and(|e| e.kind() == minijinja::ErrorKind::TemplateNotFound)
}

/// A readable text version of an HTML mail: tags dropped, blocks separated
/// by blank lines, table cells by spaces, links written as `text (url)`.
pub(crate) fn html_to_text(html: &str) -> String {
    let body = html
        .find("<body")
        .and_then(|start| html[start..].find('>').map(|end| start + end + 1))
        .map_or(html, |start| &html[start..]);
    let mut out = String::new();
    let mut href: Option<String> = None;
    let mut rest = body;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            // A `<` that opens no tag: the rest is text (once).
            rest = &rest[open..];
            break;
        };
        let tag = &rest[open + 1..open + close];
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match name.as_str() {
            "a" if !tag.starts_with('/') => {
                href = tag
                    .split("href=\"")
                    .nth(1)
                    .and_then(|v| v.split('"').next())
                    .map(str::to_owned);
            }
            "a" => {
                if let Some(url) = href.take() {
                    out.push_str(&format!(" ({url})"));
                }
            }
            "style" | "head" | "title" if !tag.starts_with('/') => {
                let end = format!("</{name}");
                if let Some(skip) = rest[open..].to_ascii_lowercase().find(&end) {
                    rest = &rest[open + skip..];
                    continue;
                }
            }
            "br" | "p" | "div" | "tr" | "li" | "h1" | "h2" | "h3" | "table" => out.push('\n'),
            "td" | "th" => out.push(' '),
            _ => {}
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&#x2f;", "/")
        .replace("&amp;", "&");
    let mut text = String::new();
    let mut blank = false;
    for line in decoded
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
    {
        if line.is_empty() {
            blank = !text.is_empty();
            continue;
        }
        if blank {
            text.push('\n');
            blank = false;
        }
        text.push_str(&line);
        text.push('\n');
    }
    text
}

/// `/_renox/mail`: recently sent messages, in development only.
pub(crate) fn preview_router() -> Router<AppState> {
    Router::new()
        .route("/_renox/mail", get(list))
        .route("/_renox/mail/{id}", get(show))
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const STYLE: &str = "<style>body{font-family:system-ui,sans-serif;max-width:60rem;margin:2rem auto;padding:0 1rem;color:#222}\
table{width:100%;border-collapse:collapse}td{padding:.5rem;border-bottom:1px solid #eee}\
iframe{width:100%;height:32rem;border:1px solid #ddd;border-radius:.5rem}pre{white-space:pre-wrap;background:#f6f6f6;padding:1rem}</style>";

async fn list(State(state): State<AppState>) -> Html<String> {
    let rows: String = state
        .mailer
        .kept()
        .iter()
        .rev()
        .map(|s| {
            format!(
                "<tr><td>{}</td><td><a href=\"/_renox/mail/{}\">{}</a></td><td>{}</td></tr>",
                s.at.format("%H:%M:%S"),
                s.id,
                escape(&s.mail.subject),
                escape(&s.mail.to.join(", "))
            )
        })
        .collect();
    let rows = if rows.is_empty() {
        "<tr><td>No mail sent yet.</td></tr>".to_owned()
    } else {
        rows
    };
    Html(format!(
        "<!doctype html><meta charset=utf-8><title>Mail · Renox</title>{STYLE}<h1>Sent mail</h1><table>{rows}</table>"
    ))
}

async fn show(State(state): State<AppState>, Path(id): Path<u64>) -> Result<Html<String>> {
    let sent = state
        .mailer
        .kept()
        .into_iter()
        .find(|s| s.id == id)
        .ok_or(Error::NotFound)?;
    let html = sent.mail.html.as_deref().map_or(String::new(), |html| {
        format!(
            "<h2>HTML</h2><iframe sandbox srcdoc=\"{}\"></iframe>",
            escape(html)
        )
    });
    let mut details = format!("<p>To: {}</p>", escape(&sent.mail.to.join(", ")));
    for (label, list) in [("Cc", &sent.mail.cc), ("Bcc", &sent.mail.bcc)] {
        if !list.is_empty() {
            details.push_str(&format!("<p>{label}: {}</p>", escape(&list.join(", "))));
        }
    }
    for (label, value) in [("Reply-To", &sent.mail.reply_to), ("From", &sent.mail.from)] {
        if let Some(value) = value {
            details.push_str(&format!("<p>{label}: {}</p>", escape(value)));
        }
    }
    if !sent.mail.attachments.is_empty() {
        let files: Vec<String> = sent
            .mail
            .attachments
            .iter()
            .map(|a| {
                format!(
                    "{} ({}, {} bytes)",
                    escape(&a.filename),
                    escape(&a.content_type),
                    a.data.len()
                )
            })
            .collect();
        details.push_str(&format!("<p>Attachments: {}</p>", files.join(", ")));
    }
    Ok(Html(format!(
        "<!doctype html><meta charset=utf-8><title>{subject} · Renox</title>{STYLE}\
         <p><a href=\"/_renox/mail\">&larr; All mail</a></p><h1>{subject}</h1>{details}{html}\
         <h2>Text</h2><pre>{text}</pre>",
        subject = escape(&sent.mail.subject),
        text = escape(&sent.mail.text),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_becomes_readable_text() {
        let html = r#"<html><head><style>p{color:red}</style></head><body>
            <h1>Hello &amp; welcome</h1><p>Click <a href="https://x.id/a?b=1&amp;c=2">here</a>.</p>
            <table><tr><td>Total</td><td>$10.00</td></tr></table></body></html>"#;
        assert_eq!(
            html_to_text(html),
            "Hello & welcome\n\nClick here (https://x.id/a?b=1&c=2).\n\nTotal $10.00\n"
        );
        // Without a <body>: a style block is skipped, and a title that is
        // never closed is dropped as a tag.
        assert_eq!(html_to_text("<style>p{}</style>Hi<p>there"), "Hi\nthere\n");
        assert_eq!(html_to_text("<title>No end"), "No end\n");
        // A `<` that opens no tag is text, kept once (it was written twice).
        assert_eq!(html_to_text("Price < 5"), "Price < 5\n");
        assert_eq!(html_to_text("<p>a</p>b <c"), "a\nb <c\n");
    }

    /// The log driver writes the mail to the log; with `APP_DEBUG` on it
    /// keeps the last 50 for `/_renox/mail`, and none with it off.
    #[tokio::test]
    async fn the_log_driver_keeps_the_last_fifty_while_debugging() {
        let mut config = Config {
            debug: true,
            ..Config::default()
        };
        let mail = MailConfig::default();
        let mailer = Mailer::open(&mail, &config).unwrap();
        for i in 0..OUTBOX + 1 {
            mailer
                .send(Mail::new("ann@example.com", format!("Mail {i}"), "Hi"))
                .await
                .unwrap();
        }
        let sent = mailer.sent();
        assert_eq!(sent.len(), OUTBOX);
        assert_eq!(sent[0].subject, "Mail 1", "the oldest is gone");

        config.debug = false;
        let quiet = Mailer::open(&mail, &config).unwrap();
        quiet
            .send(Mail::new("ann@example.com", "Hi", "Hi"))
            .await
            .unwrap();
        assert!(quiet.sent().is_empty());
    }

    /// Mails SMTP can't send are permanent errors (no retry), found before
    /// connecting: no recipient, an attachment with a bad content type.
    #[test]
    fn mails_smtp_cant_send_are_permanent_errors() {
        let from: Mailbox = "shop@example.com".parse().unwrap();
        let mut nobody = Mail::new("ann@example.com", "Hi", "Hi");
        nobody.to.clear();
        let err = message(&from, &nobody).unwrap_err();
        assert!(err.is_permanent());
        assert!(format!("{err:?}").contains("the mail has no recipient"));

        let odd = Mail::new("ann@example.com", "Hi", "Hi").attach("a.bin", "not a type", vec![1]);
        let err = message(&from, &odd).unwrap_err();
        assert!(err.is_permanent());
        assert!(
            format!("{err:?}").contains("is not a content type"),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn the_outbox_page_says_when_nothing_was_sent() {
        let app = crate::testing::TestApp::new(crate::App::new()).await;
        app.get("/_renox/mail")
            .await
            .assert_ok()
            .assert_see("No mail sent yet.");
    }
}
