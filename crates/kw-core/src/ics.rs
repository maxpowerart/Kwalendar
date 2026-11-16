//! Read-only calendars from an iCalendar (RFC 5545) feed link, e.g. Google Calendar's
//! "Secret address in iCal format". For workplaces that do not allow OAuth API access:
//! nothing is ever written back; the feed is downloaded again and replaces the local copy.
//!
//! The link itself is a secret (anyone who has it can read the calendar), so it is kept in
//! the OS keyring like a refresh token and never logged or shown.

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use sha2::{Digest, Sha256};

use crate::google::{find_link, ApiError};
use crate::models::{Account, Attendee, EventRow};
use crate::recurrence::{instance_id, Occurrence};
use crate::sync::detect_notice;
use crate::Core;

/// `accounts.kind` of feed accounts.
pub const KIND: &str = "ics";
/// A feed account has exactly one calendar, with this id.
pub const FEED_CAL: &str = "feed";
/// Feeds have no deltas, so they are downloaded at most this often (the main loop ticks every 30 s).
const MIN_INTERVAL_SECS: i64 = 120;
/// Reminder for feeds whose events carry no VALARM at all.
const DEFAULT_REMINDER_MIN: i64 = 10;

// ---------------------------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Prop {
    fn param(&self, key: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

#[derive(Default)]
struct RawEvent {
    props: Vec<Prop>,
    alarms: Vec<Vec<Prop>>,
}

impl RawEvent {
    fn get(&self, name: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.name == name)
    }
}

/// A parsed feed, ready to store.
pub struct Feed {
    /// X-WR-CALNAME (Google uses the owner's e-mail for the primary calendar).
    pub name: Option<String>,
    /// X-WR-TIMEZONE, an IANA name.
    pub time_zone: Option<String>,
    /// Whether any event carries its own reminders (VALARM).
    pub has_alarms: bool,
    pub events: Vec<EventRow>,
}

impl Feed {
    /// Calendar-level default reminders: none if the feed brings its own.
    pub fn default_reminders(&self) -> Vec<i64> {
        if self.has_alarms {
            vec![]
        } else {
            vec![DEFAULT_REMINDER_MIN]
        }
    }
}

/// Joins folded lines (CRLF followed by a space or tab).
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(rest) = line.strip_prefix(' ').or_else(|| line.strip_prefix('\t')) {
            if let Some(last) = out.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        if !line.is_empty() {
            out.push(line.to_string());
        }
    }
    out
}

/// Splits on `sep` outside double quotes.
fn split_unquoted(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        if ch == '"' {
            quoted = !quoted;
        } else if ch == sep && !quoted {
            parts.push(&s[start..i]);
            start = i + ch.len_utf8();
        }
    }
    parts.push(&s[start..]);
    parts
}

/// `NAME;P1=a;P2="b:c":value`
fn parse_line(line: &str) -> Option<Prop> {
    let mut quoted = false;
    let mut colon = None;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            ':' if !quoted => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let i = colon?;
    let mut head = split_unquoted(&line[..i], ';').into_iter();
    let name = head.next()?.trim().to_ascii_uppercase();
    let params = head
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((k.trim().to_ascii_uppercase(), v.trim().trim_matches('"').to_string()))
        })
        .collect();
    Some(Prop { name, params, value: line[i + 1..].to_string() })
}

/// TEXT values: `\n`, `\,`, `\;`, `\\`.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(x) => out.push(x),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// IANA names, `/vendor/prefix/Europe/Berlin` style ids, and common Windows (Outlook) names.
fn resolve_tz(name: &str) -> Option<Tz> {
    let name = name.trim();
    if let Ok(tz) = name.parse::<Tz>() {
        return Some(tz);
    }
    let parts: Vec<&str> = name.split('/').collect();
    for i in 1..parts.len() {
        if let Ok(tz) = parts[i..].join("/").parse::<Tz>() {
            return Some(tz);
        }
    }
    let iana = match name {
        "UTC" | "Coordinated Universal Time" => "UTC",
        "GMT Standard Time" => "Europe/London",
        "W. Europe Standard Time" => "Europe/Berlin",
        "Central European Standard Time" => "Europe/Warsaw",
        "Central Europe Standard Time" => "Europe/Budapest",
        "Romance Standard Time" => "Europe/Paris",
        "E. Europe Standard Time" => "Europe/Chisinau",
        "FLE Standard Time" => "Europe/Kiev",
        "GTB Standard Time" => "Europe/Bucharest",
        "Russian Standard Time" => "Europe/Moscow",
        "Belarus Standard Time" => "Europe/Minsk",
        "Turkey Standard Time" => "Europe/Istanbul",
        "Israel Standard Time" => "Asia/Jerusalem",
        "Arabian Standard Time" => "Asia/Dubai",
        "India Standard Time" => "Asia/Kolkata",
        "China Standard Time" => "Asia/Shanghai",
        "Singapore Standard Time" => "Asia/Singapore",
        "Tokyo Standard Time" => "Asia/Tokyo",
        "AUS Eastern Standard Time" => "Australia/Sydney",
        "Eastern Standard Time" => "America/New_York",
        "Central Standard Time" => "America/Chicago",
        "Mountain Standard Time" => "America/Denver",
        "Pacific Standard Time" => "America/Los_Angeles",
        _ => return None,
    };
    iana.parse().ok()
}

