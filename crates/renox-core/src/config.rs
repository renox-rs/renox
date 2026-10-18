use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, bail};

use crate::mail::MailConfig;
use crate::storage::StorageConfig;

/// The environment the application runs in, from `APP_ENV`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Environment {
    /// `local` (also `dev`, `development`), the default.
    Local,
    /// `testing` (also `test`).
    Testing,
    /// `production` (also `prod`); requires `APP_KEY`.
    Production,
}

impl Environment {
    fn parse(value: &str) -> anyhow::Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "local" | "dev" | "development" => Ok(Self::Local),
            "testing" | "test" => Ok(Self::Testing),
            "production" | "prod" => Ok(Self::Production),
            other => bail!("APP_ENV must be local, testing or production, got `{other}`"),
        }
    }
}

/// How strict the Content-Security-Policy header is, from `CSP`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum CspMode {
    /// Scripts from this site, inline scripts and `eval` (which Alpine.js's
    /// standard build needs) are allowed; other sites' scripts, framing by
    /// other sites and plugins are not.
    #[default]
    Relaxed,
    /// Scripts only from this site or with `nonce="{{ csp_nonce() }}"`, no
    /// `eval`: Renox switches to Alpine's CSP build (expressions are limited
    /// to properties and methods; put logic in `Alpine.data(...)`) and turns
    /// off htmx's `eval`.
    Strict,
    /// No Content-Security-Policy header.
    Off,
}

impl CspMode {
    fn parse(value: &str) -> anyhow::Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "relaxed" | "" => Ok(Self::Relaxed),
            "strict" => Ok(Self::Strict),
            "off" | "false" | "none" => Ok(Self::Off),
            other => bail!("CSP must be relaxed, strict or off, got `{other}`"),
        }
    }
}

/// The `Permissions-Policy` header, from `PERMISSIONS_POLICY`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PermissionsMode {
    /// Switches off the features a typical app never uses (camera,
    /// microphone, geolocation, payment, USB…); `App::permissions_policy`
    /// allows some back.
    #[default]
    Default,
    /// No `Permissions-Policy` header.
    Off,
    /// This header value as the base, e.g. `geolocation=(self), camera=()`.
    Custom(String),
}

impl PermissionsMode {
    fn parse(value: &str) -> anyhow::Result<Self> {
        let value = value.trim();
        match value.to_ascii_lowercase().as_str() {
            "default" | "" => Ok(Self::Default),
            "off" | "false" | "none" => Ok(Self::Off),
            _ => {
                crate::security::parse_permissions_policy(value)
                    .with_context(|| format!("PERMISSIONS_POLICY is not usable: `{value}`"))?;
                Ok(Self::Custom(value.to_owned()))
            }
        }
    }
}

setting_enum! {
    /// Where sessions live, from `SESSION_DRIVER`.
    pub enum SessionDriver ("SESSION_DRIVER") {
        /// The whole session in an encrypted cookie (the default).
        Cookie = "cookie",
        /// An id in the cookie, the session in the `sessions` table.
        Database = "database",
    }
}

setting_enum! {
    /// How log lines are written, from `LOG_FORMAT`.
    pub enum LogFormat ("LOG_FORMAT") {
        /// Readable lines (the default).
        Text = "text",
        /// One JSON object per line, for a log service.
        Json = "json",
    }
}

setting_enum! {
    /// Where the cache keeps its values, from `CACHE_STORE`.
    pub enum CacheStore ("CACHE_STORE") {
        /// In this process's memory (the default).
        Memory = "memory",
        /// In the `cache` table, shared by every server (throttles and the login lock too).
        Database = "database",
    }
}

