//! The week calendar's layout (`renox_week_grid` in templates): which cell
//! each event covers, which lane it takes when events overlap, and which
//! slots are still free. The macro prints what this returns, so the template
//! needs no date arithmetic.
//!
//! Times are read and shown in the app's time zone (`APP_TIMEZONE`), through
//! the `date` filter the page already has: an event's RFC 3339 `start` and
//! `end` become wall-clock times, and each free slot's `start` and `end` go
//! back out as RFC 3339 with the zone's offset on that day.

use std::collections::HashMap;

use renox::chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use renox::minijinja::value::{Kwargs, Value};
use renox::minijinja::{self, Error, ErrorKind, State, context};
use renox::serde_json::{Value as Json, json};

/// A template error naming the block.
fn fail(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::InvalidOperation, message.into())
}

/// The text of a member of a template value, "" when it has none.
fn text(value: &Value, name: &str) -> String {
    match value.get_attr(name) {
        Ok(found) if !found.is_undefined() && !found.is_none() => found
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| found.to_string()),
        _ => String::new(),
    }
}

/// Turns the app-zone wall clock into instants and back, caching the offset
/// of each hour it has been asked about.
struct Clock<'a> {
    local: minijinja::Expression<'a, 'a>,
    offsets: HashMap<(NaiveDate, u32), i64>,
}

impl<'a> Clock<'a> {
    fn new(state: &'a State) -> Result<Self, Error> {
        let local = state
            .env()
            .compile_expression("when | date('%Y-%m-%dT%H:%M:%S')")?;
        Ok(Self {
            local,
            offsets: HashMap::new(),
        })
    }

    /// The wall-clock time of an RFC 3339 instant (or a local time as it is).
    fn local(&self, when: &str) -> Result<NaiveDateTime, Error> {
        let shown = self
            .local
            .eval(context! { when => when })
            .map_err(|_| fail(format!("week_calendar: `{when}` is not a time")))?;
        NaiveDateTime::parse_from_str(&shown.to_string(), "%Y-%m-%dT%H:%M:%S")
            .map_err(|_| fail(format!("week_calendar: `{when}` is not a time")))
    }

    /// The zone's offset (seconds east of UTC) at a wall-clock time.
    fn offset(&mut self, at: NaiveDateTime) -> i64 {
        let key = (at.date(), at.hour());
        if let Some(found) = self.offsets.get(&key) {
            return *found;
        }
        // Read the zone at the instant that has this number in UTC, then
        // again at the instant that offset gives: right except in the hour
        // a clock change falls in.
        let mut offset = 0;
        for _ in 0..2 {
            let instant = at.and_utc().timestamp() - offset;
            let utc = DateTime::from_timestamp(instant, 0).unwrap_or_default();
            if let Ok(shown) = self.local(&utc.to_rfc3339()) {
                offset = shown.and_utc().timestamp() - instant;
            }
        }
        self.offsets.insert(key, offset);
        offset
    }

    /// `2026-10-12T08:30:00+07:00` for a wall-clock time.
    fn rfc3339(&mut self, at: NaiveDateTime) -> String {
        let offset = self.offset(at);
        if offset == 0 {
            return format!("{}Z", at.format("%Y-%m-%dT%H:%M:%S"));
        }
        let sign = if offset < 0 { '-' } else { '+' };
        let minutes = offset.abs() / 60;
        format!(
            "{}{sign}{:02}:{:02}",
            at.format("%Y-%m-%dT%H:%M:%S"),
            minutes / 60,
            minutes % 60
        )
    }
}

/// One event, cut to one day and to the hours shown.
struct Piece {
    first: usize,
    last: usize,
    day: usize,
    from: String,
    to: String,
    before: bool,
    after: bool,
    event: Value,
}

