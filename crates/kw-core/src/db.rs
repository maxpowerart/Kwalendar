use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::models::{Account, Attendee, Calendar, EventRow};

const SCHEMA_V1: &str = r#"
CREATE TABLE accounts(
  id TEXT PRIMARY KEY,
  email TEXT NOT NULL,
  name TEXT,
  color TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'ok',
  error TEXT,
  cal_sync_token TEXT,
  last_sync INTEGER,
  sort INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE calendars(
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  id TEXT NOT NULL,
  summary TEXT NOT NULL,
  color TEXT NOT NULL,
  selected INTEGER NOT NULL DEFAULT 1,
  access_role TEXT NOT NULL DEFAULT 'reader',
  is_primary INTEGER NOT NULL DEFAULT 0,
  time_zone TEXT,
  default_reminders TEXT NOT NULL DEFAULT '[]',
  sync_token TEXT,
  PRIMARY KEY(account_id, id)
);
CREATE TABLE events(
  account_id TEXT NOT NULL,
  calendar_id TEXT NOT NULL,
  id TEXT NOT NULL,
  etag TEXT,
  status TEXT NOT NULL DEFAULT 'confirmed',
  summary TEXT,
  description TEXT,
  location TEXT,
  all_day INTEGER NOT NULL DEFAULT 0,
  start_ts INTEGER,
  end_ts INTEGER,
  start_date TEXT,
  end_date TEXT,
  time_zone TEXT,
  recurrence TEXT,
  recurring_event_id TEXT,
  original_start_ts INTEGER,
  original_start_date TEXT,
  meet_link TEXT,
  html_link TEXT,
  organizer TEXT,
  organizer_self INTEGER NOT NULL DEFAULT 0,
  attendees TEXT NOT NULL DEFAULT '[]',
  reminders_default INTEGER NOT NULL DEFAULT 1,
  reminders TEXT NOT NULL DEFAULT '[]',
  color_id TEXT,
  updated TEXT,
  pending INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(account_id, calendar_id, id),
  FOREIGN KEY(account_id, calendar_id) REFERENCES calendars(account_id, id) ON DELETE CASCADE
);
CREATE INDEX ev_time ON events(end_ts, start_ts) WHERE all_day = 0;
CREATE INDEX ev_date ON events(end_date, start_date) WHERE all_day = 1;
CREATE INDEX ev_master ON events(account_id, calendar_id, recurring_event_id);
CREATE TABLE outbox(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  account_id TEXT NOT NULL,
  calendar_id TEXT NOT NULL,
  event_id TEXT NOT NULL,
  op TEXT NOT NULL,
  payload TEXT,
  etag TEXT,
  attempts INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  created_at INTEGER NOT NULL
);
CREATE TABLE fired(key TEXT PRIMARY KEY, at INTEGER NOT NULL);
"#;

pub struct Db {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct OutboxItem {
    pub id: i64,
    pub account_id: String,
    pub calendar_id: String,
    pub event_id: String,
    pub op: String,
    pub payload: Option<String>,
    pub etag: Option<String>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA temp_store=MEMORY;",
        )?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(SCHEMA_V1)?;
            conn.execute_batch("PRAGMA user_version = 1")?;
        }
        if version < 2 {
            conn.execute_batch(
                "ALTER TABLE calendars ADD COLUMN watch_id TEXT;
                 ALTER TABLE calendars ADD COLUMN watch_resource TEXT;
                 ALTER TABLE calendars ADD COLUMN watch_expires INTEGER;
                 PRAGMA user_version = 2;",
            )?;
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---------- accounts ----------

    pub fn accounts(&self) -> Result<Vec<Account>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT id, email, name, color, status, error, last_sync FROM accounts ORDER BY sort, email",
        )?;
        let rows = st
            .query_map([], |r| {
                Ok(Account {
                    id: r.get(0)?,
                    email: r.get(1)?,
                    name: r.get(2)?,
                    color: r.get(3)?,
                    status: r.get(4)?,
                    error: r.get(5)?,
                    last_sync: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn upsert_account(&self, id: &str, email: &str, name: Option<&str>) -> Result<()> {
        let c = self.conn();
        let count: i64 = c.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
        let color = crate::models::ACCOUNT_COLORS[(count as usize) % crate::models::ACCOUNT_COLORS.len()];
        c.execute(
            "INSERT INTO accounts(id, email, name, color, sort) VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET email = excluded.email, name = excluded.name, status = 'ok', error = NULL",
            params![id, email, name, color, count],
        )?;
        Ok(())
    }

    pub fn set_account_status(&self, id: &str, status: &str, error: Option<&str>) -> Result<()> {
        self.conn().execute(
            "UPDATE accounts SET status = ?2, error = ?3 WHERE id = ?1",
            params![id, status, error],
        )?;
        Ok(())
    }

    pub fn mark_account_synced(&self, id: &str, at: i64) -> Result<()> {
        self.conn().execute(
            "UPDATE accounts SET status = 'ok', error = NULL, last_sync = ?2 WHERE id = ?1",
            params![id, at],
        )?;
        Ok(())
    }

    pub fn delete_account(&self, id: &str) -> Result<()> {
        let c = self.conn();
        c.execute("DELETE FROM outbox WHERE account_id = ?1", [id])?;
        c.execute("DELETE FROM events WHERE account_id = ?1", [id])?;
        c.execute("DELETE FROM accounts WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn cal_list_token(&self, account: &str) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row("SELECT cal_sync_token FROM accounts WHERE id = ?1", [account], |r| r.get(0))
            .optional()?
            .flatten())
    }

    pub fn set_cal_list_token(&self, account: &str, token: Option<&str>) -> Result<()> {
        self.conn()
            .execute("UPDATE accounts SET cal_sync_token = ?2 WHERE id = ?1", params![account, token])?;
        Ok(())
    }

    // ---------- calendars ----------

    fn map_calendar(r: &Row) -> rusqlite::Result<Calendar> {
        let rem: String = r.get(8)?;
        Ok(Calendar {
            account_id: r.get(0)?,
            id: r.get(1)?,
            summary: r.get(2)?,
            color: r.get(3)?,
            selected: r.get(4)?,
            access_role: r.get(5)?,
            primary: r.get(6)?,
            time_zone: r.get(7)?,
            default_reminders: serde_json::from_str(&rem).unwrap_or_default(),
        })
    }

    pub fn calendars(&self) -> Result<Vec<Calendar>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT c.account_id, c.id, c.summary, c.color, c.selected, c.access_role, c.is_primary, c.time_zone, c.default_reminders
             FROM calendars c JOIN accounts a ON a.id = c.account_id
             ORDER BY a.sort, c.is_primary DESC, c.access_role = 'reader', c.summary COLLATE NOCASE",
        )?;
        let rows = st.query_map([], Self::map_calendar)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn calendar(&self, account: &str, id: &str) -> Result<Option<Calendar>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT account_id, id, summary, color, selected, access_role, is_primary, time_zone, default_reminders
                 FROM calendars WHERE account_id = ?1 AND id = ?2",
                [account, id],
                Self::map_calendar,
            )
            .optional()?)
    }

    pub fn calendar_ids(&self, account: &str) -> Result<Vec<(String, Option<String>)>> {
        let c = self.conn();
        let mut st = c.prepare("SELECT id, sync_token FROM calendars WHERE account_id = ?1")?;
        let rows = st
            .query_map([account], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_calendar(
        &self,
        account: &str,
        id: &str,
        summary: &str,
        color: &str,
        access_role: &str,
        primary: bool,
        time_zone: Option<&str>,
        default_reminders: &[i64],
        selected_default: bool,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO calendars(account_id, id, summary, color, access_role, is_primary, time_zone, default_reminders, selected)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(account_id, id) DO UPDATE SET summary = excluded.summary, color = excluded.color,
               access_role = excluded.access_role, is_primary = excluded.is_primary, time_zone = excluded.time_zone,
               default_reminders = excluded.default_reminders",
            params![
                account,
                id,
                summary,
                color,
                access_role,
                primary,
                time_zone,
                serde_json::to_string(default_reminders)?,
                selected_default
            ],
        )?;
        Ok(())
    }

    pub fn delete_calendar(&self, account: &str, id: &str) -> Result<()> {
        let c = self.conn();
        c.execute("DELETE FROM events WHERE account_id = ?1 AND calendar_id = ?2", [account, id])?;
        c.execute("DELETE FROM calendars WHERE account_id = ?1 AND id = ?2", [account, id])?;
        Ok(())
    }

    pub fn set_calendar_selected(&self, account: &str, id: &str, selected: bool) -> Result<()> {
        self.conn().execute(
            "UPDATE calendars SET selected = ?3 WHERE account_id = ?1 AND id = ?2",
            params![account, id, selected],
        )?;
        Ok(())
    }

    pub fn set_sync_token(&self, account: &str, cal: &str, token: Option<&str>) -> Result<()> {
        self.conn().execute(
            "UPDATE calendars SET sync_token = ?3 WHERE account_id = ?1 AND id = ?2",
            params![account, cal, token],
        )?;
        Ok(())
    }

    pub fn clear_calendar_events(&self, account: &str, cal: &str) -> Result<()> {
        self.conn().execute(
            "DELETE FROM events WHERE account_id = ?1 AND calendar_id = ?2 AND pending = 0",
            [account, cal],
        )?;
        Ok(())
    }

    // ---------- events ----------

    const EVENT_COLS: &'static str = "account_id, calendar_id, id, etag, status, summary, description, location, all_day,
        start_ts, end_ts, start_date, end_date, time_zone, recurrence, recurring_event_id, original_start_ts,
        original_start_date, meet_link, html_link, organizer, organizer_self, attendees, reminders_default, reminders,
        color_id, updated, pending";

    fn map_event(r: &Row) -> rusqlite::Result<EventRow> {
        let recurrence: Option<String> = r.get(14)?;
        let attendees: String = r.get(22)?;
        let reminders: String = r.get(24)?;
        Ok(EventRow {
            account_id: r.get(0)?,
            calendar_id: r.get(1)?,
            id: r.get(2)?,
            etag: r.get(3)?,
            status: r.get(4)?,
            summary: r.get(5)?,
            description: r.get(6)?,
            location: r.get(7)?,
            all_day: r.get(8)?,
            start_ts: r.get(9)?,
            end_ts: r.get(10)?,
            start_date: r.get(11)?,
            end_date: r.get(12)?,
            time_zone: r.get(13)?,
            recurrence: recurrence.and_then(|s| serde_json::from_str(&s).ok()),
            recurring_event_id: r.get(15)?,
            original_start_ts: r.get(16)?,
            original_start_date: r.get(17)?,
            meet_link: r.get(18)?,
            html_link: r.get(19)?,
            organizer: r.get(20)?,
            organizer_self: r.get(21)?,
            attendees: serde_json::from_str::<Vec<Attendee>>(&attendees).unwrap_or_default(),
            reminders_default: r.get(23)?,
            reminders: serde_json::from_str(&reminders).unwrap_or_default(),
            color_id: r.get(25)?,
            updated: r.get(26)?,
            pending: r.get(27)?,
        })
    }

    pub fn upsert_event_conn(c: &Connection, e: &EventRow) -> Result<()> {
        c.execute(
            &format!(
                "INSERT OR REPLACE INTO events({}) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28)",
                Self::EVENT_COLS
            ),
            params![
                e.account_id,
                e.calendar_id,
                e.id,
                e.etag,
                e.status,
                e.summary,
                e.description,
                e.location,
                e.all_day,
                e.start_ts,
                e.end_ts,
                e.start_date,
                e.end_date,
                e.time_zone,
                e.recurrence.as_ref().map(|r| serde_json::to_string(r).unwrap_or_default()),
                e.recurring_event_id,
                e.original_start_ts,
                e.original_start_date,
                e.meet_link,
                e.html_link,
                e.organizer,
                e.organizer_self,
                serde_json::to_string(&e.attendees)?,
                e.reminders_default,
                serde_json::to_string(&e.reminders)?,
                e.color_id,
                e.updated,
                e.pending,
            ],
        )?;
        Ok(())
    }

    pub fn upsert_event(&self, e: &EventRow) -> Result<()> {
        Self::upsert_event_conn(&self.conn(), e)
    }

    pub fn event_conn(c: &Connection, account: &str, cal: &str, id: &str) -> Result<Option<EventRow>> {
        Ok(c.query_row(
            &format!("SELECT {} FROM events WHERE account_id = ?1 AND calendar_id = ?2 AND id = ?3", Self::EVENT_COLS),
            [account, cal, id],
            Self::map_event,
        )
        .optional()?)
    }

    pub fn event(&self, account: &str, cal: &str, id: &str) -> Result<Option<EventRow>> {
        Self::event_conn(&self.conn(), account, cal, id)
    }

    pub fn delete_event(&self, account: &str, cal: &str, id: &str) -> Result<()> {
        self.conn().execute(
            "DELETE FROM events WHERE account_id = ?1 AND calendar_id = ?2 AND id = ?3",
            [account, cal, id],
        )?;
        Ok(())
    }

    /// Single events and exceptions overlapping the range, plus every recurring master
    /// (masters are expanded by the caller). Only selected calendars.
    pub fn events_for_range(&self, from_ts: i64, to_ts: i64, from_date: &str, to_date: &str) -> Result<Vec<EventRow>> {
        let c = self.conn();
        let sql = format!(
            "SELECT {} FROM events e WHERE EXISTS (SELECT 1 FROM calendars c WHERE c.account_id = e.account_id AND c.id = e.calendar_id AND c.selected = 1)
             AND (
               (e.recurrence IS NULL AND e.status != 'cancelled' AND (
                 (e.all_day = 0 AND e.end_ts > ?1 AND e.start_ts < ?2) OR
                 (e.all_day = 1 AND e.end_date > ?3 AND e.start_date < ?4)))
               OR (e.recurrence IS NOT NULL AND e.status != 'cancelled')
               OR (e.recurring_event_id IS NOT NULL AND e.status = 'cancelled')
             )",
            Self::EVENT_COLS
        );
        let mut st = c.prepare(&sql)?;
        let rows = st
            .query_map(params![from_ts, to_ts, from_date, to_date], Self::map_event)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn search(&self, q: &str, limit: i64) -> Result<Vec<EventRow>> {
        let c = self.conn();
        let like = format!("%{}%", q.replace('%', "").replace('_', ""));
        let now = chrono::Utc::now().timestamp();
        let sql = format!(
            "SELECT {} FROM events e WHERE e.status != 'cancelled'
               AND (e.summary LIKE ?1 OR e.location LIKE ?1 OR e.description LIKE ?1)
               AND EXISTS (SELECT 1 FROM calendars c WHERE c.account_id = e.account_id AND c.id = e.calendar_id AND c.selected = 1)
             ORDER BY e.recurrence IS NOT NULL, ABS(COALESCE(e.start_ts, CAST(strftime('%s', e.start_date) AS INTEGER)) - ?2)
             LIMIT ?3",
            Self::EVENT_COLS
        );
        let mut st = c.prepare(&sql)?;
        let rows = st
            .query_map(params![like, now, limit], Self::map_event)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---------- outbox ----------

    pub fn outbox_push(
        &self,
        account: &str,
        cal: &str,
        event_id: &str,
        op: &str,
        payload: Option<&serde_json::Value>,
        etag: Option<&str>,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO outbox(account_id, calendar_id, event_id, op, payload, etag, created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                account,
                cal,
                event_id,
                op,
                payload.map(|p| p.to_string()),
                etag,
                chrono::Utc::now().timestamp()
            ],
        )?;
        Ok(())
    }

    pub fn outbox_for(&self, account: &str) -> Result<Vec<OutboxItem>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT id, account_id, calendar_id, event_id, op, payload, etag FROM outbox WHERE account_id = ?1 ORDER BY id",
        )?;
        let rows = st
            .query_map([account], |r| {
                Ok(OutboxItem {
                    id: r.get(0)?,
                    account_id: r.get(1)?,
                    calendar_id: r.get(2)?,
                    event_id: r.get(3)?,
                    op: r.get(4)?,
                    payload: r.get(5)?,
                    etag: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Pending local insert for a not-yet-created event, if any.
    pub fn outbox_insert_for(&self, account: &str, cal: &str, event_id: &str) -> Result<Option<(i64, String)>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT id, payload FROM outbox WHERE account_id = ?1 AND calendar_id = ?2 AND event_id = ?3 AND op = 'insert'",
                [account, cal, event_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    pub fn outbox_set_payload(&self, id: i64, payload: &serde_json::Value) -> Result<()> {
        self.conn()
            .execute("UPDATE outbox SET payload = ?2 WHERE id = ?1", params![id, payload.to_string()])?;
        Ok(())
    }

    pub fn outbox_done(&self, id: i64) -> Result<()> {
        self.conn().execute("DELETE FROM outbox WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn outbox_drop_event(&self, account: &str, cal: &str, event_id: &str) -> Result<()> {
        self.conn().execute(
            "DELETE FROM outbox WHERE account_id = ?1 AND calendar_id = ?2 AND event_id = ?3",
            [account, cal, event_id],
        )?;
        Ok(())
    }

    pub fn outbox_failed(&self, id: i64, err: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE outbox SET attempts = attempts + 1, last_error = ?2 WHERE id = ?1",
            params![id, err],
        )?;
        Ok(())
    }

    pub fn outbox_count(&self) -> Result<i64> {
        Ok(self.conn().query_row("SELECT COUNT(*) FROM outbox", [], |r| r.get(0))?)
    }

    pub fn has_pending_for(&self, account: &str, cal: &str, event_id: &str) -> Result<bool> {
        Ok(self.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM outbox WHERE account_id = ?1 AND calendar_id = ?2 AND event_id = ?3)",
            [account, cal, event_id],
            |r| r.get(0),
        )?)
    }

    // ---------- fired reminders ----------

    pub fn was_fired(&self, key: &str) -> Result<bool> {
        Ok(self
            .conn()
            .query_row("SELECT EXISTS(SELECT 1 FROM fired WHERE key = ?1)", [key], |r| r.get(0))?)
    }

    pub fn mark_fired(&self, key: &str, at: i64) -> Result<()> {
        let c = self.conn();
        c.execute("INSERT OR IGNORE INTO fired(key, at) VALUES(?1, ?2)", params![key, at])?;
        c.execute("DELETE FROM fired WHERE at < ?1", [at - 3 * 86400])?;
        Ok(())
    }
}

/// Push channel bookkeeping (events.watch).
impl Db {
    pub fn watches(&self, account: &str) -> Result<Vec<(String, Option<String>, Option<String>, Option<i64>)>> {
        let c = self.conn();
        let mut st = c.prepare("SELECT id, watch_id, watch_resource, watch_expires FROM calendars WHERE account_id = ?1")?;
        let rows = st
            .query_map([account], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_watch(&self, account: &str, cal: &str, id: Option<&str>, resource: Option<&str>, expires: Option<i64>) -> Result<()> {
        self.conn().execute(
            "UPDATE calendars SET watch_id = ?3, watch_resource = ?4, watch_expires = ?5 WHERE account_id = ?1 AND id = ?2",
            params![account, cal, id, resource, expires],
        )?;
        Ok(())
    }
}