enum When {
    At(i64),
    Day(NaiveDate),
}

fn is_date_only(v: &str) -> bool {
    v.len() == 8 && !v.contains('T')
}

fn parse_date(v: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(v.get(..8)?, "%Y%m%d").ok()
}

/// A DATE-TIME value: UTC (`Z`), with a TZID, or floating (calendar zone, else the PC's).
/// Returns unix seconds and the IANA zone name when one applies.
fn parse_datetime(v: &str, tzid: Option<&str>, default_tz: Option<Tz>) -> Option<(i64, Option<String>)> {
    let v = v.trim();
    if let Some(utc) = v.strip_suffix('Z').or_else(|| v.strip_suffix('z')) {
        let n = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some((n.and_utc().timestamp(), None));
    }
    let n = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()?;
    match tzid.and_then(resolve_tz).or(default_tz) {
        Some(tz) => Some((tz.from_local_datetime(&n).earliest()?.timestamp(), Some(tz.name().to_string()))),
        None => Some((Local.from_local_datetime(&n).earliest()?.timestamp(), None)),
    }
}

fn parse_when(p: &Prop, default_tz: Option<Tz>) -> Option<(When, Option<String>)> {
    let v = p.value.trim();
    if p.param("VALUE").is_some_and(|x| x.eq_ignore_ascii_case("DATE")) || is_date_only(v) {
        return parse_date(v).map(|d| (When::Day(d), None));
    }
    parse_datetime(v, p.param("TZID"), default_tz).map(|(t, tz)| (When::At(t), tz))
}

/// `[+-]P[nW][nD][T[nH][nM][nS]]` → signed seconds.
fn parse_duration(s: &str) -> Option<i64> {
    let s = s.trim();
    let (sign, s) = match s.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, s.strip_prefix('+').unwrap_or(s)),
    };
    let s = s.strip_prefix('P')?;
    let (mut total, mut num, mut time) = (0i64, String::new(), false);
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let n: i64 = if c == 'T' { 0 } else { num.parse().ok()? };
        total += match c {
            'T' => {
                time = true;
                0
            }
            'W' => n * 7 * 86400,
            'D' => n * 86400,
            'H' if time => n * 3600,
            'M' if time => n * 60,
            'S' if time => n,
            _ => return None,
        };
        num.clear();
    }
    Some(sign * total)
}

fn mail(v: &str) -> String {
    let v = v.trim();
    let v = if v.len() >= 7 && v[..7].eq_ignore_ascii_case("mailto:") { &v[7..] } else { v };
    v.to_string()
}

fn response(partstat: Option<&str>) -> String {
    match partstat.map(|s| s.to_ascii_uppercase()).as_deref() {
        Some("ACCEPTED") => "accepted",
        Some("DECLINED") => "declined",
        Some("TENTATIVE") => "tentative",
        _ => "needsAction",
    }
    .to_string()
}

/// EXDATE/RDATE in the form `recurrence::expand` understands: dates as `VALUE=DATE`,
/// times converted to UTC (so Windows zone names and VTIMEZONE-only ids still work).
fn date_list_line(p: &Prop, all_day: bool, default_tz: Option<Tz>) -> Option<String> {
    if p.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("PERIOD")) {
        return None;
    }
    let vals = p.value.split(',').map(str::trim).filter(|v| !v.is_empty());
    let out: Vec<String> = if all_day {
        vals.filter_map(parse_date).map(|d| d.format("%Y%m%d").to_string()).collect()
    } else {
        vals.filter(|v| !is_date_only(v))
            .filter_map(|v| parse_datetime(v, p.param("TZID"), default_tz))
            .filter_map(|(t, _)| DateTime::from_timestamp(t, 0))
            .map(|d| d.format("%Y%m%dT%H%M%SZ").to_string())
            .collect()
    };
    if out.is_empty() {
        return None;
    }
    Some(if all_day {
        format!("{};VALUE=DATE:{}", p.name, out.join(","))
    } else {
        format!("{}:{}", p.name, out.join(","))
    })
}

