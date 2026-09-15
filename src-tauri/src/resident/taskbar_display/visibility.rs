//! Fullscreen classification is independent of the monitor strip's placement.
use super::layout::Bounds;
use std::time::{Duration, Instant};

/// Explorer may briefly cover a successfully positioned window while leaving
/// fullscreen. Keep the current display mode for at most three timer intervals;
/// native failures and already-failed placement must still fall back immediately.
#[derive(Default)]
pub struct OcclusionRetry {
    since: Option<Instant>,
}

impl OcclusionRetry {
    pub fn defer(&mut self, now: Instant, eligible: bool) -> bool {
        if !eligible {
            self.since = None;
            return false;
        }
        now.duration_since(*self.since.get_or_insert(now)) < Duration::from_millis(300)
    }

    pub fn finish(&mut self, now: Instant) -> Option<Duration> {
        self.since.take().map(|since| now.duration_since(since))
    }
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Visible,
    Disabled,
    GeometryUnavailable,
    AutoHidden,
    ShellMoving,
    Fullscreen,
    ShellFlyout,
    UnsupportedLayout,
    NoSpace,
    ShellUnavailable,
    PaintFailed,
    PositionFailed,
}

/// Browsers can retain WS_MAXIMIZE during frameless fullscreen, while WPS can
/// retain resize styles without being maximized. Accept the latter only when
/// its outer bounds exactly match the monitor: ordinary maximized frames can
/// extend beyond the screen when the taskbar auto-hides. Neither style nor
/// maximization alone identifies fullscreen, and covering only the strip is not
/// enough. Desktop hosts are excluded by the native caller before this check.
pub fn is_fullscreen(window: Bounds, monitor: Bounds, has_frame: bool, maximized: bool) -> bool {
    monitor.width() > 0
        && monitor.height() > 0
        && monitor.fits_in(window)
        && (!has_frame || (!maximized && window == monitor))
}

/// A shell menu on the other side of the taskbar must not hide the strip.
/// Yield for a real overlap, unavailable bounds, or a shell 1px placeholder.
/// Touching edges do not overlap. Recheck bounds even for the same menu handle.
pub fn should_yield_to_flyout(strip: Bounds, flyout: Option<Bounds>) -> bool {
    let Some(flyout) = flyout.filter(|r| {
        i64::from(r.right) - i64::from(r.left) > 1 && i64::from(r.bottom) - i64::from(r.top) > 1
    }) else {
        return true;
    };
    strip.left < flyout.right
        && strip.right > flyout.left
        && strip.top < flyout.bottom
        && strip.bottom > flyout.top
}

