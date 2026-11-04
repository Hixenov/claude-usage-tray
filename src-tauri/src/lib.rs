pub mod alerts;
pub mod api;
pub mod credentials;
pub mod i18n;
pub mod model;
pub mod pace;
pub mod poller;
pub mod popup;
pub mod settings;
pub mod tray;
pub mod tray_icon;
pub mod view;

use alerts::FiredSet;
use i18n::Strings;
use model::AppState;
use settings::Settings;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Manager, RunEvent};
use tauri_plugin_notification::NotificationExt;

/// State shared between the tray, the poller and the popup.
pub struct Shared {
    pub state: Mutex<AppState>,
    pub settings: Settings,
    pub strings: Strings,
    pub fired: Mutex<FiredSet>,
    pub refresh: tokio::sync::Notify,
    pub popup_open: AtomicBool,
    /// Last manual refresh, for the 10 s throttle.
    pub last_manual: Mutex<Option<Instant>>,
    /// When the popup was last closed (a tray click right after a blur-close must not reopen it).
    pub popup_closed_at: Mutex<Option<Instant>>,
    /// The positioner plugin knows where the tray icon is.
    pub tray_pos_known: AtomicBool,
    pub dark_panel: bool,
}

/// Shows a system notification titled with the app name.
pub fn notify(app: &AppHandle, body: &str) {
    let title = app.state::<Shared>().strings.t("app_name").to_string();
    let _ = app.notification().builder().title(title).body(body).show();
}

pub fn run() {
    let (settings, settings_err) = settings::load_from(&settings::search_dirs());
    let lang = i18n::resolve_lang(&settings.language, sys_locale::get_locale().as_deref());
    let shared = Shared {
        state: Mutex::new(AppState::default()),
        settings,
        strings: Strings::new(lang),
        fired: Mutex::new(FiredSet::default()),
        refresh: tokio::sync::Notify::new(),
        popup_open: AtomicBool::new(false),
        last_manual: Mutex::new(None),
        popup_closed_at: Mutex::new(None),
        tray_pos_known: AtomicBool::new(false),
        dark_panel: tray_icon::panel_is_dark(),
    };

    tauri::Builder::default()
        // Must be the first plugin: a second launch just opens the popup of the first.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            popup::show(app)
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_positioner::init())
        .manage(shared)
        .invoke_handler(tauri::generate_handler![
            popup::get_state,
            popup::get_strings,
            popup::refresh,
            popup::close_popup,
            popup::fit_popup
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            if let Err(e) = tray::create(&handle) {
                eprintln!("claude-usage-tray: system tray is not available: {e}");
                notify(&handle, &format!("System tray is not available: {e}"));
                std::process::exit(1);
            }
            if let Some(msg) = settings_err {
                let text = view::fill(
                    handle.state::<Shared>().strings.t("err_settings"),
                    &[("msg", &msg)],
                );
                notify(&handle, &text);
            }
            poller::spawn(handle);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // Closing the popup must not quit the tray app.
            if let RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
            }
        });
}
