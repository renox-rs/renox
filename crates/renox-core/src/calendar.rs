//! Calendar files (iCalendar, RFC 5545): a booking confirmation that lands in
//! the customer's calendar, a cancellation that removes it again, or a
//! `.ics` download.
//!
//! ```
//! use std::time::Duration;
//! use renox::calendar::{Event, Ics, Status};
//! use renox::chrono::{TimeZone, Utc};
//! use renox::mail::Mail;
//! use renox::Download;
//!
//! let starts = Utc.with_ymd_and_hms(2026, 11, 3, 9, 0, 0).unwrap();
//! let ends = Utc.with_ymd_and_hms(2026, 11, 3, 10, 30, 0).unwrap();
//!
//! let ics = Ics::new("-//Acme//Bookings//EN").event(
//!     Event::new("booking-42@acme.test", starts, ends)
//!         .summary("Service at Acme")
//!         .location("Bay 3")
//!         .description("Bring the key, please.\nWe close at 6 pm.")
//!         .organizer("bookings@acme.test")
//!         .attendee("ben@example.com")
//!         .alarm_before(Duration::from_secs(3600))
//!         .status(Status::Confirmed),
//! );
//!
//! // In a mail: `text/calendar; method=REQUEST`, so mail clients offer "Add to calendar".
//! let mail = Mail::new("ben@example.com", "Your booking", "See you on Tuesday.").attach_ics(&ics);
//!
//! // Or as a download (a route's `Result<Download>`).
//! let file = Download::ics(&ics, "booking.ics");
//! ```
//!
//! A change to the booking is the same event again with the same UID and a
//! higher `SEQUENCE`; a cancellation is that event with
//! [`Status::Cancelled`], which makes the file `METHOD:CANCEL`:
//!
//! ```
//! # use renox::calendar::{Event, Ics, Status};
//! # use renox::chrono::{TimeZone, Utc};
//! # let starts = Utc.with_ymd_and_hms(2026, 11, 3, 9, 0, 0).unwrap();
//! # let ends = Utc.with_ymd_and_hms(2026, 11, 3, 10, 30, 0).unwrap();
//! let cancel = Ics::new("-//Acme//Bookings//EN").event(
//!     Event::new("booking-42@acme.test", starts, ends)
//!         .organizer("bookings@acme.test")
//!         .attendee("ben@example.com")
//!         .sequence(2)
//!         .status(Status::Cancelled),
//! );
//! assert!(cancel.to_string().contains("METHOD:CANCEL\r\n"));
//! ```
//!
//! The output follows the RFC: lines end in CRLF and are folded at 75 octets
//! (never inside a multi-byte character), text is escaped (`\`, `;`, `,`
//! and newlines), and every event has `UID`, `DTSTAMP`, `DTSTART` and
//! `DTEND`. All times are written in UTC (`20261103T090000Z`); a `TZID` with
//! `APP_TIMEZONE` would need a `VTIMEZONE` block, which this module leaves
//! out, so convert local times to UTC first. `DTSTAMP` is the moment the
//! event was made, read from Renox's clock (`TestApp::travel` moves it); set
//! it with [`Event::stamp`] for a file that must be byte-for-byte stable.
//! There is no dependency, and no all-day events or recurrence yet.

use std::fmt::{self, Display, Write as _};
use std::time::Duration;

use chrono::{DateTime, Utc};

/// The longest content line, in octets, before it is folded (RFC 5545
/// section 3.1).
const LINE_LIMIT: usize = 75;

/// What the calendar file is for (`METHOD`, RFC 5546), and the `method=`
/// parameter of its content type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Method {
    /// Just publish the events: a download, no reply expected.
    Publish,
    /// Invite attendees, or update an invitation (the same UID with a higher
    /// `SEQUENCE`).
    Request,
    /// Cancel events sent before.
    Cancel,
}

impl Method {
    /// The word in the file: `PUBLISH`, `REQUEST` or `CANCEL`.
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Publish => "PUBLISH",
            Method::Request => "REQUEST",
            Method::Cancel => "CANCEL",
        }
    }
}

impl Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An event's `STATUS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Status {
    /// Not settled yet.
    Tentative,
    /// Booked.
    Confirmed,
    /// Called off: the file becomes `METHOD:CANCEL` (unless
    /// [`Ics::method`] says otherwise).
    Cancelled,
}

impl Status {
    /// The word in the file: `TENTATIVE`, `CONFIRMED` or `CANCELLED`.
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Tentative => "TENTATIVE",
            Status::Confirmed => "CONFIRMED",
            Status::Cancelled => "CANCELLED",
        }
    }
}

impl Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A person: an email address and, optionally, a name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Person {
    name: Option<String>,
    email: String,
}