/// `renox_week_grid(resources, events, start, days=7, from=8, to=18, step=30, slots=true)`.
pub(crate) fn week_grid(
    state: &State,
    resources: Value,
    events: Value,
    start: String,
    kwargs: Kwargs,
) -> Result<Value, Error> {
    let days = kwargs.get::<Option<i64>>("days")?.unwrap_or(7);
    let opens = kwargs.get::<Option<i64>>("from")?.unwrap_or(8);
    let closes = kwargs.get::<Option<i64>>("to")?.unwrap_or(18);
    let step = kwargs.get::<Option<i64>>("step")?.unwrap_or(30);
    let slots = kwargs.get::<Option<bool>>("slots")?.unwrap_or(true);
    kwargs.assert_all_used()?;
    if !(1..=31).contains(&days) {
        return Err(fail("week_calendar: `days` is between 1 and 31"));
    }
    if !(0..24).contains(&opens) || !(1..=24).contains(&closes) || opens >= closes {
        return Err(fail(
            "week_calendar: `from` and `to` are hours, from before to (0 to 24)",
        ));
    }
    if !(5..=240).contains(&step) {
        return Err(fail("week_calendar: `step` is between 5 and 240 minutes"));
    }
    let days = days as usize;
    let mut clock = Clock::new(state)?;

    let first_day = if start.len() > 10 {
        clock.local(&start)?.date()
    } else {
        NaiveDate::parse_from_str(&start, "%Y-%m-%d")
            .map_err(|_| fail(format!("week_calendar: `start` is a day, not `{start}`")))?
    };
    let per_day = ((closes - opens) * 60 + step - 1) / step;
    let per_day = per_day as usize;
    let slot_start = |n: usize| opens * 60 + n as i64 * step;
    let time_at = |minutes: i64| {
        NaiveTime::from_hms_opt((minutes / 60).min(23) as u32, (minutes % 60) as u32, 0)
            .unwrap_or_default()
    };
    let clock_text = |minutes: i64| {
        if minutes >= 24 * 60 {
            "24:00".to_owned()
        } else {
            time_at(minutes).format("%H:%M").to_string()
        }
    };

    // The header: days, and a time over the slots that start an hour.
    let mut day_heads = Vec::new();
    for d in 0..days {
        let day = first_day + Duration::days(d as i64);
        day_heads.push(json!({
            "date": day.format("%Y-%m-%d").to_string(),
            "weekday": day.weekday().num_days_from_sunday(),
            "day": day.day(),
            "month": day.month(),
        }));
    }
    let labelled: Vec<usize> = (0..per_day)
        .filter(|n| step >= 60 || 60 % step != 0 || slot_start(*n) % 60 == 0)
        .collect();
    let mut hour_heads = Vec::new();
    for d in 0..days {
        for (i, n) in labelled.iter().enumerate() {
            let end = labelled.get(i + 1).copied().unwrap_or(per_day);
            hour_heads.push(json!({
                "label": clock_text(slot_start(*n)),
                "column": d * per_day + n + 1,
                "span": end - n,
            }));
        }
    }

    // The events as pieces, cut to each day's hours.
    let mut by_resource: HashMap<String, Vec<Piece>> = HashMap::new();
    for event in events
        .try_iter()
        .map_err(|_| fail("week_calendar: `events` is a list"))?
    {
        let (begin, finish) = (text(&event, "start"), text(&event, "end"));
        let (begin, finish) = (clock.local(&begin)?, clock.local(&finish)?);
        if finish <= begin {
            continue;
        }
        for d in 0..days {
            let day = first_day + Duration::days(d as i64);
            let window =
                |hour: i64| day.and_time(NaiveTime::default()) + Duration::minutes(hour * 60);
            let (open, close) = (window(opens), window(closes));
            let (from, to) = (begin.max(open), finish.min(close));
            if to <= from {
                continue;
            }
            let from_min = (from - open).num_minutes();
            let to_min = (to - open).num_minutes();
            let first = from_min / step;
            let last = ((to_min + step - 1) / step - 1).min(per_day as i64 - 1);
            by_resource
                .entry(text(&event, "resource"))
                .or_default()
                .push(Piece {
                    first: d * per_day + first as usize,
                    last: d * per_day + last.max(first) as usize,
                    day: d,
                    from: from.format("%H:%M").to_string(),
                    to: if to == close && closes == 24 {
                        "24:00".to_owned()
                    } else {
                        to.format("%H:%M").to_string()
                    },
                    before: begin < open,
                    after: finish > close,
                    event: event.clone(),
                });
        }
    }

    let mut rows = Vec::new();
    for resource in resources
        .try_iter()
        .map_err(|_| fail("week_calendar: `resources` is a list"))?
    {
        let id = text(&resource, "id");
        let mut pieces = by_resource.remove(&id).unwrap_or_default();
        pieces.sort_by_key(|p| (p.first, p.last));
        let mut lane_ends: Vec<usize> = Vec::new();
        let mut taken = vec![false; days * per_day];
        let mut items: Vec<(usize, usize, Json)> = Vec::new();
        for piece in &pieces {
            let lane = match lane_ends.iter().position(|end| *end < piece.first) {
                Some(free) => free,
                None => {
                    lane_ends.push(0);
                    lane_ends.len() - 1
                }
            };
            lane_ends[lane] = piece.last;
            taken[piece.first..=piece.last].fill(true);
            items.push((
                piece.first,
                lane,
                json!({
                    "kind": "event",
                    "column": piece.first + 1,
                    "span": piece.last - piece.first + 1,
                    "lane": lane + 1,
                    "day": piece.day,
                    "from": piece.from,
                    "to": piece.to,
                    "label": text(&piece.event, "label"),
                    "tone": text(&piece.event, "tone"),
                    "href": text(&piece.event, "href"),
                    "before": piece.before,
                    "after": piece.after,
                }),
            ));
        }
        let lanes = lane_ends.len().max(1);
        if slots {
            for d in 0..days {
                let day = first_day + Duration::days(d as i64);
                for n in 0..per_day {
                    let cell = d * per_day + n;
                    if taken[cell] {
                        continue;
                    }
                    let begin = slot_start(n);
                    let end = (begin + step).min(closes * 60);
                    let at = |minutes: i64| {
                        day.and_time(NaiveTime::default()) + Duration::minutes(minutes)
                    };
                    items.push((
                        cell,
                        0,
                        json!({
                            "kind": "free",
                            "column": cell + 1,
                            "span": 1,
                            "lane": 1,
                            "lanes": lanes,
                            "day": d,
                            "from": clock_text(begin),
                            "to": clock_text(end),
                            "start": clock.rfc3339(at(begin)),
                            "end": clock.rfc3339(at(end)),
                        }),
                    ));
                }
            }
        }
        items.sort_by_key(|(column, lane, _)| (*column, *lane));
        let items: Vec<Json> = items.into_iter().map(|(_, _, item)| item).collect();
        rows.push(json!({
            "id": id,
            "label": text(&resource, "label"),
            "note": text(&resource, "note"),
            "lanes": lanes,
            "items": items,
        }));
    }

    Ok(Value::from_serialize(json!({
        "days": day_heads,
        "hours": hour_heads,
        "per_day": per_day,
        "columns": days * per_day,
        "rows": rows,
    })))
}
