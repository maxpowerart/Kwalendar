//! Optimistic local writes. Every change lands in SQLite immediately and is queued in the outbox.

use anyhow::{anyhow, bail, Result};
use chrono::DateTime;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::google::{event_body, reminders_json, time_json};
use crate::models::{Attendee, EventInput, EventRow};
use crate::Core;

/// Identifies the occurrence being edited. For an expanded recurrence the row may not exist yet.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditTarget {
    pub account_id: String,
    pub calendar_id: String,
    pub event_id: String,
    pub master_id: Option<String>,
    /// Occurrence start as shown (ms) — the original start for expanded occurrences.
    pub start: Option<i64>,
    pub start_date: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum DeleteScope {
    This,
    Series,
}

fn new_event_id() -> String {
    // Google accepts client ids from the base32hex alphabet; lowercase hex is a subset.
    uuid::Uuid::new_v4().simple().to_string()
}

fn apply_input(row: &mut EventRow, input: &EventInput) -> Result<()> {
    row.summary = Some(input.title.trim().to_string());
    row.location = input.location.clone().filter(|s| !s.trim().is_empty());
    row.description = input.description.clone().filter(|s| !s.trim().is_empty());
    row.all_day = input.all_day;
    if input.all_day {
        let sd = input.start_date.clone().ok_or_else(|| anyhow!(crate::tr!("нет даты начала", "no start date")))?;
        let ed = input.end_date.clone().unwrap_or_else(|| sd.clone());
        let ed = if ed <= sd {
            chrono::NaiveDate::parse_from_str(&sd, "%Y-%m-%d")
                .map(|d| (d + chrono::Duration::days(1)).to_string())
                .unwrap_or(ed)
        } else {
            ed
        };
        row.start_date = Some(sd);
        row.end_date = Some(ed);
        row.start_ts = None;
        row.end_ts = None;
    } else {
        let s = input.start.ok_or_else(|| anyhow!(crate::tr!("нет времени начала", "no start time")))? / 1000;
        let e = input.end.map(|e| e / 1000).unwrap_or(s + 3600).max(s);
        row.start_ts = Some(s);
        row.end_ts = Some(e);
        row.start_date = None;
        row.end_date = None;
        if input.time_zone.is_some() {
            row.time_zone = input.time_zone.clone();
        }
    }
    match &input.reminders {
        Some(m) => {
            row.reminders_default = false;
            row.reminders = m.clone();
        }
        None => {
            row.reminders_default = true;
            row.reminders.clear();
        }
    }
    for email in &input.attendees {
        let email = email.trim();
        if !email.is_empty() && !row.attendees.iter().any(|a| a.email.eq_ignore_ascii_case(email)) {
            row.attendees.push(Attendee {
                email: email.to_string(),
                name: None,
                response: "needsAction".into(),
                is_self: false,
                organizer: false,
                optional: false,
            });
        }
    }
    Ok(())
}

impl Core {
    fn writable_calendar(&self, account: &str, cal: &str) -> Result<crate::models::Calendar> {
        let c = self
            .db
            .calendar(account, cal)?
            .ok_or_else(|| anyhow!(crate::tr!("календарь не найден", "calendar not found")))?;
        if !c.writable() {
            bail!(crate::tr!("Календарь «{}» только для чтения", "Calendar “{}” is read-only", c.summary));
        }
        Ok(c)
    }

    pub fn create_event(&self, input: EventInput) -> Result<String> {
        self.writable_calendar(&input.account_id, &input.calendar_id)?;
        let id = new_event_id();
        let mut row = EventRow {
            account_id: input.account_id.clone(),
            calendar_id: input.calendar_id.clone(),
            id: id.clone(),
            status: "confirmed".into(),
            organizer_self: true,
            reminders_default: true,
            time_zone: Some(local_tz_name()),
            pending: true,
            ..Default::default()
        };
        apply_input(&mut row, &input)?;
        if !row.attendees.is_empty() {
            let me = self
                .db
                .accounts()?
                .into_iter()
                .find(|a| a.id == input.account_id)
                .map(|a| a.email)
                .unwrap_or_default();
            row.attendees.insert(
                0,
                Attendee { email: me, name: None, response: "accepted".into(), is_self: true, organizer: true, optional: false },
            );
        }
        let mut body = event_body(&row);
        body["id"] = json!(id);
        if input.add_meet {
            body["conferenceData"] = json!({
                "createRequest": {"requestId": new_event_id(), "conferenceSolutionKey": {"type": "hangoutsMeet"}}
            });
        }
        self.db.upsert_event(&row)?;
        self.db
            .outbox_push(&row.account_id, &row.calendar_id, &id, "insert", Some(&json!({"body": body})), None)?;
        self.changed();
        self.sync_now();
        Ok(format!("{}|{}|{}", row.account_id, row.calendar_id, id))
    }

