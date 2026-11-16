//! Thin Google Calendar API v3 client and JSON → row mapping.

use chrono::{DateTime, NaiveDate};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

use crate::models::{Attendee, EventRow};

pub const DEFAULT_API: &str = "https://www.googleapis.com/calendar/v3";

/// Partial-response field mask: only what the app renders.
const EVENT_FIELDS: &str = "id,etag,status,summary,description,location,start,end,recurrence,recurringEventId,originalStartTime,hangoutLink,conferenceData(entryPoints(entryPointType,uri)),htmlLink,organizer(email,displayName,self),attendees(email,displayName,responseStatus,self,organizer,optional),reminders,colorId,updated";

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("sync token expired")]
    Gone,
    #[error("precondition failed")]
    PreconditionFailed,
    #[error("not found")]
    NotFound,
    #[error("rate limited")]
    RateLimited,
    #[error("access revoked")]
    Revoked,
    #[error("network: {0}")]
    Network(String),
    #[error("{0}")]
    Other(String),
}

impl From<reqwest::Error> for ApiError {
    fn from(e: reqwest::Error) -> Self {
        ApiError::Network(e.to_string())
    }
}

pub struct EventsPage {
    pub items: Vec<Value>,
    pub next_page_token: Option<String>,
    pub next_sync_token: Option<String>,
}

pub async fn request(
    http: &Client,
    token: &str,
    method: Method,
    url: &str,
    body: Option<&Value>,
    if_match: Option<&str>,
) -> Result<Value, ApiError> {
    let mut rb = http.request(method, url).bearer_auth(token);
    if let Some(b) = body {
        rb = rb.json(b);
    }
    if let Some(etag) = if_match {
        rb = rb.header("If-Match", etag);
    }
    let resp = rb.send().await?;
    let status = resp.status();
    if status.is_success() {
        if status == StatusCode::NO_CONTENT {
            return Ok(Value::Null);
        }
        let text = resp.text().await?;
        if text.is_empty() {
            return Ok(Value::Null);
        }
        return serde_json::from_str(&text).map_err(|e| ApiError::Other(e.to_string()));
    }
    let text = resp.text().await.unwrap_or_default();
    Err(status_error(status.as_u16(), &text))
}

fn status_error(status: u16, text: &str) -> ApiError {
    match status {
        401 => ApiError::Unauthorized,
        410 => ApiError::Gone,
        412 => ApiError::PreconditionFailed,
        404 => ApiError::NotFound,
        429 => ApiError::RateLimited,
        403 if text.contains("ateLimitExceeded") => ApiError::RateLimited,
        _ => ApiError::Other(format!("HTTP {status}: {}", truncate(text, 300))),
    }
}