/// Application configuration, read from the process environment and `.env`.
#[derive(Clone)]
#[non_exhaustive]
pub struct Config {
    /// The application's name, from `APP_NAME` (Renox).
    pub name: String,
    /// The environment, from `APP_ENV` (local).
    pub env: Environment,
    /// Show error details and backtraces in responses. Never enable in production.
    pub debug: bool,
    /// The public base URL, from `APP_URL` (`http://{host}:{port}`).
    pub url: String,
    /// Secret used for signing and encryption. Required in production.
    pub key: Option<String>,
    /// The IP address to listen on, from `APP_HOST` (127.0.0.1).
    pub host: IpAddr,
    /// The port to listen on, from `APP_PORT` (3000).
    pub port: u16,
    /// Where templates live, from `VIEWS_PATH`.
    pub views_path: PathBuf,
    /// Files served as-is at the site root, from `PUBLIC_PATH`.
    pub public_path: PathBuf,
    /// Inactivity before a session expires, from `SESSION_LIFETIME` in
    /// minutes (default 120).
    pub session_lifetime: Duration,
    /// Name of the session cookie, from `SESSION_COOKIE`.
    pub session_cookie: String,
    /// Where sessions live, from `SESSION_DRIVER`: `cookie` (default: the
    /// whole session in an encrypted cookie) or `database` (the cookie holds
    /// an id; the `sessions` table holds the rest).
    pub session_driver: SessionDriver,
    /// How long a "remember me" login lasts, from `REMEMBER_LIFETIME` in
    /// minutes (default 43200: 30 days).
    pub remember_lifetime: Duration,
    /// SQLite database, from `DATABASE_URL`, e.g. `sqlite://storage/app.db`.
    pub database_url: String,
    /// Maximum open connections, from `DATABASE_POOL_SIZE`.
    pub database_pool_size: u32,
    /// How long a query waits for a free connection before failing, from
    /// `DATABASE_ACQUIRE_TIMEOUT` in seconds (default 5).
    pub database_acquire_timeout: Duration,
    /// Longest a PostgreSQL statement may run, from `DATABASE_STATEMENT_TIMEOUT`
    /// in seconds (default 30; 0 for none).
    pub database_statement_timeout: Option<Duration>,
    /// Longest a request may take to answer, from `REQUEST_TIMEOUT` in
    /// seconds (default 60; 0 for none). Streaming a response isn't counted.
    pub request_timeout: Option<Duration>,
    /// Default language of the app, from `APP_LOCALE`. Renox's own texts are
    /// English; other locales need a lang file (`resources/lang/es.json`).
    pub locale: String,
    /// Language used for keys missing in the request's locale, from `APP_FALLBACK_LOCALE`.
    pub fallback_locale: String,
    /// The currency of the `money` template filter, an ISO 4217 code, from
    /// `APP_CURRENCY` (default `USD`): `USD` shows `$75.00`, `EUR`
    /// `€75.00`, `IDR` `Rp 75,000`, with the page's separators.
    pub currency: String,
    /// Where translation files live, from `LANG_PATH`.
    pub lang_path: PathBuf,
    /// Mail settings, from `MAIL_*`.
    pub mail: MailConfig,
    /// Queue workers `serve` runs in-process, from `QUEUE_WORKERS` (0 turns them off).
    pub queue_workers: usize,
    /// Consecutive failed deliveries after which an outbound webhook endpoint is
    /// disabled, from `WEBHOOK_DISABLE_AFTER` (default 20, at least 1).
    pub webhook_disable_after: u32,
    /// How far a signed request's timestamp may be from now, in seconds, from
    /// `WEBHOOK_TOLERANCE_SECS` (default 300, at least 1). See `SignedRequest`.
    pub webhook_tolerance_secs: u64,
    /// Days the keys of `.offline()` routes are kept before `idempotency:prune`
    /// (and its scheduled task) removes them, from `IDEMPOTENCY_RETENTION_DAYS`
    /// (default 7, at least 1).
    pub idempotency_retention_days: u32,
    /// Whether `serve` runs scheduled tasks, from `SCHEDULER`.
    pub scheduler: bool,
    /// `text` (default) or `json` (one object per line), from `LOG_FORMAT`.
    pub log_format: LogFormat,
    /// Where logs go instead of stdout, from `LOG_FILE` (appended to).
    pub log_file: Option<PathBuf>,
    /// The zone of scheduled times and the `date` filter, from
    /// `APP_TIMEZONE`: an IANA name (`Asia/Jakarta`), an offset (`+07:00`) or
    /// `UTC`. See [`crate::timezone::Zone`].
    pub timezone: crate::timezone::Zone,
    /// `memory` or `database`, from `CACHE_STORE`.
    pub cache_store: CacheStore,
    /// Where the app keeps runtime files (maintenance flag, uploads), from `STORAGE_PATH`.
    pub storage_path: PathBuf,
    /// File storage, from `STORAGE_DISK`, `S3_*` and `STORAGE_URL`.
    pub storage: StorageConfig,
    /// Largest request body in bytes, from `UPLOAD_MAX_SIZE` in megabytes (default 10).
    pub upload_max_size: usize,
    /// Largest live component snapshot in bytes, from `LIVE_SNAPSHOT_MAX_SIZE` in kilobytes (default 64).
    pub live_snapshot_max: usize,
    /// The Content-Security-Policy, from `CSP` (`relaxed`, `strict` or `off`).
    pub csp: CspMode,
    /// The Permissions-Policy, from `PERMISSIONS_POLICY` (`default`, `off` or a
    /// full header value such as `geolocation=(self), camera=()`).
    pub permissions_policy: PermissionsMode,
    /// Values `var()` returns before looking at the environment, e.g. a
    /// webhook secret set in a test with `TestApp::with_config`.
    pub vars: std::collections::HashMap<String, String>,
    /// Search engines and analytics; used only in production.
    pub analytics: AnalyticsConfig,
    /// Reverse proxies whose `X-Forwarded-For` is believed, from `TRUSTED_PROXIES`.
    pub trusted_proxies: crate::TrustedProxies,
    /// Host names the app answers, from `TRUSTED_HOSTS` (comma-separated;
    /// `*.example.com` for subdomains). Empty: any host. When set, `APP_URL`'s
    /// host is allowed too, and other hosts get a 400.
    pub trusted_hosts: Vec<String>,
}

