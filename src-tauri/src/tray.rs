//! System tray icon, tooltip and menu.

use crate::model::ApiError;
use crate::{notify, poller, popup, tray_icon, view, Shared};
use std::sync::atomic::Ordering;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

pub const TRAY_ID: &str = "main";

#[cfg(target_os = "macos")]
const ICON_SIZE: u32 = 44;
#[cfg(not(target_os = "macos"))]
const ICON_SIZE: u32 = 32;

fn icon(app: &AppHandle) -> Image<'static> {
    let shared = app.state::<Shared>();
    let st = shared.state.lock().unwrap();
    let now = time::OffsetDateTime::now_utc();
    let error = matches!(
        st.last_error,
        Some(ApiError::NoToken | ApiError::AuthExpired)
    ) || (st.snapshot.is_none() && st.last_error.is_some());
    let bars = view::tray_bars(&st, &shared.settings, now);
    let rgba = tray_icon::render(&bars, error, ICON_SIZE, shared.dark_panel);
    Image::new_owned(rgba, ICON_SIZE, ICON_SIZE)
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let shared = app.state::<Shared>();
    let t = &shared.strings;
    let open = MenuItem::with_id(app, "open", t.t("menu_open"), true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", t.t("menu_refresh"), true, None::<&str>)?;
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        t.t("menu_autostart"),
        true,
        autostart_on,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", t.t("menu_quit"), true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &refresh,
            &PredefinedMenuItem::separator(app)?,
            &autostart,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let autostart_item = autostart.clone();
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon(app))
        .tooltip(t.t("app_name"))
        .menu(&menu)
        // macOS convention: a click opens the menu. Elsewhere a left click opens the popup.
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => popup::show(app),
            "refresh" => poller::request_refresh(app),
            "autostart" => toggle_autostart(app, &autostart_item),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();
            tauri_plugin_positioner::on_tray_event(app, &event);
            app.state::<Shared>()
                .tray_pos_known
                .store(true, Ordering::Relaxed);
            if cfg!(not(target_os = "macos")) {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    popup::toggle(app);
                }
            }
        })
        .build(app)?;
    Ok(())
}

fn toggle_autostart(app: &AppHandle, item: &CheckMenuItem<tauri::Wry>) {
    let launcher = app.autolaunch();
    let want = !launcher.is_enabled().unwrap_or(false);
    let result = if want {
        launcher.enable()
    } else {
        launcher.disable()
    };
    if let Err(e) = result {
        notify(app, &e.to_string());
    }
    let _ = item.set_checked(launcher.is_enabled().unwrap_or(false));
}

/// Redraws the icon and tooltip from the current state.
pub fn update(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let tooltip = {
        let shared = app.state::<Shared>();
        let st = shared.state.lock().unwrap();
        view::tooltip_text(
            &st,
            &shared.settings,
            &shared.strings,
            time::OffsetDateTime::now_utc(),
        )
    };
    let _ = tray.set_icon(Some(icon(app)));
    let _ = tray.set_tooltip(Some(tooltip));
}
