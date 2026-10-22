//! End-to-end: core ↔ fake Google over real HTTP. Covers first sync, batched incremental sync,
//! change notices, outbox insert with Meet, 412 conflict, single-occurrence delete, 410 resync, delete.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};

use crate::fake_google::{self, State};
use crate::models::{CoreEvent, EventInput, Notice};
use crate::{test_core_with, DeleteScope, EditTarget};

fn ev(id: &str, summary: &str, start: DateTime<Utc>, mins: i64, organizer: &str) -> Value {
    json!({
        "id": id, "status": "confirmed", "summary": summary,
        "start": {"dateTime": start.to_rfc3339(), "timeZone": "Europe/Moscow"},
        "end": {"dateTime": (start + Duration::minutes(mins)).to_rfc3339(), "timeZone": "Europe/Moscow"},
        "organizer": {"email": organizer, "self": organizer == "me@x.com"},
        "reminders": {"useDefault": true}
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn full_sync_cycle_against_fake_google() {
    let t0 = DateTime::from_timestamp((Utc::now() + Duration::days(2)).timestamp() / 3600 * 3600, 0).unwrap();

    let mut st = State::default();
    st.calendars = vec![
        json!({"id": "main", "summary": "Основной", "backgroundColor": "#5B8DEF", "accessRole": "owner", "primary": true,
               "timeZone": "Europe/Moscow", "defaultReminders": [{"method": "popup", "minutes": 10}], "selected": true}),
        json!({"id": "team", "summary": "Команда", "backgroundColor": "#39B8C9", "accessRole": "writer",
               "timeZone": "Europe/Moscow", "selected": true}),
    ];
    let mut standup = ev("standup", "Стендап", t0, 30, "me@x.com");
    standup["recurrence"] = json!(["RRULE:FREQ=DAILY;COUNT=10"]);
    st.put("main", standup);
    st.put("main", ev("demo", "Демо", t0 + Duration::hours(3), 60, "ilya@x.com"));
    st.put("team", ev("retro", "Ретро", t0 + Duration::hours(5), 60, "me@x.com"));
    let fake = fake_google::start(st).await;

    let notices: Arc<Mutex<Vec<Notice>>> = Arc::default();
    let sink = notices.clone();
    let core = test_core_with(
        Some(fake.base.clone()),
        Box::new(move |e| {
            if let CoreEvent::Notice(n) = e {
                sink.lock().unwrap().push(n)
            }
        }),
    );
    core.db.upsert_account("acc", "me@x.com", None).unwrap();
    core.tokens.lock().unwrap().insert("acc".into(), ("token".into(), i64::MAX / 2));

    let from = (t0 - Duration::hours(1)).timestamp_millis();
    let to = (t0 + Duration::days(3)).timestamp_millis();
    let titles = |core: &crate::Core| -> Vec<String> {
        core.instances(from, to).unwrap().into_iter().map(|i| i.title).collect()
    };

    // 1. First sync: calendars, events, sync tokens.
    core.sync_all().await;
    let acc = &core.db.accounts().unwrap()[0];
    assert_eq!(acc.status, "ok", "{:?}", acc.error);
    assert_eq!(core.db.calendars().unwrap().len(), 2);
    assert!(core.db.calendar_ids("acc").unwrap().iter().all(|(_, t)| t.is_some()));
    let t = titles(&core);
    assert_eq!(t.iter().filter(|x| *x == "Стендап").count(), 3);
    assert!(t.contains(&"Демо".to_string()) && t.contains(&"Ретро".to_string()));
    assert!(notices.lock().unwrap().is_empty(), "no notices on first load");

    // 2. Steady state goes through one batch call; a remote move shows up with a notice.
    let batches = fake.state.lock().unwrap().count("POST batch");
    {
        let mut s = fake.state.lock().unwrap();
        s.put("main", ev("demo", "Демо", t0 + Duration::hours(4), 60, "ilya@x.com"));
    }
    core.sync_all().await;
    assert_eq!(fake.state.lock().unwrap().count("POST batch"), batches + 1);
    let demo = core.db.event("acc", "main", "demo").unwrap().unwrap();
    assert_eq!(demo.start_ts, Some((t0 + Duration::hours(4)).timestamp()));
    assert!(notices.lock().unwrap().iter().any(|n| matches!(n, Notice::Moved { title, .. } if title == "Демо")));

    // 3. Local create with Meet reaches the server; the row stops being pending and gets the link.
    let key = core
        .create_event(EventInput {
            account_id: "acc".into(),
            calendar_id: "main".into(),
            title: "Созвон с Петей".into(),
            all_day: false,
            start: Some((t0 + Duration::hours(6)).timestamp_millis()),
            end: Some((t0 + Duration::hours(6) + Duration::minutes(30)).timestamp_millis()),
            start_date: None,
            end_date: None,
            time_zone: Some("Europe/Moscow".into()),
            location: None,
            description: None,
            reminders: Some(vec![5]),
            attendees: vec!["petya@x.com".into()],
            add_meet: true,
        })
        .unwrap();
    let id = key.rsplit('|').next().unwrap().to_string();
    core.sync_all().await;
    assert!(fake.state.lock().unwrap().get("main", &id).is_some(), "insert reached server");
    let row = core.db.event("acc", "main", &id).unwrap().unwrap();
    assert!(!row.pending);
    assert_eq!(row.meet_link.as_deref(), Some("https://meet.google.com/new-link"));
    assert_eq!(core.db.outbox_count().unwrap(), 0);

    // 4. Conflict: server changed the event after our last sync; our patch carries the old etag → 412.
    {
        let mut s = fake.state.lock().unwrap();
        s.put("main", ev("demo", "Демо (перенесено Ильёй)", t0 + Duration::hours(4), 60, "ilya@x.com"));
    }
    let target = |id: &str| EditTarget {
        account_id: "acc".into(),
        calendar_id: "main".into(),
        event_id: id.into(),
        master_id: None,
        start: None,
        start_date: None,
    };
    core.move_event(target("demo"), (t0 + Duration::hours(7)).timestamp_millis(), (t0 + Duration::hours(8)).timestamp_millis())
        .unwrap();
    core.sync_all().await;
    let demo = core.db.event("acc", "main", "demo").unwrap().unwrap();
    assert_eq!(demo.summary.as_deref(), Some("Демо (перенесено Ильёй)"), "server wins");
    assert!(notices.lock().unwrap().iter().any(|n| matches!(n, Notice::Conflict { .. })));

    // 5. Deleting one occurrence of the series hides it locally and clears the outbox.
    let occ = core.instances(from, to).unwrap().into_iter().filter(|i| i.title == "Стендап").nth(1).unwrap();
    core.delete_event(
        EditTarget {
            account_id: occ.account_id.clone(),
            calendar_id: occ.calendar_id.clone(),
            event_id: occ.event_id.clone(),
            master_id: occ.master_id.clone(),
            start: Some(occ.start),
            start_date: None,
        },
        DeleteScope::This,
    )
    .unwrap();
    core.sync_all().await;
    assert_eq!(titles(&core).iter().filter(|x| *x == "Стендап").count(), 2);
    assert_eq!(core.db.outbox_count().unwrap(), 0);

    // 6. Expired sync token (410) → transparent full resync.
    fake.state.lock().unwrap().expire_next = true;
    core.sync_all().await;
    assert_eq!(core.db.accounts().unwrap()[0].status, "ok");
    assert!(titles(&core).contains(&"Ретро".to_string()));

    // 7. Deleting a single event cancels it on the server.
    core.delete_event(
        EditTarget { calendar_id: "team".into(), ..target("retro") },
        DeleteScope::This,
    )
    .unwrap();
    assert!(!titles(&core).contains(&"Ретро".to_string()), "gone locally right away");
    core.sync_all().await;
    assert_eq!(fake.state.lock().unwrap().get("team", "retro").unwrap()["status"], "cancelled");
    assert!(core.db.event("acc", "team", "retro").unwrap().is_none());
}

/// OAuth installed-app flow end to end: loopback redirect, PKCE, code exchange, keyring, refresh.
#[tokio::test(flavor = "multi_thread")]
async fn oauth_login_and_refresh_against_fake_endpoints() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // Fake token + userinfo endpoint.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen2 = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else { break };
            let mut buf = vec![0u8; 16384];
            let n = s.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            seen2.lock().unwrap().push(req.clone());
            let body = if req.starts_with("POST /token") && req.contains("grant_type=authorization_code") {
                assert!(req.contains("code=the-code") && req.contains("code_verifier="), "PKCE exchange: {req}");
                r#"{"access_token":"at1","expires_in":3600,"refresh_token":"rt1"}"#
            } else if req.starts_with("POST /token") && req.contains("grant_type=refresh_token") {
                assert!(req.contains("refresh_token=rt1"));
                r#"{"access_token":"at2","expires_in":3600}"#
            } else if req.starts_with("GET /userinfo") {
                r#"{"sub":"kw-test-user-7f3a","email":"tester@example.com","name":"Тест"}"#
            } else {
                "{}"
            };
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = s.write_all(resp.as_bytes()).await;
        }
    });

    let mut core = crate::Core::with_db(
        crate::Db::open_in_memory().unwrap(),
        &std::env::temp_dir().join(format!("kw-test-{}", uuid::Uuid::new_v4().simple())),
        Box::new(|_| {}),
    )
    .unwrap();
    core.token_url = format!("http://127.0.0.1:{port}/token");
    core.userinfo_url = format!("http://127.0.0.1:{port}/userinfo");
    {
        let mut cfg = core.config.write().unwrap();
        cfg.google_client_id = "client-id".into();
        cfg.google_client_secret = "secret".into();
    }
    let core = Arc::new(core);

    // The "browser": follow the auth URL's redirect_uri back to the app with a code.
    let browser = |url: String| {
        let q = url.split_once('?').unwrap().1.to_string();
        let get = |k: &str| {
            q.split('&')
                .find_map(|p| p.strip_prefix(&format!("{k}=")))
                .map(|v| urlencoding::decode(v).unwrap().into_owned())
                .unwrap()
        };
        assert_eq!(get("code_challenge_method"), "S256");
        assert_eq!(get("access_type"), "offline");
        assert!(get("scope").contains("auth/calendar"));
        let redirect = get("redirect_uri");
        let state = get("state");
        tokio::spawn(async move {
            reqwest::get(format!("{redirect}/?state={state}&code=the-code&scope=x")).await.unwrap();
        });
    };
    let acc = core.add_account(browser, None).await.unwrap();
    assert_eq!(acc.email, "tester@example.com");
    assert_eq!(crate::auth::load_refresh_token("kw-test-user-7f3a").unwrap().as_deref(), Some("rt1"));

    // Expired access token → refresh via the stored refresh token.
    core.tokens.lock().unwrap().clear();
    assert_eq!(core.access_token("kw-test-user-7f3a").await.unwrap(), "at2");

    crate::auth::delete_refresh_token("kw-test-user-7f3a");
    assert!(crate::auth::load_refresh_token("kw-test-user-7f3a").unwrap().is_none());
    assert!(seen.lock().unwrap().iter().any(|r| r.starts_with("GET /userinfo") && r.contains("Bearer at1")));
}

