//! Minimal in-process stand-in for the Google Calendar API v3, for end-to-end sync tests.
//! Implements the parts Kwalendar uses: calendarList, events list (full / window / syncToken),
//! get/insert/patch/delete with etags and If-Match, and multipart batch.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Default)]
pub struct State {
    pub calendars: Vec<Value>,
    /// cal id → (version of last change, event)
    pub events: HashMap<String, Vec<(u64, Value)>>,
    pub version: u64,
    /// Next incremental events.list answers 410 Gone.
    pub expire_next: bool,
    pub log: Vec<String>,
}

impl State {
    pub fn put(&mut self, cal: &str, mut ev: Value) -> Value {
        self.version += 1;
        ev["etag"] = json!(format!("\"{}\"", self.version));
        let id = ev["id"].as_str().unwrap_or_default().to_string();
        let list = self.events.entry(cal.to_string()).or_default();
        list.retain(|(_, e)| e["id"] != id.as_str());
        list.push((self.version, ev.clone()));
        ev
    }

    pub fn get(&self, cal: &str, id: &str) -> Option<Value> {
        self.events.get(cal)?.iter().find(|(_, e)| e["id"] == id).map(|(_, e)| e.clone())
    }

    pub fn count(&self, prefix: &str) -> usize {
        self.log.iter().filter(|l| l.starts_with(prefix)).count()
    }
}

pub struct Fake {
    /// API base to hand to the core, e.g. `http://127.0.0.1:1234/calendar/v3`.
    pub base: String,
    pub state: Arc<Mutex<State>>,
}

pub async fn start(state: State) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let state = Arc::new(Mutex::new(state));
    let st = state.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { break };
            let st = st.clone();
            tokio::spawn(async move {
                let Some((method, path, headers, body)) = read_request(&mut sock).await else { return };
                let (status, ctype, out) = if path.starts_with("/batch/") {
                    batch(&st, &headers, &body)
                } else {
                    let (s, v) = route(&st, &method, &path, &headers, &body);
                    (s, "application/json".to_string(), if v.is_null() { String::new() } else { v.to_string() })
                };
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
                    out.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    Fake { base: format!("http://127.0.0.1:{port}/calendar/v3"), state }
}

async fn read_request(sock: &mut tokio::net::TcpStream) -> Option<(String, String, HashMap<String, String>, String)> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let head_end = loop {
        let n = sock.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_string();
    let path = first.next()?.to_string();
    let headers: HashMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let len: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = sock.read(&mut tmp).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    Some((method, path, headers, String::from_utf8_lossy(&body).to_string()))
}

fn query(path: &str) -> (String, HashMap<String, String>) {
    let (p, q) = path.split_once('?').unwrap_or((path, ""));
    let map = q
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), urlencoding::decode(v).map(|c| c.into_owned()).unwrap_or_default()))
        .collect();
    (p.to_string(), map)
}