/// One event (`VEVENT`). Build it with [`Event::new`] and the setters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    uid: String,
    starts: DateTime<Utc>,
    ends: DateTime<Utc>,
    stamp: DateTime<Utc>,
    summary: Option<String>,
    description: Option<String>,
    location: Option<String>,
    url: Option<String>,
    organizer: Option<Person>,
    attendees: Vec<Person>,
    alarms: Vec<Duration>,
    status: Option<Status>,
    sequence: u32,
}

impl Event {
    /// An event that starts at `starts` and ends at `ends`. `uid` identifies
    /// it for good: send the same UID (with a higher [`sequence`](Self::sequence))
    /// to change or cancel it. Something like `booking-42@acme.test`.
    pub fn new(uid: impl Into<String>, starts: DateTime<Utc>, ends: DateTime<Utc>) -> Self {
        Self {
            uid: uid.into(),
            starts,
            ends,
            stamp: crate::clock::system_now().into(),
            summary: None,
            description: None,
            location: None,
            url: None,
            organizer: None,
            attendees: Vec::new(),
            alarms: Vec::new(),
            status: None,
            sequence: 0,
        }
    }

    /// The title (`SUMMARY`).
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// The longer text (`DESCRIPTION`); newlines are kept.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Where it takes place (`LOCATION`).
    pub fn location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    /// A page about the event (`URL`).
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Who sends the invitation: an email address (`ORGANIZER`).
    pub fn organizer(mut self, email: impl Into<String>) -> Self {
        self.organizer = Some(Person {
            name: None,
            email: email.into(),
        });
        self
    }

    /// The organizer with a display name.
    pub fn organizer_named(mut self, name: impl Into<String>, email: impl Into<String>) -> Self {
        self.organizer = Some(Person {
            name: Some(name.into()),
            email: email.into(),
        });
        self
    }

    /// Invites `email` (`ATTENDEE`, asking for a reply). Call it again for
    /// more people.
    pub fn attendee(mut self, email: impl Into<String>) -> Self {
        self.attendees.push(Person {
            name: None,
            email: email.into(),
        });
        self
    }

    /// An attendee with a display name.
    pub fn attendee_named(mut self, name: impl Into<String>, email: impl Into<String>) -> Self {
        self.attendees.push(Person {
            name: Some(name.into()),
            email: email.into(),
        });
        self
    }

    /// A reminder `before` the start (a `VALARM`). Call it again for more.
    pub fn alarm_before(mut self, before: Duration) -> Self {
        self.alarms.push(before);
        self
    }

    /// The event's `STATUS`.
    pub fn status(mut self, status: Status) -> Self {
        self.status = Some(status);
        self
    }

    /// The revision (`SEQUENCE`, 0 for the first version). A change or a
    /// cancellation of an event already sent carries a higher number than
    /// the last one, or calendar clients ignore it.
    pub fn sequence(mut self, sequence: u32) -> Self {
        self.sequence = sequence;
        self
    }

    /// Sets `DTSTAMP`, which [`Event::new`] reads from the clock, for a file
    /// that must come out the same every time (tests, stored copies).
    pub fn stamp(mut self, at: DateTime<Utc>) -> Self {
        self.stamp = at;
        self
    }

    /// The UID.
    pub fn uid(&self) -> &str {
        &self.uid
    }

    fn write(&self, out: &mut String) {
        line(out, "BEGIN", &[], "VEVENT");
        text_line(out, "UID", &self.uid);
        line(out, "DTSTAMP", &[], &utc(self.stamp));
        line(out, "DTSTART", &[], &utc(self.starts));
        line(out, "DTEND", &[], &utc(self.ends));
        if let Some(summary) = &self.summary {
            text_line(out, "SUMMARY", summary);
        }
        if let Some(description) = &self.description {
            text_line(out, "DESCRIPTION", description);
        }
        if let Some(location) = &self.location {
            text_line(out, "LOCATION", location);
        }
        if let Some(url) = &self.url {
            line(out, "URL", &[], &clean(url));
        }
        if let Some(organizer) = &self.organizer {
            person_line(out, "ORGANIZER", organizer, &[]);
        }
        for attendee in &self.attendees {
            person_line(
                out,
                "ATTENDEE",
                attendee,
                &[("ROLE", "REQ-PARTICIPANT"), ("RSVP", "TRUE")],
            );
        }
        if let Some(status) = self.status {
            line(out, "STATUS", &[], status.as_str());
        }
        line(out, "SEQUENCE", &[], &self.sequence.to_string());
        for before in &self.alarms {
            line(out, "BEGIN", &[], "VALARM");
            line(out, "ACTION", &[], "DISPLAY");
            text_line(
                out,
                "DESCRIPTION",
                self.summary.as_deref().unwrap_or("Reminder"),
            );
            line(out, "TRIGGER", &[], &format!("-{}", duration(*before)));
            line(out, "END", &[], "VALARM");
        }
        line(out, "END", &[], "VEVENT");
    }
}