/// The rrule crate wants UNTIL in UTC when DTSTART has a zone; some feeds send local or date UNTILs.
fn normalize_rrule(rule: &str, all_day: bool, tz: Option<Tz>) -> String {
    if all_day {
        return rule.to_string();
    }
    rule.split(';')
        .map(|part| {
            let Some(v) = part.strip_prefix("UNTIL=") else { return part.to_string() };
            if v.ends_with('Z') || v.ends_with('z') {
                return part.to_string();
            }
            let local = if is_date_only(v) { format!("{v}T235959") } else { v.to_string() };
            match parse_datetime(&local, None, tz).and_then(|(t, _)| DateTime::from_timestamp(t, 0)) {
                Some(d) => format!("UNTIL={}", d.format("%Y%m%dT%H%M%SZ")),
                None => part.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Popup reminders (minutes before start) from VALARMs; e-mail alarms and absolute triggers are skipped.
fn alarm_minutes(alarms: &[Vec<Prop>]) -> Vec<i64> {
    let mut out: Vec<i64> = alarms
        .iter()
        .filter(|a| {
            a.iter()
                .find(|p| p.name == "ACTION")
                .map(|p| matches!(p.value.trim().to_ascii_uppercase().as_str(), "DISPLAY" | "AUDIO"))
                .unwrap_or(true)
        })
        .filter_map(|a| a.iter().find(|p| p.name == "TRIGGER"))
        .filter(|t| t.param("VALUE").is_none_or(|v| v.eq_ignore_ascii_case("DURATION")))
        .filter(|t| t.param("RELATED").is_none_or(|v| v.eq_ignore_ascii_case("START")))
        .filter_map(|t| parse_duration(&t.value))
        .filter(|s| *s <= 0)
        .map(|s| -s / 60)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Google appends a "-::~:~::~ … Join with Google Meet … -::~:~::~" block to descriptions.
fn clean_description(d: String) -> Option<String> {
    let d = match d.find("-::~:~::~") {
        Some(i) => d[..i].trim_end().to_string(),
        None => d,
    };
    Some(d).filter(|s| !s.trim().is_empty())
}

struct Ctx<'a> {
    account: &'a str,
    cal: &'a str,
    tz: Option<Tz>,
    self_email: Option<String>,
}

fn to_row(ev: &RawEvent, ctx: &Ctx) -> Option<EventRow> {
    let uid = ev.get("UID")?.value.trim().replace('|', "_");
    if uid.is_empty() {
        return None;
    }
    let (start, tz) = parse_when(ev.get("DTSTART")?, ctx.tz)?;
    let end = ev.get("DTEND").and_then(|p| parse_when(p, ctx.tz)).map(|(w, _)| w).or_else(|| {
        let d = parse_duration(&ev.get("DURATION")?.value)?;
        Some(match start {
            When::At(t) => When::At(t + d),
            When::Day(day) => When::Day(day + Duration::days((d / 86400).max(1))),
        })
    });
    let cancelled = ev.get("STATUS").is_some_and(|p| p.value.trim().eq_ignore_ascii_case("CANCELLED"));
    let text = |name: &str| ev.get(name).map(|p| unescape(&p.value)).filter(|s| !s.trim().is_empty());

    let mut row = EventRow {
        account_id: ctx.account.to_string(),
        calendar_id: ctx.cal.to_string(),
        id: uid.clone(),
        status: if cancelled { "cancelled" } else { "confirmed" }.into(),
        summary: text("SUMMARY"),
        location: text("LOCATION"),
        description: text("DESCRIPTION").and_then(clean_description),
        updated: ev.get("LAST-MODIFIED").map(|p| p.value.trim().to_string()),
        reminders_default: true,
        ..Default::default()
    };
    match start {
        When::Day(d) => {
            row.all_day = true;
            let e = match end {
                Some(When::Day(e)) if e > d => e,
                _ => d + Duration::days(1),
            };
            row.start_date = Some(d.to_string());
            row.end_date = Some(e.to_string());
        }
        When::At(t) => {
            row.start_ts = Some(t);
            row.end_ts = Some(match end {
                Some(When::At(e)) if e >= t => e,
                _ => t,
            });
            row.time_zone = tz;
        }
    }

    if let Some(rid) = ev.get("RECURRENCE-ID") {
        // A changed or cancelled occurrence of a series.
        let (orig, _) = parse_when(rid, ctx.tz)?;
        let occ = match orig {
            When::At(t) => {
                row.original_start_ts = Some(t);
                Occurrence::Timed(t)
            }
            When::Day(d) => {
                row.original_start_date = Some(d.to_string());
                Occurrence::Day(d)
            }
        };
        row.id = instance_id(&uid, &occ);
        row.recurring_event_id = Some(uid);
    } else {
        if cancelled {
            return None;
        }
        let zone = row.time_zone.as_deref().and_then(resolve_tz).or(ctx.tz);
        let rec: Vec<String> = ev
            .props
            .iter()
            .filter_map(|p| match p.name.as_str() {
                "RRULE" => Some(format!("RRULE:{}", normalize_rrule(p.value.trim(), row.all_day, zone))),
                "EXDATE" | "RDATE" => date_list_line(p, row.all_day, ctx.tz),
                _ => None,
            })
            .collect();
        if rec.iter().any(|l| l.starts_with("RRULE:") || l.starts_with("RDATE")) {
            row.recurrence = Some(rec);
        }
    }

    let self_email = ctx.self_email.as_deref();
    let is_self = |email: &str| self_email.is_some_and(|s| s.eq_ignore_ascii_case(email));
    let organizer_email = ev.get("ORGANIZER").map(|p| mail(&p.value));
    if let Some(p) = ev.get("ORGANIZER") {
        row.organizer = p.param("CN").map(String::from).or_else(|| organizer_email.clone());
        row.organizer_self = organizer_email.as_deref().is_some_and(is_self);
    }
    row.attendees = ev
        .props
        .iter()
        .filter(|p| p.name == "ATTENDEE")
        .map(|p| {
            let email = mail(&p.value);
            Attendee {
                name: p.param("CN").filter(|cn| *cn != email).map(String::from),
                response: response(p.param("PARTSTAT")),
                is_self: is_self(&email),
                organizer: organizer_email.as_deref().is_some_and(|o| o.eq_ignore_ascii_case(&email)),
                optional: p.param("ROLE").is_some_and(|r| r.eq_ignore_ascii_case("OPT-PARTICIPANT")),
                email,
            }
        })
        .filter(|a| a.email.contains('@'))
        .collect();

    row.meet_link = ev
        .get("X-GOOGLE-CONFERENCE")
        .map(|p| p.value.trim().to_string())
        .filter(|u| u.starts_with("https://"))
        .or_else(|| row.location.as_deref().and_then(find_link))
        .or_else(|| text("DESCRIPTION").as_deref().and_then(find_link));
    row.html_link = ev
        .get("URL")
        .map(|p| p.value.trim().to_string())
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"));

    let reminders = alarm_minutes(&ev.alarms);
    if !reminders.is_empty() {
        row.reminders_default = false;
        row.reminders = reminders;
    }
    Some(row)
}

/// Parses an iCalendar document into rows for `account`/`cal`.
pub fn parse(text: &str, account: &str, cal: &str) -> Feed {
    let mut cal_props: Vec<Prop> = Vec::new();
    let mut events: Vec<RawEvent> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut event: Option<RawEvent> = None;
    let mut alarm: Option<Vec<Prop>> = None;

    for line in unfold(text) {
        let Some(p) = parse_line(&line) else { continue };
        match p.name.as_str() {
            "BEGIN" => {
                let comp = p.value.trim().to_ascii_uppercase();
                if comp == "VEVENT" && event.is_none() {
                    event = Some(RawEvent::default());
                } else if comp == "VALARM" && event.is_some() {
                    alarm = Some(Vec::new());
                }
                stack.push(comp);
            }
            "END" => {
                let comp = p.value.trim().to_ascii_uppercase();
                while let Some(top) = stack.pop() {
                    if top == comp {
                        break;
                    }
                }
                if comp == "VALARM" {
                    if let (Some(a), Some(e)) = (alarm.take(), event.as_mut()) {
                        e.alarms.push(a);
                    }
                } else if comp == "VEVENT" {
                    events.extend(event.take());
                }
            }
            _ => match stack.last().map(String::as_str) {
                Some("VALARM") => alarm.iter_mut().for_each(|a| a.push(p.clone())),
                Some("VEVENT") => event.iter_mut().for_each(|e| e.props.push(p.clone())),
                Some("VCALENDAR") if stack.len() == 1 => cal_props.push(p),
                _ => {}
            },
        }
    }

    let cal_prop = |n: &str| cal_props.iter().find(|p| p.name == n).map(|p| unescape(&p.value).trim().to_string());
    let name = cal_prop("X-WR-CALNAME").filter(|s| !s.is_empty());
    let tz = cal_prop("X-WR-TIMEZONE").as_deref().and_then(resolve_tz);
    // Google names a person's primary calendar after their address: that is "me" for RSVP display.
    let self_email = name.clone().filter(|n| n.contains('@') && !n.contains(char::is_whitespace));
    let ctx = Ctx { account, cal, tz, self_email };

    let has_alarms = events.iter().any(|e| !e.alarms.is_empty());
    let mut rows: HashMap<String, EventRow> = HashMap::new();
    for ev in &events {
        if let Some(row) = to_row(ev, &ctx) {
            rows.insert(row.id.clone(), row);
        }
    }
    Feed {
        name,
        time_zone: tz.map(|t| t.name().to_string()),
        has_alarms,
        events: rows.into_values().collect(),
    }
}

// ---------------------------------------------------------------------------------------------
// Fetching and syncing
// ---------------------------------------------------------------------------------------------

enum Fetched {
    NotModified,
    Body { text: String, etag: Option<String>, hash: String },
}

/// `webcal://` → `https://`; only http(s) links are accepted.
pub fn normalize_url(url: &str) -> Result<String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let url = if lower.starts_with("webcal://") || lower.starts_with("webcals://") {
        format!("https://{}", &url[url.find("://").unwrap() + 3..])
    } else {
        url.to_string()
    };
    let l = url.to_ascii_lowercase();
    if !(l.starts_with("https://") || l.starts_with("http://")) || url.len() < 12 || url.contains(char::is_whitespace) {
        bail!(crate::tr!(
            "Нужна ссылка вида https://… или webcal://… на файл .ics",
            "Expected an https://… or webcal://… link to an .ics file"
        ));
    }
    Ok(url)
}

/// Host part of a link, shown in place of an e-mail (the full link is a secret).
fn host(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let h = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let h = h.rsplit('@').next().unwrap_or(h);
    match h.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => name.to_string(),
        _ => h.to_string(),
    }
}

fn feed_http_error(code: u16) -> ApiError {
    ApiError::Other(match code {
        401 | 403 | 404 | 410 => crate::tr!(
            "Ссылка iCal не работает (HTTP {}) — проверьте её или добавьте календарь заново",
            "The iCal link does not work (HTTP {}) — check it or add the calendar again",
            code
        ),
        _ => crate::tr!("Сервер календаря ответил HTTP {}", "The calendar server answered HTTP {}", code),
    })
}

async fn fetch(http: &reqwest::Client, url: &str, etag: Option<&str>) -> Result<Fetched, ApiError> {
    // `without_url`: the link is a secret and must not end up in logs or error texts.
    let net = |e: reqwest::Error| ApiError::Network(e.without_url().to_string());
    let mut req = http.get(url).header(reqwest::header::ACCEPT, "text/calendar, */*;q=0.5");
    if let Some(e) = etag.filter(|e| !e.is_empty()) {
        req = req.header(reqwest::header::IF_NONE_MATCH, e);
    }
    let resp = req.send().await.map_err(net)?;
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(Fetched::NotModified);
    }
    if !status.is_success() {
        return Err(feed_http_error(status.as_u16()));
    }
    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let text = resp.text().await.map_err(net)?;
    if !text.contains("BEGIN:VCALENDAR") {
        return Err(ApiError::Other(crate::tr!(
            "По ссылке не календарь iCal (.ics)",
            "The link does not lead to an iCal (.ics) calendar"
        )));
    }
    let hash = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    Ok(Fetched::Body { text, etag, hash })
}

/// `calendars.sync_token` of a feed holds "<etag>\n<sha256 of the body>".
fn validators(token: Option<&str>) -> (Option<&str>, Option<&str>) {
    match token.and_then(|t| t.split_once('\n')) {
        Some((e, h)) => (Some(e).filter(|e| !e.is_empty()), Some(h)),
        None => (None, None),
    }
}

#[cfg(not(test))]
fn store_link(id: &str, url: &str) -> Result<()> {
    crate::auth::store_refresh_token(id, url)
}

#[cfg(not(test))]
fn load_link(id: &str) -> Result<Option<String>> {
    crate::auth::load_refresh_token(id)
}

#[cfg(test)]
fn test_links() -> &'static std::sync::Mutex<HashMap<String, String>> {
    static LINKS: std::sync::OnceLock<std::sync::Mutex<HashMap<String, String>>> = std::sync::OnceLock::new();
    LINKS.get_or_init(Default::default)
}