/// Incremental `events.list` for many calendars in one HTTP round trip (Calendar API batch, ≤ 50 parts).
/// The outer error is for the batch call itself; each part carries its own result.
pub async fn batch_events(
    http: &Client,
    base: &str,
    token: &str,
    reqs: &[(String, String)],
) -> Result<Vec<Result<EventsPage, ApiError>>, ApiError> {
    use std::fmt::Write;
    let Some(idx) = base.find("/calendar/v3") else {
        return Err(ApiError::Other("batch: unexpected API base".into()));
    };
    let batch_url = format!("{}/batch{}", &base[..idx], &base[idx..]);
    let prefix = &base[idx..];
    let boundary = "kwalendar_batch";
    let fields = enc(&format!("nextPageToken,nextSyncToken,items({EVENT_FIELDS})"));
    let mut body = String::new();
    for (i, (cal, st)) in reqs.iter().enumerate() {
        let _ = write!(
            body,
            "--{boundary}\r\nContent-Type: application/http\r\nContent-ID: <item{i}>\r\n\r\nGET {prefix}/calendars/{}/events?maxResults=2500&singleEvents=false&syncToken={}&fields={fields}\r\n\r\n",
            enc(cal),
            enc(st)
        );
    }
    let _ = write!(body, "--{boundary}--\r\n");
    let resp = http
        .post(&batch_url)
        .bearer_auth(token)
        .header(reqwest::header::CONTENT_TYPE, format!("multipart/mixed; boundary={boundary}"))
        .body(body)
        .send()
        .await?;
    let status = resp.status();
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let text = resp.text().await?;
    if !status.is_success() {
        return Err(status_error(status.as_u16(), &text));
    }
    let resp_boundary = ct
        .split(';')
        .find_map(|p| p.trim().strip_prefix("boundary="))
        .map(|b| b.trim_matches('"').to_string())
        .ok_or_else(|| ApiError::Other("batch: no boundary".into()))?;

    let mut out: Vec<Result<EventsPage, ApiError>> =
        (0..reqs.len()).map(|_| Err(ApiError::Other("batch: part missing".into()))).collect();
    for part in parse_multipart(&text, &resp_boundary) {
        let Some(i) = part.content_id.and_then(|id| id.rsplit("item").next().and_then(|n| n.parse::<usize>().ok()))
        else {
            continue;
        };
        if i >= out.len() {
            continue;
        }
        out[i] = if (200..300).contains(&part.status) {
            serde_json::from_str::<Value>(&part.body)
                .map(|v| EventsPage {
                    items: v["items"].as_array().cloned().unwrap_or_default(),
                    next_page_token: v["nextPageToken"].as_str().map(String::from),
                    next_sync_token: v["nextSyncToken"].as_str().map(String::from),
                })
                .map_err(|e| ApiError::Other(format!("batch part: {e}")))
        } else {
            Err(status_error(part.status, &part.body))
        };
    }
    Ok(out)
}

struct Part {
    content_id: Option<String>,
    status: u16,
    body: String,
}