/// A calendar file: a product id and some events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ics {
    prodid: String,
    method: Option<Method>,
    events: Vec<Event>,
}

impl Ics {
    /// An empty calendar. `prodid` names your app in the file, in the form
    /// `-//Company//Product//EN`.
    pub fn new(prodid: impl Into<String>) -> Self {
        Self {
            prodid: prodid.into(),
            method: None,
            events: Vec::new(),
        }
    }

    /// Adds an event.
    pub fn event(mut self, event: Event) -> Self {
        self.events.push(event);
        self
    }

    /// Sets the `METHOD`. Without it the method follows the events:
    /// `CANCEL` when one is [`Status::Cancelled`], else `REQUEST` when one
    /// has attendees, else `PUBLISH`.
    pub fn method(mut self, method: Method) -> Self {
        self.method = Some(method);
        self
    }

    /// The events.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// The `METHOD` the file carries.
    pub fn effective_method(&self) -> Method {
        if let Some(method) = self.method {
            method
        } else if self
            .events
            .iter()
            .any(|e| e.status == Some(Status::Cancelled))
        {
            Method::Cancel
        } else if self.events.iter().any(|e| !e.attendees.is_empty()) {
            Method::Request
        } else {
            Method::Publish
        }
    }

    /// The content type to send it with:
    /// `text/calendar; method=REQUEST; charset=UTF-8` (the method matches
    /// the file's, as mail clients expect).
    pub fn content_type(&self) -> String {
        format!(
            "text/calendar; method={}; charset=UTF-8",
            self.effective_method()
        )
    }

    /// The file's bytes (UTF-8, CRLF line endings).
    pub fn to_bytes(&self) -> Vec<u8> {
        self.to_string().into_bytes()
    }
}

impl Display for Ics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        line(&mut out, "BEGIN", &[], "VCALENDAR");
        line(&mut out, "VERSION", &[], "2.0");
        text_line(&mut out, "PRODID", &self.prodid);
        line(&mut out, "CALSCALE", &[], "GREGORIAN");
        line(&mut out, "METHOD", &[], self.effective_method().as_str());
        for event in &self.events {
            event.write(&mut out);
        }
        line(&mut out, "END", &[], "VCALENDAR");
        f.write_str(&out)
    }
}

/// `20261103T090000Z`.
fn utc(at: DateTime<Utc>) -> String {
    at.format("%Y%m%dT%H%M%SZ").to_string()
}

/// An ISO 8601 duration for `TRIGGER`: `PT1H`, `P1D`, `P1DT1H30M`.
fn duration(d: Duration) -> String {
    let secs = d.as_secs();
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (hours, rest) = (rest / 3_600, rest % 3_600);
    let (minutes, seconds) = (rest / 60, rest % 60);
    let mut out = String::from("P");
    if days > 0 {
        let _ = write!(out, "{days}D");
    }
    if hours + minutes + seconds > 0 || days == 0 {
        out.push('T');
        if hours > 0 {
            let _ = write!(out, "{hours}H");
        }
        if minutes > 0 {
            let _ = write!(out, "{minutes}M");
        }
        if seconds > 0 || hours + minutes == 0 {
            let _ = write!(out, "{seconds}S");
        }
    }
    out
}

/// Escapes a TEXT value (RFC 5545 section 3.3.11): backslash, semicolon and
/// comma get a backslash, any newline becomes `\n`, other control
/// characters are dropped.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str("\\n");
            }
            '\n' => out.push_str("\\n"),
            '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// A value that isn't TEXT (a URI, an address): control characters out, so a
/// value can't start another line.
fn clean(value: &str) -> String {
    value.chars().filter(|c| !c.is_control()).collect()
}

/// A parameter value: quoted when it holds `:`, `;` or `,`; `"` and control
/// characters are dropped (a parameter value can't carry them).
fn param_value(value: &str) -> String {
    let value: String = value
        .chars()
        .filter(|c| *c != '"' && !c.is_control())
        .collect();
    if value.contains([':', ';', ',']) {
        format!("\"{value}\"")
    } else {
        value
    }
}

fn text_line(out: &mut String, name: &str, value: &str) {
    line(out, name, &[], &escape(value));
}

