use std::sync::{atomic::Ordering, Arc};

use tauri::{Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use super::{runtime::ResidentState, PANEL_LABEL, TRAY_ID};

const WIDTH: f64 = 390.0;
const HEIGHT: f64 = 610.0;

pub fn open(app: &tauri::AppHandle) -> tauri::Result<()> {
    let state = app.state::<Arc<ResidentState>>();
    if !state.enabled() {
        return Ok(());
    }
    *state
        .panel_requested_at
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(std::time::Instant::now());
    log::info!(
        "resident_panel_open_requested ready={}",
        state.panel_ready.load(Ordering::Relaxed)
    );
    state.panel_open.store(true, Ordering::Relaxed);
    ensure_created(app)?;
    // Showing never waits for Vue, preference I/O, samples, or icon resolution.
    // Usually startup has already prepared the hidden WebView; an early click
    // still reveals its first frame while frontend initialization completes.
    show(app)?;
    state.wake();
    Ok(())
}

/// Prepare one hidden WebView without changing foreground or sampling intent.
/// Leave the native callback before creating it to keep WebView2 responsive.
pub fn prewarm(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if app.state::<Arc<ResidentState>>().enabled() {
            if let Err(error) = ensure_created(&app) {
                log::warn!("resident_panel_prewarm_failed error={error}");
            }
        }
    });
}

fn ensure_created(app: &tauri::AppHandle) -> tauri::Result<()> {
    let state = app.state::<Arc<ResidentState>>();
    // Startup preparation and an early click share one creation lock.
    let creation = state
        .panel_creation
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if app.get_webview_window(PANEL_LABEL).is_none() {
        let started = std::time::Instant::now();
        let builder =
            WebviewWindowBuilder::new(app, PANEL_LABEL, WebviewUrl::App("tray-panel.html".into()))
                .title("MangoDisk")
                .inner_size(WIDTH, HEIGHT)
                .resizable(false)
                .decorations(false)
                .visible(false)
                .focused(false)
                .skip_taskbar(true)
                .always_on_top(true)
                .shadow(true);
        // A titled macOS window keeps the system's rounded frame and shadow.
        // Overlay lets the panel content fill it without a separate title bar.
        #[cfg(target_os = "macos")]
        let builder = builder
            // The tray panel may appear while another application is active.
            // Deliver that first click to its controls instead of consuming it
            // only for activation while the OS completes the focus transition.
            .accept_first_mouse(true)
            .decorations(true)
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true);
        let window = builder.build()?;
        #[cfg(target_os = "macos")]
        window.with_webview(|webview| {
            use objc2_app_kit::{NSWindow, NSWindowButton};

            // Tauri runs this callback on the main thread and owns the NSWindow
            // throughout the callback. Only public AppKit APIs are used here.
            let native = unsafe { &*webview.ns_window().cast::<NSWindow>() };
            for kind in [
                NSWindowButton::CloseButton,
                NSWindowButton::MiniaturizeButton,
                NSWindowButton::ZoomButton,
            ] {
                if let Some(button) = native.standardWindowButton(kind) {
                    button.setHidden(true);
                }
            }
        })?;
        let app = app.clone();
        window.on_window_event(move |event| match event {
            WindowEvent::Focused(false) => {
                log::info!("resident_panel_focus_changed focused=false");
                // Windows activates the taskbar before delivering the tray click.
                // Preserve open intent until that click toggles it, otherwise the
                // same click would immediately reopen the panel we just hid.
                if tray_owns_focus(&app) {
                    log::info!("resident_panel_blur_deferred reason=tray_interaction");
                } else {
                    hide(&app);
                }
            }
            WindowEvent::Focused(true) => {
                log::info!("resident_panel_focus_changed focused=true");
                let state = app.state::<Arc<ResidentState>>();
                if let Some(started) = state
                    .panel_requested_at
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take()
                {
                    log::info!(
                        "resident_panel_focused elapsed_ms={}",
                        started.elapsed().as_millis()
                    );
                };
            }
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                hide(&app);
            }
            _ => {}
        });
        log::info!(
            "resident_panel_created elapsed_ms={}",
            started.elapsed().as_millis()
        );
    }
    drop(creation);
    Ok(())
}

