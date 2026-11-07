//! Incremental Google → local sync and local → Google outbox push.

use std::sync::atomic::Ordering;
use std::time::Duration;

use futures::StreamExt;
use serde_json::Value;

use crate::db::Db;
use crate::google::{self, ApiError};
use crate::models::{Account, EventRow, Notice};
use crate::{auth, Core};

/// Window fetched first on a brand-new calendar so the screen fills in seconds.
const QUICK_PAST_DAYS: i64 = 30;
const QUICK_FUTURE_DAYS: i64 = 90;
const CALENDAR_CONCURRENCY: usize = 4;

impl Core {
    pub(crate) async fn access_token(&self, account: &str) -> Result<String, ApiError> {
        let now = chrono::Utc::now().timestamp();
        if let Some((t, exp)) = self.tokens.lock().unwrap().get(account) {
            if *exp - 60 > now {
                return Ok(t.clone());
            }
        }
        let (client_id, client_secret) = self.config().client();
        let (tok, exp) = auth::refresh(&self.http, &self.token_url, &client_id, &client_secret, account).await?;
        self.tokens.lock().unwrap().insert(account.to_string(), (tok.clone(), exp));
        Ok(tok)
    }

    fn drop_token(&self, account: &str) {
        self.tokens.lock().unwrap().remove(account);
    }

    pub(crate) async fn sync_all(&self) {
        let Ok(_guard) = self.sync_lock.try_lock() else { return };
        let accounts: Vec<Account> = self
            .db
            .accounts()
            .unwrap_or_default()
            .into_iter()
            .filter(|a| a.status != "reauth")
            .collect();
        if accounts.is_empty() {
            self.set_sync_state("idle", None);
            return;
        }
        self.set_sync_state("syncing", None);
        let results = futures::future::join_all(accounts.iter().map(|a| self.sync_account_retry(a))).await;
        let before: Vec<String> = accounts.iter().map(|a| a.status.clone()).collect();

        let mut offline = false;
        let mut error = None;
        for (acc, res) in accounts.iter().zip(results) {
            match res {
                Ok(()) => {}
                Err(ApiError::Network(e)) => {
                    log::warn!("{}: offline: {e}", acc.email);
                    offline = true;
                    let _ = self.db.set_account_status(&acc.id, "offline", Some(&e));
                }
                Err(ApiError::Revoked) => {
                    let _ = self.db.set_account_status(&acc.id, "reauth", Some(&crate::tr!("Доступ отозван — войдите снова", "Access revoked — sign in again")));
                    self.notice(Notice::Reauth { email: acc.email.clone() });
                }
                Err(e) => {
                    log::error!("{}: sync failed: {e}", acc.email);
                    let _ = self.db.set_account_status(&acc.id, "error", Some(&e.to_string()));
                    error = Some(e.to_string());
                }
            }
        }
        if offline {
            self.set_sync_state("offline", None);
        } else if let Some(e) = error {
            self.set_sync_state("error", Some(e));
        } else {
            self.set_sync_state("idle", None);
        }
        let after: Vec<String> = self
            .db
            .accounts()
            .unwrap_or_default()
            .into_iter()
            .filter(|a| accounts.iter().any(|b| b.id == a.id))
            .map(|a| a.status)
            .collect();
        if self.dirty.swap(false, Ordering::Relaxed) || before != after {
            self.changed();
        }
    }

    async fn sync_account_retry(&self, acc: &Account) -> Result<(), ApiError> {
        let mut delay = Duration::from_secs(2);
        for attempt in 0..3 {
            match self.sync_account(acc).await {
                Err(ApiError::Unauthorized) if attempt == 0 => self.drop_token(&acc.id),
                Err(ApiError::RateLimited) if attempt < 2 => {
                    let jitter = rand::random::<u64>() % 1000;
                    tokio::time::sleep(delay + Duration::from_millis(jitter)).await;
                    delay *= 2;
                }
                other => return other,
            }
        }
        Err(ApiError::RateLimited)
    }