/// Push: every calendar gets a watch channel pointing at the relay; a relay bump wakes sync.
#[tokio::test(flavor = "multi_thread")]
async fn push_watches_and_relay_wakeup() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut st = State::default();
    st.calendars = vec![
        json!({"id": "main", "summary": "Основной", "backgroundColor": "#5B8DEF", "accessRole": "owner", "primary": true, "selected": true}),
        json!({"id": "holidays", "summary": "Праздники", "backgroundColor": "#999999", "accessRole": "reader", "selected": true}),
    ];
    let fake = fake_google::start(st).await;

    // Fake relay: /wait answers with the current counter.
    let seq = Arc::new(AtomicU64::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let s2 = seq.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else { break };
            let s2 = s2.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf).await;
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                let body = format!("{{\"seq\":{}}}", s2.load(Ordering::SeqCst));
                let _ = s
                    .write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes())
                    .await;
            });
        }
    });

    let core = test_core_with(Some(fake.base.clone()), Box::new(|_| {}));
    let mut cfg = core.config();
    cfg.relay_url = relay.clone();
    core.set_config(cfg).unwrap();
    let key = core.config().relay_key;
    assert_eq!(key.len(), 32, "channel key generated");
    core.db.upsert_account("acc", "me@x.com", None).unwrap();
    core.tokens.lock().unwrap().insert("acc".into(), ("token".into(), i64::MAX / 2));

    core.sync_all().await;
    let w = core.db.watches("acc").unwrap();
    let main = w.iter().find(|x| x.0 == "main").unwrap();
    assert_eq!(main.2.as_deref(), Some("res-main"), "main calendar watched");
    let hol = w.iter().find(|x| x.0 == "holidays").unwrap();
    assert!(hol.1.is_none() && hol.3.is_some(), "unsupported calendar remembered, not retried every sync");
    let watches_before = fake.state.lock().unwrap().count("POST /calendar/v3/calendars/main/events/watch");
    core.sync_all().await;
    assert_eq!(
        fake.state.lock().unwrap().count("POST /calendar/v3/calendars/main/events/watch"),
        watches_before,
        "live channel is not re-created"
    );

    // Relay reports a change → the push loop triggers a sync.
    tokio::spawn(core.clone().run_push_loop());
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert!(core.push_active());
    seq.store(1, Ordering::SeqCst);
    tokio::time::timeout(std::time::Duration::from_secs(5), core.trigger.notified())
        .await
        .expect("push should wake the sync loop");
}
