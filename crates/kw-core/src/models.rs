use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub color: String,
    /// "ok" | "syncing" | "reauth" | "offline" | "error"
    pub status: String,
    pub error: Option<String>,
    pub last_sync: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Calendar {
    pub account_id: String,
    pub id: String,
    pub summary: String,
    pub color: String,
    pub selected: bool,
    pub access_role: String,
    pub primary: bool,
    pub time_zone: Option<String>,
    pub default_reminders: Vec<i64>,
}

impl Calendar {
    pub fn writable(&self) -> bool {
        self.access_role == "owner" || self.access_role == "writer"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attendee {
    pub email: String,
    pub name: Option<String>,
    /// needsAction | declined | tentative | accepted
    pub response: String,
    #[serde(default)]
    pub is_self: bool,
    #[serde(default)]
    pub organizer: bool,
    #[serde(default)]
    pub optional: bool,
}

/// One stored Google event (a single event, a recurring master, or an exception).
#[derive(Debug, Clone, Default)]
pub struct EventRow {
    pub account_id: String,
    pub calendar_id: String,
    pub id: String,
    pub etag: Option<String>,
    pub status: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    pub all_day: bool,
    /// Unix seconds, timed events only.
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    /// YYYY-MM-DD, all-day events only. `end_date` is exclusive (Google semantics).
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub time_zone: Option<String>,
    /// Raw RFC 5545 lines (RRULE/EXDATE/RDATE) for recurring masters.
    pub recurrence: Option<Vec<String>>,
    pub recurring_event_id: Option<String>,
    pub original_start_ts: Option<i64>,
    pub original_start_date: Option<String>,
    pub meet_link: Option<String>,
    pub html_link: Option<String>,
    pub organizer: Option<String>,
    pub organizer_self: bool,
    pub attendees: Vec<Attendee>,
    pub reminders_default: bool,
    pub reminders: Vec<i64>,
    pub color_id: Option<String>,
    pub updated: Option<String>,
    pub pending: bool,
}

impl EventRow {
    pub fn self_response(&self) -> Option<String> {
        self.attendees.iter().find(|a| a.is_self).map(|a| a.response.clone())
    }
}

/// A concrete occurrence sent to the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub key: String,
    pub account_id: String,
    pub calendar_id: String,
    /// Id to use for API calls on this occurrence (Google instance id for expanded recurrences).
    pub event_id: String,
    pub master_id: Option<String>,
    pub title: String,
    /// Unix milliseconds. For all-day events: local midnight of start/end dates.
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub meet_link: Option<String>,
    pub html_link: Option<String>,
    pub color: String,
    pub attendees: Vec<Attendee>,
    pub organizer: Option<String>,
    pub organizer_self: bool,
    pub self_response: Option<String>,
    pub recurring: bool,
    pub can_edit: bool,
    pub pending: bool,
    pub reminders: Vec<i64>,
    pub reminders_default: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventInput {
    pub account_id: String,
    pub calendar_id: String,
    pub title: String,
    pub all_day: bool,
    /// Unix ms (timed)
    pub start: Option<i64>,
    pub end: Option<i64>,
    /// YYYY-MM-DD (all-day, end exclusive)
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub time_zone: Option<String>,
    pub location: Option<String>,
    pub description: Option<String>,
    /// None = calendar defaults
    pub reminders: Option<Vec<i64>>,
    #[serde(default)]
    pub attendees: Vec<String>,
    #[serde(default)]
    pub add_meet: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    /// "idle" | "syncing" | "offline" | "error"
    pub state: String,
    pub last_sync: Option<i64>,
    pub pending: i64,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub key: String,
    pub instance: Instance,
    pub minutes_before: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Notice {
    /// Someone else moved an event.
    Moved { title: String, old_start: i64, new_start: i64, all_day: bool },
    /// A new invitation waiting for an answer.
    Invite { title: String, start: i64, organizer: Option<String> },
    /// Our change lost to a newer server version.
    Conflict { title: String },
    /// Account needs to sign in again.
    Reauth { email: String },
    Error { message: String },
}

/// Events the core pushes to the shell.
#[derive(Debug, Clone)]
pub enum CoreEvent {
    DataChanged,
    Sync(SyncStatus),
    Reminder(Reminder),
    Notice(Notice),
}

pub const ACCOUNT_COLORS: [&str; 6] = ["#A57CF0", "#5B8DEF", "#E5604F", "#3DBE8B", "#F0A04B", "#39B8C9"];

/// Google event colorId palette.
pub fn event_color(id: &str) -> Option<&'static str> {
    Some(match id {
        "1" => "#7986CB",
        "2" => "#33B679",
        "3" => "#8E24AA",
        "4" => "#E67C73",
        "5" => "#F6BF26",
        "6" => "#F4511E",
        "7" => "#039BE5",
        "8" => "#616161",
        "9" => "#3F51B5",
        "10" => "#0B8043",
        "11" => "#D50000",
        _ => return None,
    })
}
