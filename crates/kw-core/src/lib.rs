//! Kwalendar core. Local-first: the UI only ever reads SQLite; network work happens in background loops.

pub mod auth;
pub mod config;
pub mod db;
pub mod google;
pub mod models;
pub mod recurrence;
mod push;
mod reminders;
mod sync;
mod writes;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use anyhow::{bail, Result};
use chrono::{Local, TimeZone};
use serde::Serialize;
use tokio::sync::Notify;

pub use config::Config;
pub use db::Db;
pub use models::*;
pub use writes::{DeleteScope, EditTarget};

type Emitter = Box<dyn Fn(CoreEvent) + Send + Sync>;

pub struct Core {
    pub db: Db,
    pub http: reqwest::Client,
    dir: PathBuf,
    config: RwLock<Config>,
    tokens: Mutex<HashMap<String, (String, i64)>>,
    trigger: Notify,
    focused: AtomicBool,
    sync_lock: tokio::sync::Mutex<()>,
    sync_state: Mutex<(String, Option<String>)>,
    emit: Emitter,
    reminders: reminders::ReminderState,
    muted_until: AtomicI64,
    /// Set by sync when local data changed; flushed as one UI refresh per sync pass.
    dirty: AtomicBool,
    last_focus_sync: AtomicI64,
    /// Overridable for tests and proxies (`KW_GOOGLE_API_BASE`, `KW_GOOGLE_TOKEN_URL`).
    pub(crate) api_base: String,
    pub(crate) token_url: String,
    pub(crate) userinfo_url: String,
    push_ok: AtomicBool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub accounts: Vec<Account>,
    pub calendars: Vec<Calendar>,
    pub config: Config,
    pub sync: SyncStatus,
    pub muted_until: i64,
    pub push_active: bool,
}

impl Core {
    pub fn open(dir: &Path, emit: impl Fn(CoreEvent) + Send + Sync + 'static) -> Result<Arc<Self>> {
        std::fs::create_dir_all(dir)?;
        let db = Db::open(&dir.join("kwalendar.db"))?;
        Ok(Arc::new(Self::with_db(db, dir, Box::new(emit))?))
    }