    /// Loads the row for an occurrence, materialising an exception row for expanded recurrences.
    fn row_for(&self, t: &EditTarget) -> Result<EventRow> {
        if let Some(r) = self.db.event(&t.account_id, &t.calendar_id, &t.event_id)? {
            return Ok(r);
        }
        let master_id = t.master_id.as_deref().ok_or_else(|| anyhow!(crate::tr!("событие не найдено", "event not found")))?;
        let master = self
            .db
            .event(&t.account_id, &t.calendar_id, master_id)?
            .ok_or_else(|| anyhow!(crate::tr!("серия не найдена", "series not found")))?;
        let mut r = master.clone();
        r.id = t.event_id.clone();
        r.etag = None;
        r.recurrence = None;
        r.recurring_event_id = Some(master_id.to_string());
        if master.all_day {
            let d = t.start_date.clone().ok_or_else(|| anyhow!(crate::tr!("нет даты", "no date")))?;
            let days = match (&master.start_date, &master.end_date) {
                (Some(s), Some(e)) => {
                    let p = |x: &str| chrono::NaiveDate::parse_from_str(x, "%Y-%m-%d").ok();
                    match (p(s), p(e)) {
                        (Some(s), Some(e)) => (e - s).num_days().max(1),
                        _ => 1,
                    }
                }
                _ => 1,
            };
            let sd = chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")?;
            r.original_start_date = Some(d.clone());
            r.start_date = Some(d);
            r.end_date = Some((sd + chrono::Duration::days(days)).to_string());
        } else {
            let s = t.start.ok_or_else(|| anyhow!(crate::tr!("нет времени", "no time")))? / 1000;
            let dur = master.end_ts.unwrap_or(0) - master.start_ts.unwrap_or(0);
            r.original_start_ts = Some(s);
            r.start_ts = Some(s);
            r.end_ts = Some(s + dur);
        }
        Ok(r)
    }

    fn queue_patch(&self, row: &EventRow, body: Value, send: &str) -> Result<()> {
        if let Some((oid, payload)) = self.db.outbox_insert_for(&row.account_id, &row.calendar_id, &row.id)? {
            // Not created on the server yet: fold the change into the pending insert.
            let mut p: Value = serde_json::from_str(&payload).unwrap_or(json!({"body": {}}));
            if let Some(obj) = body.as_object() {
                for (k, v) in obj {
                    p["body"][k] = v.clone();
                }
            }
            self.db.outbox_set_payload(oid, &p)?;
        } else {
            self.db.outbox_push(
                &row.account_id,
                &row.calendar_id,
                &row.id,
                "patch",
                Some(&json!({"body": body, "send": send})),
                row.etag.as_deref(),
            )?;
        }
        Ok(())
    }

    pub fn update_event(&self, target: EditTarget, input: EventInput) -> Result<()> {
        self.writable_calendar(&target.account_id, &target.calendar_id)?;
        let mut row = self.row_for(&target)?;
        apply_input(&mut row, &input)?;
        row.pending = true;
        row.status = "confirmed".into();
        let (start, end) = time_json(&row);
        let mut body = json!({
            "summary": row.summary,
            "location": row.location.clone().unwrap_or_default(),
            "description": row.description.clone().unwrap_or_default(),
            "start": start,
            "end": end,
            "reminders": reminders_json(row.reminders_default, &row.reminders),
        });
        if !row.attendees.is_empty() {
            body["attendees"] = json!(row
                .attendees
                .iter()
                .map(|a| json!({"email": a.email, "responseStatus": a.response, "optional": a.optional}))
                .collect::<Vec<_>>());
        }
        self.db.upsert_event(&row)?;
        self.queue_patch(&row, body, "all")?;
        self.changed();
        self.sync_now();
        Ok(())
    }