fn person_line(out: &mut String, name: &str, person: &Person, extra: &[(&str, &str)]) {
    let mut params: Vec<(&str, String)> = Vec::new();
    if let Some(cn) = person.name.as_deref().filter(|n| !n.trim().is_empty()) {
        params.push(("CN", param_value(cn)));
    }
    for (key, value) in extra {
        params.push((key, (*value).to_owned()));
    }
    let params: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    line(
        out,
        name,
        &params,
        &format!("mailto:{}", clean(&person.email)),
    );
}

/// Writes `NAME;PARAM=value:value`, folded, with CRLF.
fn line(out: &mut String, name: &str, params: &[(&str, &str)], value: &str) {
    let mut content = String::from(name);
    for (key, v) in params {
        let _ = write!(content, ";{key}={v}");
    }
    content.push(':');
    content.push_str(value);
    fold(out, &content);
}

/// Folds `content` so no line is longer than 75 octets: the break is a CRLF
/// and one space, which a reader removes. The space counts toward the next
/// line's 75, and a character is never split.
fn fold(out: &mut String, content: &str) {
    let mut used = 0;
    for c in content.chars() {
        let size = c.len_utf8();
        if used + size > LINE_LIMIT {
            out.push_str("\r\n ");
            used = 1;
        }
        out.push(c);
        used += size;
    }
    out.push_str("\r\n");
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 3, h, m, 0).unwrap()
    }

    #[test]
    fn text_is_escaped() {
        assert_eq!(escape("a,b;c\\d"), "a\\,b\\;c\\\\d");
        assert_eq!(
            escape("one\r\ntwo\nthree\rfour"),
            "one\\ntwo\\nthree\\nfour"
        );
        assert_eq!(escape("bell\u{7}"), "bell");
    }

    #[test]
    fn durations() {
        assert_eq!(duration(Duration::from_secs(3600)), "PT1H");
        assert_eq!(duration(Duration::from_secs(900)), "PT15M");
        assert_eq!(duration(Duration::from_secs(86_400)), "P1D");
        assert_eq!(duration(Duration::from_secs(90_000)), "P1DT1H");
        assert_eq!(duration(Duration::from_secs(5_430)), "PT1H30M30S");
        assert_eq!(duration(Duration::ZERO), "PT0S");
    }

    #[test]
    fn folding_counts_octets_and_keeps_characters_whole() {
        let mut out = String::new();
        // 3-octet characters: 25 fit in 75; the next line holds 24 (one octet is the space).
        fold(&mut out, &format!("X:{}", "€".repeat(60)));
        for physical in out.split("\r\n").filter(|l| !l.is_empty()) {
            assert!(physical.len() <= LINE_LIMIT, "{} octets", physical.len());
        }
        let unfolded = out.replace("\r\n ", "").replace("\r\n", "");
        assert_eq!(unfolded, format!("X:{}", "€".repeat(60)));
        assert!(out.ends_with("\r\n"));
    }

    #[test]
    fn a_line_of_exactly_75_octets_is_not_folded() {
        let mut out = String::new();
        fold(&mut out, &"a".repeat(75));
        assert_eq!(out, format!("{}\r\n", "a".repeat(75)));
        let mut out = String::new();
        fold(&mut out, &"a".repeat(76));
        assert_eq!(out, format!("{}\r\n a\r\n", "a".repeat(75)));
    }

    #[test]
    fn the_method_follows_the_events() {
        let event = || Event::new("u", at(9, 0), at(10, 0)).stamp(at(8, 0));
        assert_eq!(
            Ics::new("-//T//T//EN").event(event()).effective_method(),
            Method::Publish
        );
        assert_eq!(
            Ics::new("-//T//T//EN")
                .event(event().attendee("a@b.test"))
                .effective_method(),
            Method::Request
        );
        assert_eq!(
            Ics::new("-//T//T//EN")
                .event(event().attendee("a@b.test").status(Status::Cancelled))
                .effective_method(),
            Method::Cancel
        );
        assert_eq!(
            Ics::new("-//T//T//EN")
                .method(Method::Request)
                .event(event())
                .content_type(),
            "text/calendar; method=REQUEST; charset=UTF-8"
        );
    }

    #[test]
    fn values_cannot_start_another_line() {
        let ics = Ics::new("-//T//T//EN").event(
            Event::new("u", at(9, 0), at(10, 0))
                .stamp(at(8, 0))
                .url("https://x.test/\r\nATTENDEE:evil")
                .organizer_named("Eve\", \"x:y\r\nBEGIN:VEVENT", "eve@x.test\r\nBEGIN:VEVENT"),
        );
        let text = ics.to_string();
        for physical in text.split("\r\n") {
            assert!(!physical.starts_with("ATTENDEE:evil"), "{text}");
        }
        assert_eq!(
            text.split("\r\n").filter(|l| *l == "BEGIN:VEVENT").count(),
            1,
            "{text}"
        );
    }
}
