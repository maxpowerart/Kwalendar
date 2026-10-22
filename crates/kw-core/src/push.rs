//! Optional push: Google calls a webhook on the relay (`relay/`), the app long-polls the relay
//! and syncs as soon as something changed. Polling stays as a safety net, just much rarer.

use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::google::{self, ApiError};
use crate::Core;

/// Renew channels this long before Google expires them.
const RENEW_BEFORE_MS: i64 = 24 * 3600_000;
/// Calendars that refuse push (some read-only/public ones) are retried this rarely.
const UNSUPPORTED_RETRY_MS: i64 = 7 * 24 * 3600_000;

impl Core {
    pub(crate) fn push_enabled(&self) -> bool {
        let c = self.config();
        !c.relay_url.trim().is_empty() && !c.relay_key.is_empty()
    }

    /// Push is configured and the relay answered recently.
    pub fn push_active(&self) -> bool {
        self.push_enabled() && self.push_ok.load(Ordering::Relaxed)
    }

    /// Makes sure every calendar of the account has a live watch channel pointing at the relay.
    pub(crate) async fn ensure_watches(&self, account: &str, token: &str) -> Result<(), ApiError> {
        if !self.push_enabled() {
            return Ok(());
        }
        let cfg = self.config();
        let address = format!("{}/google/{}", cfg.relay_url.trim().trim_end_matches('/'), cfg.relay_key);
        let now = chrono::Utc::now().timestamp_millis();
        let watches = self.db.watches(account).map_err(|e| ApiError::Other(e.to_string()))?;
        for (cal, id, resource, expires) in watches {
            if expires.is_some_and(|e| e - RENEW_BEFORE_MS > now) {
                continue;
            }
            if let (Some(id), Some(res)) = (&id, &resource) {
                let _ = google::stop_channel(&self.http, &self.api_base, token, id, res).await;
            }
            match google::watch_events(&self.http, &self.api_base, token, &cal, &address, &cfg.relay_key).await {
                Ok(v) => {
                    let exp = v["expiration"]
                        .as_str()
                        .and_then(|s| s.parse::<i64>().ok())
                        .or_else(|| v["expiration"].as_i64())
                        .unwrap_or(now + 7 * 24 * 3600_000);
                    let _ = self.db.set_watch(account, &cal, v["id"].as_str(), v["resourceId"].as_str(), Some(exp));
                }
                Err(e @ (ApiError::Unauthorized | ApiError::Network(_))) => return Err(e),
                Err(e) => {
                    log::info!("push not available for {cal}: {e}");
                    let _ = self.db.set_watch(account, &cal, None, None, Some(now + UNSUPPORTED_RETRY_MS));
                }
            }
        }
        Ok(())
    }

    /// Long-polls the relay; any new sequence number means Google reported a change.
    pub async fn run_push_loop(self: std::sync::Arc<Self>) {
        let mut seq: Option<u64> = None;
        loop {
            if !self.push_enabled() {
                self.push_ok.store(false, Ordering::Relaxed);
                seq = None;
                tokio::time::sleep(Duration::from_secs(20)).await;
                continue;
            }
            let cfg = self.config();
            let url = format!(
                "{}/wait/{}?since={}",
                cfg.relay_url.trim().trim_end_matches('/'),
                cfg.relay_key,
                seq.unwrap_or(0)
            );
            let res = self.http.get(&url).timeout(Duration::from_secs(75)).send().await;
            match res.and_then(|r| r.error_for_status()) {
                Ok(r) => {
                    let s = r.json::<serde_json::Value>().await.ok().and_then(|v| v["seq"].as_u64()).unwrap_or(0);
                    self.push_ok.store(true, Ordering::Relaxed);
                    if seq.is_some_and(|old| old != s) {
                        log::info!("push: change reported, syncing");
                        self.sync_now();
                    }
                    seq = Some(s);
                }
                Err(e) => {
                    log::warn!("push relay unreachable: {e}");
                    self.push_ok.store(false, Ordering::Relaxed);
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
            }
        }
    }
}
