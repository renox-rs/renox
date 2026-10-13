//! Tasks that run on a schedule, defined in code and run by `serve` (or
//! `my-app schedule:work`).
//!
//! ```
//! # use renox::prelude::*;
//! use renox::chrono::Weekday;
//! # async fn sync(_: &AppState) -> Result { Ok(()) }
//! # async fn report(_: &AppState) -> Result { Ok(()) }
//! # let _ =
//! App::new().schedule(|s| {
//!     s.every_minutes(5, "sync-stock", |state| async move { sync(&state).await })
//!         .weekdays()
//!         .between("08:00", "17:00");
//!     s.daily_at("02:00", "cleanup", |state| async move {
//!         renox::db::sql("DELETE FROM carts WHERE updated_at < ?")
//!             .bind(renox::db::now() - renox::chrono::TimeDelta::days(30))
//!             .execute(&state.db)
//!             .await?;
//!         Ok(())
//!     });
//!     s.cron("30 9 * * 1-5", "standup", |state| async move { report(&state).await })
//!         .timezone("Europe/Amsterdam")
//!         .on_failure(|err, _state| async move {
//!             tracing::error!(error = ?err, "standup report failed");
//!         });
//!     s.weekly_on(Weekday::Mon, "07:00", "weekly-report", |state| async move {
//!         report(&state).await
//!     });
//!     s.monthly_on(1, "00:05", "invoices", |state| async move { report(&state).await });
//! })
//! # ;
//! ```
//!
//! Times are in `APP_TIMEZONE` (an IANA name such as `Asia/Jakarta`, an
//! offset such as `+07:00`, or UTC), or in a task's own `timezone`. With
//! daylight saving time, a run in the hour that clocks skip happens right
//! after the jump, and one in the hour that repeats happens once. A task
//! whose previous run hasn't finished is skipped rather than run twice.
//!
//! Several processes may run the same schedule (e.g. `serve` on two servers
//! sharing a PostgreSQL database): each run is claimed in the `cache` table
//! first, so only one of them runs it. `my-app schedule:run NAME` runs one
//! task now, `schedule:list` shows when each runs next.

use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use chrono::{Datelike, NaiveDateTime, TimeDelta, Timelike, Weekday};
use tokio::sync::watch;

use crate::queue::unix_now;
use crate::timezone::Zone;
use crate::{AppState, Error, Result};

type TaskFn = Arc<dyn Fn(AppState) -> Pin<Box<dyn Future<Output = Result> + Send>> + Send + Sync>;
type FailFn =
    Arc<dyn Fn(AppState, Error) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;
type DoneFn = Arc<dyn Fn(AppState) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

const PING_BEFORE: usize = 0;
const PING_AFTER: usize = 1;
const PING_SUCCESS: usize = 2;
const PING_FAILURE: usize = 3;

/// When a run is "never" (an impossible schedule, or filters that exclude
/// every run).
const NEVER: i64 = i64::MAX;

#[derive(Debug, Clone, PartialEq)]
enum When {
    /// Every `n` seconds, on multiples of `n` since midnight in the zone.
    Every(i64),
    /// When the wall clock matches a cron expression.
    Cron(Box<Cron>),
}

impl When {
    /// How long a run's claim is kept: past the slot, so a process whose
    /// clock is a little behind doesn't run it again, and no longer.
    fn claim_for(&self) -> i64 {
        match self {
            When::Every(seconds) => seconds.max(&60) + 60,
            When::Cron(_) => 60 * 60,
        }
    }
}