#[cfg(test)]
fn store_link(id: &str, url: &str) -> Result<()> {
    test_links().lock().unwrap().insert(id.into(), url.into());
    Ok(())
}

#[cfg(test)]
fn load_link(id: &str) -> Result<Option<String>> {
    Ok(test_links().lock().unwrap().get(id).cloned())
}

impl Core {
    /// Adds a read-only calendar from an iCal link. Downloads it once first, so a wrong link fails here.
    pub async fn add_feed(&self, url: &str, name: Option<String>) -> Result<Account> {
        let url = normalize_url(url)?;
        for acc in self.db.accounts()?.into_iter().filter(|a| a.kind == KIND) {
            if load_link(&acc.id).ok().flatten().as_deref() == Some(url.as_str()) {
                bail!(crate::tr!("Этот календарь уже добавлен", "This calendar is already added"));
            }
        }
        let Fetched::Body { text, etag, hash } = fetch(&self.http, &url, None).await.map_err(|e| anyhow!("{e}"))? else {
            bail!("unexpected 304");
        };
        let meta = parse(&text, "", FEED_CAL);
        let label = name
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .or(meta.name.clone())
            .unwrap_or_else(|| host(&url));

        let id = format!("ics-{}", uuid::Uuid::new_v4().simple());
        store_link(&id, &url)?;
        let color = self.db.insert_feed_account(&id, &host(&url), &label, KIND)?;
        self.db.upsert_calendar(
            &id,
            FEED_CAL,
            &label,
            &color,
            "reader",
            true,
            meta.time_zone.as_deref(),
            &meta.default_reminders(),
            true,
        )?;
        self.apply_feed(&id, &text, false)?;
        self.db
            .set_sync_token(&id, FEED_CAL, Some(&format!("{}\n{hash}", etag.unwrap_or_default())))?;
        self.db.mark_account_synced(&id, chrono::Utc::now().timestamp_millis())?;
        self.changed();
        self.db
            .accounts()?
            .into_iter()
            .find(|a| a.id == id)
            .ok_or_else(|| anyhow!("account vanished"))
    }

