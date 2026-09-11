mod application_icons;
pub mod autostart;
mod diagnostics;
pub mod main_window;
pub mod memory_release;
pub mod panel;
pub mod preferences;
pub mod runtime;

use std::sync::{atomic::Ordering, Arc};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

pub const PANEL_LABEL: &str = "tray-panel";
pub const TRAY_ID: &str = "resident";

pub fn install(app: &tauri::AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "resident-open", "MangoDisk", true, None::<&str>)?;
    let quit = PredefinedMenuItem::quit(app, None)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(tauri::include_image!("icons/icon.png"))
        .tooltip("MangoDisk")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "resident-open" {
                main_window::request(app, main_window::Destination::Main, "tray_menu");
            }
        })
        .on_tray_icon_event(|tray, event| {
            #[cfg(windows)]
            if matches!(event, TrayIconEvent::Leave { .. }) {
                panel::tray_pointer_left(tray.app_handle());
            }
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                let state = app.state::<Arc<runtime::ResidentState>>();
                let was_open = state.panel_open.load(Ordering::Relaxed);
                log::info!("resident_tray_clicked panel_open={was_open}");
                if was_open {
                    panel::hide(app);
                } else {
                    // WebView2 creation must leave the native event handler to avoid
                    // the Windows synchronous window-creation deadlock.
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(error) = panel::open(&app) {
                            log::warn!("resident_panel_open_failed error={error}");
                        }
                    });
                }
            }
        });
    let tray = builder.build(app)?;
    let preferences = preferences::load(app);
    tray.set_visible(preferences.enabled)?;
    let state = runtime::start(app, preferences);
    app.manage(state);
    log::info!("resident_started enabled={}", preferences.enabled);
    Ok(())
}

/// macOS keeps its native close/reopen convention; Windows stays resident only
/// while the tray feature is enabled. Explicit Quit never enters this handler.
pub fn handle_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() != crate::MAIN_WINDOW_LABEL {
        return;
    }
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        let enabled = window
            .try_state::<Arc<runtime::ResidentState>>()
            .is_some_and(|state| state.enabled());
        if cfg!(target_os = "macos") || enabled {
            // A hidden tray cannot provide a reopening affordance on Windows.
            // macOS retains Dock reopening even when monitoring is disabled.
            api.prevent_close();
            match main_window::hide(window.app_handle(), enabled) {
                Ok(()) => log::info!("main_window_hidden reason=close_requested"),
                Err(error) => log::warn!("main_window_hide_failed error={error}"),
            }
        } else {
            // A previously opened, hidden panel is still a native window. Closing
            // the main window must therefore explicitly quit when residency is off.
            window.app_handle().exit(0);
        }
    }
}
