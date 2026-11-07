//! Local expansion of recurring masters into occurrences.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
use rrule::RRuleSet;

use crate::models::{event_color, Calendar, EventRow, Instance};

/// Upper bound per master per query; protects against runaway rules.
const MAX_OCCURRENCES: u16 = 1500;

pub enum Occurrence {
    Timed(i64),
    Day(NaiveDate),
}

/// Title shown for events without a name, in the UI language.
pub fn untitled() -> String {
    crate::tr!("(без названия)", "(no title)")
}

fn parse_tz(name: Option<&str>) -> chrono_tz::Tz {
    name.and_then(|n| n.parse().ok()).unwrap_or(chrono_tz::UTC)
}

/// Occurrence starts of `master` whose span overlaps [from, to) (unix seconds).
pub fn expand(master: &EventRow, cal_tz: Option<&str>, from: i64, to: i64) -> Vec<Occurrence> {
    let Some(lines) = &master.recurrence else { return vec![] };
    let rules = lines.join("\n");

    if master.all_day {
        let Some(start) = master.start_date.as_deref().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        else {
            return vec![];
        };
        let days = master
            .end_date
            .as_deref()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
            .map(|e| (e - start).num_days().max(1))
            .unwrap_or(1);
        let rtz = rrule::Tz::UTC;
        let Some(dt) = rtz.from_local_datetime(&start.and_hms_opt(0, 0, 0).unwrap()).single() else {
            return vec![];
        };
        // All-day EXDATE/RDATE come as VALUE=DATE; normalise to UTC midnight datetimes the parser accepts.
        let rules = rules
            .lines()
            .map(|l| {
                if (l.starts_with("EXDATE;VALUE=DATE:") || l.starts_with("RDATE;VALUE=DATE:")) && !l.contains('T') {
                    let (head, vals) = l.split_once(':').unwrap();
                    let name = head.split(';').next().unwrap();
                    let vals: Vec<String> = vals.split(',').map(|v| format!("{v}T000000Z")).collect();
                    format!("{name}:{}", vals.join(","))
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let Ok(set) = RRuleSet::new(dt).set_from_string(&rules) else {
            return vec![Occurrence::Day(start)];
        };
        let after = rtz.timestamp_opt(from - days * 86400 - 86400, 0).unwrap();
        let before = rtz.timestamp_opt(to + 86400, 0).unwrap();
        return set
            .after(after)
            .before(before)
            .all(MAX_OCCURRENCES)
            .dates
            .into_iter()
            .map(|d| Occurrence::Day(d.date_naive()))
            .collect();
    }

    let (Some(start_ts), Some(end_ts)) = (master.start_ts, master.end_ts) else { return vec![] };
    let dur = (end_ts - start_ts).max(0);
    let tz = parse_tz(master.time_zone.as_deref().or(cal_tz));
    let rtz = rrule::Tz::from(tz);
    let Some(dt) = rtz.timestamp_opt(start_ts, 0).single() else { return vec![] };
    let set = match RRuleSet::new(dt).set_from_string(&rules) {
        Ok(s) => s,
        Err(e) => {
            log::debug!("rrule parse failed for {}: {e}", master.id);
            return if start_ts < to && end_ts > from { vec![Occurrence::Timed(start_ts)] } else { vec![] };
        }
    };
    let after = rtz.timestamp_opt(from - dur - 1, 0).unwrap();
    let before = rtz.timestamp_opt(to, 0).unwrap();
    set.after(after)
        .before(before)
        .all(MAX_OCCURRENCES)
        .dates
        .into_iter()
        .map(|d| d.timestamp())
        .filter(|s| *s < to && s + dur.max(1) > from)
        .map(Occurrence::Timed)
        .collect()
}

pub fn instance_id(master_id: &str, occ: &Occurrence) -> String {
    match occ {
        Occurrence::Timed(ts) => format!(
            "{master_id}_{}",
            DateTime::<Utc>::from_timestamp(*ts, 0).unwrap().format("%Y%m%dT%H%M%SZ")
        ),
        Occurrence::Day(d) => format!("{master_id}_{}", d.format("%Y%m%d")),
    }
}

pub fn local_midnight_ms(date: &str) -> i64 {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .and_then(|d| Local.from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap()).earliest())
        .map(|d| d.timestamp_millis())
        .unwrap_or(0)
}

pub fn to_instance(e: &EventRow, cal: &Calendar, event_id: String, start: Occurrence) -> Instance {
    let color = e
        .color_id
        .as_deref()
        .and_then(event_color)
        .map(String::from)
        .unwrap_or_else(|| cal.color.clone());
    let (start_ms, end_ms, start_date, end_date) = match start {
        Occurrence::Timed(ts) => {
            let dur = e.end_ts.unwrap_or(ts) - e.start_ts.unwrap_or(ts);
            (ts * 1000, (ts + dur) * 1000, None, None)
        }
        Occurrence::Day(d) => {
            let days = match (e.start_date.as_deref(), e.end_date.as_deref()) {
                (Some(s), Some(en)) => match (
                    NaiveDate::parse_from_str(s, "%Y-%m-%d"),
                    NaiveDate::parse_from_str(en, "%Y-%m-%d"),
                ) {
                    (Ok(s), Ok(en)) => (en - s).num_days().max(1),
                    _ => 1,
                },
                _ => 1,
            };
            let sd = d.to_string();
            let ed = (d + chrono::Duration::days(days)).to_string();
            (local_midnight_ms(&sd), local_midnight_ms(&ed), Some(sd), Some(ed))
        }
    };
    let can_edit = cal.writable() && (e.organizer_self || e.attendees.is_empty() || cal.access_role == "owner");
    Instance {
        key: format!("{}|{}|{}", e.account_id, e.calendar_id, event_id),
        account_id: e.account_id.clone(),
        calendar_id: e.calendar_id.clone(),
        master_id: e.recurring_event_id.clone().or_else(|| e.recurrence.as_ref().map(|_| e.id.clone())),
        event_id,
        title: e.summary.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(untitled),
        start: start_ms,
        end: end_ms,
        all_day: e.all_day,
        start_date,
        end_date,
        location: e.location.clone(),
        description: e.description.clone(),
        meet_link: e.meet_link.clone(),
        html_link: e.html_link.clone(),
        color,
        attendees: e.attendees.clone(),
        organizer: e.organizer.clone(),
        organizer_self: e.organizer_self,
        self_response: e.self_response(),
        recurring: e.recurrence.is_some() || e.recurring_event_id.is_some(),
        can_edit,
        pending: e.pending,
        reminders: if e.reminders_default { cal.default_reminders.clone() } else { e.reminders.clone() },
        reminders_default: e.reminders_default,
    }
}

/// Turns stored rows (from `Db::events_for_range`) into concrete occurrences within [from, to) seconds.
pub fn build_instances(rows: Vec<EventRow>, calendars: &[Calendar], from: i64, to: i64) -> Vec<Instance> {
    let cals: HashMap<(&str, &str), &Calendar> =
        calendars.iter().map(|c| ((c.account_id.as_str(), c.id.as_str()), c)).collect();

    // Exceptions (moved or cancelled occurrences) suppress the generated occurrence at their original start.
    let mut overridden: HashSet<String> = HashSet::new();
    for r in &rows {
        if let Some(master) = &r.recurring_event_id {
            let key = match (&r.original_start_ts, &r.original_start_date) {
                (Some(ts), _) => format!("{}|{}|{master}|{ts}", r.account_id, r.calendar_id),
                (None, Some(d)) => format!("{}|{}|{master}|{d}", r.account_id, r.calendar_id),
                _ => continue,
            };
            overridden.insert(key);
        }
    }

    let mut out = Vec::new();
    for r in &rows {
        let Some(cal) = cals.get(&(r.account_id.as_str(), r.calendar_id.as_str())) else { continue };
        if r.status == "cancelled" {
            continue;
        }
        if r.recurrence.is_some() {
            for occ in expand(r, cal.time_zone.as_deref(), from, to) {
                let okey = match &occ {
                    Occurrence::Timed(ts) => format!("{}|{}|{}|{ts}", r.account_id, r.calendar_id, r.id),
                    Occurrence::Day(d) => format!("{}|{}|{}|{d}", r.account_id, r.calendar_id, r.id),
                };
                if overridden.contains(&okey) {
                    continue;
                }
                let id = instance_id(&r.id, &occ);
                let mut inst = to_instance(r, cal, id, occ);
                inst.master_id = Some(r.id.clone());
                out.push(inst);
            }
        } else if r.all_day {
            let Some(d) = r.start_date.as_deref().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()) else {
                continue;
            };
            out.push(to_instance(r, cal, r.id.clone(), Occurrence::Day(d)));
        } else if let Some(ts) = r.start_ts {
            out.push(to_instance(r, cal, r.id.clone(), Occurrence::Timed(ts)));
        }
    }
    out.sort_by_key(|i| (i.start, -(i.end - i.start)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cal() -> Calendar {
        Calendar {
            account_id: "a".into(),
            id: "c".into(),
            summary: "Cal".into(),
            color: "#5B8DEF".into(),
            selected: true,
            access_role: "owner".into(),
            primary: true,
            time_zone: Some("Europe/Moscow".into()),
            default_reminders: vec![10],
        }
    }

    fn master() -> EventRow {
        EventRow {
            account_id: "a".into(),
            calendar_id: "c".into(),
            id: "m".into(),
            status: "confirmed".into(),
            summary: Some("Стендап".into()),
            // 2026-10-19 09:30 MSK = 06:30Z, Monday
            start_ts: Some(1792391400),
            end_ts: Some(1792393200),
            time_zone: Some("Europe/Moscow".into()),
            recurrence: Some(vec!["RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR".into()]),
            reminders_default: true,
            ..Default::default()
        }
    }

    #[test]
    fn expands_weekdays_in_week() {
        let from = 1792368000; // 2026-10-19T00:00Z
        let to = from + 7 * 86400;
        let occ = expand(&master(), None, from, to);
        assert_eq!(occ.len(), 5);
    }

    #[test]
    fn exception_and_cancellation_suppress_occurrences() {
        let from = 1792368000;
        let to = from + 7 * 86400;
        let moved = EventRow {
            id: "m_20261020T063000Z".into(),
            recurring_event_id: Some("m".into()),
            original_start_ts: Some(1792391400 + 86400),
            start_ts: Some(1792391400 + 86400 + 3600),
            end_ts: Some(1792393200 + 86400 + 3600),
            recurrence: None,
            ..master()
        };
        let cancelled = EventRow {
            id: "m_20261021T063000Z".into(),
            status: "cancelled".into(),
            recurring_event_id: Some("m".into()),
            original_start_ts: Some(1792391400 + 2 * 86400),
            recurrence: None,
            ..master()
        };
        let list = build_instances(vec![master(), moved, cancelled], &[cal()], from, to);
        assert_eq!(list.len(), 4);
        assert!(list.iter().any(|i| i.event_id == "m_20261020T063000Z" && i.start == (1792391400 + 86400 + 3600) * 1000));
        assert_eq!(list[0].event_id, "m_20261019T063000Z");
        assert_eq!(list[0].reminders, vec![10]);
    }

    #[test]
    fn expands_all_day_yearly() {
        let m = EventRow {
            all_day: true,
            start_ts: None,
            end_ts: None,
            start_date: Some("2020-10-24".into()),
            end_date: Some("2020-10-25".into()),
            recurrence: Some(vec!["RRULE:FREQ=YEARLY".into()]),
            ..master()
        };
        let from = 1792368000;
        let occ = expand(&m, None, from, from + 7 * 86400);
        assert_eq!(occ.len(), 1);
        match &occ[0] {
            Occurrence::Day(d) => assert_eq!(d.to_string(), "2026-10-24"),
            _ => panic!(),
        }
    }
}