    /// One poll of a feed account. Skipped if it was checked less than `MIN_INTERVAL_SECS` ago.
    pub(crate) async fn sync_feed(&self, acc: &Account, force: bool) -> Result<(), ApiError> {
        let now = chrono::Utc::now().timestamp_millis();
        let recent = acc.last_sync.is_some_and(|t| now - t < MIN_INTERVAL_SECS * 1000);
        if !force && acc.status == "ok" && recent {
            return Ok(());
        }
        let other = |e: anyhow::Error| ApiError::Other(e.to_string());
        let url = load_link(&acc.id).map_err(other)?.ok_or_else(|| {
            ApiError::Other(crate::tr!(
                "Ссылка iCal не найдена — добавьте календарь заново",
                "The iCal link is missing — add the calendar again"
            ))
        })?;
        let token = self
            .db
            .calendar_ids(&acc.id)
            .map_err(other)?
            .into_iter()
            .find(|(id, _)| id == FEED_CAL)
            .and_then(|(_, t)| t);
        let (etag, old_hash) = validators(token.as_deref());
        if let Fetched::Body { text, etag, hash } = fetch(&self.http, &url, etag).await? {
            if old_hash != Some(hash.as_str()) {
                self.apply_feed(&acc.id, &text, old_hash.is_some()).map_err(other)?;
            }
            self.db
                .set_sync_token(&acc.id, FEED_CAL, Some(&format!("{}\n{hash}", etag.unwrap_or_default())))
                .map_err(other)?;
        }
        self.db.mark_account_synced(&acc.id, now).map_err(other)?;
        Ok(())
    }

