//! The "today" popup: a small frameless window next to the tray icon with the day's events.
//! Created hidden at startup so a click on the tray shows it instantly.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Rect, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

pub const LABEL: &str = "today";
const WIDTH: f64 = 368.0;
const HEIGHT: f64 = 600.0;
/// Gap between the popup and the taskbar / screen edge, logical px.
const MARGIN: f64 = 12.0;

#[derive(Default)]
pub struct TodayState {
    pinned: AtomicBool,
    /// When the popup last hid because it lost focus. A click on the tray icon steals focus first,
    /// so without this the same click would hide the popup and show it again.
    blurred_at: Mutex<Option<Instant>>,
}

#[derive(Clone, Serialize)]
struct OpenArgs {
    /// Focus the quick-add field (Ctrl+Alt+K, "New event…").
    quick: bool,
}

pub fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    app.manage(TodayState::default());
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html?view=today".into()))
        .title("Kwalendar")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .shadow(true)
        .visible(false)
        .focused(false)
        .background_color(tauri::window::Color(0x0F, 0x10, 0x13, 0xFF))
        .build()
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(LABEL)
}

pub fn is_pinned(app: &AppHandle) -> bool {
    app.try_state::<TodayState>().map(|s| s.pinned.load(Ordering::Relaxed)).unwrap_or(false)
}

/// Left click on the tray icon.
pub fn toggle(app: &AppHandle, anchor: Option<Rect>) {
    let Some(w) = window(app) else { return };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        return;
    }
    if let Some(s) = app.try_state::<TodayState>() {
        let recent = s.blurred_at.lock().ok().and_then(|t| *t).is_some_and(|t| t.elapsed() < Duration::from_millis(300));
        if recent {
            return;
        }
    }
    show(app, anchor, false);
}

/// Shows the popup next to the tray (or `anchor`) and resets it to today.
pub fn show(app: &AppHandle, anchor: Option<Rect>, quick: bool) {
    let Some(w) = window(app) else { return };
    let anchor = anchor.or_else(|| app.tray_by_id("main").and_then(|t| t.rect().ok().flatten()));
    if let Err(e) = place(app, &w, anchor) {
        log::warn!("today popup position: {e}");
    }
    let _ = w.emit_to(LABEL, "kw:today-open", OpenArgs { quick });
    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
}

pub fn hide(app: &AppHandle) {
    if let Some(w) = window(app) {
        let _ = w.hide();
    }
}

/// Lost focus: hide unless pinned.
pub fn on_blur<R: Runtime>(w: &tauri::Window<R>) {
    let app = w.app_handle();
    let Some(s) = app.try_state::<TodayState>() else { return };
    if s.pinned.load(Ordering::Relaxed) {
        return;
    }
    if w.is_visible().unwrap_or(false) {
        if let Ok(mut t) = s.blurred_at.lock() {
            *t = Some(Instant::now());
        }
        let _ = w.hide();
    }
}

pub fn set_pinned(app: &AppHandle, pinned: bool) {
    if let Some(s) = app.try_state::<TodayState>() {
        s.pinned.store(pinned, Ordering::Relaxed);
    }
}

/// Puts the window against the taskbar edge the tray icon sits on, like Windows flyouts.
fn place(app: &AppHandle, w: &WebviewWindow, anchor: Option<Rect>) -> tauri::Result<()> {
    let (ax, ay, aw, ah) = match anchor {
        Some(r) => {
            let p = r.position.to_physical::<f64>(1.0);
            let s = r.size.to_physical::<f64>(1.0);
            (p.x, p.y, s.width, s.height)
        }
        None => {
            let c = app.cursor_position()?;
            (c.x, c.y, 0.0, 0.0)
        }
    };
    let (cx, cy) = (ax + aw / 2.0, ay + ah / 2.0);
    let monitor = app.monitor_from_point(cx, cy)?.or(app.primary_monitor()?);
    let Some(m) = monitor else { return Ok(()) };
    let scale = m.scale_factor();
    let wa = m.work_area();
    let (left, top) = (wa.position.x as f64, wa.position.y as f64);
    let (right, bottom) = (left + wa.size.width as f64, top + wa.size.height as f64);
    let (pw, ph) = (WIDTH * scale, HEIGHT * scale);
    let gap = MARGIN * scale;

    let clamp_x = |x: f64| x.max(left + gap).min(right - pw - gap);
    let clamp_y = |y: f64| y.max(top + gap).min(bottom - ph - gap);
    let (x, y) = if anchor.is_none() {
        // Hotkey without a known tray position: bottom-right corner of the work area.
        (right - pw - gap, bottom - ph - gap)
    } else if cy >= bottom {
        (clamp_x(cx - pw / 2.0), bottom - ph - gap)
    } else if cy <= top {
        (clamp_x(cx - pw / 2.0), top + gap)
    } else if cx >= right {
        (right - pw - gap, clamp_y(cy - ph / 2.0))
    } else if cx <= left {
        (left + gap, clamp_y(cy - ph / 2.0))
    } else {
        // Overflow area or an unusual layout: just above the icon.
        (clamp_x(cx - pw / 2.0), clamp_y(ay - ph - gap))
    };
    w.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))?;
    // Moving between monitors with different scaling can resize the window; keep it logical.
    w.set_size(LogicalSize::new(WIDTH, HEIGHT))?;
    Ok(())
}