    /// Edits the whole series from one of its occurrences: fields apply to the master, and the
    /// time change made to this occurrence (shift + new duration) is applied to the series start.
    pub fn update_series(&self, target: EditTarget, input: EventInput) -> Result<()> {
        let cal = self.writable_calendar(&target.account_id, &target.calendar_id)?;
        let master_id = target.master_id.clone().unwrap_or(target.event_id.clone());
        let mut m = self
            .db
            .event(&target.account_id, &target.calendar_id, &master_id)?
            .ok_or_else(|| anyhow!(crate::tr!("серия не найдена", "series not found")))?;
        if m.recurrence.is_none() {
            bail!(crate::tr!("событие не повторяется", "the event does not repeat"));
        }
        let keep_times = (m.all_day, m.start_ts, m.end_ts, m.start_date.clone(), m.end_date.clone());
        apply_input(&mut m, &input)?;
        if input.all_day != keep_times.0 {
            bail!(crate::tr!(
                "для серии нельзя переключить «весь день» — измените одно событие или создайте серию заново",
                "“All day” cannot be switched for a whole series — change one event or create the series again"
            ));
        }
        if input.all_day {
            let p = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
            let occ = target.start_date.as_deref().and_then(p);
            let new = input.start_date.as_deref().and_then(p);
            let (Some(ms), Some(me)) = (keep_times.3.as_deref().and_then(p), keep_times.4.as_deref().and_then(p)) else {
                bail!(crate::tr!("нет дат серии", "the series has no dates"));
            };
            let shift = match (occ, new) {
                (Some(o), Some(n)) => n - o,
                _ => chrono::Duration::zero(),
            };
            let len = match (new, input.end_date.as_deref().and_then(p)) {
                (Some(s), Some(e)) => (e - s).max(chrono::Duration::days(1)),
                _ => me - ms,
            };
            m.start_date = Some((ms + shift).to_string());
            m.end_date = Some((ms + shift + len).to_string());
        } else {
            let (Some(ms), Some(_)) = (keep_times.1, keep_times.2) else { bail!(crate::tr!("нет времени серии", "the series has no times")) };
            let occ = target.start.map(|s| s / 1000).unwrap_or(ms);
            let new_start = input.start.map(|s| s / 1000).unwrap_or(occ);
            let new_end = input.end.map(|e| e / 1000).unwrap_or(new_start + 3600);
            let start = ms + (new_start - occ);
            m.start_ts = Some(start);
            m.end_ts = Some(start + (new_end - new_start).max(0));
        }
        // Recurring events need an explicit zone on start/end.
        if m.time_zone.is_none() {
            m.time_zone = cal.time_zone.clone().or_else(|| Some(local_tz_name()));
        }
        m.pending = true;
        let (start, end) = time_json(&m);
        let mut body = json!({
            "summary": m.summary,
            "location": m.location.clone().unwrap_or_default(),
            "description": m.description.clone().unwrap_or_default(),
            "start": start,
            "end": end,
            "recurrence": m.recurrence,
            "reminders": reminders_json(m.reminders_default, &m.reminders),
        });
        if !m.attendees.is_empty() {
            body["attendees"] = json!(m
                .attendees
                .iter()
                .map(|a| json!({"email": a.email, "responseStatus": a.response, "optional": a.optional}))
                .collect::<Vec<_>>());
        }
        self.db.upsert_event(&m)?;
        self.queue_patch(&m, body, "all")?;
        self.changed();
        self.sync_now();
        Ok(())
    }