/// Clears a task's `running` flag when dropped.
struct Running(Arc<AtomicBool>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[derive(Clone)]
struct Task {
    name: String,
    when: When,
    run: TaskFn,
    running: Arc<AtomicBool>,
    zone: Option<Zone>,
    /// Weekdays it may run on, a bit per `Weekday::num_days_from_monday`.
    days: u8,
    /// Minutes of the day it may run between (inclusive; may wrap midnight).
    between: Option<(u32, u32)>,
    on_failure: Option<FailFn>,
    on_success: Option<DoneFn>,
    /// URLs to GET: before a run, after it, after a success, after a failure.
    pings: [Vec<String>; 4],
}

impl Task {
    fn allows(&self, local: NaiveDateTime) -> bool {
        if self.days & (1 << local.weekday().num_days_from_monday()) == 0 {
            return false;
        }
        match self.between {
            None => true,
            Some((from, to)) => {
                let minute = local.hour() * 60 + local.minute();
                if from <= to {
                    (from..=to).contains(&minute)
                } else {
                    minute >= from || minute <= to
                }
            }
        }
    }

    /// The first run after `now`, in unix seconds.
    fn next_run(&self, now: i64, app_zone: Zone) -> i64 {
        if self.days == 0 {
            return NEVER;
        }
        let zone = self.zone.unwrap_or(app_zone);
        let mut after = now;
        // Filters skip runs one by one: a minute's interval over a weekend is
        // ~2,900 steps; give up (never) past a few years of them.
        for _ in 0..2_000_000 {
            let at = match &self.when {
                When::Every(n) => {
                    let offset = zone.offset_at(after);
                    ((after + offset).div_euclid(*n) + 1) * n - offset
                }
                When::Cron(cron) => match cron_next(cron, after, zone) {
                    Some(at) => at,
                    None => return NEVER,
                },
            };
            if self.allows(zone.local(at)) {
                return at;
            }
            after = at;
        }
        NEVER
    }

    /// GETs the `which` ping URLs (health checks); failures are logged.
    async fn ping(&self, state: &AppState, which: usize) {
        for url in &self.pings[which] {
            let sent = state
                .http
                .get(url)
                .timeout(Duration::from_secs(10))
                .retry(1, Duration::from_secs(1))
                .send()
                .await;
            match sent {
                Ok(res) if res.ok() => {}
                Ok(res) => {
                    tracing::warn!(task = %self.name, url, status = %res.status(), "schedule ping answered with an error")
                }
                Err(err) => {
                    tracing::warn!(task = %self.name, url, error = ?err, "schedule ping failed")
                }
            }
        }
    }

    /// Runs the task once, with its hooks and pings, in a fresh context.
    async fn execute(&self, state: AppState) -> Result {
        self.ping(&state, PING_BEFORE).await;
        let outcome = self.execute_inner(state.clone()).await;
        self.ping(&state, PING_AFTER).await;
        let which = if outcome.is_ok() {
            PING_SUCCESS
        } else {
            PING_FAILURE
        };
        self.ping(&state, which).await;
        outcome
    }

    async fn execute_inner(&self, state: AppState) -> Result {
        let run = crate::context::scope_app(state.clone(), (self.run)(state.clone()));
        let run = std::panic::AssertUnwindSafe(run);
        let outcome = match futures_util::FutureExt::catch_unwind(run).await {
            Ok(outcome) => outcome,
            Err(_) => Err(anyhow!("scheduled task `{}` panicked", self.name).into()),
        };
        match outcome {
            Ok(()) => {
                if let Some(done) = &self.on_success {
                    crate::context::scope_app(state.clone(), done(state)).await;
                }
                Ok(())
            }
            Err(err) => {
                tracing::error!(task = %self.name, error = ?err, "scheduled task failed");
                let report = crate::report::ErrorReport::new(
                    &state,
                    crate::report::ReportKind::ScheduledTask,
                    format!("{err:?}")
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                    format!("{err:?}"),
                    Some(self.name.clone()),
                );
                crate::report::send(&state, report);
                if let Some(failed) = &self.on_failure {
                    let message = format!("{err:?}");
                    crate::context::scope_app(state.clone(), failed(state, err)).await;
                    return Err(anyhow!(message).into());
                }
                Err(err)
            }
        }
    }
}

/// The app's scheduled tasks.
#[derive(Clone, Default)]
pub struct Schedule {
    tasks: Vec<Task>,
    error: Option<String>,
}

/// A task just added to the [`Schedule`], to narrow when it runs or add
/// hooks. It derefs to the schedule, so adding tasks can go on in a chain.
pub struct ScheduledTask<'a> {
    schedule: &'a mut Schedule,
    /// `None` when the task couldn't be added (the error is kept).
    index: Option<usize>,
}

impl Deref for ScheduledTask<'_> {
    type Target = Schedule;

    fn deref(&self) -> &Schedule {
        self.schedule
    }
}

impl DerefMut for ScheduledTask<'_> {
    fn deref_mut(&mut self) -> &mut Schedule {
        self.schedule
    }
}

impl ScheduledTask<'_> {
    fn edit(self, change: impl FnOnce(&mut Task) -> anyhow::Result<()>) -> Self {
        if let Some(index) = self.index {
            let task = &mut self.schedule.tasks[index];
            if let Err(err) = change(task) {
                let name = task.name.clone();
                self.schedule
                    .error
                    .get_or_insert(format!("task `{name}`: {err}"));
            }
        }
        self
    }

    /// Only Monday to Friday.
    pub fn weekdays(self) -> Self {
        self.edit(|t| {
            t.days &= 0b0001_1111;
            Ok(())
        })
    }

    /// Only Saturday and Sunday.
    pub fn weekends(self) -> Self {
        self.edit(|t| {
            t.days &= 0b0110_0000;
            Ok(())
        })
    }

    /// Only on these days.
    pub fn days(self, days: &[Weekday]) -> Self {
        let mask = days
            .iter()
            .fold(0u8, |mask, day| mask | 1 << day.num_days_from_monday());
        self.edit(|t| {
            t.days &= mask;
            Ok(())
        })
    }

    /// Only between two times of day (`HH:MM`, both included); `"22:00"` to
    /// `"06:00"` spans midnight.
    pub fn between(self, from: &str, to: &str) -> Self {
        let window = parse_time(from).and_then(|from| Ok((from, parse_time(to)?)));
        self.edit(|t| {
            let (from, to) = window?;
            t.between = Some(((from / 60) as u32, (to / 60) as u32));
            Ok(())
        })
    }

    /// Runs in this time zone instead of `APP_TIMEZONE` (`Asia/Jakarta`,
    /// `+07:00`, `UTC`).
    pub fn timezone(self, zone: &str) -> Self {
        let zone = zone.parse::<Zone>();
        self.edit(|t| {
            t.zone = Some(zone?);
            Ok(())
        })
    }

    /// Runs after a run fails or panics, with the error (e.g. to alert
    /// someone). The failure is logged either way.
    pub fn on_failure<F, Fut>(self, hook: F) -> Self
    where
        F: Fn(Error, AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let hook: FailFn = Arc::new(move |state, err| Box::pin(hook(err, state)));
        self.edit(|t| {
            t.on_failure = Some(hook);
            Ok(())
        })
    }

    /// GETs `url` before each run, e.g. a health check's "start" URL
    /// (Healthchecks.io, Cronitor, Better Stack). A failing ping is logged,
    /// never stops the task.
    pub fn ping_before(self, url: impl Into<String>) -> Self {
        self.ping(PING_BEFORE, url.into())
    }

    /// GETs `url` after each run, however it went.
    pub fn then_ping(self, url: impl Into<String>) -> Self {
        self.ping(PING_AFTER, url.into())
    }

    /// GETs `url` after a successful run.
    pub fn ping_on_success(self, url: impl Into<String>) -> Self {
        self.ping(PING_SUCCESS, url.into())
    }

    /// GETs `url` after a failed run.
    pub fn ping_on_failure(self, url: impl Into<String>) -> Self {
        self.ping(PING_FAILURE, url.into())
    }

    fn ping(self, which: usize, url: String) -> Self {
        self.edit(|t| {
            t.pings[which].push(url);
            Ok(())
        })
    }

    /// Runs after a run succeeds.
    pub fn on_success<F, Fut>(self, hook: F) -> Self
    where
        F: Fn(AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let hook: DoneFn = Arc::new(move |state| Box::pin(hook(state)));
        self.edit(|t| {
            t.on_success = Some(hook);
            Ok(())
        })
    }
}

