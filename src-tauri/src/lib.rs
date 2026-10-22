mod toast;
mod updater;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use kw_core::{Account, Config, Core, CoreEvent, DeleteScope, EditTarget, EventInput, Instance, Notice, Snapshot};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_opener::OpenerExt as _;

pub(crate) struct AppState {
    pub(crate) core: Arc<Core>,
    start_hidden: AtomicBool,
}

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub(crate) fn open_external(app: &AppHandle, url: &str) {
    if url.starts_with("https://") || url.starts_with("http://") {
        if let Err(e) = app.opener().open_url(url, None::<&str>) {
            log::error!("open url: {e}");
        }
    }
}

#[tauri::command]
fn snapshot(s: State<AppState>) -> Res<Snapshot> {
    s.core.snapshot().map_err(err)
}

#[tauri::command]
fn instances(s: State<AppState>, from: i64, to: i64) -> Res<Vec<Instance>> {
    s.core.instances(from, to).map_err(err)
}

#[tauri::command]
fn search(s: State<AppState>, q: String) -> Res<Vec<Instance>> {
    s.core.search(&q).map_err(err)
}

#[tauri::command]
fn set_calendar_selected(s: State<AppState>, account_id: String, calendar_id: String, selected: bool) -> Res<()> {
    s.core.set_calendar_selected(&account_id, &calendar_id, selected).map_err(err)
}

#[tauri::command]
async fn add_account(app: AppHandle, s: State<'_, AppState>, hint: Option<String>) -> Res<Account> {
    let core = s.core.clone();
    let app2 = app.clone();
    core.add_account(move |url| open_external(&app2, &url), hint).await.map_err(err)
}

#[tauri::command]
fn remove_account(s: State<AppState>, account_id: String) -> Res<()> {
    s.core.remove_account(&account_id).map_err(err)
}

#[tauri::command]
fn sync_now(s: State<AppState>) {
    s.core.sync_now();
}

#[tauri::command]
fn create_event(s: State<AppState>, input: EventInput) -> Res<String> {
    s.core.create_event(input).map_err(err)
}

#[tauri::command]
fn update_event(s: State<AppState>, target: EditTarget, input: EventInput) -> Res<()> {
    s.core.update_event(target, input).map_err(err)
}

#[tauri::command]
fn update_series(s: State<AppState>, target: EditTarget, input: EventInput) -> Res<()> {
    s.core.update_series(target, input).map_err(err)
}

#[tauri::command]
fn move_event(s: State<AppState>, target: EditTarget, start: i64, end: i64) -> Res<()> {
    s.core.move_event(target, start, end).map_err(err)
}

#[tauri::command]
fn delete_event(s: State<AppState>, target: EditTarget, scope: DeleteScope) -> Res<()> {
    s.core.delete_event(target, scope).map_err(err)
}

#[tauri::command]
fn respond(s: State<AppState>, target: EditTarget, response: String) -> Res<()> {
    s.core.respond(target, &response).map_err(err)
}

#[tauri::command]
fn set_config(s: State<AppState>, mut config: Config) -> Res<()> {
    let current = s.core.config();
    if config.google_client_secret == "••••" {
        config.google_client_secret = current.google_client_secret;
    }
    s.core.set_config(config).map_err(err)
}

#[tauri::command]
fn mute(s: State<AppState>, minutes: i64) {
    s.core.mute(minutes);
}

#[tauri::command]
fn open_url(app: AppHandle, url: String) {
    open_external(&app, &url);
}

#[tauri::command]
fn app_ready(app: AppHandle, s: State<AppState>) {
    // The window stays hidden until the UI painted with data: no white flash, no empty frame.
    if !s.start_hidden.swap(false, Ordering::Relaxed) {
        toast::show_main(&app);
    }
}

#[tauri::command]
fn autostart_get(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn autostart_set(app: AppHandle, enabled: bool) -> Res<()> {
    let al = app.autolaunch();
    if enabled { al.enable() } else { al.disable() }.map_err(err)
}

#[tauri::command]
fn test_notification(app: AppHandle, s: State<AppState>) {
    let now = chrono::Utc::now().timestamp_millis();
    let inst = Instance {
        key: "test".into(),
        account_id: String::new(),
        calendar_id: String::new(),
        event_id: "test".into(),
        master_id: None,
        title: "Проверка уведомлений".into(),
        start: now + 10 * 60_000,
        end: now + 40 * 60_000,
        all_day: false,
        start_date: None,
        end_date: None,
        location: None,
        description: None,
        meet_link: Some("https://meet.google.com/".into()),
        html_link: None,
        color: "#8EA2FF".into(),
        attendees: vec![],
        organizer: None,
        organizer_self: true,
        self_response: None,
        recurring: false,
        can_edit: false,
        pending: false,
        reminders: vec![10],
        reminders_default: true,
    };
    toast::reminder(&app, s.core.clone(), kw_core::Reminder { key: "test".into(), instance: inst, minutes_before: 10 });
}

fn update_tray_tooltip(app: &AppHandle, core: &Core) {
    let now = chrono::Utc::now().timestamp_millis();
    let next = core
        .instances(now, now + 24 * 3600_000)
        .ok()
        .and_then(|l| l.into_iter().find(|i| !i.all_day && i.start >= now && i.self_response.as_deref() != Some("declined")));
    let text = match next {
        Some(i) => {
            use chrono::TimeZone;
            let t = chrono::Local
                .timestamp_millis_opt(i.start)
                .single()
                .map(|d| d.format("%H:%M").to_string())
                .unwrap_or_default();
            format!("Kwalendar — далее {t} {}", i.title)
        }
        None => "Kwalendar".into(),
    };
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(text));
    }
}

