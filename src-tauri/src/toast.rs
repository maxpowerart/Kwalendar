//! Native Windows toasts with action buttons (join / snooze), driven by core events.

use std::sync::Arc;

use chrono::{Local, TimeZone};
use kw_core::{Core, Instance, Notice, Reminder};
use tauri::{AppHandle, Emitter};

pub const AUMID: &str = "com.kwalendar.app";

fn hm(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.format("%H:%M").to_string())
        .unwrap_or_default()
}

fn day_label(ms: i64) -> String {
    const MONTHS: [&str; 12] = [
        "января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября", "октября", "ноября", "декабря",
    ];
    let Some(d) = Local.timestamp_millis_opt(ms).single() else { return String::new() };
    let today = Local::now().date_naive();
    let date = d.date_naive();
    if date == today {
        "сегодня".into()
    } else if date == today.succ_opt().unwrap_or(today) {
        "завтра".into()
    } else {
        use chrono::Datelike;
        format!("{} {}", date.day(), MONTHS[date.month0() as usize])
    }
}

fn when_text(r: &Reminder) -> String {
    let i = &r.instance;
    let now = chrono::Utc::now().timestamp_millis();
    let left = (i.start - now) / 60_000;
    let lead = if left <= 0 {
        "Начинается сейчас".to_string()
    } else if left < 60 {
        format!("Через {left} мин")
    } else if left < 24 * 60 {
        format!("Через {} ч {} мин", left / 60, left % 60).replace(" 0 мин", "")
    } else {
        format!("{}", day_label(i.start))
    };
    if i.all_day {
        format!("{lead} · весь день")
    } else {
        format!("{lead} · {}–{}", hm(i.start), hm(i.end))
    }
}

fn detail_text(i: &Instance) -> String {
    let mut parts = Vec::new();
    if i.meet_link.is_some() {
        parts.push(if i.meet_link.as_deref().unwrap_or("").contains("zoom") { "Zoom" } else { "Видеовстреча" }.to_string());
    }
    if let Some(l) = &i.location {
        if !l.starts_with("http") {
            parts.push(l.clone());
        }
    }
    if !i.attendees.is_empty() {
        parts.push(format!("{} участн.", i.attendees.len()));
    }
    parts.join(" · ")
}

pub fn show_main(app: &AppHandle) {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Makes our AppUserModelID known to the shell so toasts carry the app's name and icon (unpackaged apps).
#[cfg(windows)]
pub fn register_aumid(icon_path: Option<&std::path::Path>) {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok((key, _)) = hkcu.create_subkey(format!("Software\\Classes\\AppUserModelId\\{AUMID}")) {
        let _ = key.set_value("DisplayName", &"Kwalendar");
        if let Some(p) = icon_path {
            let _ = key.set_value("IconUri", &p.to_string_lossy().to_string());
        }
    }
}

#[cfg(not(windows))]
pub fn register_aumid(_icon_path: Option<&std::path::Path>) {}

#[cfg(windows)]
pub fn reminder(app: &AppHandle, core: Arc<Core>, r: Reminder) {
    use tauri_winrt_notification::{Scenario, Sound, Toast};
    let inst = r.instance.clone();
    let mut toast = Toast::new(AUMID)
        .title(&inst.title)
        .text1(&when_text(&r))
        .text2(&detail_text(&inst))
        .scenario(Scenario::Reminder)
        .sound(Some(Sound::Reminder));
    if inst.meet_link.is_some() {
        toast = toast.add_button("Присоединиться", "join");
    }
    toast = toast.add_button("Через 5 мин", "snooze").add_button("Скрыть", "dismiss");
    let app2 = app.clone();
    let toast = toast.on_activated(move |action| {
        match action.as_deref() {
            Some("join") => {
                if let Some(link) = &inst.meet_link {
                    crate::open_external(&app2, link);
                }
            }
            Some("snooze") => core.snooze(inst.clone(), 5),
            Some("dismiss") => {}
            _ => {
                show_main(&app2);
                let _ = app2.emit("kw:open-event", inst.clone());
            }
        }
        Ok(())
    });
    if let Err(e) = toast.show() {
        log::error!("toast failed: {e}");
    }
}

#[cfg(windows)]
pub fn notice(app: &AppHandle, n: &Notice) {
    use tauri_winrt_notification::Toast;
    let (title, text) = match n {
        Notice::Moved { title, old_start, new_start, all_day } => (
            format!("Перенесено: {title}"),
            if *all_day {
                format!("{} → {}", day_label(*old_start), day_label(*new_start))
            } else {
                format!("{}, {} → {}, {}", day_label(*old_start), hm(*old_start), day_label(*new_start), hm(*new_start))
            },
        ),
        Notice::Invite { title, start, organizer } => (
            format!("Приглашение: {title}"),
            format!(
                "{}, {}{}",
                day_label(*start),
                hm(*start),
                organizer.as_ref().map(|o| format!(" · от {o}")).unwrap_or_default()
            ),
        ),
        Notice::Reauth { email } => ("Нужно войти снова".into(), format!("{email}: доступ к календарю отозван")),
        Notice::Conflict { title } => ("Изменение не сохранено".into(), format!("«{title}» изменили на сервере — показана свежая версия")),
        Notice::Error { message } => ("Kwalendar".into(), message.clone()),
    };
    let app2 = app.clone();
    let res = Toast::new(AUMID)
        .title(&title)
        .text1(&text)
        .on_activated(move |_| {
            show_main(&app2);
            Ok(())
        })
        .show();
    if let Err(e) = res {
        log::error!("toast failed: {e}");
    }
}

#[cfg(not(windows))]
pub fn reminder(_app: &AppHandle, _core: Arc<Core>, _r: Reminder) {}

#[cfg(not(windows))]
pub fn notice(_app: &AppHandle, _n: &Notice) {}
