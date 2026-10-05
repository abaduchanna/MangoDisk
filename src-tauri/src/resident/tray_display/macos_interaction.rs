//! Transient status-item menus preserve left-click delivery on macOS 27.
use objc2::MainThreadMarker;
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon};

use crate::resident::{diagnostics::Failure, panel};

#[derive(Debug, PartialEq, Eq)]
enum EntryAction {
    TogglePanel,
    ShowMenu,
}

fn entry_action(button: MouseButton, state: MouseButtonState) -> Option<EntryAction> {
    // Act on press, before the status button's focus transition completes. A
    // release must never reopen a panel already closed by the same press.
    match (button, state) {
        (MouseButton::Left, MouseButtonState::Down) => Some(EntryAction::TogglePanel),
        (MouseButton::Right, MouseButtonState::Down) => Some(EntryAction::ShowMenu),
        _ => None,
    }
}

pub(super) fn click(tray: &TrayIcon, button: MouseButton, state: MouseButtonState) {
    let Some(action) = entry_action(button, state) else {
        return;
    };
    log::info!(
        "resident_macos_entry source={} button={button:?} state={state:?} action={action:?}",
        tray.id().as_ref()
    );
    let tray = tray.clone();
    // Leave Tauri's tray-listener lock before a nested native menu event loop.
    tauri::async_runtime::spawn(async move {
        let app = tray.app_handle();
        match action {
            EntryAction::TogglePanel => {
                if let Err(error) = panel::toggle_from(app, tray.id().as_ref(), None) {
                    Failure::record("panel_entry", &error);
                }
            }
            EntryAction::ShowMenu => {
                let result = super::menu(app, &super::labels::Labels::load(app))
                    .and_then(|menu| show_menu(&tray, menu));
                if let Err(error) = result {
                    Failure::record("tray_menu_dispatch", &error);
                }
            }
        }
    });
}

fn show_menu(tray: &TrayIcon, menu: tauri::menu::Menu<tauri::Wry>) -> tauri::Result<()> {
    let app = tray.app_handle().clone();
    let tray = tray.clone();
    app.run_on_main_thread(move || {
        panel::hide(tray.app_handle());
        let result = tray.set_menu(Some(menu)).and_then(|()| {
            tray.with_inner_tray_icon(|inner| {
                let mtm = MainThreadMarker::new().expect("tray menus run on the main thread");
                let item = inner.ns_status_item().ok_or_else(|| {
                    tauri::Error::Io(std::io::Error::other("tray_menu_status_item_missing"))
                })?;
                let button = item.button(mtm).ok_or_else(|| {
                    tauri::Error::Io(std::io::Error::other("tray_menu_button_missing"))
                })?;
                // Retain native objects without borrowing tray-icon's RefCell
                // across performClick's reentrant event loop (upstream #365).
                // AppKit owns this status button's menu action. The retained
                // item/button remain alive on the main thread until it returns.
                unsafe { button.performClick(None) };
                Ok::<(), tauri::Error>(())
            })?
        });
        // Clear both AppKit's menu and tray-icon's retained copy, including on
        // failure. Locale changes never attach a menu during ordinary refresh.
        let detached = tray.set_menu(None::<tauri::menu::Menu<tauri::Wry>>);
        if let Err(error) = result {
            Failure::record("tray_menu_present", &error);
        }
        if let Err(error) = detached {
            Failure::record("tray_menu_detach", &error);
        } else {
            log::info!(
                "resident_macos_menu_finished source={} attached=false",
                tray.id().as_ref()
            );
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_press_routes_once_and_releases_never_reopen_the_panel() {
        assert_eq!(
            entry_action(MouseButton::Left, MouseButtonState::Down),
            Some(EntryAction::TogglePanel)
        );
        assert_eq!(
            entry_action(MouseButton::Right, MouseButtonState::Down),
            Some(EntryAction::ShowMenu)
        );
        for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
            assert_eq!(entry_action(button, MouseButtonState::Up), None);
        }
        assert_eq!(
            entry_action(MouseButton::Middle, MouseButtonState::Down),
            None
        );
    }
}