impl Schedule {
    fn add<F, Fut>(&mut self, name: &str, when: anyhow::Result<When>, task: F) -> ScheduledTask<'_>
    where
        F: Fn(AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        let when = match when {
            Ok(when) => when,
            Err(err) => {
                self.error.get_or_insert(format!("task `{name}`: {err}"));
                return ScheduledTask {
                    schedule: self,
                    index: None,
                };
            }
        };
        if self.tasks.iter().any(|t| t.name == name) {
            self.error
                .get_or_insert(format!("task `{name}` is scheduled twice"));
        }
        self.tasks.push(Task {
            name: name.to_owned(),
            when,
            run: Arc::new(move |state| Box::pin(task(state))),
            running: Arc::new(AtomicBool::new(false)),
            zone: None,
            days: 0b0111_1111,
            between: None,
            on_failure: None,
            on_success: None,
            pings: Default::default(),
        });
        let index = Some(self.tasks.len() - 1);
        ScheduledTask {
            schedule: self,
            index,
        }
    }

    /// Every `interval`, aligned to the clock (every 5 minutes runs at :00, :05, ...).
    pub fn every<F, Fut>(&mut self, interval: Duration, name: &str, task: F) -> ScheduledTask<'_>
    where
        F: Fn(AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        let seconds = interval.as_secs().max(1) as i64;
        self.add(name, Ok(When::Every(seconds)), task)
    }

    /// Every minute, on the minute.
    pub fn every_minute<F, Fut>(&mut self, name: &str, task: F) -> ScheduledTask<'_>
    where
        F: Fn(AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        self.every(Duration::from_secs(60), name, task)
    }

    /// Every `minutes` minutes, aligned to the clock (every 15 runs at :00, :15, ...).
    pub fn every_minutes<F, Fut>(&mut self, minutes: u64, name: &str, task: F) -> ScheduledTask<'_>
    where
        F: Fn(AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        self.every(Duration::from_secs(minutes * 60), name, task)
    }

    /// Every hour, on the hour.
    pub fn hourly<F, Fut>(&mut self, name: &str, task: F) -> ScheduledTask<'_>
    where
        F: Fn(AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        self.every(Duration::from_secs(3600), name, task)
    }

    /// When the wall clock matches a cron expression: minute, hour, day of
    /// month, month, day of week (`0`/`7` or `SUN`), with `*`, `a-b`, lists
    /// and `/step`; also `@hourly`, `@daily`, `@weekly`, `@monthly`,
    /// `@yearly`. `"30 9 * * 1-5"` is 09:30 on weekdays.
    pub fn cron<F, Fut>(&mut self, expression: &str, name: &str, task: F) -> ScheduledTask<'_>
    where
        F: Fn(AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        let when = Cron::parse(expression).map(|c| When::Cron(Box::new(c)));
        self.add(name, when, task)
    }

    /// Every day at `HH:MM`.
    pub fn daily_at<F, Fut>(&mut self, time: &str, name: &str, task: F) -> ScheduledTask<'_>
    where
        F: Fn(AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        let when = at_time(time, "*", "*");
        self.add(name, when, task)
    }

    /// Every week on `day` at `HH:MM`.
    pub fn weekly_on<F, Fut>(
        &mut self,
        day: Weekday,
        time: &str,
        name: &str,
        task: F,
    ) -> ScheduledTask<'_>
    where
        F: Fn(AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        let when = at_time(time, "*", &day.num_days_from_sunday().to_string());
        self.add(name, when, task)
    }

