//! Calendar files (#48): golden `.ics` files, a structural check of RFC 5545
//! (CRLF, folding at 75 octets, escaping, required properties), updates and
//! cancellations (UID, SEQUENCE), the memory mailer and a routed
//! `Download::ics`.
//!
//! CI has no network, so no online validator is called: the golden files in
//! `fixtures/calendar/` were checked once by hand (an online validator and an
//! import into a calendar client; see the PR), and `check` below re-checks
//! the rules of the RFC on every file the tests make.

use std::time::Duration;

use renox::Download;
use renox::calendar::{Event, Ics, Method, Status};
use renox::chrono::{DateTime, TimeZone, Utc};
use renox::mail::Mail;
use renox::prelude::*;
use renox::testing::TestApp;

fn at(day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, day, hour, minute, 0)
        .unwrap()
}

/// A golden file, with the CRLF line endings it must keep.
fn golden(name: &str) -> String {
    let path = format!(
        "{}/tests/it/fixtures/calendar/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert!(
        text.ends_with("\r\n") && !text.replace("\r\n", "").contains(['\r', '\n']),
        "{name} must use CRLF line endings only"
    );
    text
}

/// The booking of the issue: a long description with every character that
/// needs escaping, and text in three scripts so folding meets multi-byte
/// characters.
fn booking(sequence: u32, status: Status) -> Ics {
    Ics::new("-//Acme//Bookings//EN").event(
        Event::new("booking-42@acme.test", at(3, 9, 0), at(3, 10, 30))
            .stamp(at(1, 8, 0))
            .summary("Service at Acme, Bay 3; bring the bike")
            .location("Calle Mayor 12, Piso 3º, Málaga")
            .description(
                "Bring the key, the manual; and the old tyres.\nBackslash \\ kept.\r\n\
                 Cuéntanos qué ruido hace la bicicleta: ¿frena mal, cruje la cadena, \
                 o falla el cambio? 自転車の整備の予約です。",
            )
            .url("https://acme.test/bookings/42")
            .organizer_named("Acme Bookings", "bookings@acme.test")
            .attendee_named("Ben \"B\" Ortiz, Jr.", "ben@example.com")
            .alarm_before(Duration::from_secs(3600))
            .alarm_before(Duration::from_secs(86_400))
            .sequence(sequence)
            .status(status),
    )
}

/// The rules of RFC 5545 that matter to a calendar client, checked on the
/// text itself rather than on what the writer meant to write. Returns the
/// unfolded content lines.
fn check(ics: &str) -> Vec<String> {
    assert!(ics.ends_with("\r\n"), "ends with CRLF");
    assert!(
        !ics.replace("\r\n", "").contains(['\r', '\n']),
        "a bare CR or LF"
    );
    let mut lines: Vec<String> = Vec::new();
    for physical in ics.strip_suffix("\r\n").unwrap().split("\r\n") {
        assert!(
            physical.len() <= 75,
            "{} octets: {physical}",
            physical.len()
        );
        assert!(
            !physical.chars().any(|c| c.is_control() && c != '\t'),
            "control character in {physical:?}"
        );
        if let Some(rest) = physical.strip_prefix(' ') {
            lines
                .last_mut()
                .expect("a fold on the first line")
                .push_str(rest);
        } else {
            lines.push(physical.to_owned());
        }
    }
    assert_eq!(lines.first().map(String::as_str), Some("BEGIN:VCALENDAR"));
    assert_eq!(lines.last().map(String::as_str), Some("END:VCALENDAR"));
    for required in ["VERSION:2.0", "CALSCALE:GREGORIAN"] {
        assert!(lines.iter().any(|l| l == required), "{required}");
    }
    assert!(lines.iter().any(|l| l.starts_with("PRODID:")));
    // BEGIN/END pairs nest properly, and every VEVENT has what RFC 5545 requires.
    let mut stack: Vec<&str> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for l in &lines {
        if let Some(name) = l.strip_prefix("BEGIN:") {
            stack.push(name);
            if name == "VEVENT" {
                seen.clear();
            }
        } else if let Some(name) = l.strip_prefix("END:") {
            assert_eq!(stack.pop(), Some(name), "END:{name} without its BEGIN");
            if name == "VEVENT" {
                for required in ["UID", "DTSTAMP", "DTSTART", "DTEND"] {
                    assert!(seen.contains(&required), "VEVENT without {required}");
                }
            }
        } else if stack.last() == Some(&"VEVENT") {
            seen.push(l.split([':', ';']).next().unwrap());
        }
        // Times are UTC: 20261103T090000Z.
        if ["DTSTART", "DTEND", "DTSTAMP"]
            .iter()
            .any(|n| l.starts_with(&format!("{n}:")))
        {
            let value = l.split_once(':').unwrap().1;
            assert!(
                value.len() == 16 && value.ends_with('Z') && value.as_bytes()[8] == b'T',
                "{l}"
            );
        }
        // TEXT never holds an unescaped comma or semicolon.
        if ["SUMMARY:", "DESCRIPTION:", "LOCATION:", "UID:", "PRODID:"]
            .iter()
            .any(|n| l.starts_with(n))
        {
            let value = l.split_once(':').unwrap().1;
            let mut escaped = false;
            for c in value.chars() {
                if escaped {
                    assert!(matches!(c, '\\' | ';' | ',' | 'n' | 'N'), "{l}");
                    escaped = false;
                } else {
                    assert!(!matches!(c, ';' | ','), "unescaped {c:?} in {l}");
                    escaped = c == '\\';
                }
            }
            assert!(!escaped, "a trailing backslash in {l}");
        }
    }
    assert!(stack.is_empty(), "unclosed {stack:?}");
    lines
}

