use std::ffi::{c_char, c_void, CStr};
use tauri::WebviewWindow;

type SelectionCallback = extern "C" fn(*const c_char, f64, f64, *const c_char);
type DismissCallback = extern "C" fn(f64, f64, bool);
type FilterCallback = extern "C" fn(*const c_char) -> bool;

extern "C" {
    fn liteask_mac_is_accessibility_trusted(prompt: bool) -> bool;
    fn liteask_mac_open_accessibility_settings();
    fn liteask_mac_set_selection_enabled(enabled: bool);
    fn liteask_mac_configure_floating_panel(ns_window: *mut c_void, non_activating: bool);
    fn liteask_mac_show_inactive(ns_window: *mut c_void);
    fn liteask_mac_activate_and_focus(ns_window: *mut c_void);
    fn liteask_mac_start_selection_monitor(
        on_selection: SelectionCallback,
        on_dismiss: DismissCallback,
        is_blocked_app: FilterCallback,
    ) -> bool;
}

extern "C" fn on_selection_trampoline(
    text: *const c_char,
    x: f64,
    y: f64,
    app_id: *const c_char,
) {
    if text.is_null() {
        return;
    }
    let text_str = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    let app_str = if app_id.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(app_id) }
            .to_string_lossy()
            .into_owned()
    };

    super::handle_text_selected(text_str, x, y, app_str);
}

extern "C" fn on_dismiss_trampoline(x: f64, y: f64, is_click: bool) {
    super::handle_dismiss_signal(x, y, is_click);
}

extern "C" fn is_blocked_trampoline(app_id: *const c_char) -> bool {
    if app_id.is_null() {
        return false;
    }
    let app_str = unsafe { CStr::from_ptr(app_id) }.to_string_lossy();
    super::is_app_blacklisted(&app_str)
}

pub fn is_accessibility_trusted(prompt: bool) -> bool {
    unsafe { liteask_mac_is_accessibility_trusted(prompt) }
}

pub fn open_accessibility_settings() {
    unsafe { liteask_mac_open_accessibility_settings() }
}

pub fn set_enabled(enabled: bool) {
    unsafe { liteask_mac_set_selection_enabled(enabled) }
}

pub fn start_monitor() -> bool {
    unsafe {
        liteask_mac_start_selection_monitor(
            on_selection_trampoline,
            on_dismiss_trampoline,
            is_blocked_trampoline,
        )
    }
}

pub fn configure_floating_panel(window: &WebviewWindow, non_activating: bool) {
    if let Ok(ns_win) = window.ns_window() {
        unsafe { liteask_mac_configure_floating_panel(ns_win, non_activating) };
    }
}

pub fn show_inactive(window: &WebviewWindow) {
    if let Ok(ns_win) = window.ns_window() {
        unsafe { liteask_mac_show_inactive(ns_win) };
    } else {
        let _ = window.show();
    }
}

pub fn activate_and_focus(window: &WebviewWindow) {
    if let Ok(ns_win) = window.ns_window() {
        unsafe { liteask_mac_activate_and_focus(ns_win) };
    } else {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