/// Explorer can focus WorkerW instead of the Progman handle returned by
/// GetShellWindow. Both desktop hosts are frameless and cover the monitor,
/// but neither represents application fullscreen. Check the shell process too:
/// a similarly named window from another application must not bypass detection.
pub fn is_shell_desktop(class: &str, process_id: u32, shell_process_id: u32) -> bool {
    process_id != 0 && process_id == shell_process_id && matches!(class, "WorkerW" | "Progman")
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRIP: Bounds = Bounds {
        left: 2040,
        top: 1784,
        right: 2192,
        bottom: 1856,
    };

    #[test]
    fn taskbar_app_menu_on_the_left_keeps_the_strip_visible() {
        assert!(!should_yield_to_flyout(
            STRIP,
            Some(Bounds {
                left: 384,
                top: 1562,
                right: 896,
                bottom: 1780,
            })
        ));
        // A menu may extend below the strip without overlapping horizontally.
        assert!(!should_yield_to_flyout(
            STRIP,
            Some(Bounds {
                left: 384,
                top: 1562,
                right: 896,
                bottom: 1930,
            })
        ));
    }

    #[test]
    fn flyout_overlap_including_a_single_pixel_yields_the_strip() {
        assert!(should_yield_to_flyout(STRIP, Some(STRIP)));
        assert!(should_yield_to_flyout(
            STRIP,
            Some(Bounds {
                left: 2191,
                top: 1855,
                right: 2300,
                bottom: 1900,
            })
        ));
        assert!(!should_yield_to_flyout(
            STRIP,
            Some(Bounds {
                left: 2192,
                top: 1855,
                right: 2300,
                bottom: 1900,
            })
        ));
        assert!(!should_yield_to_flyout(
            STRIP,
            Some(Bounds {
                bottom: STRIP.top,
                top: 1500,
                ..STRIP
            })
        ));
    }

    #[test]
    fn unavailable_or_empty_flyout_bounds_yield_until_measurable() {
        assert!(should_yield_to_flyout(STRIP, None));
        assert!(should_yield_to_flyout(STRIP, Some(Bounds::default())));
        // Notification Center may focus a 1x1 proxy outside its visible surface.
        assert!(should_yield_to_flyout(
            STRIP,
            Some(Bounds {
                left: 0,
                top: 0,
                right: 1,
                bottom: 1,
            })
        ));
    }

    #[test]
    fn both_explorer_desktop_hosts_are_excluded_from_fullscreen() {
        assert!(is_shell_desktop("WorkerW", 42, 42));
        assert!(is_shell_desktop("Progman", 42, 42));
    }

    #[test]
    fn other_windows_and_unknown_processes_are_not_desktop_hosts() {
        assert!(!is_shell_desktop("Chrome_WidgetWin_1", 42, 42));
        assert!(!is_shell_desktop("CabinetWClass", 42, 42));
        assert!(!is_shell_desktop("WorkerW", 7, 42));
        assert!(!is_shell_desktop("Progman", 0, 0));
    }

    #[test]
    fn continuous_occlusion_cannot_extend_the_retry_deadline() {
        let now = Instant::now();
        let mut retry = OcclusionRetry::default();
        assert!(retry.defer(now, true));
        assert!(retry.defer(now + Duration::from_millis(299), true));
        assert!(!retry.defer(now + Duration::from_millis(300), true));
        assert!(!retry.defer(now + Duration::from_secs(1), true));
    }

    #[test]
    fn native_failure_does_not_wait_for_an_occlusion_retry() {
        let now = Instant::now();
        let mut retry = OcclusionRetry::default();
        assert!(retry.defer(now, true));
        assert!(!retry.defer(now + Duration::from_millis(100), false));
        assert_eq!(retry.finish(now + Duration::from_millis(100)), None);
    }

    #[test]
    fn recovery_or_cancellation_starts_a_fresh_retry_window() {
        let now = Instant::now();
        let mut retry = OcclusionRetry::default();
        assert!(retry.defer(now, true));
        assert_eq!(
            retry.finish(now + Duration::from_millis(100)),
            Some(Duration::from_millis(100))
        );
        assert_eq!(retry.finish(now + Duration::from_millis(100)), None);
        assert!(retry.defer(now + Duration::from_secs(2), true));
    }

    const MONITOR: Bounds = Bounds {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };

    #[test]
    fn fullscreen_does_not_depend_on_previous_maximized_state() {
        assert!(is_fullscreen(MONITOR, MONITOR, false, true));
        assert!(is_fullscreen(MONITOR, MONITOR, false, false));
        // Restoring the browser frame must restore the strip even if Explorer's
        // work-area geometry has not yet caught up with the fullscreen transition.
        assert!(!is_fullscreen(MONITOR, MONITOR, true, true));
    }

    #[test]
    fn fullscreen_can_retain_resize_style_without_being_maximized() {
        // WPS keeps WS_THICKFRAME while sizing its non-maximized fullscreen
        // window exactly to the monitor. Style presence alone must not reject it.
        assert!(is_fullscreen(MONITOR, MONITOR, true, false));
        assert!(!is_fullscreen(MONITOR, MONITOR, true, true));
        assert!(is_fullscreen(MONITOR, MONITOR, false, true));
        assert!(!is_fullscreen(
            Bounds {
                right: 1919,
                ..MONITOR
            },
            MONITOR,
            true,
            false
        ));
        assert!(!is_fullscreen(
            Bounds {
                left: -8,
                right: 1928,
                ..MONITOR
            },
            MONITOR,
            true,
            false
        ));
    }

    #[test]
    fn maximized_auto_hide_window_is_not_fullscreen() {
        let window = Bounds {
            left: -8,
            top: -8,
            right: 1928,
            bottom: 1088,
        };
        assert!(!is_fullscreen(window, MONITOR, true, true));
    }

    #[test]
    fn covering_only_the_taskbar_does_not_hide_the_strip() {
        let window = Bounds {
            top: 500,
            ..MONITOR
        };
        assert!(!is_fullscreen(window, MONITOR, false, false));
    }

    #[test]
    fn fullscreen_on_another_monitor_does_not_hide_the_strip() {
        let secondary = Bounds {
            left: -1920,
            right: 0,
            ..MONITOR
        };
        assert!(!is_fullscreen(secondary, MONITOR, false, false));
        assert!(is_fullscreen(secondary, secondary, false, false));
        assert!(!is_fullscreen(MONITOR, Bounds::default(), false, false));
    }
}