    /// Every month on `day` (1–31; months without that day are skipped, so
    /// use 28 or lower for every month) at `HH:MM`.
    pub fn monthly_on<F, Fut>(
        &mut self,
        day: u32,
        time: &str,
        name: &str,
        task: F,
    ) -> ScheduledTask<'_>
    where
        F: Fn(AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        let when = at_time(time, &day.to_string(), "*");
        self.add(name, when, task)
    }

    pub(crate) fn check(&self) -> anyhow::Result<()> {
        match &self.error {
            Some(error) => bail!("invalid schedule: {error}"),
            None => Ok(()),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
}

/// A scheduled task's next run, from [`Schedule::upcoming`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct UpcomingRun {
    /// The task's name.
    pub name: String,
    /// When it runs next; `None` if it never will (a date that can't come).
    pub at: Option<crate::db::DateTime>,
    /// The zone its times are in.
    pub zone: Zone,
}

impl Schedule {
    /// Each task with its next run (in `zone` unless the task has its own),
    /// as `schedule:list` shows them.
    pub fn upcoming(&self, zone: Zone) -> Vec<UpcomingRun> {
        let now = unix_now();
        self.tasks
            .iter()
            .map(|t| {
                let at = t.next_run(now, zone);
                UpcomingRun {
                    name: t.name.clone(),
                    at: (at != i64::MAX).then(|| crate::db::from_unix(at)),
                    zone: t.zone.unwrap_or(zone),
                }
            })
            .collect()
    }

    /// Runs the task `name` now, whatever its schedule (`schedule:run`).
    pub async fn run_now(&self, state: AppState, name: &str) -> Result {
        let task = self
            .tasks
            .iter()
            .find(|t| t.name == name)
            .ok_or_else(|| Error::from(anyhow!("no scheduled task `{name}`")))?;
        task.execute(state).await
    }

    /// Runs tasks as they fall due until `shutdown` flips to true.
    pub(crate) async fn run(
        self,
        state: AppState,
        zone: Zone,
        mut shutdown: watch::Receiver<bool>,
    ) {
        let mut next: Vec<i64> = self
            .tasks
            .iter()
            .map(|t| t.next_run(unix_now(), zone))
            .collect();
        loop {
            let Some(soonest) = next.iter().min().copied() else {
                return;
            };
            // A task that never runs still wakes the loop once a day, harmlessly.
            let wait = (soonest.min(unix_now() + 86_400) - unix_now()).max(0);
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(wait as u64)) => {}
                _ = shutdown.changed() => return,
            }
            let now = unix_now();
            for (task, at) in self.tasks.iter().zip(next.iter_mut()) {
                if *at > now {
                    continue;
                }
                let slot = *at;
                *at = task.next_run(now, zone);
                if task.running.swap(true, Ordering::SeqCst) {
                    tracing::warn!(task = %task.name, "skipped: the previous run is still going");
                    continue;
                }
                let (task, state) = (task.clone(), state.clone());
                tokio::spawn(crate::clock::carry(async move {
                    // Clears `running` however the run ends, panics included.
                    let _running = Running(task.running.clone());
                    if !claim(&state, &task.name, slot, task.when.claim_for()).await {
                        tracing::debug!(task = %task.name, "skipped: another process runs it");
                        return;
                    }
                    tracing::info!(task = %task.name, "scheduled task started");
                    let _ = task.execute(state).await;
                }));
            }
        }
    }
}

/// Claims the run of `task` due at `slot`, so that processes sharing the
/// database run it once: the first to insert the claim wins. The claim is
/// kept `keep` seconds. If the database can't be reached the task runs
/// anyway, as a single process would.
#[doc(hidden)]
pub async fn claim(state: &AppState, task: &str, slot: i64, keep: i64) -> bool {
    let now = unix_now();
    prune_claims(state, now).await;
    let claimed = crate::db::sql(
        "INSERT INTO cache (key, value, expires_at) VALUES (?, 'null', ?) \
         ON CONFLICT (key) DO NOTHING",
    )
    .bind(format!("{CLAIM_PREFIX}{task}:{slot}"))
    .bind(now + keep)
    .execute(&state.db)
    .await;
    match claimed {
        Ok(rows) => rows == 1,
        Err(err) => {
            tracing::warn!(task, error = %err, "could not claim the run; running it anyway");
            true
        }
    }
}

/// Framework rows in the `cache` table start with `renox:`; `Cache::flush` keeps them.
const CLAIM_PREFIX: &str = "renox:schedule:";

/// Deletes expired claims, at most once a minute per process.
async fn prune_claims(state: &AppState, now: i64) {
    static LAST: AtomicI64 = AtomicI64::new(0);
    let last = LAST.load(Ordering::Relaxed);
    if now - last < 60
        || LAST
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
    {
        return;
    }
    delete_expired_claims(state, now).await;
}

/// Deletes the claims expired at `now`; a failure is logged.
async fn delete_expired_claims(state: &AppState, now: i64) {
    let pruned = crate::db::sql("DELETE FROM cache WHERE key LIKE ? AND expires_at < ?")
        .bind(format!("{CLAIM_PREFIX}%"))
        .bind(now)
        .execute(&state.db)
        .await;
    if let Err(err) = pruned {
        tracing::warn!(error = %err, "could not clear old schedule claims");
    }
}

