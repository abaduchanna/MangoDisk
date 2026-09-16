//! Child hosting and DPI conversion. Windows 10 space reservation is owned by
//! the separate companion lease, not by these window-parenting primitives.
use std::ptr;
use windows_sys::{
    core::w,
    Win32::{
        Foundation::*,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
};

pub unsafe fn parent(shell: HWND) -> HWND {
    // Windows 10's rebar shares clipping/order with the task buttons. Windows
    // 11 may retain a compatibility rebar limited to the centered icon area;
    // host under Shell_TrayWnd so its unused sides remain available.
    if super::position::read_environment() != super::position::Environment::Windows10 {
        return shell;
    }
    // Explorer creates the rebar after its top-level taskbar. Wait for that
    // host instead of permanently attaching to a transient startup hierarchy.
    FindWindowExW(shell, ptr::null_mut(), w!("ReBarWindow32"), ptr::null())
}

pub struct DpiContext(DPI_AWARENESS_CONTEXT);

impl DpiContext {
    pub unsafe fn enter(parent: HWND) -> Result<Self, (&'static str, u32)> {
        let context = GetWindowDpiAwarenessContext(parent);
        // All placement/model rectangles are physical pixels. Do not silently
        // embed in a DPI-virtualized replacement taskbar or reset process DPI.
        if GetAwarenessFromDpiAwarenessContext(context) != DPI_AWARENESS_PER_MONITOR_AWARE {
            return Err((
                "parent_dpi",
                GetAwarenessFromDpiAwarenessContext(context) as u32,
            ));
        }
        let previous = SetThreadDpiAwarenessContext(context);
        if previous.is_null() {
            Err(("thread_dpi", GetLastError()))
        } else {
            Ok(Self(previous))
        }
    }
}

impl Drop for DpiContext {
    fn drop(&mut self) {
        // Only the native hosting thread changes context. Tauri/WebView threads
        // retain their original DPI awareness throughout window creation.
        unsafe {
            SetThreadDpiAwarenessContext(self.0);
        }
    }
}

/// Keep opaque GDI painting composed above Explorer too. Switching from
/// SetLayeredWindowAttributes to UpdateLayeredWindow requires clearing the bit.
pub unsafe fn background(hwnd: HWND, enabled: bool) -> Result<(), u32> {
    let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    for value in [
        style & !(WS_EX_LAYERED as isize),
        style | WS_EX_LAYERED as isize,
    ] {
        SetLastError(0);
        if SetWindowLongPtrW(hwnd, GWL_EXSTYLE, value) == 0 && GetLastError() != 0 {
            return Err(GetLastError());
        }
    }
    if enabled && SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA) == 0 {
        return Err(GetLastError());
    }
    Ok(())
}

/// A rebar may be narrower than the taskbar. Never place pixels outside its
/// client area: the OS would clip them even if SetWindowPos reports success.
pub unsafe fn client_bounds(parent: HWND) -> Option<super::layout::Bounds> {
    let mut rect = RECT::default();
    let mut origin = POINT::default();
    if GetClientRect(parent, &mut rect) == 0
        || windows_sys::Win32::Graphics::Gdi::ClientToScreen(parent, &mut origin) == 0
    {
        return None;
    }
    Some(super::layout::Bounds {
        left: origin.x + rect.left,
        top: origin.y + rect.top,
        right: origin.x + rect.right,
        bottom: origin.y + rect.bottom,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_hosting_preserves_parent_geometry_and_thread_dpi() {
        unsafe {
            // Exercise real Win32 state in hidden windows owned by this test;
            // never touch Explorer or depend on an interactive desktop. Layered
            // presentation is exercised using the manifest-bearing application
            // because Cargo library test executables do not embed that resource.
            let original = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            assert!(!original.is_null());
            let restore = DpiContext(original);
            let parent = CreateWindowExW(
                0,
                w!("STATIC"),
                ptr::null(),
                WS_POPUP,
                80,
                120,
                500,
                80,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            );
            assert!(!parent.is_null());
            let before = client_bounds(parent).unwrap();
            let entered = DpiContext::enter(parent).unwrap();
            let child = CreateWindowExW(
                WS_EX_NOACTIVATE,
                w!("STATIC"),
                ptr::null(),
                WS_CHILD | WS_CLIPSIBLINGS,
                4,
                4,
                100,
                40,
                parent,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            );
            assert!(
                !child.is_null(),
                "child creation failed: {}",
                GetLastError()
            );
            drop(entered);
            assert_ne!(
                AreDpiAwarenessContextsEqual(
                    GetThreadDpiAwarenessContext(),
                    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
                ),
                0
            );
            assert_eq!(GetParent(child), parent);
            assert_eq!(
                GetWindowLongPtrW(child, GWL_EXSTYLE) & WS_EX_TOPMOST as isize,
                0
            );
            assert_eq!(client_bounds(parent), Some(before));
            DestroyWindow(child);
            assert_eq!(
                client_bounds(parent),
                Some(before),
                "removal must not leave reserved space"
            );
            DestroyWindow(parent);
            drop(restore);
        }
    }
}