fn handle_core_event(app: &AppHandle, ev: CoreEvent) {
    match ev {
        CoreEvent::DataChanged => {
            let _ = app.emit("kw:changed", ());
            if let Some(s) = app.try_state::<AppState>() {
                update_tray_tooltip(app, &s.core);
            }
        }
        CoreEvent::Sync(status) => {
            let _ = app.emit("kw:sync", status);
        }
        CoreEvent::Reminder(r) => {
            let _ = app.emit("kw:reminder", &r);
            if let Some(s) = app.try_state::<AppState>() {
                toast::reminder(app, s.core.clone(), r);
            }
        }
        CoreEvent::Notice(n) => {
            let _ = app.emit("kw:notice", &n);
            let focused = app
                .get_webview_window("main")
                .and_then(|w| w.is_focused().ok())
                .unwrap_or(false);
            // In-app banner while the user looks at the window; native toast otherwise.
            if !focused || matches!(n, Notice::Reauth { .. }) {
                toast::notice(app, &n);
            }
        }
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Открыть Kwalendar", true, None::<&str>)?;
    let new = MenuItem::with_id(app, "new", "Новое событие…", true, Some("Ctrl+Alt+K"))?;
    let sync = MenuItem::with_id(app, "sync", "Синхронизировать", true, None::<&str>)?;
    let mute = MenuItem::with_id(app, "mute", "Тишина на 1 час", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &new, &sep, &sync, &mute, &sep2, &quit])?;
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().expect("app icon"))
        .tooltip("Kwalendar")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| match ev.id().as_ref() {
            "open" => toast::show_main(app),
            "new" => {
                toast::show_main(app);
                let _ = app.emit("kw:quickadd", ());
            }
            "sync" => {
                if let Some(s) = app.try_state::<AppState>() {
                    s.core.sync_now();
                }
            }
            "mute" => {
                if let Some(s) = app.try_state::<AppState>() {
                    s.core.mute(60);
                    let _ = app.emit("kw:changed", ());
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                toast::show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,kw_core=info")).init();
    let start_hidden = std::env::args().any(|a| a == "--hidden");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| toast::show_main(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcuts(["ctrl+alt+k"])
                .expect("shortcut")
                .with_handler(|app, _shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        toast::show_main(app);
                        let _ = app.emit("kw:quickadd", ());
                    }
                })
                .build(),
        )
        .setup(move |app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let icon = dir.join("icon.png");
            // Rewritten every start so a new app icon reaches toasts too.
            let _ = std::fs::write(&icon, include_bytes!("../icons/128x128@2x.png"));
            toast::register_aumid(Some(&icon));

            let handle = app.handle().clone();
            let core = Core::open(&dir, move |ev| handle_core_event(&handle, ev))?;
            app.manage(updater::Updates::default());
            app.manage(AppState { core: core.clone(), start_hidden: AtomicBool::new(start_hidden) });
            build_tray(app.handle())?;
            update_tray_tooltip(app.handle(), &core);

            tauri::async_runtime::spawn(core.clone().run_sync_loop());
            tauri::async_runtime::spawn(core.clone().run_reminder_loop());
            tauri::async_runtime::spawn(core.clone().run_push_loop());
            tauri::async_runtime::spawn(updater::background(app.handle().clone()));

            // Safety net: show the window even if the UI never reports ready.
            if !start_hidden {
                let h = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                    if let Some(s) = h.try_state::<AppState>() {
                        if !s.start_hidden.load(Ordering::Relaxed) {
                            if let Some(w) = h.get_webview_window("main") {
                                if !w.is_visible().unwrap_or(true) {
                                    let _ = w.show();
                                }
                            }
                        }
                    }
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                // Closing hides to tray: reminders keep working.
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::Focused(f) => {
                if let Some(s) = window.app_handle().try_state::<AppState>() {
                    s.core.set_focused(*f);
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            instances,
            search,
            set_calendar_selected,
            add_account,
            remove_account,
            sync_now,
            create_event,
            update_event,
            update_series,
            move_event,
            delete_event,
            respond,
            set_config,
            mute,
            open_url,
            app_ready,
            autostart_get,
            autostart_set,
            test_notification,
            updater::update_info,
            updater::update_check,
            updater::update_list,
            updater::update_manifest,
            updater::update_skip,
            updater::update_install
        ])
        .run(tauri::generate_context!())
        .expect("error while running Kwalendar");
}