    async fn sync_account(&self, acc: &Account) -> Result<(), ApiError> {
        let token = self.access_token(&acc.id).await?;
        self.push_outbox(&acc.id, &token).await?;
        self.sync_calendar_list(&acc.id, &token).await?;
        self.ensure_watches(&acc.id, &token).await?;

        let all = self.db.calendar_ids(&acc.id).map_err(other)?;
        let (incremental, fresh): (Vec<_>, Vec<_>) = all.into_iter().partition(|(_, t)| t.is_some());
        // Steady state: one batched round trip checks every calendar of the account.
        let mut cals = fresh;
        if incremental.len() >= 2 {
            cals.extend(self.sync_batch(&acc.id, incremental, &token).await?);
        } else {
            cals.extend(incremental);
        }
        let results: Vec<Result<(), ApiError>> = futures::stream::iter(cals)
            .map(|(cal, sync_token)| {
                let token = token.clone();
                let account = acc.id.clone();
                async move { self.sync_calendar(&account, &cal, sync_token, &token).await }
            })
            .buffer_unordered(CALENDAR_CONCURRENCY)
            .collect()
            .await;
        for r in results {
            r?;
        }
        self.db
            .mark_account_synced(&acc.id, chrono::Utc::now().timestamp_millis())
            .map_err(other)?;
        Ok(())
    }