    fn with_db(db: Db, dir: &Path, emit: Emitter) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("Kwalendar/0.1 (gzip)")
            .gzip(true)
            .pool_idle_timeout(Duration::from_secs(90))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            db,
            http,
            dir: dir.to_path_buf(),
            config: RwLock::new(Config::load(dir)),
            tokens: Mutex::new(HashMap::new()),
            trigger: Notify::new(),
            focused: AtomicBool::new(true),
            sync_lock: tokio::sync::Mutex::new(()),
            sync_state: Mutex::new(("idle".into(), None)),
            emit,
            reminders: reminders::ReminderState::default(),
            muted_until: AtomicI64::new(0),
            dirty: AtomicBool::new(false),
            last_focus_sync: AtomicI64::new(0),
            api_base: std::env::var("KW_GOOGLE_API_BASE").unwrap_or_else(|_| google::DEFAULT_API.into()),
            token_url: std::env::var("KW_GOOGLE_TOKEN_URL").unwrap_or_else(|_| auth::TOKEN_URL.into()),
            userinfo_url: std::env::var("KW_GOOGLE_USERINFO_URL").unwrap_or_else(|_| auth::USERINFO_URL.into()),
            push_ok: AtomicBool::new(false),
        })
    }

    pub fn config(&self) -> Config {
        self.config.read().unwrap().clone()
    }

    pub fn set_config(&self, mut cfg: Config) -> Result<()> {
        if !cfg.relay_url.trim().is_empty() && cfg.relay_key.is_empty() {
            cfg.relay_key = uuid::Uuid::new_v4().simple().to_string();
        }
        cfg.save(&self.dir)?;
        *self.config.write().unwrap() = Config::load(&self.dir);
        self.reminders.invalidate();
        Ok(())
    }

    pub fn set_focused(&self, focused: bool) {
        let was = self.focused.swap(focused, Ordering::Relaxed);
        if focused && !was {
            // Returning to the window is the moment the user looks: refresh now (but not on every alt-tab).
            let now = chrono::Utc::now().timestamp();
            if now - self.last_focus_sync.swap(now, Ordering::Relaxed) >= 15 {
                self.sync_now();
            }
        }
    }

    pub fn sync_now(&self) {
        self.trigger.notify_one();
    }

    pub fn mute(&self, minutes: i64) {
        let until = if minutes <= 0 { 0 } else { chrono::Utc::now().timestamp_millis() + minutes * 60_000 };
        self.muted_until.store(until, Ordering::Relaxed);
    }

    fn changed(&self) {
        self.reminders.invalidate();
        (self.emit)(CoreEvent::DataChanged);
    }

    fn notice(&self, n: Notice) {
        (self.emit)(CoreEvent::Notice(n));
    }

    pub fn sync_status(&self) -> SyncStatus {
        let (state, message) = self.sync_state.lock().unwrap().clone();
        let accounts = self.db.accounts().unwrap_or_default();
        SyncStatus {
            state,
            last_sync: accounts.iter().filter_map(|a| a.last_sync).max(),
            pending: self.db.outbox_count().unwrap_or(0),
            message,
        }
    }

    fn set_sync_state(&self, state: &str, message: Option<String>) {
        *self.sync_state.lock().unwrap() = (state.to_string(), message);
        (self.emit)(CoreEvent::Sync(self.sync_status()));
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        let mut config = self.config();
        config.google_client_secret = if config.google_client_secret.is_empty() { String::new() } else { "••••".into() };
        Ok(Snapshot {
            accounts: self.db.accounts()?,
            calendars: self.db.calendars()?,
            config,
            sync: self.sync_status(),
            muted_until: self.muted_until.load(Ordering::Relaxed),
            push_active: self.push_active(),
        })
    }

    /// Occurrences overlapping [from_ms, to_ms).
    pub fn instances(&self, from_ms: i64, to_ms: i64) -> Result<Vec<Instance>> {
        let from = from_ms.div_euclid(1000);
        let to = to_ms.div_euclid(1000) + 1;
        let day = |ts: i64| Local.timestamp_opt(ts, 0).single().map(|d| d.date_naive()).unwrap_or_default();
        let fd = (day(from) - chrono::Duration::days(1)).to_string();
        let td = (day(to) + chrono::Duration::days(1)).to_string();
        let rows = self.db.events_for_range(from, to, &fd, &td)?;
        let cals = self.db.calendars()?;
        let mut list = recurrence::build_instances(rows, &cals, from, to);
        list.retain(|i| i.end > from_ms && i.start < to_ms || (i.start == i.end && i.start >= from_ms && i.start < to_ms));
        Ok(list)
    }

    pub fn search(&self, q: &str) -> Result<Vec<Instance>> {
        let q = q.trim();
        if q.is_empty() {
            return Ok(vec![]);
        }
        let cals = self.db.calendars()?;
        let now = chrono::Utc::now().timestamp();
        let mut out = Vec::new();
        for row in self.db.search(q, 40)? {
            if row.recurrence.is_some() {
                // Show the next occurrence of a series (or the latest one if it ended).
                let occ = recurrence::build_instances(vec![row.clone()], &cals, now, now + 400 * 86400);
                if let Some(first) = occ.into_iter().next() {
                    out.push(first);
                    continue;
                }
            }
            out.extend(recurrence::build_instances(vec![row], &cals, i64::MIN / 4, i64::MAX / 4));
        }
        out.truncate(30);
        Ok(out)
    }

    pub fn set_calendar_selected(&self, account: &str, id: &str, selected: bool) -> Result<()> {
        self.db.set_calendar_selected(account, id, selected)?;
        self.changed();
        Ok(())
    }

    pub async fn add_account(&self, open_url: impl FnOnce(String), hint: Option<String>) -> Result<Account> {
        let cfg = self.config();
        if !cfg.has_client() {
            bail!("Укажите OAuth Client ID в настройках");
        }
        let li = auth::login(
            &self.http,
            &self.token_url,
            &self.userinfo_url,
            cfg.google_client_id.trim(),
            cfg.google_client_secret.trim(),
            hint.as_deref(),
            open_url,
        )
        .await?;
        self.db.upsert_account(&li.user.sub, &li.user.email, li.user.name.as_deref())?;
        self.tokens
            .lock()
            .unwrap()
            .insert(li.user.sub.clone(), (li.access_token, li.expires_at));
        self.changed();
        self.sync_now();
        let acc = self
            .db
            .accounts()?
            .into_iter()
            .find(|a| a.id == li.user.sub)
            .ok_or_else(|| anyhow::anyhow!("account vanished"))?;
        Ok(acc)
    }

    pub fn remove_account(&self, id: &str) -> Result<()> {
        auth::delete_refresh_token(id);
        self.tokens.lock().unwrap().remove(id);
        self.db.delete_account(id)?;
        self.changed();
        (self.emit)(CoreEvent::Sync(self.sync_status()));
        Ok(())
    }

    pub fn snooze(&self, inst: Instance, minutes: i64) {
        self.reminders.snooze(inst, minutes);
    }

    /// Background loop: incremental sync, faster while the window is focused.
    pub async fn run_sync_loop(self: Arc<Self>) {
        loop {
            self.sync_all().await;
            let cfg = self.config();
            let focused = self.focused.load(Ordering::Relaxed);
            let mut secs = if focused { cfg.poll_focused_secs } else { cfg.poll_background_secs };
            if self.push_active() {
                // Push wakes us on changes; polling is only a safety net.
                secs = secs.max(if focused { 120 } else { 900 });
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(secs)) => {}
                _ = self.trigger.notified() => {}
            }
        }
    }

    /// Background loop: fires due reminders; detects wake-from-sleep by wall-clock jumps.
    pub async fn run_reminder_loop(self: Arc<Self>) {
        let mut last = chrono::Utc::now().timestamp();
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            let now = chrono::Utc::now().timestamp();
            if now - last > 60 {
                log::info!("clock jumped {}s (sleep/resume) — syncing", now - last);
                self.reminders.invalidate();
                self.sync_now();
            }
            last = now;
            self.check_reminders(now * 1000);
        }
    }
}

#[cfg(test)]
pub(crate) fn test_core() -> Arc<Core> {
    test_core_with(None, Box::new(|_| {}))
}

#[cfg(test)]
pub(crate) fn test_core_with(api_base: Option<String>, emit: Emitter) -> Arc<Core> {
    let dir = std::env::temp_dir().join(format!("kw-test-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut core = Core::with_db(Db::open_in_memory().unwrap(), &dir, emit).unwrap();
    if let Some(b) = api_base {
        core.api_base = b;
    }
    Arc::new(core)
}

#[cfg(test)]
mod fake_google;
#[cfg(test)]
mod e2e_tests;