impl std::fmt::Debug for Config {
    // Secrets show as `[hidden]`, so a logged config doesn't leak them.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("name", &self.name)
            .field("env", &self.env)
            .field("debug", &self.debug)
            .field("url", &self.url)
            .field("key", &self.key.as_ref().map(|_| "[hidden]"))
            .field("host", &self.host)
            .field("port", &self.port)
            .field("views_path", &self.views_path)
            .field("public_path", &self.public_path)
            .field("permissions_policy", &self.permissions_policy)
            .field("session_lifetime", &self.session_lifetime)
            .field("session_cookie", &self.session_cookie)
            .field("session_driver", &self.session_driver)
            .field("remember_lifetime", &self.remember_lifetime)
            .field("database_url", &self.database_url)
            .field("database_pool_size", &self.database_pool_size)
            .field("database_acquire_timeout", &self.database_acquire_timeout)
            .field(
                "database_statement_timeout",
                &self.database_statement_timeout,
            )
            .field("request_timeout", &self.request_timeout)
            .field("locale", &self.locale)
            .field("fallback_locale", &self.fallback_locale)
            .field("currency", &self.currency)
            .field("lang_path", &self.lang_path)
            .field("mail", &self.mail)
            .field("queue_workers", &self.queue_workers)
            .field("webhook_disable_after", &self.webhook_disable_after)
            .field("webhook_tolerance_secs", &self.webhook_tolerance_secs)
            .field(
                "idempotency_retention_days",
                &self.idempotency_retention_days,
            )
            .field("scheduler", &self.scheduler)
            .field("log_format", &self.log_format)
            .field("log_file", &self.log_file)
            .field("timezone", &self.timezone)
            .field("cache_store", &self.cache_store)
            .field("storage_path", &self.storage_path)
            .field("storage", &self.storage)
            .field("upload_max_size", &self.upload_max_size)
            .field("live_snapshot_max", &self.live_snapshot_max)
            .field("csp", &self.csp)
            .field(
                "vars",
                &self.vars.keys().collect::<std::collections::BTreeSet<_>>(),
            )
            .field("analytics", &self.analytics)
            .field("trusted_proxies", &self.trusted_proxies)
            .field("trusted_hosts", &self.trusted_hosts)
            .finish()
    }
}