fn parse_time(time: &str) -> anyhow::Result<i64> {
    let (hour, minute) = time.split_once(':').context("expected HH:MM")?;
    let (hour, minute): (i64, i64) = (hour.trim().parse()?, minute.trim().parse()?);
    if !(0..24).contains(&hour) || !(0..60).contains(&minute) {
        bail!("`{time}` is not a time of day");
    }
    Ok(hour * 3600 + minute * 60)
}

/// A cron schedule at `HH:MM` on these days of the month and week.
fn at_time(time: &str, day_of_month: &str, day_of_week: &str) -> anyhow::Result<When> {
    let seconds = parse_time(time)?;
    let expression = format!(
        "{} {} {day_of_month} * {day_of_week}",
        seconds % 3600 / 60,
        seconds / 3600
    );
    Ok(When::Cron(Box::new(Cron::parse(&expression)?)))
}

/// A parsed cron expression: a bit per allowed value of each field.
#[derive(Debug, Clone, PartialEq)]
struct Cron {
    minutes: u64,
    hours: u32,
    days: u32,
    months: u16,
    weekdays: u8,
    /// Day of month and day of week both restricted: either may match, as in
    /// every cron.
    either_day: bool,
}

const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];
const WEEKDAYS: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];

impl Cron {
    fn parse(expression: &str) -> anyhow::Result<Self> {
        let expression = match expression.trim() {
            "@hourly" => "0 * * * *",
            "@daily" | "@midnight" => "0 0 * * *",
            "@weekly" => "0 0 * * 0",
            "@monthly" => "0 0 1 * *",
            "@yearly" | "@annually" => "0 0 1 1 *",
            other => other,
        };
        let fields: Vec<&str> = expression.split_whitespace().collect();
        let [minute, hour, day, month, weekday] = fields[..] else {
            bail!("`{expression}` needs 5 fields: minute hour day-of-month month day-of-week");
        };
        let weekdays = field(weekday, 0, 7, &WEEKDAYS, 0)?;
        let cron = Cron {
            minutes: field(minute, 0, 59, &[], 0)?,
            hours: field(hour, 0, 23, &[], 0)? as u32,
            days: field(day, 1, 31, &[], 0)? as u32,
            months: field(month, 1, 12, &MONTHS, 1)? as u16,
            // 7 is Sunday too.
            weekdays: ((weekdays | weekdays >> 7) & 0x7f) as u8,
            either_day: !day.starts_with('*') && !weekday.starts_with('*'),
        };
        let probe = NaiveDateTime::default();
        if cron.next_after(probe).is_none() {
            bail!("`{expression}` never matches a date");
        }
        Ok(cron)
    }

    fn day_matches(&self, at: NaiveDateTime) -> bool {
        let day = self.days & (1 << at.day()) != 0;
        let weekday = self.weekdays & (1 << at.weekday().num_days_from_sunday()) != 0;
        if self.either_day {
            day || weekday
        } else {
            day && weekday
        }
    }

    /// The first matching wall-clock minute after `after`.
    fn next_after(&self, after: NaiveDateTime) -> Option<NaiveDateTime> {
        let mut at = after.with_second(0)?.with_nanosecond(0)? + TimeDelta::minutes(1);
        // Feb 29 on a Monday, say, can be decades away; stop after 30 years.
        let limit = at + TimeDelta::days(366 * 30);
        while at <= limit {
            let midnight = at.date().and_hms_opt(0, 0, 0)?;
            if self.months & (1 << at.month()) == 0 {
                let (year, month) = if at.month() == 12 {
                    (at.year() + 1, 1)
                } else {
                    (at.year(), at.month() + 1)
                };
                at = chrono::NaiveDate::from_ymd_opt(year, month, 1)?.and_hms_opt(0, 0, 0)?;
            } else if !self.day_matches(at) {
                at = midnight + TimeDelta::days(1);
            } else if self.hours & (1 << at.hour()) == 0 {
                at = at.with_minute(0)? + TimeDelta::hours(1);
            } else if self.minutes & (1 << at.minute()) == 0 {
                at += TimeDelta::minutes(1);
            } else {
                return Some(at);
            }
        }
        None
    }
}

/// One cron field as a bit set over `low..=high`. `names` are accepted for
/// values from `first_name` on (`JAN` = 1, `SUN` = 0).
fn field(text: &str, low: u32, high: u32, names: &[&str], first_name: u32) -> anyhow::Result<u64> {
    let value = |part: &str| -> anyhow::Result<u32> {
        let upper = part.to_ascii_uppercase();
        if let Some(i) = names.iter().position(|n| *n == upper) {
            return Ok(i as u32 + first_name);
        }
        let n: u32 = part
            .parse()
            .with_context(|| format!("`{part}` is not a number in `{text}`"))?;
        if !(low..=high).contains(&n) {
            bail!("`{n}` is out of range {low}-{high} in `{text}`");
        }
        Ok(n)
    };
    let mut bits = 0u64;
    for part in text.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((range, step)) => {
                let step: u32 = step
                    .parse()
                    .with_context(|| format!("bad step in `{text}`"))?;
                if step == 0 {
                    bail!("a step of 0 in `{text}`");
                }
                (range, step)
            }
            None => (part, 1),
        };
        let (from, to) = if range == "*" {
            (low, high)
        } else if let Some((from, to)) = range.split_once('-') {
            (value(from)?, value(to)?)
        } else {
            let from = value(range)?;
            (from, if part.contains('/') { high } else { from })
        };
        if from > to {
            bail!("`{range}` runs backwards in `{text}`");
        }
        for n in (from..=to).step_by(step as usize) {
            bits |= 1 << n;
        }
    }
    Ok(bits)
}