    /// Moves/resizes an occurrence (drag & drop). Times in ms.
    pub fn move_event(&self, target: EditTarget, start: i64, end: i64) -> Result<()> {
        self.writable_calendar(&target.account_id, &target.calendar_id)?;
        let mut row = self.row_for(&target)?;
        if row.all_day {
            bail!(crate::tr!("перетаскивание целого дня пока не поддерживается", "dragging all-day events is not supported yet"));
        }
        row.start_ts = Some(start / 1000);
        row.end_ts = Some((end / 1000).max(start / 1000 + 300));
        row.pending = true;
        let (s, e) = time_json(&row);
        self.db.upsert_event(&row)?;
        self.queue_patch(&row, json!({"start": s, "end": e}), "all")?;
        self.changed();
        self.sync_now();
        Ok(())
    }

    pub fn delete_event(&self, target: EditTarget, scope: DeleteScope) -> Result<()> {
        self.writable_calendar(&target.account_id, &target.calendar_id)?;
        let (acc, cal) = (target.account_id.as_str(), target.calendar_id.as_str());
        if scope == DeleteScope::Series {
            let master = target.master_id.clone().unwrap_or(target.event_id.clone());
            self.db.conn().execute(
                "DELETE FROM events WHERE account_id = ?1 AND calendar_id = ?2 AND recurring_event_id = ?3",
                [acc, cal, master.as_str()],
            )?;
            return self.delete_row(acc, cal, &master);
        }
        if self.db.event(acc, cal, &target.event_id)?.is_none() {
            // Expanded occurrence: store a cancelled exception locally so it disappears now.
            let mut r = self.row_for(&target)?;
            r.status = "cancelled".into();
            r.pending = true;
            self.db.upsert_event(&r)?;
            self.db.outbox_push(acc, cal, &target.event_id, "delete", None, None)?;
            self.changed();
            self.sync_now();
            return Ok(());
        }
        self.delete_row(acc, cal, &target.event_id)
    }

    fn delete_row(&self, acc: &str, cal: &str, id: &str) -> Result<()> {
        if self.db.outbox_insert_for(acc, cal, id)?.is_some() {
            // Never reached the server: just forget it.
            self.db.outbox_drop_event(acc, cal, id)?;
            self.db.delete_event(acc, cal, id)?;
        } else {
            if let Some(mut r) = self.db.event(acc, cal, id)? {
                r.status = "cancelled".into();
                r.pending = true;
                self.db.upsert_event(&r)?;
            }
            self.db.outbox_drop_event(acc, cal, id)?;
            self.db.outbox_push(acc, cal, id, "delete", None, None)?;
        }
        self.changed();
        self.sync_now();
        Ok(())
    }

    /// Answers an invitation: accepted | declined | tentative.
    pub fn respond(&self, target: EditTarget, response: &str) -> Result<()> {
        if !matches!(response, "accepted" | "declined" | "tentative") {
            bail!(crate::tr!("неизвестный ответ", "unknown response"));
        }
        let mut row = self.row_for(&target)?;
        let mut found = false;
        for a in row.attendees.iter_mut() {
            if a.is_self {
                a.response = response.to_string();
                found = true;
            }
        }
        if !found {
            bail!(crate::tr!("вы не участник этого события", "you are not a guest of this event"));
        }
        row.pending = true;
        let attendees: Vec<Value> = row
            .attendees
            .iter()
            .map(|a| json!({"email": a.email, "responseStatus": a.response, "optional": a.optional}))
            .collect();
        self.db.upsert_event(&row)?;
        let mut body = json!({"attendees": attendees});
        if row.etag.is_none() && row.recurring_event_id.is_some() {
            // New exception row: Google needs the occurrence's times too.
            let (s, e) = time_json(&row);
            body["start"] = s;
            body["end"] = e;
        }
        self.queue_patch(&row, body, "all")?;
        self.changed();
        self.sync_now();
        Ok(())
    }
}

pub fn local_tz_name() -> String {
    iana_from_env().unwrap_or_else(|| {
        let off = chrono::Local::now().offset().local_minus_utc();
        // Fixed-offset fallback in Etc/GMT notation (sign inverted by convention).
        if off % 3600 == 0 {
            let h = -off / 3600;
            if h == 0 { "UTC".into() } else { format!("Etc/GMT{:+}", h) }
        } else {
            "UTC".into()
        }
    })
}

fn iana_from_env() -> Option<String> {
    if let Ok(tz) = std::env::var("TZ") {
        if tz.parse::<chrono_tz::Tz>().is_ok() {
            return Some(tz);
        }
    }
    windows_tz()
}

