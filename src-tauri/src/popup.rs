//! The popup window. It is destroyed (not hidden) on close so the WebView frees its memory.

use crate::view::{build_view, PopupView};
use crate::{poller, Shared};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_positioner::{Position, WindowExt};

pub const LABEL: &str = "popup";
const WIDTH: f64 = 320.0;
const MAX_HEIGHT: f64 = 480.0;
/// A tray click arriving this soon after a blur-close belongs to the same gesture.
const REOPEN_GUARD: Duration = Duration::from_millis(300);

pub fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.set_focus();
        return;
    }
    if let Err(e) = create(app) {
        eprintln!("claude-usage-monitor: cannot open popup: {e}");
    }
}

pub fn toggle(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.destroy();
        return;
    }
    let closed_at = *app.state::<Shared>().popup_closed_at.lock().unwrap();
    if closed_at.is_some_and(|t| t.elapsed() < REOPEN_GUARD) {
        return;
    }
    show(app);
}

fn create(app: &AppHandle) -> tauri::Result<()> {
    dbg_log("popup: create");
    let title = app.state::<Shared>().strings.t("app_name").to_string();
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(WIDTH, 200.0)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .focused(false)
        .build()?;
    let handle = app.clone();
    window.on_window_event(move |event| {
        dbg_log(&format!("popup event: {event:?}"));
        match event {
        // A hidden window also gets focus events while it is being created;
        // only a blur of the visible popup means "user clicked elsewhere".
        WindowEvent::Focused(false) => {
            if let Some(w) = handle.get_webview_window(LABEL) {
                if w.is_visible().unwrap_or(false) {
                    let _ = w.destroy();
                }
            }
        }
        WindowEvent::Destroyed => {
            let shared = handle.state::<Shared>();
            shared.popup_open.store(false, Ordering::Relaxed);
            *shared.popup_closed_at.lock().unwrap() = Some(Instant::now());
        }
        _ => {}
    }});
    app.state::<Shared>().popup_open.store(true, Ordering::Relaxed);
    poller::request_refresh(app);
    Ok(())
}

fn position(app: &AppHandle) -> Position {
    let tray_known = app.state::<Shared>().tray_pos_known.load(Ordering::Relaxed);
    match (tray_known, cfg!(target_os = "macos")) {
        (true, true) => Position::TrayBottomCenter,
        (true, false) => Position::TrayCenter,
        (false, true) => Position::TopRight,
        (false, false) => Position::BottomRight,
    }
}

/// Sends fresh data to the popup if it is open.
pub fn emit_state(app: &AppHandle) {
    if app.get_webview_window(LABEL).is_some() {
        let _ = app.emit_to(LABEL, "state", current_view(app));
    }
}

fn current_view(app: &AppHandle) -> PopupView {
    let shared = app.state::<Shared>();
    let st = shared.state.lock().unwrap();
    build_view(&st, &shared.settings, &shared.strings, time::OffsetDateTime::now_utc())
}

#[tauri::command]
pub fn get_state(app: AppHandle) -> PopupView {
    current_view(&app)
}

#[tauri::command]
pub fn get_strings(app: AppHandle) -> HashMap<String, String> {
    app.state::<Shared>().strings.all().clone()
}

#[tauri::command]
pub fn refresh(app: AppHandle) {
    poller::request_refresh(&app);
}

#[tauri::command]
pub fn close_popup(app: AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.destroy();
    }
}

/// Called by the page after rendering: sizes the window to its content, places it
/// next to the tray icon and shows it.
#[tauri::command]
pub fn fit_popup(app: AppHandle, height: f64) {
    dbg_log(&format!("popup: fit {height}"));
    let Some(w) = app.get_webview_window(LABEL) else { return };
    let _ = w.set_size(LogicalSize::new(WIDTH, height.clamp(80.0, MAX_HEIGHT)));
    let _ = w.as_ref().window().move_window(position(&app));
    if !w.is_visible().unwrap_or(false) {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Debug-build-only trace to stderr.
fn dbg_log(msg: &str) {
    if cfg!(debug_assertions) {
        eprintln!("[debug] {msg}");
    }
}