/// The first moment after `after` matching `cron` in `zone`. A wall-clock
/// time that doesn't happen (the hour skipped in spring) runs right after
/// the jump; one that happens twice (autumn) runs the first time.
fn cron_next(cron: &Cron, after: i64, zone: Zone) -> Option<i64> {
    let mut local = zone.local(after);
    loop {
        local = cron.next_after(local)?;
        if let Some(at) = zone.resolve(local) {
            if at > after {
                return Some(at);
            }
            // The repeated hour's second pass: its first was already past.
            continue;
        }
        // Skipped by daylight saving time: the first minute after the gap.
        let at = (1..=180).find_map(|m| zone.resolve(local + TimeDelta::minutes(m)))?;
        if at > after {
            return Some(at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-09-27 10:07:30 UTC, a Sunday.
    const NOW: i64 = 1_790_503_650;

    fn zone(name: &str) -> Zone {
        name.parse().unwrap()
    }

    fn build(define: impl FnOnce(&mut Schedule)) -> Task {
        let mut schedule = Schedule::default();
        define(&mut schedule);
        schedule.check().unwrap();
        schedule.tasks.pop().unwrap()
    }

    fn local(at: i64, zone: Zone) -> String {
        zone.local(at).format("%a %Y-%m-%d %H:%M").to_string()
    }

    #[test]
    fn intervals_align_to_the_clock() {
        let task = build(|s| {
            s.every_minutes(5, "a", |_| async { Ok(()) });
        });
        let at = task.next_run(NOW, Zone::UTC);
        assert_eq!((at % 300, at - NOW), (0, 150));
        let wib = zone("+07:00");
        assert_eq!(local(task.next_run(NOW, wib), wib), "Sun 2026-09-27 17:10");
    }

    #[test]
    fn daily_times_use_the_zone() {
        let task = build(|s| {
            s.daily_at("02:00", "a", |_| async { Ok(()) });
        });
        let jakarta = zone("Asia/Jakarta");
        // 10:07 UTC is 17:07 in Jakarta; the next 02:00 there is 19:00 UTC today.
        let at = task.next_run(NOW, jakarta);
        assert_eq!(local(at, jakarta), "Mon 2026-09-28 02:00");
        assert_eq!(at - NOW, 8 * 3600 + 52 * 60 + 30);
        let ny = zone("-05:00");
        assert_eq!(local(task.next_run(NOW, ny), ny), "Mon 2026-09-28 02:00");
        // UTC+14 is already Monday.
        let kiribati = zone("+14:00");
        let soon = build(|s| {
            s.daily_at("00:30", "a", |_| async { Ok(()) });
        })
        .next_run(NOW, kiribati);
        assert_eq!(soon - NOW, 22 * 60 + 30);
        // A run due exactly now is the next day's.
        let midnight = NOW - NOW.rem_euclid(86_400);
        let task = build(|s| {
            s.daily_at("00:00", "a", |_| async { Ok(()) });
        });
        assert_eq!(task.next_run(midnight, Zone::UTC), midnight + 86_400);
    }

    #[test]
    fn cron_expressions() {
        let next = |expression: &str| {
            let task = build(|s| {
                s.cron(expression, "a", |_| async { Ok(()) });
            });
            local(task.next_run(NOW, Zone::UTC), Zone::UTC)
        };
        assert_eq!(next("30 9 * * 1-5"), "Mon 2026-09-28 09:30");
        assert_eq!(next("*/15 * * * *"), "Sun 2026-09-27 10:15");
        assert_eq!(next("0 0 1 * *"), "Thu 2026-10-01 00:00");
        assert_eq!(next("0 12 * JAN,jul SAT"), "Sat 2027-01-02 12:00");
        assert_eq!(next("0 8 * * 7"), "Sun 2026-10-04 08:00", "7 is Sunday");
        assert_eq!(next("0 0 29 2 *"), "Tue 2028-02-29 00:00");
        // Day of month and day of week both set: either matches.
        assert_eq!(next("0 0 13 * FRI"), "Fri 2026-10-02 00:00");
        assert_eq!(next("5-10/5 22 * * *"), "Sun 2026-09-27 22:05");
        assert_eq!(next("@monthly"), "Thu 2026-10-01 00:00");
        assert_eq!(next("0 */6 * * *"), "Sun 2026-09-27 12:00");
    }

    #[test]
    fn weekly_and_monthly() {
        let task = build(|s| {
            s.weekly_on(Weekday::Wed, "07:15", "a", |_| async { Ok(()) });
        });
        assert_eq!(
            local(task.next_run(NOW, Zone::UTC), Zone::UTC),
            "Wed 2026-09-30 07:15"
        );
        let task = build(|s| {
            s.monthly_on(31, "00:00", "a", |_| async { Ok(()) });
        });
        let at = task.next_run(NOW, Zone::UTC);
        assert_eq!(local(at, Zone::UTC), "Sat 2026-10-31 00:00");
        assert_eq!(
            local(task.next_run(at, Zone::UTC), Zone::UTC),
            "Thu 2026-12-31 00:00"
        );
    }

    #[test]
    fn filters_narrow_runs() {
        let task = build(|s| {
            s.hourly("a", |_| async { Ok(()) })
                .weekdays()
                .between("09:00", "17:00");
        });
        let first = task.next_run(NOW, Zone::UTC);
        assert_eq!(local(first, Zone::UTC), "Mon 2026-09-28 09:00");
        let evening = NOW + 86_400 + 7 * 3600; // Monday 17:07
        assert_eq!(
            local(task.next_run(evening, Zone::UTC), Zone::UTC),
            "Tue 2026-09-29 09:00"
        );
        let night = build(|s| {
            s.every_minutes(30, "a", |_| async { Ok(()) })
                .between("23:00", "01:00");
        });
        assert_eq!(
            local(night.next_run(NOW, Zone::UTC), Zone::UTC),
            "Sun 2026-09-27 23:00"
        );
        assert_eq!(
            local(night.next_run(NOW + 14 * 3600, Zone::UTC), Zone::UTC), // Monday 00:07
            "Mon 2026-09-28 00:30"
        );
        let task = build(|s| {
            s.every_minute("a", |_| async { Ok(()) })
                .days(&[Weekday::Sat]);
        });
        assert_eq!(
            local(task.next_run(NOW, Zone::UTC), Zone::UTC),
            "Sat 2026-10-03 00:00"
        );
        let never = build(|s| {
            s.every_minute("a", |_| async { Ok(()) })
                .weekdays()
                .weekends();
        });
        assert_eq!(never.next_run(NOW, Zone::UTC), NEVER);
    }

    #[test]
    fn a_task_zone_wins_and_follows_daylight_saving() {
        let task = build(|s| {
            s.daily_at("02:30", "a", |_| async { Ok(()) })
                .timezone("Europe/Amsterdam");
        });
        let ams = zone("Europe/Amsterdam");
        // The night of 2027-03-28 skips 02:00–03:00: it runs at 03:00 instead.
        let before = ams.resolve("2027-03-27T12:00:00".parse().unwrap()).unwrap();
        let spring = task.next_run(before, Zone::UTC);
        assert_eq!(local(spring, ams), "Sun 2027-03-28 03:00");
        assert_eq!(
            local(task.next_run(spring, Zone::UTC), ams),
            "Mon 2027-03-29 02:30"
        );
        // 2026-10-25 has 02:30 twice: it runs once.
        let before = ams.resolve("2026-10-24T12:00:00".parse().unwrap()).unwrap();
        let autumn = task.next_run(before, Zone::UTC);
        assert_eq!(local(autumn, ams), "Sun 2026-10-25 02:30");
        let after = task.next_run(autumn, Zone::UTC);
        assert_eq!(local(after, ams), "Mon 2026-10-26 02:30");
        assert_eq!(after - autumn, 25 * 3600);
    }

    #[test]
    fn mistakes_are_boot_errors() {
        for (bad, why) in [("25:00", "not a time of day"), ("9", "expected HH:MM")] {
            let mut s = Schedule::default();
            s.daily_at(bad, "report", |_| async { Ok(()) });
            let err = s.check().unwrap_err().to_string();
            assert!(err.contains("report") && err.contains(why), "{err}");
        }
        for (bad, why) in [
            ("* * * *", "5 fields"),
            ("60 * * * *", "out of range"),
            ("0 0 31 2 *", "never matches"),
            ("*/0 * * * *", "step of 0"),
            ("5-1 * * * *", "backwards"),
            ("0 0 * FOO *", "not a number"),
        ] {
            let mut s = Schedule::default();
            s.cron(bad, "c", |_| async { Ok(()) });
            let err = s.check().unwrap_err().to_string();
            assert!(err.contains(why), "{bad}: {err}");
        }
        let mut s = Schedule::default();
        s.hourly("a", |_| async { Ok(()) }).timezone("Mars/Olympus");
        assert!(s.check().unwrap_err().to_string().contains("Mars/Olympus"));
        let mut s = Schedule::default();
        s.hourly("a", |_| async { Ok(()) }).between("9", "17:00");
        assert!(s.check().is_err());
        let mut s = Schedule::default();
        s.hourly("a", |_| async { Ok(()) })
            .hourly("a", |_| async { Ok(()) });
        assert!(
            s.check()
                .unwrap_err()
                .to_string()
                .contains("scheduled twice")
        );
    }

    #[test]
    fn chains_and_upcoming() {
        let mut schedule = Schedule::default();
        schedule
            .every_minute("a", |_| async { Ok(()) })
            .every_minutes(15, "b", |_| async { Ok(()) })
            .hourly("c", |_| async { Ok(()) })
            .every(Duration::ZERO, "d", |_| async { Ok(()) });
        let whens: Vec<When> = schedule.tasks.iter().map(|t| t.when.clone()).collect();
        assert_eq!(
            whens,
            [
                When::Every(60),
                When::Every(900),
                When::Every(3600),
                When::Every(1)
            ],
            "a zero interval runs every second rather than never"
        );
        schedule.check().unwrap();
        let upcoming = schedule.upcoming(Zone::UTC);
        assert_eq!(upcoming.len(), 4);
        let at = |i: usize| upcoming[i].at.unwrap().timestamp();
        assert_eq!(at(1) % 900, 0, "aligned to the quarter hour");
        assert_eq!(at(2) % 3600, 0, "on the hour");
    }

    /// Filters no run ever passes: the task never runs (a date that can't
    /// come, `0 0 31 2 *`, is refused when the schedule is made).
    #[test]
    fn tasks_that_never_run_have_no_next_run() {
        let mut schedule = Schedule::default();
        schedule
            .hourly("never-on-the-hour", |_| async { Ok(()) })
            .between("10:30", "10:40");
        schedule.check().unwrap();
        let upcoming = schedule.upcoming(Zone::UTC);
        assert!(upcoming.iter().all(|run| run.at.is_none()), "{upcoming:?}");
    }

    /// Autumn's repeated hour: a run already made in its first pass isn't
    /// made again in the second.
    #[test]
    fn the_repeated_hour_runs_once() {
        let task = build(|s| {
            s.cron("30 1 * * *", "a", |_| async { Ok(()) });
        });
        let ny = zone("America/New_York");
        // 2026-11-01 06:15 UTC is the second 01:15 in New York (EST).
        let second_pass = 1_793_513_700;
        assert_eq!(local(second_pass, ny), "Sun 2026-11-01 01:15");
        let at = task.next_run(second_pass, ny);
        assert_eq!(local(at, ny), "Mon 2026-11-02 01:30");
    }

    /// A task that couldn't be added ignores the settings chained after
    /// it; a chain reads the schedule it builds.
    #[test]
    fn settings_after_a_task_that_failed_are_ignored() {
        let mut schedule = Schedule::default();
        let task = schedule.cron("not cron", "bad", |_| async { Ok(()) });
        assert!(task.upcoming(Zone::UTC).is_empty());
        task.weekdays();
        assert!(schedule.check().is_err());
    }

    /// The run loop: nothing to do ends it; a due cron-style task already
    /// claimed by another process is skipped; a task not due yet waits.
    /// Claims that can't be written let the run go ahead.
    #[tokio::test]
    async fn the_run_loop_skips_runs_claimed_elsewhere() {
        let (logs, _logged) = crate::test_logs::capture();
        let app = crate::testing::TestApp::new(crate::App::new()).await;
        let state = app.state().clone();
        let (stop, stopped) = watch::channel(false);
        // Nothing scheduled: returns at once.
        Schedule::default()
            .run(state.clone(), Zone::UTC, stopped.clone())
            .await;

        // The cache table gone: a claim can't be pruned or written, and the
        // run goes ahead as in a single process.
        crate::db::sql("ALTER TABLE cache RENAME TO cache_away")
            .execute(&state.db)
            .await
            .unwrap();
        assert!(claim(&state, "lonely", 60, 60).await);
        delete_expired_claims(&state, unix_now()).await;
        assert!(
            logs.has(&["could not claim the run; running it anyway"]),
            "{}",
            logs.text()
        );
        assert!(
            logs.has(&["could not clear old schedule claims"]),
            "{}",
            logs.text()
        );
        crate::db::sql("ALTER TABLE cache_away RENAME TO cache")
            .execute(&state.db)
            .await
            .unwrap();

        let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut schedule = Schedule::default();
        let counted = runs.clone();
        schedule.cron("* * * * *", "minutely", move |_| {
            let counted = counted.clone();
            async move {
                counted.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });
        let nightly = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted_nightly = nightly.clone();
        schedule.daily_at("00:00", "nightly", move |_| {
            let counted = counted_nightly.clone();
            async move {
                counted.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });
        // A second before the next minute, whose run another process has
        // claimed.
        let now = unix_now();
        let slot = (now / 60 + 1) * 60;
        let offset = slot - 1 - now;
        assert!(claim(&state, "minutely", slot, 3600).await);
        let looping = tokio::spawn(crate::clock::with_offset(
            offset,
            schedule.run(state.clone(), Zone::UTC, stopped),
        ));
        // Wait for the loop to reach that minute: about a second, but longer
        // under a slow run (coverage on PostgreSQL took over 2.5 s).
        for _ in 0..100 {
            if logs.has(&["skipped: another process runs it", "minutely"]) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        stop.send(true).unwrap();
        looping.await.unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 0);
        // The loop reached the minutely run and found it claimed; the nightly
        // one wasn't due.
        assert!(
            logs.has(&["skipped: another process runs it", "minutely"]),
            "{}",
            logs.text()
        );
        assert_eq!(nightly.load(Ordering::SeqCst), 0);
    }
}