fn parse_multipart(text: &str, boundary: &str) -> Vec<Part> {
    let text = text.replace("\r\n", "\n");
    let mut parts = Vec::new();
    for chunk in text.split(&format!("--{boundary}")) {
        let chunk = chunk.trim_start_matches('\n');
        if chunk.starts_with("--") || chunk.trim().is_empty() {
            continue;
        }
        let Some((outer, inner)) = chunk.split_once("\n\n") else { continue };
        let content_id = outer
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("content-id:"))
            .map(|l| l[11..].trim().trim_matches(['<', '>']).to_string());
        let status = inner
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        let body = inner.split_once("\n\n").map(|(_, b)| b.trim().to_string()).unwrap_or_default();
        parts.push(Part { content_id, status, body });
    }
    parts
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn enc(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

pub async fn calendar_list(
    http: &Client,
    base: &str,
    token: &str,
    sync_token: Option<&str>,
) -> Result<(Vec<Value>, Option<String>), ApiError> {
    let mut items = Vec::new();
    let mut page: Option<String> = None;
    loop {
        let mut url = format!("{base}/users/me/calendarList?maxResults=250&showDeleted=true");
        if let Some(t) = sync_token {
            url.push_str(&format!("&syncToken={}", enc(t)));
        }
        if let Some(p) = &page {
            url.push_str(&format!("&pageToken={}", enc(p)));
        }
        let v = request(http, token, Method::GET, &url, None, None).await?;
        if let Some(arr) = v["items"].as_array() {
            items.extend(arr.iter().cloned());
        }
        page = v["nextPageToken"].as_str().map(String::from);
        if page.is_none() {
            return Ok((items, v["nextSyncToken"].as_str().map(String::from)));
        }
    }
}

/// One page of events. With `sync_token` → incremental; with `window` → bounded quick fetch (no sync token).
pub async fn events_page(
    http: &Client,
    base: &str,
    token: &str,
    calendar_id: &str,
    sync_token: Option<&str>,
    window: Option<(DateTime<chrono::Utc>, DateTime<chrono::Utc>)>,
    page_token: Option<&str>,
) -> Result<EventsPage, ApiError> {
    let mut url = format!(
        "{base}/calendars/{}/events?maxResults=2500&singleEvents=false&fields={}",
        enc(calendar_id),
        enc(&format!("nextPageToken,nextSyncToken,items({EVENT_FIELDS})"))
    );
    if let Some(t) = sync_token {
        url.push_str(&format!("&syncToken={}", enc(t)));
    } else if let Some((a, b)) = window {
        url.push_str(&format!("&timeMin={}&timeMax={}", enc(&a.to_rfc3339()), enc(&b.to_rfc3339())));
    }
    if let Some(p) = page_token {
        url.push_str(&format!("&pageToken={}", enc(p)));
    }
    let v = request(http, token, Method::GET, &url, None, None).await?;
    Ok(EventsPage {
        items: v["items"].as_array().cloned().unwrap_or_default(),
        next_page_token: v["nextPageToken"].as_str().map(String::from),
        next_sync_token: v["nextSyncToken"].as_str().map(String::from),
    })
}

pub async fn get_event(http: &Client, base: &str, token: &str, cal: &str, id: &str) -> Result<Value, ApiError> {
    let url = format!("{base}/calendars/{}/events/{}?fields={}", enc(cal), enc(id), enc(EVENT_FIELDS));
    request(http, token, Method::GET, &url, None, None).await
}

pub async fn insert_event(http: &Client, base: &str, token: &str, cal: &str, body: &Value) -> Result<Value, ApiError> {
    let url = format!(
        "{base}/calendars/{}/events?conferenceDataVersion=1&sendUpdates=all&fields={}",
        enc(cal),
        enc(EVENT_FIELDS)
    );
    request(http, token, Method::POST, &url, Some(body), None).await
}

pub async fn patch_event(
    http: &Client,
    base: &str,
    token: &str,
    cal: &str,
    id: &str,
    body: &Value,
    etag: Option<&str>,
    send_updates: &str,
) -> Result<Value, ApiError> {
    let url = format!(
        "{base}/calendars/{}/events/{}?conferenceDataVersion=1&sendUpdates={}&fields={}",
        enc(cal),
        enc(id),
        send_updates,
        enc(EVENT_FIELDS)
    );
    request(http, token, Method::PATCH, &url, Some(body), etag).await
}

/// Subscribes a webhook channel to changes of a calendar (push notifications).
pub async fn watch_events(http: &Client, base: &str, token: &str, cal: &str, address: &str, channel_token: &str) -> Result<Value, ApiError> {
    let url = format!("{base}/calendars/{}/events/watch", enc(cal));
    let body = serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "type": "web_hook",
        "address": address,
        "token": channel_token,
        "params": {"ttl": "604800"}
    });
    request(http, token, Method::POST, &url, Some(&body), None).await
}

pub async fn stop_channel(http: &Client, base: &str, token: &str, id: &str, resource: &str) -> Result<(), ApiError> {
    let url = format!("{base}/channels/stop");
    request(http, token, Method::POST, &url, Some(&serde_json::json!({"id": id, "resourceId": resource})), None)
        .await
        .map(|_| ())
}

pub async fn delete_event(http: &Client, base: &str, token: &str, cal: &str, id: &str) -> Result<(), ApiError> {
    let url = format!("{base}/calendars/{}/events/{}?sendUpdates=all", enc(cal), enc(id));
    request(http, token, Method::DELETE, &url, None, None).await.map(|_| ())
}

// ---------- mapping ----------

fn parse_time(v: &Value) -> (Option<i64>, Option<String>, Option<String>) {
    // (ts, date, tz)
    let tz = v["timeZone"].as_str().map(String::from);
    if let Some(dt) = v["dateTime"].as_str() {
        let ts = DateTime::parse_from_rfc3339(dt).ok().map(|d| d.timestamp());
        return (ts, None, tz);
    }
    if let Some(d) = v["date"].as_str() {
        return (None, Some(d.to_string()), tz);
    }
    (None, None, tz)
}