#[test]
fn a_booking_matches_the_golden_file() {
    let ics = booking(0, Status::Confirmed).to_string();
    check(&ics);
    assert_eq!(ics, golden("booking.ics"));
}

#[test]
fn a_change_keeps_the_uid_and_raises_the_sequence() {
    let ics = booking(1, Status::Confirmed).to_string();
    let lines = check(&ics);
    assert!(lines.contains(&"UID:booking-42@acme.test".to_owned()));
    assert!(lines.contains(&"SEQUENCE:1".to_owned()));
    assert!(lines.contains(&"METHOD:REQUEST".to_owned()));
    assert_eq!(ics, golden("booking-update.ics"));
}

#[test]
fn a_cancellation_is_method_cancel_with_the_same_uid() {
    let ics = booking(2, Status::Cancelled);
    assert_eq!(ics.effective_method(), Method::Cancel);
    let text = ics.to_string();
    let lines = check(&text);
    assert!(lines.contains(&"METHOD:CANCEL".to_owned()));
    assert!(lines.contains(&"STATUS:CANCELLED".to_owned()));
    assert!(lines.contains(&"UID:booking-42@acme.test".to_owned()));
    assert!(lines.contains(&"SEQUENCE:2".to_owned()));
    assert_eq!(text, golden("booking-cancel.ics"));
}

/// `DTSTAMP` is read from the clock, so `TestApp::travel` moves it.
#[renox::test]
async fn the_stamp_comes_from_the_clock() {
    let app = TestApp::new(App::new()).await;
    let stamp = |ics: &Ics| {
        ics.to_string()
            .lines()
            .find(|l| l.starts_with("DTSTAMP:"))
            .unwrap()
            .to_owned()
    };
    let make = || Ics::new("-//T//T//EN").event(Event::new("a", at(3, 9, 0), at(3, 10, 0)));
    let now = stamp(&make());
    app.travel(Duration::from_secs(2 * 86_400));
    let later = app.at_travelled_time(async { stamp(&make()) }).await;
    assert_ne!(now, later);
    assert!(later > now, "{later} is after {now}");
}

/// Folding at 75 octets is counted in bytes: a line of three-byte characters
/// is cut between characters, and unfolding gives the text back.
#[test]
fn long_multibyte_text_is_folded_between_characters() {
    let summary = "予約".repeat(80);
    let ics = Ics::new("-//T//T//EN")
        .event(Event::new("u", at(3, 9, 0), at(3, 10, 0)).summary(summary.clone()))
        .to_string();
    let lines = check(&ics);
    assert!(lines.contains(&format!("SUMMARY:{summary}")));
    assert!(ics.contains("\r\n "), "the line was folded");
}

#[test]
fn a_mail_carries_the_calendar_file() {
    let ics = booking(0, Status::Confirmed);
    let mail = Mail::new("ben@example.com", "Your booking", "See you.").attach_ics(&ics);
    let file = &mail.attachments[0];
    assert_eq!(file.filename, "invite.ics");
    assert_eq!(
        file.content_type,
        "text/calendar; method=REQUEST; charset=UTF-8"
    );
    assert_eq!(file.data, ics.to_string().into_bytes());

    let cancel = Mail::new("ben@example.com", "Cancelled", "Sorry.")
        .attach_ics(&booking(1, Status::Cancelled));
    assert_eq!(cancel.attachments[0].filename, "cancellation.ics");
    assert!(cancel.attachments[0].content_type.contains("method=CANCEL"));
}

#[renox::test]
async fn the_memory_mailer_keeps_the_ics_attachment() {
    let kernel = App::with_config({
        let mut c = Config::default();
        c.key = Some(renox::generate_key());
        c
    })
    .boot()
    .await
    .unwrap();
    let ics = booking(0, Status::Confirmed);
    kernel
        .mailer()
        .send(Mail::new("ben@example.com", "Your booking", "See you.").attach_ics(&ics))
        .await
        .unwrap();
    let sent = kernel.mailer().sent().pop().unwrap();
    assert_eq!(sent.attachments.len(), 1);
    assert_eq!(sent.attachments[0].filename, "invite.ics");
    assert_eq!(
        String::from_utf8(sent.attachments[0].data.clone()).unwrap(),
        golden("booking.ics")
    );
}

/// A route that returns a calendar download (and its future is `Send`).
async fn download() -> Download {
    Download::ics(&booking(0, Status::Confirmed), "booking.ics")
}

struct Calendar;

impl Module for Calendar {
    fn name(&self) -> &'static str {
        "calendar-test"
    }

    fn routes(&self) -> Routes {
        Routes::new().get("/booking.ics", download)
    }
}

#[renox::test]
async fn a_download_is_text_calendar() {
    let app = TestApp::new(App::new().module(Calendar)).await;
    let res = app.get("/booking.ics").await;
    res.assert_ok();
    assert_eq!(
        res.header("content-type"),
        Some("text/calendar; method=REQUEST; charset=UTF-8")
    );
    assert!(
        res.header("content-disposition")
            .unwrap()
            .contains("filename=\"booking.ics\"")
    );
    assert_eq!(res.text(), golden("booking.ics"));
}