#[cfg(windows)]
fn tray_owns_focus(app: &tauri::AppHandle) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetForegroundWindow, WindowFromPoint, GA_ROOT,
    };
    let Some(window) = app.get_webview_window(PANEL_LABEL) else {
        return false;
    };
    let Some(rect) = app
        .tray_by_id(TRAY_ID)
        .and_then(|tray| tray.rect().ok().flatten())
    else {
        return false;
    };
    let Ok(cursor) = window.cursor_position() else {
        return false;
    };
    let position = rect.position.to_physical::<f64>(1.0);
    let size = rect.size.to_physical::<f64>(1.0);
    if !contains(
        (cursor.x, cursor.y),
        (position.x, position.y, size.width, size.height),
    ) {
        return false;
    }
    // Cursor position alone is insufficient: Alt+Tab must still dismiss the
    // panel when the pointer happens to remain over the tray icon.
    unsafe {
        let surface = WindowFromPoint(windows_sys::Win32::Foundation::POINT {
            x: cursor.x as i32,
            y: cursor.y as i32,
        });
        !surface.is_null() && GetAncestor(surface, GA_ROOT) == GetForegroundWindow()
    }
}

#[cfg(not(windows))]
fn tray_owns_focus(_app: &tauri::AppHandle) -> bool {
    false
}

#[cfg(windows)]
pub fn tray_pointer_left(app: &tauri::AppHandle) {
    // Dismiss a deferred blur if a tray press was abandoned or a context menu
    // took focus. A focused panel stays open when the pointer enters it.
    if let Some(window) = app.get_webview_window(PANEL_LABEL) {
        if !window.is_focused().unwrap_or(false) {
            hide(app);
        }
    }
}

pub fn hide(app: &tauri::AppHandle) {
    let state = app.state::<Arc<ResidentState>>();
    state.panel_open.store(false, Ordering::Relaxed);
    *state
        .panel_requested_at
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = None;
    if let Some(window) = app.get_webview_window(PANEL_LABEL) {
        if let Err(error) = window.hide() {
            log::warn!("resident_panel_hide_failed error={error}");
        }
    }
    state.wake();
}

pub fn ready(app: &tauri::AppHandle) {
    let state = app.state::<Arc<ResidentState>>();
    state.panel_ready.store(true, Ordering::Relaxed);
    // A prewarmed page becoming ready must not reveal itself or steal focus.
    log::info!("resident_panel_ready");
}