    /// Applies single-page incremental results from batches of up to 50 calendars.
    /// Returns calendars that still need the regular path (more pages, expired token, part errors).
    async fn sync_batch(
        &self,
        account: &str,
        cals: Vec<(String, Option<String>)>,
        token: &str,
    ) -> Result<Vec<(String, Option<String>)>, ApiError> {
        let mut rest = Vec::new();
        let notify = self.config().notify_changes;
        for chunk in cals.chunks(50) {
            let reqs: Vec<(String, String)> =
                chunk.iter().map(|(c, t)| (c.clone(), t.clone().unwrap_or_default())).collect();
            let results = match google::batch_events(&self.http, &self.api_base, token, &reqs).await {
                Ok(r) => r,
                Err(ApiError::Unauthorized) => return Err(ApiError::Unauthorized),
                Err(ApiError::Network(e)) => return Err(ApiError::Network(e)),
                Err(e) => {
                    log::warn!("batch failed ({e}), falling back to per-calendar requests");
                    rest.extend(chunk.iter().cloned());
                    continue;
                }
            };
            for ((cal, st), res) in chunk.iter().zip(results) {
                match res {
                    Ok(page) if page.next_page_token.is_none() => {
                        if !page.items.is_empty() {
                            self.dirty.store(true, Ordering::Relaxed);
                            let notices = self.apply_items(account, cal, &page.items, true).map_err(other)?;
                            if notify {
                                notices.into_iter().for_each(|n| self.notice(n));
                            }
                        }
                        if let Some(next) = page.next_sync_token {
                            self.db.set_sync_token(account, cal, Some(&next)).map_err(other)?;
                        }
                    }
                    Ok(_) | Err(ApiError::Gone) | Err(ApiError::Other(_)) | Err(ApiError::NotFound) => {
                        rest.push((cal.clone(), st.clone()))
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(rest)
    }

    async fn sync_calendar_list(&self, account: &str, token: &str) -> Result<(), ApiError> {
        let sync_token = self.db.cal_list_token(account).map_err(other)?;
        let (items, next) = match google::calendar_list(&self.http, &self.api_base, token, sync_token.as_deref()).await {
            Err(ApiError::Gone) => {
                self.db.set_cal_list_token(account, None).map_err(other)?;
                google::calendar_list(&self.http, &self.api_base, token, None).await?
            }
            r => r?,
        };
        let full = sync_token.is_none();
        if !items.is_empty() {
            self.dirty.store(true, Ordering::Relaxed);
        }
        let mut seen = Vec::new();
        for c in &items {
            let Some(id) = c["id"].as_str() else { continue };
            if c["deleted"].as_bool().unwrap_or(false) {
                self.db.delete_calendar(account, id).map_err(other)?;
                continue;
            }
            seen.push(id.to_string());
            let defaults: Vec<i64> = c["defaultReminders"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter(|r| r["method"] == "popup")
                        .filter_map(|r| r["minutes"].as_i64())
                        .collect()
                })
                .unwrap_or_default();
            let primary = c["primary"].as_bool().unwrap_or(false);
            let summary = c["summaryOverride"].as_str().or(c["summary"].as_str()).unwrap_or(id);
            self.db
                .upsert_calendar(
                    account,
                    id,
                    summary,
                    c["backgroundColor"].as_str().unwrap_or("#5B8DEF"),
                    c["accessRole"].as_str().unwrap_or("reader"),
                    primary,
                    c["timeZone"].as_str(),
                    &defaults,
                    c["selected"].as_bool().unwrap_or(primary) && !c["hidden"].as_bool().unwrap_or(false),
                )
                .map_err(other)?;
        }
        if full {
            for (id, _) in self.db.calendar_ids(account).map_err(other)? {
                if !seen.contains(&id) {
                    self.db.delete_calendar(account, &id).map_err(other)?;
                }
            }
        }
        self.db.set_cal_list_token(account, next.as_deref()).map_err(other)?;
        Ok(())
    }

    async fn sync_calendar(
        &self,
        account: &str,
        cal: &str,
        sync_token: Option<String>,
        token: &str,
    ) -> Result<(), ApiError> {
        if let Some(st) = sync_token {
            match self.pull_pages(account, cal, Some(&st), None, token, true).await {
                Err(ApiError::Gone) => {
                    log::info!("{cal}: sync token expired, full resync");
                    self.db.set_sync_token(account, cal, None).map_err(other)?;
                    self.db.clear_calendar_events(account, cal).map_err(other)?;
                }
                r => return r,
            }
        }
        // First time: fill the visible window quickly, then do the full pass that yields a sync token.
        let now = chrono::Utc::now();
        let window = (
            now - chrono::Duration::days(QUICK_PAST_DAYS),
            now + chrono::Duration::days(QUICK_FUTURE_DAYS),
        );
        self.pull_pages(account, cal, None, Some(window), token, false).await?;
        self.changed();
        self.pull_pages(account, cal, None, None, token, false).await
    }

    async fn pull_pages(
        &self,
        account: &str,
        cal: &str,
        sync_token: Option<&str>,
        window: Option<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>,
        token: &str,
        incremental: bool,
    ) -> Result<(), ApiError> {
        let mut page: Option<String> = None;
        let mut changed = false;
        loop {
            let p = google::events_page(&self.http, &self.api_base, token, cal, sync_token, window, page.as_deref()).await?;
            if !p.items.is_empty() {
                changed = true;
                let notices = self.apply_items(account, cal, &p.items, incremental).map_err(other)?;
                let cfg = self.config();
                if cfg.notify_changes {
                    for n in notices {
                        self.notice(n);
                    }
                }
            }
            match p.next_page_token {
                Some(t) => page = Some(t),
                None => {
                    if window.is_none() {
                        if let Some(next) = p.next_sync_token {
                            self.db.set_sync_token(account, cal, Some(&next)).map_err(other)?;
                        }
                    }
                    break;
                }
            }
        }
        if changed {
            self.dirty.store(true, Ordering::Relaxed);
        }
        Ok(())
    }

    fn apply_items(&self, account: &str, cal: &str, items: &[Value], incremental: bool) -> anyhow::Result<Vec<Notice>> {
        let mut notices = Vec::new();
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        let pending: std::collections::HashSet<String> = {
            let mut st = tx.prepare("SELECT event_id FROM outbox WHERE account_id = ?1 AND calendar_id = ?2")?;
            let ids = st
                .query_map([account, cal], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<_>>()?;
            ids
        };
        for item in items {
            let Some(row) = google::parse_event(account, cal, item) else { continue };
            if pending.contains(&row.id) {
                continue; // local change not pushed yet wins
            }
            let existing = Db::event_conn(&tx, account, cal, &row.id)?;
            if row.status == "cancelled" && row.recurring_event_id.is_none() {
                tx.execute(
                    "DELETE FROM events WHERE account_id = ?1 AND calendar_id = ?2 AND (id = ?3 OR recurring_event_id = ?3)",
                    [account, cal, row.id.as_str()],
                )?;
                continue;
            }
            if row.status == "cancelled" {
                // Cancelled occurrence of a series: keep only what is needed to suppress it.
                let mut r = existing.clone().unwrap_or(row.clone());
                r.status = "cancelled".into();
                r.recurring_event_id = row.recurring_event_id.clone();
                r.original_start_ts = row.original_start_ts.or(r.original_start_ts);
                r.original_start_date = row.original_start_date.clone().or(r.original_start_date);
                r.recurrence = None;
                r.pending = false;
                Db::upsert_event_conn(&tx, &r)?;
                continue;
            }
            if incremental {
                if let Some(n) = detect_notice(existing.as_ref(), &row) {
                    notices.push(n);
                }
            }
            Db::upsert_event_conn(&tx, &row)?;
        }
        tx.commit()?;
        Ok(notices)
    }

    async fn push_outbox(&self, account: &str, token: &str) -> Result<(), ApiError> {
        let items = self.db.outbox_for(account).map_err(other)?;
        if items.is_empty() {
            return Ok(());
        }
        let mut touched = false;
        for item in items {
            let payload: Value = item
                .payload
                .as_deref()
                .and_then(|p| serde_json::from_str(p).ok())
                .unwrap_or(Value::Null);
            let body = &payload["body"];
            let send = payload["send"].as_str().unwrap_or("all");
            let (cal, id) = (item.calendar_id.as_str(), item.event_id.as_str());
            let res = match item.op.as_str() {
                "insert" => google::insert_event(&self.http, &self.api_base, token, cal, body).await.map(Some),
                "patch" => google::patch_event(&self.http, &self.api_base, token, cal, id, body, item.etag.as_deref(), send)
                    .await
                    .map(Some),
                "delete" => google::delete_event(&self.http, &self.api_base, token, cal, id).await.map(|_| None),
                _ => Ok(None),
            };
            touched = true;
            match res {
                Ok(v) => {
                    self.db.outbox_done(item.id).map_err(other)?;
                    self.after_push(account, &item.op, cal, id, v).map_err(other)?;
                }
                Err(ApiError::PreconditionFailed) => {
                    self.db.outbox_done(item.id).map_err(other)?;
                    let title = self
                        .db
                        .event(account, cal, id)
                        .ok()
                        .flatten()
                        .and_then(|e| e.summary)
                        .unwrap_or_default();
                    if let Ok(v) = google::get_event(&self.http, &self.api_base, token, cal, id).await {
                        if let Some(row) = google::parse_event(account, cal, &v) {
                            self.db.upsert_event(&row).map_err(other)?;
                        }
                    }
                    self.notice(Notice::Conflict { title });
                }
                Err(ApiError::NotFound) | Err(ApiError::Gone) => {
                    // The event no longer exists on the server.
                    self.db.outbox_done(item.id).map_err(other)?;
                    if self.db.event(account, cal, id).ok().flatten().is_some_and(|e| e.recurring_event_id.is_none()) {
                        self.db.delete_event(account, cal, id).map_err(other)?;
                    }
                }
                Err(ApiError::Other(msg)) if item.op == "insert" && msg.contains("HTTP 409") => {
                    // Retried insert with our client-generated id: already there.
                    self.db.outbox_done(item.id).map_err(other)?;
                    if let Ok(v) = google::get_event(&self.http, &self.api_base, token, cal, id).await {
                        self.after_push(account, "insert", cal, id, Some(v)).map_err(other)?;
                    }
                }
                Err(ApiError::Other(msg)) => {
                    log::error!("outbox {} {} failed: {msg}", item.op, id);
                    self.db.outbox_failed(item.id, &msg).map_err(other)?;
                    let attempts: i64 = self
                        .db
                        .conn()
                        .query_row("SELECT attempts FROM outbox WHERE id = ?1", [item.id], |r| r.get(0))
                        .unwrap_or(0);
                    if attempts >= 5 {
                        self.db.outbox_done(item.id).map_err(other)?;
                        self.notice(Notice::Error { message: crate::tr!("Не удалось сохранить изменение: {msg}", "Could not save a change: {msg}") });
                    }
                    break; // keep order: later edits depend on this one
                }
                Err(e) => return Err(e),
            }
        }
        if touched {
            self.dirty.store(true, Ordering::Relaxed);
        }
        Ok(())
    }

    fn after_push(&self, account: &str, op: &str, cal: &str, id: &str, server: Option<Value>) -> anyhow::Result<()> {
        let still_pending = self.db.has_pending_for(account, cal, id)?;
        if let Some(v) = server {
            if let Some(mut row) = google::parse_event(account, cal, &v) {
                row.pending = still_pending;
                if !still_pending {
                    self.db.upsert_event(&row)?;
                } else if let Some(mut local) = self.db.event(account, cal, id)? {
                    // Newer local edits are queued: only adopt the server etag so the next patch matches.
                    local.etag = row.etag;
                    self.db.upsert_event(&local)?;
                }
            }
        } else if op == "delete" && !still_pending {
            match self.db.event(account, cal, id)? {
                Some(e) if e.recurring_event_id.is_some() => {
                    let mut e = e;
                    e.pending = false;
                    self.db.upsert_event(&e)?;
                }
                Some(_) => self.db.delete_event(account, cal, id)?,
                None => {}
            }
        }
        Ok(())
    }
}

fn detect_notice(existing: Option<&EventRow>, new: &EventRow) -> Option<Notice> {
    let title = new.summary.clone().unwrap_or_else(crate::recurrence::untitled);
    match existing {
        Some(old) if old.status != "cancelled" => {
            if new.organizer_self || new.recurrence.is_some() {
                return None;
            }
            let moved_timed = !new.all_day && old.start_ts.is_some() && old.start_ts != new.start_ts;
            let moved_day = new.all_day && old.start_date.is_some() && old.start_date != new.start_date;
            if moved_timed || moved_day {
                let ms = |e: &EventRow| {
                    e.start_ts
                        .map(|t| t * 1000)
                        .or_else(|| e.start_date.as_deref().map(crate::recurrence::local_midnight_ms))
                        .unwrap_or(0)
                };
                return Some(Notice::Moved { title, old_start: ms(old), new_start: ms(new), all_day: new.all_day });
            }
            None
        }
        None => {
            let needs = new.self_response().as_deref() == Some("needsAction") && !new.organizer_self;
            let upcoming = new.start_ts.map(|t| t > chrono::Utc::now().timestamp()).unwrap_or(true);
            if needs && upcoming {
                Some(Notice::Invite {
                    title,
                    start: new
                        .start_ts
                        .map(|t| t * 1000)
                        .or_else(|| new.start_date.as_deref().map(crate::recurrence::local_midnight_ms))
                        .unwrap_or(0),
                    organizer: new.organizer.clone(),
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

fn other(e: impl std::fmt::Display) -> ApiError {
    ApiError::Other(e.to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::models::Notice;
    use crate::test_core;

    #[test]
    fn applies_deltas_and_reports_changes() {
        let core = test_core();
        core.db.upsert_account("acc", "me@x.com", None).unwrap();
        core.db
            .upsert_calendar("acc", "cal", "C", "#5B8DEF", "owner", true, Some("Europe/Moscow"), &[10], true)
            .unwrap();
        let future = chrono::Utc::now() + chrono::Duration::days(3);
        let at = |h: i64| (future + chrono::Duration::hours(h)).to_rfc3339();

        // Full sync: a series, a single event and an invitation — no notices on first load.
        let first = vec![
            json!({"id": "s", "status": "confirmed", "summary": "Стендап",
                   "start": {"dateTime": at(0), "timeZone": "Europe/Moscow"}, "end": {"dateTime": at(1)},
                   "recurrence": ["RRULE:FREQ=DAILY;COUNT=10"], "organizer": {"self": true}}),
            json!({"id": "demo", "status": "confirmed", "summary": "Демо",
                   "start": {"dateTime": at(2)}, "end": {"dateTime": at(3)}, "organizer": {"email": "ilya@x.com"}}),
        ];
        assert!(core.apply_items("acc", "cal", &first, false).unwrap().is_empty());

        // Incremental: demo moved by someone else, one standup cancelled, a new invite.
        let delta = vec![
            json!({"id": "demo", "status": "confirmed", "summary": "Демо",
                   "start": {"dateTime": at(4)}, "end": {"dateTime": at(5)}, "organizer": {"email": "ilya@x.com"}}),
            json!({"id": "s_x", "status": "cancelled", "recurringEventId": "s",
                   "originalStartTime": {"dateTime": at(24)}}),
            json!({"id": "inv", "status": "confirmed", "summary": "Интервью",
                   "start": {"dateTime": at(6)}, "end": {"dateTime": at(7)}, "organizer": {"email": "hr@x.com"},
                   "attendees": [{"email": "me@x.com", "self": true, "responseStatus": "needsAction"}]}),
        ];
        let notices = core.apply_items("acc", "cal", &delta, true).unwrap();
        assert!(notices.iter().any(|n| matches!(n, Notice::Moved { title, .. } if title == "Демо")));
        assert!(notices.iter().any(|n| matches!(n, Notice::Invite { title, .. } if title == "Интервью")));

        let from = future.timestamp_millis() - 3600_000;
        let list = core.instances(from, from + 4 * 86_400_000).unwrap();
        let standups = list.iter().filter(|i| i.title == "Стендап").count();
        assert_eq!(standups, 3, "4 days of a daily series minus one cancelled occurrence");

        // A cancelled single event disappears entirely.
        core.apply_items("acc", "cal", &[json!({"id": "demo", "status": "cancelled"})], true).unwrap();
        assert!(core.db.event("acc", "cal", "demo").unwrap().is_none());
    }
}