pub(crate) fn find_link(text: &str) -> Option<String> {
    const HOSTS: [&str; 4] = ["meet.google.com/", "zoom.us/", "teams.microsoft.com/", "telemost.yandex.ru/"];
    for word in text.split(|c: char| c.is_whitespace() || c == '"' || c == '<' || c == '>' || c == '\'') {
        if word.starts_with("https://") && HOSTS.iter().any(|h| word.contains(h)) {
            return Some(word.trim_end_matches(['.', ',', ')', ';']).to_string());
        }
    }
    None
}

/// Maps a Google event resource to a row. Returns None for items we can't place in time.
pub fn parse_event(account: &str, calendar: &str, v: &Value) -> Option<EventRow> {
    let id = v["id"].as_str()?.to_string();
    let status = v["status"].as_str().unwrap_or("confirmed").to_string();
    let (start_ts, start_date, tz) = parse_time(&v["start"]);
    let (end_ts, end_date, _) = parse_time(&v["end"]);
    let (orig_ts, orig_date, _) = parse_time(&v["originalStartTime"]);
    let all_day = start_date.is_some();

    let meet_link = v["hangoutLink"]
        .as_str()
        .map(String::from)
        .or_else(|| {
            v["conferenceData"]["entryPoints"].as_array().and_then(|eps| {
                eps.iter()
                    .find(|e| e["entryPointType"] == "video")
                    .and_then(|e| e["uri"].as_str().map(String::from))
            })
        })
        .or_else(|| v["location"].as_str().and_then(find_link))
        .or_else(|| v["description"].as_str().and_then(find_link));

    let attendees = v["attendees"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    Some(Attendee {
                        email: a["email"].as_str()?.to_string(),
                        name: a["displayName"].as_str().map(String::from),
                        response: a["responseStatus"].as_str().unwrap_or("needsAction").to_string(),
                        is_self: a["self"].as_bool().unwrap_or(false),
                        organizer: a["organizer"].as_bool().unwrap_or(false),
                        optional: a["optional"].as_bool().unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let reminders_default = v["reminders"]["useDefault"].as_bool().unwrap_or(true);
    let reminders = v["reminders"]["overrides"]
        .as_array()
        .map(|o| {
            o.iter()
                .filter(|r| r["method"] == "popup")
                .filter_map(|r| r["minutes"].as_i64())
                .collect()
        })
        .unwrap_or_default();

    let recurrence = v["recurrence"].as_array().map(|r| {
        r.iter().filter_map(|l| l.as_str().map(String::from)).collect::<Vec<_>>()
    });

    if status != "cancelled" && start_ts.is_none() && start_date.is_none() {
        return None;
    }

    Some(EventRow {
        account_id: account.to_string(),
        calendar_id: calendar.to_string(),
        id,
        etag: v["etag"].as_str().map(String::from),
        status,
        summary: v["summary"].as_str().map(String::from),
        description: v["description"].as_str().map(String::from),
        location: v["location"].as_str().map(String::from),
        all_day,
        start_ts,
        end_ts: end_ts.or(start_ts),
        start_date: start_date.clone(),
        end_date: end_date.or_else(|| {
            start_date.as_deref().and_then(|d| {
                NaiveDate::parse_from_str(d, "%Y-%m-%d").ok().map(|n| (n + chrono::Duration::days(1)).to_string())
            })
        }),
        time_zone: tz,
        recurrence: recurrence.filter(|r| !r.is_empty()),
        recurring_event_id: v["recurringEventId"].as_str().map(String::from),
        original_start_ts: orig_ts,
        original_start_date: orig_date,
        meet_link,
        html_link: v["htmlLink"].as_str().map(String::from),
        organizer: v["organizer"]["displayName"]
            .as_str()
            .or(v["organizer"]["email"].as_str())
            .map(String::from),
        organizer_self: v["organizer"]["self"].as_bool().unwrap_or(false),
        attendees,
        reminders_default,
        reminders,
        color_id: v["colorId"].as_str().map(String::from),
        updated: v["updated"].as_str().map(String::from),
        pending: false,
    })
}

/// Builds a Google event body from a row (used for inserts and full patches).
pub fn event_body(e: &EventRow) -> Value {
    let mut body = json!({
        "summary": e.summary.clone().unwrap_or_default(),
    });
    if let Some(l) = &e.location {
        body["location"] = json!(l);
    }
    if let Some(d) = &e.description {
        body["description"] = json!(d);
    }
    let (start, end) = time_json(e);
    body["start"] = start;
    body["end"] = end;
    body["reminders"] = reminders_json(e.reminders_default, &e.reminders);
    if !e.attendees.is_empty() {
        body["attendees"] = json!(e
            .attendees
            .iter()
            .map(|a| json!({"email": a.email, "responseStatus": a.response}))
            .collect::<Vec<_>>());
    }
    body
}

pub fn time_json(e: &EventRow) -> (Value, Value) {
    if e.all_day {
        (json!({"date": e.start_date}), json!({"date": e.end_date}))
    } else {
        let fmt = |ts: Option<i64>| {
            DateTime::from_timestamp(ts.unwrap_or(0), 0)
                .map(|d| d.to_rfc3339())
                .unwrap_or_default()
        };
        let mut s = json!({"dateTime": fmt(e.start_ts)});
        let mut en = json!({"dateTime": fmt(e.end_ts)});
        if let Some(tz) = &e.time_zone {
            s["timeZone"] = json!(tz);
            en["timeZone"] = json!(tz);
        }
        (s, en)
    }
}

pub fn reminders_json(use_default: bool, minutes: &[i64]) -> Value {
    if use_default {
        json!({"useDefault": true})
    } else {
        json!({
            "useDefault": false,
            "overrides": minutes.iter().map(|m| json!({"method": "popup", "minutes": m})).collect::<Vec<_>>()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timed_event_with_meet() {
        let v = json!({
            "id": "abc", "etag": "\"1\"", "status": "confirmed", "summary": "Review",
            "start": {"dateTime": "2026-10-22T15:00:00+03:00", "timeZone": "Europe/Moscow"},
            "end": {"dateTime": "2026-10-22T16:30:00+03:00"},
            "conferenceData": {"entryPoints": [{"entryPointType": "video", "uri": "https://meet.google.com/x"}]},
            "attendees": [{"email": "me@x", "self": true, "responseStatus": "accepted"}],
            "reminders": {"useDefault": false, "overrides": [{"method": "popup", "minutes": 10}, {"method": "email", "minutes": 60}]}
        });
        let e = parse_event("a", "c", &v).unwrap();
        assert_eq!(e.start_ts, Some(1792670400));
        assert_eq!(e.end_ts.unwrap() - e.start_ts.unwrap(), 5400);
        assert_eq!(e.meet_link.as_deref(), Some("https://meet.google.com/x"));
        assert_eq!(e.reminders, vec![10]);
        assert!(!e.reminders_default);
        assert_eq!(e.self_response().as_deref(), Some("accepted"));
    }

    #[test]
    fn parses_all_day_and_cancelled() {
        let v = json!({"id": "d", "start": {"date": "2026-10-24"}, "end": {"date": "2026-10-25"}});
        let e = parse_event("a", "c", &v).unwrap();
        assert!(e.all_day);
        assert_eq!(e.end_date.as_deref(), Some("2026-10-25"));
        let c = json!({"id": "x", "status": "cancelled"});
        assert_eq!(parse_event("a", "c", &c).unwrap().status, "cancelled");
    }

    #[test]
    fn finds_zoom_link_in_description() {
        assert_eq!(
            find_link("Join: https://us02web.zoom.us/j/123?pwd=x."),
            Some("https://us02web.zoom.us/j/123?pwd=x".into())
        );
    }
}