#[cfg(windows)]
fn windows_tz() -> Option<String> {
    // `tzutil /g` prints the Windows zone id; map the common ones to IANA.
    let out = std::process::Command::new("tzutil").arg("/g").output().ok()?;
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let map = [
        ("Russian Standard Time", "Europe/Moscow"),
        ("Kaliningrad Standard Time", "Europe/Kaliningrad"),
        ("Russia Time Zone 3", "Europe/Samara"),
        ("Ekaterinburg Standard Time", "Asia/Yekaterinburg"),
        ("Omsk Standard Time", "Asia/Omsk"),
        ("N. Central Asia Standard Time", "Asia/Novosibirsk"),
        ("North Asia Standard Time", "Asia/Krasnoyarsk"),
        ("North Asia East Standard Time", "Asia/Irkutsk"),
        ("Yakutsk Standard Time", "Asia/Yakutsk"),
        ("Vladivostok Standard Time", "Asia/Vladivostok"),
        ("Magadan Standard Time", "Asia/Magadan"),
        ("Russia Time Zone 11", "Asia/Kamchatka"),
        ("FLE Standard Time", "Europe/Kiev"),
        ("Belarus Standard Time", "Europe/Minsk"),
        ("GTB Standard Time", "Europe/Bucharest"),
        ("W. Europe Standard Time", "Europe/Berlin"),
        ("Central European Standard Time", "Europe/Warsaw"),
        ("Romance Standard Time", "Europe/Paris"),
        ("GMT Standard Time", "Europe/London"),
        ("UTC", "UTC"),
        ("Eastern Standard Time", "America/New_York"),
        ("Central Standard Time", "America/Chicago"),
        ("Mountain Standard Time", "America/Denver"),
        ("Pacific Standard Time", "America/Los_Angeles"),
        ("Georgian Standard Time", "Asia/Tbilisi"),
        ("Caucasus Standard Time", "Asia/Yerevan"),
        ("Azerbaijan Standard Time", "Asia/Baku"),
        ("Central Asia Standard Time", "Asia/Almaty"),
        ("West Asia Standard Time", "Asia/Tashkent"),
        ("Arabian Standard Time", "Asia/Dubai"),
        ("Turkey Standard Time", "Europe/Istanbul"),
        ("Israel Standard Time", "Asia/Jerusalem"),
        ("India Standard Time", "Asia/Kolkata"),
        ("China Standard Time", "Asia/Shanghai"),
        ("Tokyo Standard Time", "Asia/Tokyo"),
        ("Singapore Standard Time", "Asia/Singapore"),
    ];
    map.iter().find(|(w, _)| *w == id).map(|(_, i)| i.to_string())
}

#[cfg(not(windows))]
fn windows_tz() -> Option<String> {
    None
}

