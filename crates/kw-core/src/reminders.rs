//! Reminder scheduling: a cached list of upcoming fire times, checked every few seconds.

use std::sync::atomic::Ordering;
use std::sync::Mutex;

use crate::models::{CoreEvent, Instance, Reminder};
use crate::Core;

/// A reminder that was due while the PC slept still fires if we are at most this late.
const GRACE_MS: i64 = 15 * 60_000;
const HORIZON_MS: i64 = 26 * 3600_000;
const CACHE_TTL_MS: i64 = 10 * 60_000;

struct Due {
    at: i64,
    key: String,
    minutes: i64,
    inst: Instance,
}

#[derive(Default)]
pub(crate) struct ReminderState {
    cache: Mutex<Option<(i64, Vec<Due>)>>,
    snoozed: Mutex<Vec<(i64, Instance)>>,
}

impl ReminderState {
    pub fn invalidate(&self) {
        *self.cache.lock().unwrap() = None;
    }

    pub fn snooze(&self, inst: Instance, minutes: i64) {
        let at = chrono::Utc::now().timestamp_millis() + minutes.max(1) * 60_000;
        self.snoozed.lock().unwrap().push((at, inst));
    }
}

impl Core {
    fn rebuild_reminders(&self, now: i64) -> Vec<Due> {
        let Ok(list) = self.instances(now - GRACE_MS, now + HORIZON_MS) else { return vec![] };
        let mut out = Vec::new();
        for inst in list {
            if inst.self_response.as_deref() == Some("declined") {
                continue;
            }
            for &m in &inst.reminders {
                let at = inst.start - m * 60_000;
                if at > now - GRACE_MS && at <= now + HORIZON_MS {
                    out.push(Due { at, key: format!("{}|{}|{}", inst.key, inst.start, m), minutes: m, inst: inst.clone() });
                }
            }
        }
        out.sort_by_key(|d| d.at);
        out
    }

    pub(crate) fn check_reminders(&self, now: i64) {
        let cfg = self.config();
        let muted = self.muted_until.load(Ordering::Relaxed) > now;
        let mut fire: Vec<Reminder> = Vec::new();
        {
            let mut cache = self.reminders.cache.lock().unwrap();
            let stale = cache.as_ref().map(|(built, _)| now - built > CACHE_TTL_MS).unwrap_or(true);
            if stale {
                *cache = Some((now, self.rebuild_reminders(now)));
            }
            if let Some((_, due)) = cache.as_mut() {
                for d in due.iter().filter(|d| d.at <= now && d.at > now - GRACE_MS) {
                    if self.db.was_fired(&d.key).unwrap_or(true) {
                        continue;
                    }
                    let _ = self.db.mark_fired(&d.key, now / 1000);
                    if cfg.reminders_enabled && !muted {
                        fire.push(Reminder { key: d.key.clone(), instance: d.inst.clone(), minutes_before: d.minutes });
                    }
                }
                due.retain(|d| d.at > now - GRACE_MS);
            }
        }
        {
            let mut sn = self.reminders.snoozed.lock().unwrap();
            let (ready, rest): (Vec<_>, Vec<_>) = sn.drain(..).partition(|(at, _)| *at <= now);
            *sn = rest;
            for (_, inst) in ready {
                let minutes = ((inst.start - now) / 60_000).max(0);
                fire.push(Reminder { key: format!("{}|snooze|{now}", inst.key), instance: inst, minutes_before: minutes });
            }
        }
        for r in fire {
            (self.emit)(CoreEvent::Reminder(r));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::models::EventRow;
    use crate::test_core;

    #[test]
    fn fires_once_at_due_time() {
        let core = test_core();
        core.db.upsert_account("acc", "me@x", None).unwrap();
        core.db
            .upsert_calendar("acc", "cal", "C", "#fff", "owner", true, None, &[10], true)
            .unwrap();
        let now = chrono::Utc::now().timestamp();
        core.db
            .upsert_event(&EventRow {
                account_id: "acc".into(),
                calendar_id: "cal".into(),
                id: "e1".into(),
                status: "confirmed".into(),
                summary: Some("Review".into()),
                start_ts: Some(now + 600),
                end_ts: Some(now + 3600),
                reminders_default: true,
                ..Default::default()
            })
            .unwrap();
        // Not due yet 30 s before.
        core.check_reminders((now - 30) * 1000);
        assert!(!core.db.was_fired(&format!("acc|cal|e1|{}|10", (now + 600) * 1000)).unwrap());
        core.check_reminders((now + 1) * 1000);
        assert!(core.db.was_fired(&format!("acc|cal|e1|{}|10", (now + 600) * 1000)).unwrap());
    }
}