fn show(app: &tauri::AppHandle) -> tauri::Result<()> {
    let state = app.state::<Arc<ResidentState>>();
    if !state.panel_open.load(Ordering::Relaxed) || !state.enabled() {
        return Ok(());
    }
    let Some(window) = app.get_webview_window(PANEL_LABEL) else {
        return Ok(());
    };
    let anchor = app
        .tray_by_id(TRAY_ID)
        .and_then(|tray| tray.rect().ok().flatten());
    let anchor = anchor.map(|rect| {
        let position = rect.position.to_physical::<f64>(1.0);
        let size = rect.size.to_physical::<f64>(1.0);
        (
            position.x + size.width / 2.0,
            position.y + size.height / 2.0,
        )
    });
    // Tray rectangles are physical pixels. On macOS monitor_from_point uses
    // logical screen points, so match against physical monitor bounds ourselves.
    let monitor = match anchor {
        Some(point) => app.available_monitors()?.into_iter().find(|monitor| {
            contains(
                point,
                (
                    monitor.position().x as f64,
                    monitor.position().y as f64,
                    monitor.size().width as f64,
                    monitor.size().height as f64,
                ),
            )
        }),
        None => None,
    };
    let monitor = match monitor {
        Some(monitor) => Some(monitor),
        None => app.primary_monitor()?,
    };
    if let Some(monitor) = monitor {
        let area = monitor.work_area();
        let scale = monitor.scale_factor();
        let width = (WIDTH * scale).min(area.size.width as f64);
        let height = (HEIGHT * scale).min(area.size.height as f64);
        window.set_size(tauri::LogicalSize::new(width / scale, height / scale))?;
        let work = (
            area.position.x as f64,
            area.position.y as f64,
            area.size.width as f64,
            area.size.height as f64,
        );
        let anchor = anchor.unwrap_or((work.0 + work.2 - width / 2.0, work.1));
        let (x, y) = position(anchor, work, (width, height), 6.0 * scale);
        window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))?;
        log::info!(
            "resident_panel_positioned scale={scale} x={x} y={y} width={width} height={height}"
        );
    }
    window.show()?;
    #[cfg(target_os = "macos")]
    window.with_webview(|webview| {
        use objc2_app_kit::NSWindow;

        // Activation restores the app's main/key windows. Select the panel first,
        // then let Tauri perform its native focus/activation sequence across macOS
        // versions; this prevents the product window from being brought forward.
        let native = unsafe { &*webview.ns_window().cast::<NSWindow>() };
        native.makeMainWindow();
        native.makeKeyAndOrderFront(None);
    })?;
    window.set_focus()?;
    if let Some(started) = state
        .panel_requested_at
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
    {
        log::info!(
            "resident_panel_show_dispatched elapsed_ms={}",
            started.elapsed().as_millis()
        );
    }
    Ok(())
}

fn contains(point: (f64, f64), bounds: (f64, f64, f64, f64)) -> bool {
    point.0 >= bounds.0
        && point.0 < bounds.0 + bounds.2
        && point.1 >= bounds.1
        && point.1 < bounds.1 + bounds.3
}

/// Use physical coordinates consistently across Retina/non-Retina displays and taskbar edges.
fn position(
    anchor: (f64, f64),
    area: (f64, f64, f64, f64),
    size: (f64, f64),
    gap: f64,
) -> (f64, f64) {
    let (left, top, width, height) = area;
    let x = (anchor.0 - size.0 / 2.0).clamp(left, left + (width - size.0).max(0.0));
    let y = if anchor.1 < top + height / 2.0 {
        top + gap
    } else {
        top + height - size.1 - gap
    };
    (x, y.clamp(top, top + (height - size.1).max(0.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_monitor_selection_handles_retina_edges_and_negative_origins() {
        assert!(contains((2248.0, 24.0), (0.0, 0.0, 3840.0, 2160.0)));
        assert!(!contains((3840.0, 24.0), (0.0, 0.0, 3840.0, 2160.0)));
        assert!(contains((-100.0, 24.0), (-1920.0, 0.0, 1920.0, 1080.0)));
        assert!(!contains((0.0, -1.0), (0.0, 0.0, 3840.0, 2160.0)));
    }

    #[test]
    fn panel_fits_top_bottom_and_negative_origin_monitors() {
        assert_eq!(
            position(
                (1900.0, 10.0),
                (0.0, 24.0, 1920.0, 1056.0),
                (390.0, 610.0),
                6.0
            ),
            (1530.0, 30.0)
        );
        assert_eq!(
            position(
                (20.0, 1080.0),
                (0.0, 0.0, 1920.0, 1040.0),
                (390.0, 610.0),
                6.0
            ),
            (0.0, 424.0)
        );
        assert_eq!(
            position(
                (-10.0, 0.0),
                (-1920.0, 24.0, 1920.0, 1056.0),
                (780.0, 900.0),
                12.0
            ),
            (-780.0, 36.0)
        );
        assert_eq!(
            position((0.0, 0.0), (0.0, 0.0, 300.0, 400.0), (300.0, 400.0), 6.0),
            (0.0, 0.0)
        );
    }
}