fn route(st: &Mutex<State>, method: &str, path: &str, headers: &HashMap<String, String>, body: &str) -> (u16, Value) {
    let (p, q) = query(path);
    let mut s = st.lock().unwrap();
    s.log.push(format!("{method} {p}{}", if q.contains_key("syncToken") { " sync" } else { "" }));
    if !headers.get("authorization").is_some_and(|a| a.starts_with("Bearer ")) {
        return (401, json!({"error": "unauthorized"}));
    }
    let segs: Vec<String> = p
        .trim_start_matches("/calendar/v3/")
        .split('/')
        .map(|x| urlencoding::decode(x).map(|c| c.into_owned()).unwrap_or_default())
        .collect();
    let segs: Vec<&str> = segs.iter().map(String::as_str).collect();
    match (method, segs.as_slice()) {
        ("GET", ["users", "me", "calendarList"]) => {
            let items = if q.contains_key("syncToken") { vec![] } else { s.calendars.clone() };
            (200, json!({"items": items, "nextSyncToken": "cl"}))
        }
        ("GET", ["calendars", cal, "events"]) => {
            let list = s.events.get(*cal).cloned().unwrap_or_default();
            if let Some(t) = q.get("syncToken") {
                if s.expire_next {
                    s.expire_next = false;
                    return (410, json!({"error": "fullSyncRequired"}));
                }
                let since: u64 = t.trim_start_matches('v').parse().unwrap_or(0);
                let items: Vec<Value> = list.into_iter().filter(|(v, _)| *v > since).map(|(_, e)| e).collect();
                return (200, json!({"items": items, "nextSyncToken": format!("v{}", s.version)}));
            }
            let items: Vec<Value> = list.into_iter().map(|(_, e)| e).filter(|e| e["status"] != "cancelled").collect();
            if q.contains_key("timeMin") {
                (200, json!({"items": items}))
            } else {
                (200, json!({"items": items, "nextSyncToken": format!("v{}", s.version)}))
            }
        }
        ("GET", ["calendars", cal, "events", id]) => match s.get(cal, id) {
            Some(e) => (200, e),
            None => (404, json!({})),
        },
        ("POST", ["calendars", cal, "events"]) => {
            let mut ev: Value = serde_json::from_str(body).unwrap_or(json!({}));
            if s.get(cal, ev["id"].as_str().unwrap_or("")).is_some() {
                return (409, json!({"error": "duplicate"}));
            }
            ev["status"] = json!("confirmed");
            ev["organizer"] = json!({"email": "me@x.com", "self": true});
            if ev.get("conferenceData").is_some() {
                ev["hangoutLink"] = json!("https://meet.google.com/new-link");
            }
            (200, s.put(cal, ev))
        }
        ("PATCH", ["calendars", cal, "events", id]) => {
            let Some(mut cur) = s.get(cal, id) else { return (404, json!({})) };
            if let Some(m) = headers.get("if-match") {
                if cur["etag"].as_str() != Some(m.as_str()) {
                    return (412, json!({"error": "conditionNotMet"}));
                }
            }
            let patch: Value = serde_json::from_str(body).unwrap_or(json!({}));
            for (k, v) in patch.as_object().cloned().unwrap_or_default() {
                cur[k] = v;
            }
            (200, s.put(cal, cur))
        }
        ("POST", ["calendars", cal, "events", "watch"]) => {
            let req: Value = serde_json::from_str(body).unwrap_or(json!({}));
            if *cal == "holidays" {
                return (400, json!({"error": "pushNotSupportedForRequestedResource"}));
            }
            let exp = chrono::Utc::now().timestamp_millis() + 7 * 86_400_000;
            (200, json!({"id": req["id"], "resourceId": format!("res-{cal}"), "address": req["address"], "token": req["token"], "expiration": exp.to_string()}))
        }
        ("POST", ["channels", "stop"]) => (204, Value::Null),
        ("DELETE", ["calendars", cal, "events", id]) => {
            let Some(mut cur) = s.get(cal, id) else { return (410, json!({})) };
            cur["status"] = json!("cancelled");
            s.put(cal, cur);
            (204, Value::Null)
        }
        _ => (404, json!({"error": format!("no route {method} {p}")})),
    }
}

fn batch(st: &Mutex<State>, headers: &HashMap<String, String>, body: &str) -> (u16, String, String) {
    st.lock().unwrap().log.push("POST batch".into());
    let boundary = headers
        .get("content-type")
        .and_then(|c| c.split("boundary=").nth(1))
        .unwrap_or("")
        .to_string();
    let mut out = String::new();
    for chunk in body.split(&format!("--{boundary}")) {
        let chunk = chunk.trim_start_matches("\r\n");
        if chunk.starts_with("--") || chunk.trim().is_empty() {
            continue;
        }
        let Some((outer, inner)) = chunk.split_once("\r\n\r\n") else { continue };
        let id = outer
            .lines()
            .find(|l| l.starts_with("Content-ID:"))
            .map(|l| l[11..].trim().trim_matches(['<', '>']).to_string())
            .unwrap_or_default();
        let mut req = inner.lines().next().unwrap_or("").split_whitespace();
        let (m, p) = (req.next().unwrap_or("GET"), req.next().unwrap_or("/"));
        let (status, v) = route(st, m, p, headers, "");
        out.push_str(&format!(
            "--resp_b\r\nContent-Type: application/http\r\nContent-ID: <response-{id}>\r\n\r\nHTTP/1.1 {status} X\r\nContent-Type: application/json\r\n\r\n{v}\r\n"
        ));
    }
    out.push_str("--resp_b--\r\n");
    (200, "multipart/mixed; boundary=resp_b".into(), out)
}
