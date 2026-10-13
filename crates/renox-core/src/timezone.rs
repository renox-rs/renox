//! The app's time zone (`APP_TIMEZONE`): an IANA name such as
//! `Asia/Jakarta` or `Europe/Amsterdam` (daylight saving time included), a
//! fixed offset such as `+07:00`, or `UTC`. Scheduled tasks and the `date`
//! template filter use it.
//!
//! ```
//! use renox::timezone::Zone;
//!
//! let amsterdam: Zone = "Europe/Amsterdam".parse().unwrap();
//! let winter = 1_767_225_600; // 2026-01-01 00:00 UTC
//! let summer = 1_782_864_000; // 2026-07-01 00:00 UTC
//! assert_eq!(amsterdam.offset_at(winter), 3600);
//! assert_eq!(amsterdam.offset_at(summer), 7200);
//! assert_eq!("+07:00".parse::<Zone>().unwrap().offset_at(winter), 7 * 3600);
//! ```

use std::fmt;
use std::str::FromStr;

use anyhow::{Context, bail};
use chrono::{FixedOffset, LocalResult, NaiveDateTime, Offset, TimeZone};

/// A time zone: UTC, a fixed offset or an IANA zone (with its daylight
/// saving rules). Made by parsing (`"Asia/Jakarta".parse::<Zone>()`,
/// `"+07:00".parse()`), or [`Zone::UTC`] and [`Zone::fixed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zone(Kind);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A fixed offset from UTC, in seconds (`UTC` is 0).
    Fixed(i32),
    /// An IANA zone, with its daylight saving time rules.
    Named(chrono_tz::Tz),
}

impl Default for Zone {
    fn default() -> Self {
        Zone::UTC
    }
}

impl Zone {
    /// Coordinated Universal Time.
    pub const UTC: Zone = Zone(Kind::Fixed(0));

    /// A fixed offset from UTC, in seconds east (`Zone::fixed(7 * 3600)` is
    /// `+07:00`), with no daylight saving time.
    pub const fn fixed(seconds: i32) -> Zone {
        Zone(Kind::Fixed(seconds))
    }

    /// The offset from UTC at the moment `unix` (seconds), in seconds.
    pub fn offset_at(&self, unix: i64) -> i64 {
        match &self.0 {
            Kind::Fixed(offset) => i64::from(*offset),
            Kind::Named(tz) => {
                let utc = chrono::DateTime::from_timestamp(unix, 0).unwrap_or_default();
                i64::from(
                    tz.offset_from_utc_datetime(&utc.naive_utc())
                        .fix()
                        .local_minus_utc(),
                )
            }
        }
    }

    /// The wall-clock time at the moment `unix`.
    pub fn local(&self, unix: i64) -> NaiveDateTime {
        let utc = chrono::DateTime::from_timestamp(unix, 0).unwrap_or_default();
        (utc + chrono::TimeDelta::seconds(self.offset_at(unix))).naive_utc()
    }

    /// The moment a wall-clock time happens. In the hour clocks skip in
    /// spring it doesn't (`None`); in the hour repeated in autumn it's the
    /// first one.
    pub fn resolve(&self, local: NaiveDateTime) -> Option<i64> {
        let result = match &self.0 {
            Kind::Fixed(offset) => FixedOffset::east_opt(*offset)?
                .from_local_datetime(&local)
                .map(|t| t.timestamp()),
            Kind::Named(tz) => tz.from_local_datetime(&local).map(|t| t.timestamp()),
        };
        match result {
            LocalResult::Single(at) => Some(at),
            LocalResult::Ambiguous(first, second) => Some(first.min(second)),
            LocalResult::None => None,
        }
    }

    /// The fixed offset in effect at `unix`, for chrono's formatting.
    pub fn fixed_at(&self, unix: i64) -> FixedOffset {
        FixedOffset::east_opt(self.offset_at(unix) as i32)
            .unwrap_or_else(|| FixedOffset::east_opt(0).expect("zero offset"))
    }
}

impl FromStr for Zone {
    type Err = anyhow::Error;

    /// `UTC`, `Z`, an offset (`+07:00`, `-03:30`) or an IANA name.
    fn from_str(value: &str) -> anyhow::Result<Self> {
        let value = value.trim();
        if value.is_empty() || value.eq_ignore_ascii_case("utc") || value == "Z" {
            return Ok(Zone::UTC);
        }
        if let Some(sign) = value
            .strip_prefix('+')
            .map(|rest| (1, rest))
            .or_else(|| value.strip_prefix('-').map(|rest| (-1, rest)))
        {
            let (sign, rest) = sign;
            let (hour, minute) = rest
                .split_once(':')
                .context("expected an offset like +07:00")?;
            let (hour, minute): (i32, i32) = (hour.parse()?, minute.parse()?);
            if !(0..=14).contains(&hour) || !(0..60).contains(&minute) {
                bail!("`{value}` is not a UTC offset");
            }
            return Ok(Zone::fixed(sign * (hour * 3600 + minute * 60)));
        }
        match value.parse::<chrono_tz::Tz>() {
            Ok(tz) => Ok(Zone(Kind::Named(tz))),
            Err(_) => bail!(
                "unknown time zone `{value}`: use UTC, an offset like +07:00 or an IANA name \
                 like Asia/Jakarta"
            ),
        }
    }
}

impl fmt::Display for Zone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Kind::Fixed(0) => f.write_str("UTC"),
            Kind::Fixed(offset) => {
                let sign = if *offset < 0 { '-' } else { '+' };
                let offset = offset.abs();
                write!(f, "{sign}{:02}:{:02}", offset / 3600, offset % 3600 / 60)
            }
            Kind::Named(tz) => f.write_str(tz.name()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> NaiveDateTime {
        text.parse().unwrap()
    }

    #[test]
    fn parses_every_form() {
        assert_eq!("UTC".parse::<Zone>().unwrap(), Zone::UTC);
        assert_eq!("".parse::<Zone>().unwrap(), Zone::UTC);
        assert_eq!(
            "-03:30".parse::<Zone>().unwrap(),
            Zone::fixed(-(3 * 3600 + 1800))
        );
        assert_eq!(
            "Asia/Jakarta".parse::<Zone>().unwrap().to_string(),
            "Asia/Jakarta"
        );
        assert_eq!("+07:00".parse::<Zone>().unwrap().to_string(), "+07:00");
        assert!("Mars/Olympus".parse::<Zone>().is_err());
        assert!("+25:00".parse::<Zone>().is_err());
        assert!("+7".parse::<Zone>().is_err());
    }

    #[test]
    fn daylight_saving_gaps_and_repeats() {
        let ams: Zone = "Europe/Amsterdam".parse().unwrap();
        // 2026-03-29: 02:00 → 03:00, so 02:30 doesn't happen.
        assert_eq!(ams.resolve(at("2026-03-29T02:30:00")), None);
        let three = ams.resolve(at("2026-03-29T03:00:00")).unwrap();
        assert_eq!(ams.offset_at(three), 7200);
        // 2026-10-25: 03:00 → 02:00, so 02:30 happens twice; the first counts.
        let twice = ams.resolve(at("2026-10-25T02:30:00")).unwrap();
        assert_eq!(ams.offset_at(twice), 7200);
        assert_eq!(ams.local(twice), at("2026-10-25T02:30:00"));
        assert_eq!(
            ams.local(twice + 3600),
            at("2026-10-25T02:30:00"),
            "the repeat"
        );
    }
}