/// Google Search Console, Google Analytics 4 and Google Tag Manager, from
/// `GOOGLE_SITE_VERIFICATION`, `GA4_MEASUREMENT_ID`, `GA4_API_SECRET` and
/// `GTM_CONTAINER_ID`. Tags are added to pages only in production.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct AnalyticsConfig {
    /// The `content` of Search Console's `google-site-verification` meta tag.
    pub google_site_verification: Option<String>,
    /// e.g. `G-XXXXXXXXXX`: adds the GA4 tag to every page.
    pub ga4_measurement_id: Option<String>,
    /// For events sent from the server (Measurement Protocol).
    pub ga4_api_secret: Option<String>,
    /// e.g. `GTM-XXXXXXX`: adds the Tag Manager container to every page.
    pub gtm_container_id: Option<String>,
}

impl std::fmt::Debug for AnalyticsConfig {
    // Secrets show as `[hidden]`, so a logged config doesn't leak them.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnalyticsConfig")
            .field("google_site_verification", &self.google_site_verification)
            .field("ga4_measurement_id", &self.ga4_measurement_id)
            .field(
                "ga4_api_secret",
                &self.ga4_api_secret.as_ref().map(|_| "[hidden]"),
            )
            .field("gtm_container_id", &self.gtm_container_id)
            .finish()
    }
}

impl Config {
    /// Loads `.env` (if present) and reads the configuration from the environment.
    pub fn load() -> crate::Result<Self> {
        let _ = dotenvy::dotenv();
        Self::from_env()
    }

    /// Reads the configuration from the environment only, without loading `.env`.
    pub fn from_env() -> crate::Result<Self> {
        Self::from_vars(|name| env::var(name).ok())
    }

    /// Reads the configuration from `get` (a variable's value by name), e.g.
    /// a map in a test: `Config::from_vars(|name| vars.get(name).cloned())`.
    /// A value that doesn't fit is an error naming its variable.
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> crate::Result<Self> {
        Ok(Self::read(get)?)
    }