#[allow(dead_code)]
fn ts_rfc3339(ts: i64) -> String {
    DateTime::from_timestamp(ts, 0).map(|d| d.to_rfc3339()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_core;

    fn setup() -> std::sync::Arc<Core> {
        let core = test_core();
        core.db.upsert_account("acc", "me@x.com", None).unwrap();
        core.db
            .upsert_calendar("acc", "cal", "Main", "#5B8DEF", "owner", true, Some("Europe/Moscow"), &[10], true)
            .unwrap();
        core
    }

    fn input() -> EventInput {
        EventInput {
            account_id: "acc".into(),
            calendar_id: "cal".into(),
            title: "Call".into(),
            all_day: false,
            start: Some(1792670400000),
            end: Some(1792672200000),
            start_date: None,
            end_date: None,
            time_zone: Some("Europe/Moscow".into()),
            location: None,
            description: None,
            reminders: None,
            attendees: vec![],
            add_meet: false,
        }
    }

    #[test]
    fn create_then_edit_folds_into_insert() {
        let core = setup();
        let key = core.create_event(input()).unwrap();
        let id = key.rsplit('|').next().unwrap().to_string();
        let list = core.instances(1792627200000, 1792713600000).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].pending);
        assert_eq!(list[0].reminders, vec![10]);

        let mut inp = input();
        inp.title = "Call with Peter".into();
        let target = EditTarget {
            account_id: "acc".into(),
            calendar_id: "cal".into(),
            event_id: id.clone(),
            master_id: None,
            start: None,
            start_date: None,
        };
        core.update_event(target.clone(), inp).unwrap();
        let ob = core.db.outbox_for("acc").unwrap();
        assert_eq!(ob.len(), 1, "edit must fold into pending insert");
        assert!(ob[0].payload.as_ref().unwrap().contains("Call with Peter"));

        core.delete_event(target, DeleteScope::This).unwrap();
        assert!(core.db.outbox_for("acc").unwrap().is_empty());
        assert!(core.instances(1792627200000, 1792713600000).unwrap().is_empty());
    }

    #[test]
    fn deleting_one_occurrence_hides_it() {
        let core = setup();
        let master = EventRow {
            account_id: "acc".into(),
            calendar_id: "cal".into(),
            id: "series1".into(),
            etag: Some("\"e\"".into()),
            status: "confirmed".into(),
            summary: Some("Stand-up".into()),
            start_ts: Some(1792391400),
            end_ts: Some(1792393200),
            time_zone: Some("Europe/Moscow".into()),
            recurrence: Some(vec!["RRULE:FREQ=DAILY;COUNT=5".into()]),
            reminders_default: true,
            organizer_self: true,
            ..Default::default()
        };
        core.db.upsert_event(&master).unwrap();
        let from = 1792368000000;
        let to = from + 7 * 86_400_000;
        let list = core.instances(from, to).unwrap();
        assert_eq!(list.len(), 5);
        let second = &list[1];
        core.delete_event(
            EditTarget {
                account_id: "acc".into(),
                calendar_id: "cal".into(),
                event_id: second.event_id.clone(),
                master_id: second.master_id.clone(),
                start: Some(second.start),
                start_date: None,
            },
            DeleteScope::This,
        )
        .unwrap();
        assert_eq!(core.instances(from, to).unwrap().len(), 4);
        let ob = core.db.outbox_for("acc").unwrap();
        assert_eq!(ob[0].op, "delete");
        assert_eq!(ob[0].event_id, "series1_20261020T063000Z");
    }

    #[test]
    fn series_edit_shifts_every_occurrence() {
        let core = setup();
        core.db
            .upsert_event(&EventRow {
                account_id: "acc".into(),
                calendar_id: "cal".into(),
                id: "s1".into(),
                etag: Some("\"e\"".into()),
                status: "confirmed".into(),
                summary: Some("Stand-up".into()),
                start_ts: Some(1792391400),
                end_ts: Some(1792393200),
                time_zone: Some("Europe/Moscow".into()),
                recurrence: Some(vec!["RRULE:FREQ=DAILY;COUNT=5".into()]),
                reminders_default: true,
                organizer_self: true,
                ..Default::default()
            })
            .unwrap();
        let from = 1792368000000;
        let to = from + 7 * 86_400_000;
        let second = core.instances(from, to).unwrap()[1].clone();
        let mut inp = input();
        inp.title = "Morning sync".into();
        // 09:30 → 10:00, 30 min → 45 min
        inp.start = Some(second.start + 1_800_000);
        inp.end = Some(second.start + 1_800_000 + 2_700_000);
        core.update_series(
            EditTarget {
                account_id: "acc".into(),
                calendar_id: "cal".into(),
                event_id: second.event_id.clone(),
                master_id: second.master_id.clone(),
                start: Some(second.start),
                start_date: None,
            },
            inp,
        )
        .unwrap();
        let list = core.instances(from, to).unwrap();
        assert_eq!(list.len(), 5);
        for i in &list {
            assert_eq!(i.title, "Morning sync");
            assert_eq!((i.start / 1000 - 1792391400) % 86400, 1800, "every day moved by 30 min");
            assert_eq!(i.end - i.start, 2_700_000);
        }
        let ob = core.db.outbox_for("acc").unwrap();
        assert_eq!((ob[0].op.as_str(), ob[0].event_id.as_str()), ("patch", "s1"));
        assert!(ob[0].payload.as_ref().unwrap().contains("RRULE"));
    }
}