    /// Replaces the stored copy with the feed's contents; reports moves and new invitations.
    fn apply_feed(&self, account: &str, text: &str, notify: bool) -> Result<()> {
        let feed = parse(text, account, FEED_CAL);
        if let Some(c) = self.db.calendar(account, FEED_CAL)? {
            self.db.upsert_calendar(
                account,
                FEED_CAL,
                &c.summary,
                &c.color,
                "reader",
                true,
                feed.time_zone.as_deref(),
                &feed.default_reminders(),
                c.selected,
            )?;
        }
        let old = self.db.replace_calendar_events(account, FEED_CAL, &feed.events)?;
        if notify && self.config().notify_changes {
            for row in &feed.events {
                if let Some(n) = detect_notice(old.get(&row.id), row) {
                    self.notice(n);
                }
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;
    use crate::models::Calendar;
    use crate::recurrence::build_instances;

    const SAMPLE: &str = "BEGIN:VCALENDAR
PRODID:-//Google Inc//Google Calendar 70.9054//EN
VERSION:2.0
CALSCALE:GREGORIAN
METHOD:PUBLISH
X-WR-CALNAME:me@company.com
X-WR-TIMEZONE:Europe/Warsaw
BEGIN:VTIMEZONE
TZID:Europe/Warsaw
BEGIN:STANDARD
TZOFFSETFROM:+0200
TZOFFSETTO:+0100
DTSTART:19701025T030000
END:STANDARD
END:VTIMEZONE
BEGIN:VEVENT
DTSTART;TZID=Europe/Warsaw:20261019T093000
DTEND;TZID=Europe/Warsaw:20261019T094500
RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR
EXDATE;TZID=Europe/Warsaw:20261022T093000
DTSTAMP:20261101T120000Z
ORGANIZER;CN=Anna:mailto:anna@company.com
UID:standup@google.com
ATTENDEE;CUTYPE=INDIVIDUAL;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED;CN=me@company.com;X-NUM-GUESTS=0:mailto:me@company.com
X-GOOGLE-CONFERENCE:https://meet.google.com/abc-defg-hij
DESCRIPTION:Daily sync\\n\\n-::~:~::~:~:~:~:~:~:~::~:~::-\\nJoin with Google Meet: https://meet.google.com/abc-defg-hij\\n-::~:~::~:~::-
SUMMARY:Stand-up
BEGIN:VALARM
ACTION:DISPLAY
DESCRIPTION:This is an event reminder
TRIGGER:-P0DT0H10M0S
END:VALARM
BEGIN:VALARM
ACTION:EMAIL
TRIGGER:-P1D
END:VALARM
END:VEVENT
BEGIN:VEVENT
DTSTART;TZID=Europe/Warsaw:20261020T110000
DTEND;TZID=Europe/Warsaw:20261020T111500
RECURRENCE-ID;TZID=Europe/Warsaw:20261020T093000
UID:standup@google.com
SUMMARY:Stand-up (moved)
END:VEVENT
BEGIN:VEVENT
DTSTART;VALUE=DATE:20261023
DTEND;VALUE=DATE:20261024
UID:offsite@google.com
SUMMARY:Team offsite\\, Kraków
LOCATION:Kraków
END:VEVENT
BEGIN:VEVENT
DTSTART:20261021T120000Z
DTEND:20261021T130000Z
UID:review@google.com
SUMMARY:Design review with a long title that
  gets folded
ORGANIZER;CN=Piotr:mailto:piotr@company.com
ATTENDEE;PARTSTAT=NEEDS-ACTION;CN=Me:mailto:me@company.com
ATTENDEE;ROLE=OPT-PARTICIPANT;PARTSTAT=TENTATIVE;CN=\"Kowalski, Jan\":mailto:jan@company.com
END:VEVENT
BEGIN:VEVENT
DTSTART:20261021T150000Z
DTEND:20261021T160000Z
UID:gone@google.com
STATUS:CANCELLED
SUMMARY:Cancelled
END:VEVENT
END:VCALENDAR
";

    fn sample() -> String {
        SAMPLE.replace('\n', "\r\n")
    }

    fn cal() -> Calendar {
        Calendar {
            account_id: "a".into(),
            id: FEED_CAL.into(),
            summary: "Work".into(),
            color: "#5B8DEF".into(),
            selected: true,
            access_role: "reader".into(),
            primary: true,
            time_zone: Some("Europe/Warsaw".into()),
            default_reminders: vec![],
        }
    }

    #[test]
    fn parses_google_feed() {
        let feed = parse(&sample(), "a", FEED_CAL);
        assert_eq!(feed.name.as_deref(), Some("me@company.com"));
        assert_eq!(feed.time_zone.as_deref(), Some("Europe/Warsaw"));
        assert!(feed.has_alarms);
        assert_eq!(feed.events.len(), 4, "the cancelled single event is dropped");
        let by = |id: &str| feed.events.iter().find(|e| e.id == id).unwrap();

        let m = by("standup@google.com");
        assert_eq!(m.start_ts, Some(1792395000), "09:30 CEST = 07:30Z");
        assert_eq!(m.time_zone.as_deref(), Some("Europe/Warsaw"));
        assert_eq!(
            m.recurrence.as_ref().unwrap(),
            &vec!["RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR".to_string(), "EXDATE:20261022T073000Z".to_string()]
        );
        assert_eq!(m.meet_link.as_deref(), Some("https://meet.google.com/abc-defg-hij"));
        assert_eq!(m.description.as_deref(), Some("Daily sync"));
        assert_eq!((m.reminders_default, m.reminders.clone()), (false, vec![10]), "e-mail alarm ignored");
        assert_eq!(m.self_response().as_deref(), Some("accepted"));
        assert_eq!(m.organizer.as_deref(), Some("Anna"));

        let x = by("standup@google.com_20261020T073000Z");
        assert_eq!(x.recurring_event_id.as_deref(), Some("standup@google.com"));
        assert_eq!(x.original_start_ts, Some(1792395000 + 86400));

        let o = by("offsite@google.com");
        assert!(o.all_day);
        assert_eq!((o.start_date.as_deref(), o.end_date.as_deref()), (Some("2026-10-23"), Some("2026-10-24")));
        assert_eq!(o.summary.as_deref(), Some("Team offsite, Kraków"));

        let r = by("review@google.com");
        assert_eq!(r.summary.as_deref(), Some("Design review with a long title that gets folded"));
        assert_eq!(r.attendees.len(), 2);
        assert_eq!(r.attendees[1].name.as_deref(), Some("Kowalski, Jan"));
        assert!(r.attendees[1].optional);
        assert_eq!(r.self_response().as_deref(), Some("needsAction"));
        assert!(!r.organizer_self);
    }

    #[test]
    fn expands_series_with_exception_and_exdate() {
        let feed = parse(&sample(), "a", FEED_CAL);
        let from = 1792368000; // 2026-10-19T00:00Z, Monday
        let list = build_instances(feed.events, &[cal()], from, from + 7 * 86400);
        let standups: Vec<_> = list.iter().filter(|i| i.title == "Stand-up").collect();
        assert_eq!(standups.len(), 3, "Mon, Wed, Fri: Tue moved, Thu excluded");
        assert!(list.iter().any(|i| i.title == "Stand-up (moved)" && i.start == (1792395000 + 86400 + 5400) * 1000));
        assert!(list.iter().all(|i| !i.can_edit), "feed calendars are read-only");
    }

    #[test]
    fn small_helpers() {
        assert_eq!(parse_duration("-P0DT0H30M0S"), Some(-1800));
        assert_eq!(parse_duration("PT1H30M"), Some(5400));
        assert_eq!(parse_duration("P1W"), Some(604800));
        assert_eq!(parse_duration("P1M"), None);
        assert_eq!(resolve_tz("/mozilla.org/20050126_1/Europe/Berlin"), Some(chrono_tz::Europe::Berlin));
        assert_eq!(resolve_tz("Central European Standard Time"), Some(chrono_tz::Europe::Warsaw));
        assert_eq!(
            normalize_url(" webcal://calendar.google.com/calendar/ical/x/private-y/basic.ics").unwrap(),
            "https://calendar.google.com/calendar/ical/x/private-y/basic.ics"
        );
        assert!(normalize_url("calendar.google.com").is_err());
        assert_eq!(host("https://calendar.google.com/calendar/ical/x/basic.ics"), "calendar.google.com");
        assert_eq!(
            normalize_rrule("FREQ=WEEKLY;UNTIL=20261231", false, Some(chrono_tz::UTC)),
            "FREQ=WEEKLY;UNTIL=20261231T235959Z"
        );
    }

    /// Serves `body` with an ETag and answers 304 to a matching If-None-Match.
    async fn serve(body: Arc<Mutex<String>>, hits: Arc<AtomicUsize>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                let body = body.clone();
                let hits = hits.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
                    hits.fetch_add(1, Ordering::SeqCst);
                    let text = body.lock().unwrap().clone();
                    let etag = format!("\"{}\"", text.len());
                    let resp = if req.contains("/missing") {
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                    } else if req.contains(&format!("if-none-match: {etag}")) {
                        format!("HTTP/1.1 304 Not Modified\r\nETag: {etag}\r\nConnection: close\r\n\r\n")
                    } else {
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/calendar\r\nETag: {etag}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                            text.len()
                        )
                    };
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn add_and_poll_feed() {
        let body = Arc::new(Mutex::new(sample()));
        let hits = Arc::new(AtomicUsize::new(0));
        let base = serve(body.clone(), hits.clone()).await;
        let core = crate::test_core();

        let err = core.add_feed(&format!("{base}/missing.ics"), None).await.unwrap_err().to_string();
        assert!(err.contains("404"), "{err}");
        assert!(core.db.accounts().unwrap().is_empty());

        let url = format!("{base}/calendar/ical/me/private-secret/basic.ics");
        let acc = core.add_feed(&url, None).await.unwrap();
        assert_eq!((acc.kind.as_str(), acc.name.as_deref(), acc.email.as_str()), (KIND, Some("me@company.com"), "127.0.0.1"));
        assert!(core.add_feed(&url, Some("Again".into())).await.is_err(), "duplicate link");
        let c = core.db.calendar(&acc.id, FEED_CAL).unwrap().unwrap();
        assert!(!c.writable());
        assert_eq!(core.db.event(&acc.id, FEED_CAL, "review@google.com").unwrap().unwrap().attendees.len(), 2);

        // Unchanged feed: answered 304, nothing rewritten.
        let before = hits.load(Ordering::SeqCst);
        let acc = core.db.accounts().unwrap().remove(0);
        core.sync_feed(&acc, false).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), before, "throttled: checked moments ago");
        core.sync_feed(&acc, true).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), before + 1);

        // The offsite is removed upstream; the review moves.
        *body.lock().unwrap() = sample()
            .replace("UID:offsite@google.com", "UID:offsite2@google.com\r\nSTATUS:CANCELLED")
            .replace("DTSTART:20261021T120000Z\r\nDTEND:20261021T130000Z", "DTSTART:20261021T140000Z\r\nDTEND:20261021T150000Z");
        core.sync_feed(&acc, true).await.unwrap();
        assert!(core.db.event(&acc.id, FEED_CAL, "offsite@google.com").unwrap().is_none());
        assert_eq!(core.db.event(&acc.id, FEED_CAL, "review@google.com").unwrap().unwrap().start_ts, Some(1792591200));

        // Writes are refused.
        let target = crate::EditTarget {
            account_id: acc.id.clone(),
            calendar_id: FEED_CAL.into(),
            event_id: "review@google.com".into(),
            master_id: None,
            start: None,
            start_date: None,
        };
        assert!(core.respond(target, "accepted").is_err());

        core.remove_account(&acc.id).unwrap();
        assert!(core.db.accounts().unwrap().is_empty());
    }
}