    fn read(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let v = Vars(get);
        let env = Environment::parse(&v.or("APP_ENV", "local"))?;
        let debug = v.bool("APP_DEBUG", env == Environment::Local)?;
        let host = v
            .or("APP_HOST", "127.0.0.1")
            .parse()
            .context("APP_HOST must be an IP address")?;
        let port = v
            .or("APP_PORT", "3000")
            .parse()
            .context("APP_PORT must be a port number")?;
        let key = v.get("APP_KEY").filter(|k| !k.is_empty());

        if env == Environment::Production && key.is_none() {
            bail!("APP_KEY must be set in production");
        }

        Ok(Self {
            name: v.or("APP_NAME", "Renox"),
            env,
            debug,
            url: v.or("APP_URL", &format!("http://{host}:{port}")),
            key,
            host,
            port,
            views_path: v.or("VIEWS_PATH", "resources/views").into(),
            public_path: v.or("PUBLIC_PATH", "public").into(),
            session_lifetime: Duration::from_secs(
                v.or("SESSION_LIFETIME", "120")
                    .parse::<u64>()
                    .context("SESSION_LIFETIME must be a number of minutes")?
                    .saturating_mul(60),
            ),
            session_cookie: v.or("SESSION_COOKIE", "renox_session"),
            session_driver: SessionDriver::parse(&v.or("SESSION_DRIVER", "cookie"))?,
            remember_lifetime: Duration::from_secs(
                v.or("REMEMBER_LIFETIME", "43200")
                    .parse::<u64>()
                    .context("REMEMBER_LIFETIME must be a number of minutes")?
                    .saturating_mul(60),
            ),
            database_url: v.or("DATABASE_URL", "sqlite://storage/app.db"),
            database_pool_size: v
                .or("DATABASE_POOL_SIZE", "8")
                .parse()
                .ok()
                .filter(|n| *n > 0)
                .context("DATABASE_POOL_SIZE must be a number above 0")?,
            database_acquire_timeout: v
                .seconds("DATABASE_ACQUIRE_TIMEOUT", 5)?
                .unwrap_or(Duration::from_secs(5)),
            database_statement_timeout: v.seconds("DATABASE_STATEMENT_TIMEOUT", 30)?,
            request_timeout: v.seconds("REQUEST_TIMEOUT", 60)?,
            locale: v.or("APP_LOCALE", "en"),
            fallback_locale: v.or("APP_FALLBACK_LOCALE", "en"),
            currency: match v.or("APP_CURRENCY", "USD").trim().to_ascii_uppercase() {
                code if code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase()) => code,
                other => {
                    bail!("APP_CURRENCY must be an ISO 4217 code like USD or EUR, got `{other}`")
                }
            },
            lang_path: v.or("LANG_PATH", "resources/lang").into(),
            mail: MailConfig {
                mailer: crate::mail::MailDriver::parse(&v.or("MAIL_MAILER", "log"))?,
                host: v.or("MAIL_HOST", "localhost"),
                port: v
                    .get("MAIL_PORT")
                    .filter(|p| !p.is_empty())
                    .map(|p| p.parse())
                    .transpose()
                    .context("MAIL_PORT must be a port number")?,
                username: v.get("MAIL_USERNAME").filter(|v| !v.is_empty()),
                password: v.get("MAIL_PASSWORD").filter(|v| !v.is_empty()),
                encryption: crate::mail::MailEncryption::parse(
                    &v.or("MAIL_ENCRYPTION", "starttls"),
                )?,
                from_address: v.or("MAIL_FROM_ADDRESS", "hello@example.com"),
                from_name: v.get("MAIL_FROM_NAME").filter(|v| !v.is_empty()),
                timeout: v
                    .seconds("MAIL_TIMEOUT", 10)?
                    .unwrap_or(Duration::from_secs(10)),
                failover: v
                    .or("MAIL_FAILOVER", "")
                    .split(',')
                    .map(|name| name.trim().to_owned())
                    .filter(|name| !name.is_empty())
                    .collect(),
            },
            queue_workers: v
                .or("QUEUE_WORKERS", "2")
                .parse()
                .context("QUEUE_WORKERS must be a number")?,
            webhook_disable_after: v
                .or("WEBHOOK_DISABLE_AFTER", "20")
                .parse()
                .ok()
                .filter(|n| *n > 0)
                .context("WEBHOOK_DISABLE_AFTER must be a number of at least 1")?,
            webhook_tolerance_secs: v
                .or("WEBHOOK_TOLERANCE_SECS", "300")
                .parse()
                .ok()
                .filter(|n| *n > 0)
                .context("WEBHOOK_TOLERANCE_SECS must be a number of at least 1")?,
            idempotency_retention_days: v
                .or("IDEMPOTENCY_RETENTION_DAYS", "7")
                .parse()
                .ok()
                .filter(|n| *n > 0)
                .context("IDEMPOTENCY_RETENTION_DAYS must be a number of at least 1")?,
            scheduler: v.bool("SCHEDULER", true)?,
            log_format: LogFormat::parse(&v.or("LOG_FORMAT", "text"))?,
            log_file: v
                .get("LOG_FILE")
                .filter(|p| !p.is_empty())
                .map(PathBuf::from),
            timezone: v
                .or("APP_TIMEZONE", "UTC")
                .parse()
                .map_err(|err| anyhow::anyhow!("APP_TIMEZONE: {err}"))?,
            cache_store: CacheStore::parse(&v.or("CACHE_STORE", "memory"))?,
            storage_path: v.or("STORAGE_PATH", "storage").into(),
            storage: StorageConfig {
                disk: crate::storage::DiskDriver::parse(&v.or("STORAGE_DISK", "local"))?,
                bucket: v.optional("S3_BUCKET"),
                region: v.optional("S3_REGION"),
                endpoint: v.optional("S3_ENDPOINT"),
                access_key_id: v.optional("S3_ACCESS_KEY_ID"),
                secret_access_key: v.optional("S3_SECRET_ACCESS_KEY"),
                url: v.optional("STORAGE_URL"),
                root: None,
            },
            upload_max_size: v
                .or("UPLOAD_MAX_SIZE", "10")
                .parse::<usize>()
                .ok()
                .and_then(|mb| mb.checked_mul(1024 * 1024))
                .context("UPLOAD_MAX_SIZE must be a number of megabytes")?,
            live_snapshot_max: v
                .or("LIVE_SNAPSHOT_MAX_SIZE", "64")
                .parse::<usize>()
                .ok()
                .and_then(|kb| kb.checked_mul(1024))
                .context("LIVE_SNAPSHOT_MAX_SIZE must be a number of kilobytes")?,
            csp: CspMode::parse(&v.or("CSP", "relaxed"))?,
            permissions_policy: PermissionsMode::parse(&v.or("PERMISSIONS_POLICY", "default"))?,
            trusted_proxies: crate::TrustedProxies::read(&v.or("TRUSTED_PROXIES", ""))?,
            trusted_hosts: v
                .or("TRUSTED_HOSTS", "")
                .split(',')
                .map(|host| host.trim().to_ascii_lowercase())
                .filter(|host| !host.is_empty())
                .collect(),
            vars: Default::default(),
            analytics: AnalyticsConfig {
                google_site_verification: v.optional("GOOGLE_SITE_VERIFICATION"),
                ga4_measurement_id: v.optional("GA4_MEASUREMENT_ID"),
                ga4_api_secret: v.optional("GA4_API_SECRET"),
                gtm_container_id: v.optional("GTM_CONTAINER_ID"),
            },
        })
    }

    /// Any other setting, e.g. an API key: `config.vars`, else the
    /// environment (which includes `.env`). Empty values count as missing.
    pub fn var(&self, name: &str) -> Option<String> {
        self.vars
            .get(name)
            .cloned()
            .or_else(|| env::var(name).ok())
            .filter(|v| !v.is_empty())
    }

    /// The address to listen on: `host` and `port`.
    pub fn addr(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            name: "Renox".into(),
            env: Environment::Local,
            debug: true,
            url: "http://127.0.0.1:3000".into(),
            key: None,
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 3000,
            views_path: "resources/views".into(),
            public_path: "public".into(),
            session_lifetime: Duration::from_secs(120 * 60),
            session_cookie: "renox_session".into(),
            session_driver: SessionDriver::Cookie,
            remember_lifetime: Duration::from_secs(43_200 * 60),
            database_url: "sqlite::memory:".into(),
            database_pool_size: 8,
            // Generous: parallel test suites open many SQLite files at once.
            // (An in-memory database still fails fast; see `db::connect`.)
            database_acquire_timeout: Duration::from_secs(30),
            database_statement_timeout: Some(Duration::from_secs(30)),
            request_timeout: Some(Duration::from_secs(60)),
            locale: "en".into(),
            fallback_locale: "en".into(),
            currency: "USD".into(),
            lang_path: "resources/lang".into(),
            mail: MailConfig {
                mailer: crate::mail::MailDriver::Memory,
                ..MailConfig::default()
            },
            queue_workers: 0,
            webhook_disable_after: 20,
            webhook_tolerance_secs: 300,
            idempotency_retention_days: 7,
            scheduler: false,
            log_format: LogFormat::Text,
            log_file: None,
            timezone: crate::timezone::Zone::default(),
            cache_store: CacheStore::Memory,
            storage_path: "storage".into(),
            storage: StorageConfig::default(),
            upload_max_size: 10 * 1024 * 1024,
            live_snapshot_max: 64 * 1024,
            csp: CspMode::Relaxed,
            permissions_policy: PermissionsMode::Default,
            vars: Default::default(),
            analytics: AnalyticsConfig::default(),
            trusted_proxies: Default::default(),
            trusted_hosts: Vec::new(),
        }
    }
}

/// Variables by name, for `Config::from_vars`.
struct Vars<F>(F);

impl<F: Fn(&str) -> Option<String>> Vars<F> {
    fn get(&self, name: &str) -> Option<String> {
        (self.0)(name)
    }

    fn optional(&self, name: &str) -> Option<String> {
        self.get(name).filter(|v| !v.is_empty())
    }

    fn or(&self, name: &str, default: &str) -> String {
        self.get(name).unwrap_or_else(|| default.to_owned())
    }

    /// A number of seconds from `name`; `None` when it is 0 (no limit).
    fn seconds(&self, name: &str, default: u64) -> anyhow::Result<Option<Duration>> {
        let value: u64 = self
            .or(name, &default.to_string())
            .parse()
            .with_context(|| format!("{name} must be a number of seconds"))?;
        Ok((value > 0).then(|| Duration::from_secs(value)))
    }

    fn bool(&self, name: &str, default: bool) -> anyhow::Result<bool> {
        match self.get(name) {
            None => Ok(default),
            Some(v) => match v.to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => Ok(true),
                "0" | "false" | "no" | "off" | "" => Ok(false),
                other => bail!("{name} must be true or false, got `{other}`"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_environment_aliases() {
        assert_eq!(Environment::parse("local").unwrap(), Environment::Local);
        assert_eq!(Environment::parse("DEV").unwrap(), Environment::Local);
        assert_eq!(Environment::parse("test").unwrap(), Environment::Testing);
        assert_eq!(Environment::parse("prod").unwrap(), Environment::Production);
        assert!(Environment::parse("staging").is_err());
    }

    fn load(pairs: &[(&str, &str)]) -> anyhow::Result<Config> {
        let vars: std::collections::HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Config::read(|name| vars.get(name).cloned())
    }

    #[test]
    fn defaults_without_any_variable() {
        let c = load(&[]).unwrap();
        assert_eq!(c.env, Environment::Local);
        assert!(c.debug, "local defaults to debug");
        assert_eq!(
            (c.port, c.database_pool_size, c.upload_max_size),
            (3000, 8, 10 * 1024 * 1024)
        );
        assert_eq!(c.database_acquire_timeout, Duration::from_secs(5));
        assert_eq!(c.request_timeout, Some(Duration::from_secs(60)));
        assert_eq!(c.cache_store, CacheStore::Memory);
        assert_eq!(c.csp, CspMode::Relaxed);
        assert_eq!(c.permissions_policy, PermissionsMode::Default);
        assert!(c.key.is_none() && c.mail.port.is_none());
    }

    #[test]
    fn debug_hides_secrets() {
        let mut c = load(&[("APP_KEY", "base64:c2VjcmV0"), ("MAIL_PASSWORD", "hunter2")]).unwrap();
        c.vars
            .insert("XENDIT_SECRET_KEY".into(), "xnd_live_123".into());
        let shown = format!("{c:?}");
        for secret in ["c2VjcmV0", "hunter2", "xnd_live_123"] {
            assert!(!shown.contains(secret), "{secret} in {shown}");
        }
        assert!(
            shown.contains("XENDIT_SECRET_KEY"),
            "variable names stay: {shown}"
        );
        assert!(shown.contains("[hidden]"));
    }

    #[test]
    fn session_driver() {
        assert_eq!(load(&[]).unwrap().session_driver, SessionDriver::Cookie);
        assert_eq!(
            load(&[("SESSION_DRIVER", "database")])
                .unwrap()
                .session_driver,
            SessionDriver::Database
        );
        assert!(load(&[("SESSION_DRIVER", "redis")]).is_err());
    }

    #[test]
    fn log_format_and_file() {
        let c = load(&[]).unwrap();
        assert_eq!((c.log_format.as_str(), c.log_file), ("text", None));
        let c = load(&[("LOG_FORMAT", "json"), ("LOG_FILE", "storage/logs/app.log")]).unwrap();
        assert_eq!(c.log_format, LogFormat::Json);
        assert_eq!(c.log_file, Some(PathBuf::from("storage/logs/app.log")));
        let err = load(&[("LOG_FORMAT", "xml")]).unwrap_err();
        assert!(err.to_string().contains("LOG_FORMAT"), "{err}");
    }

    #[test]
    fn currency() {
        assert_eq!(load(&[]).unwrap().currency, "USD");
        assert_eq!(load(&[("APP_CURRENCY", " idr ")]).unwrap().currency, "IDR");
        for bad in ["US", "rupiah", "U$D"] {
            let err = load(&[("APP_CURRENCY", bad)]).unwrap_err();
            assert!(err.to_string().contains("APP_CURRENCY"), "{err}");
        }
    }

    #[test]
    fn reads_every_kind_of_value() {
        let c = load(&[
            ("APP_ENV", "production"),
            ("APP_KEY", "base64:abc"),
            ("APP_DEBUG", "off"),
            ("APP_PORT", "8080"),
            ("DATABASE_POOL_SIZE", "3"),
            ("REQUEST_TIMEOUT", "0"),
            ("DATABASE_STATEMENT_TIMEOUT", "5"),
            ("UPLOAD_MAX_SIZE", "2"),
            ("LIVE_SNAPSHOT_MAX_SIZE", "2"),
            ("MAIL_PORT", "2525"),
            ("MAIL_USERNAME", ""),
            ("MAIL_FAILOVER", "backup, log"),
            ("CSP", "strict"),
            ("PERMISSIONS_POLICY", "geolocation=(self), camera=()"),
            ("TRUSTED_PROXIES", "10.0.0.0/8"),
            ("TRUSTED_HOSTS", "shop.example.com, *.Example.org"),
            ("S3_BUCKET", "files"),
        ])
        .unwrap();
        assert_eq!(c.env, Environment::Production);
        assert!(!c.debug);
        assert_eq!((c.port, c.database_pool_size), (8080, 3));
        assert_eq!(c.request_timeout, None, "0 means no limit");
        assert_eq!(c.database_statement_timeout, Some(Duration::from_secs(5)));
        assert_eq!(c.upload_max_size, 2 * 1024 * 1024);
        assert_eq!(c.live_snapshot_max, 2048);
        assert_eq!(c.mail.port, Some(2525));
        assert_eq!(c.mail.username, None, "empty means unset");
        assert_eq!(c.mail.failover, ["backup", "log"]);
        assert_eq!(c.csp, CspMode::Strict);
        assert_eq!(
            c.permissions_policy,
            PermissionsMode::Custom("geolocation=(self), camera=()".into())
        );
        assert!(c.trusted_proxies.contains("10.1.2.3".parse().unwrap()));
        assert_eq!(c.trusted_hosts, ["shop.example.com", "*.example.org"]);
        assert_eq!(c.storage.bucket.as_deref(), Some("files"));
    }

    #[test]
    fn permissions_policy_takes_default_off_or_a_header_value() {
        for (value, want) in [
            ("", PermissionsMode::Default),
            ("DEFAULT", PermissionsMode::Default),
            ("off", PermissionsMode::Off),
            ("false", PermissionsMode::Off),
            (
                "camera=(self \"https://a.example\"), payment=*",
                PermissionsMode::Custom("camera=(self \"https://a.example\"), payment=*".into()),
            ),
        ] {
            let c = load(&[("PERMISSIONS_POLICY", value)]).unwrap();
            assert_eq!(c.permissions_policy, want, "{value}");
        }
        // `CSP=off` leaves it alone.
        let c = load(&[("CSP", "off")]).unwrap();
        assert_eq!(c.permissions_policy, PermissionsMode::Default);
    }

    #[test]
    fn refuses_bad_values_with_the_variable_name() {
        for (name, value) in [
            ("APP_ENV", "staging"),
            ("APP_DEBUG", "maybe"),
            ("APP_PORT", "http"),
            ("APP_HOST", "localhost"),
            ("DATABASE_POOL_SIZE", "0"),
            ("REQUEST_TIMEOUT", "-1"),
            ("UPLOAD_MAX_SIZE", "99999999999999999"),
            ("MAIL_PORT", "99999"),
            ("CSP", "loose"),
            ("PERMISSIONS_POLICY", "camera"),
            ("PERMISSIONS_POLICY", "Camera=(self)"),
            ("PERMISSIONS_POLICY", "camera=(http://x.example)"),
            ("TRUSTED_PROXIES", "proxy.local"),
            ("SESSION_DRIVER", "redis"),
            ("CACHE_STORE", "redis"),
            ("LOG_FORMAT", "xml"),
            ("MAIL_MAILER", "sendgrid"),
            ("MAIL_ENCRYPTION", "ssl3"),
            ("STORAGE_DISK", "ftp"),
            ("APP_TIMEZONE", "Mars/Olympus"),
        ] {
            let err = load(&[(name, value)])
                .err()
                .map(|e| format!("{e:#}"))
                .unwrap_or_default();
            assert!(
                err.contains(name) || err.contains(value),
                "{name}={value}: {err}"
            );
        }
        let err = load(&[("APP_ENV", "production")]).unwrap_err();
        assert!(
            format!("{err}").contains("APP_KEY"),
            "production needs a key"
        );
    }

    /// Settings print and serialize as the word `.env` takes.
    #[test]
    fn settings_print_and_serialize_as_their_word() {
        assert_eq!(SessionDriver::Database.to_string(), "database");
        assert_eq!(
            serde_json::to_string(&CacheStore::Database).unwrap(),
            r#""database""#
        );
        assert_eq!(format!("{}", LogFormat::Json), "json");
    }
}
