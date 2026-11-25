use parking_lot::RwLock;
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
#[cfg(not(target_os = "windows"))]
use tauri::LogicalPosition;
#[cfg(target_os = "windows")]
use tauri::PhysicalPosition;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;

pub const TOOLBAR_WIDTH: f64 = 208.0;
pub const TOOLBAR_HEIGHT: f64 = 44.0;
pub const ACTION_WIDTH: f64 = 440.0;
pub const ACTION_HEIGHT: f64 = 380.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionPayload {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub app_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct ToolbarRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub visible: bool,
}

pub struct SelectionRuntime {
    pub app: AppHandle,
    pub enabled: RwLock<bool>,
    pub auto_popup: RwLock<bool>,
    pub blacklist: RwLock<Vec<String>>,
    pub last_selection: RwLock<Option<SelectionPayload>>,
    pub toolbar_rect: RwLock<ToolbarRect>,
    pub dismiss_epoch: AtomicU64,
}

static RUNTIME: OnceLock<SelectionRuntime> = OnceLock::new();

pub fn init(app: AppHandle, enabled: bool, auto_popup: bool, blacklist: Vec<String>) -> bool {
    let _ = RUNTIME.set(SelectionRuntime {
        app,
        enabled: RwLock::new(enabled),
        auto_popup: RwLock::new(auto_popup),
        blacklist: RwLock::new(
            blacklist
                .into_iter()
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect(),
        ),
        last_selection: RwLock::new(None),
        toolbar_rect: RwLock::new(ToolbarRect::default()),
        dismiss_epoch: AtomicU64::new(0),
    });

    set_enabled(enabled, auto_popup);

    #[cfg(target_os = "macos")]
    {
        return macos::start_monitor();
    }
    #[cfg(target_os = "windows")]
    {
        return windows::start_monitor();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

pub fn set_enabled(enabled: bool, auto_popup: bool) {
    if let Some(rt) = RUNTIME.get() {
        *rt.enabled.write() = enabled;
        *rt.auto_popup.write() = auto_popup;
        if !enabled {
            hide_toolbar(&rt.app);
        }
    }

    let monitor_active = enabled && auto_popup;
    #[cfg(target_os = "macos")]
    macos::set_enabled(monitor_active);
    #[cfg(target_os = "windows")]
    windows::set_enabled(monitor_active);
}

pub fn update_blacklist(list: Vec<String>) {
    if let Some(rt) = RUNTIME.get() {
        let normalized = list
            .into_iter()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        *rt.blacklist.write() = normalized;
    }
}

pub fn is_app_blacklisted(app_id: &str) -> bool {
    let lower = app_id.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }
    if let Some(rt) = RUNTIME.get() {
        let list = rt.blacklist.read();
        return list.iter().any(|entry| lower == *entry || lower.ends_with(entry));
    }
    false
}

pub fn get_last_selection() -> Option<SelectionPayload> {
    RUNTIME
        .get()
        .and_then(|rt| rt.last_selection.read().clone())
}

pub fn check_accessibility(prompt: bool) -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::is_accessibility_trusted(prompt)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = prompt;
        true
    }
}

pub fn open_accessibility_settings() {
    #[cfg(target_os = "macos")]
    macos::open_accessibility_settings();
}

pub fn reset_accessibility_permission() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::reset_accessibility_permission()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
static REOPEN_CALLBACK: OnceLock<fn(&AppHandle)> = OnceLock::new();

#[cfg(target_os = "macos")]
extern "C" fn on_reopen_trampoline() {
    if let (Some(rt), Some(cb)) = (RUNTIME.get(), REOPEN_CALLBACK.get()) {
        let app = rt.app.clone();
        let app_cb = app.clone();
        let cb = *cb;
        let _ = app.run_on_main_thread(move || {
            cb(&app_cb);
        });
    }
}

pub fn install_reopen_handler(on_reopen: fn(&AppHandle)) {
    #[cfg(target_os = "macos")]
    {
        let _ = REOPEN_CALLBACK.set(on_reopen);
        macos::install_reopen_handler(on_reopen_trampoline);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = on_reopen;
    }
}

pub fn ensure_monitor_started() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::start_monitor()
    }
    #[cfg(target_os = "windows")]
    {
        windows::start_monitor()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

pub fn configure_floating_window(window: &WebviewWindow, non_activating: bool) {
    #[cfg(target_os = "macos")]
    macos::configure_floating_panel(window, non_activating);

    #[cfg(target_os = "windows")]
    windows::configure_floating_panel(window, non_activating);

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (window, non_activating);
    }
}

pub fn activate_and_focus_window(window: &WebviewWindow, floating: bool) {
    #[cfg(target_os = "macos")]
    macos::activate_and_focus(window, floating);

    #[cfg(not(target_os = "macos"))]
    {
        let _ = floating;
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Called by platform hooks when a valid non-empty selection is detected.
/// Coordinates `(x, y)` are logical screen points on macOS and physical pixels on Windows.
pub fn handle_text_selected(text: String, x: f64, y: f64, app_id: String) {
    let Some(rt) = RUNTIME.get() else {
        return;
    };

    if !*rt.enabled.read() || !*rt.auto_popup.read() {
        return;
    }

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }

    let payload = SelectionPayload {
        text: trimmed.to_string(),
        x,
        y,
        app_id,
    };
    *rt.last_selection.write() = Some(payload.clone());

    let expected_epoch = rt.dismiss_epoch.load(Ordering::SeqCst);
    let app = rt.app.clone();
    let app_cb = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(rt) = RUNTIME.get() {
            if !*rt.enabled.read()
                || !*rt.auto_popup.read()
                || rt.dismiss_epoch.load(Ordering::SeqCst) != expected_epoch
            {
                return;
            }
        }
        show_toolbar_near(&app_cb, payload);
    });
}

/// Called by platform hooks on mouse click, scroll, or keypress to dismiss the toolbar
/// if the interaction happened outside the toolbar's screen bounds.
pub fn handle_dismiss_signal(x: f64, y: f64, is_click: bool) {
    let Some(rt) = RUNTIME.get() else {
        return;
    };

    let rect = rt.toolbar_rect.read().clone();
    if is_click && rect.visible {
        // Give a small 4px cushion around toolbar rect so clicking buttons inside never dismisses prematurely
        let inside = x >= rect.x - 4.0
            && x <= rect.x + rect.width + 4.0
            && y >= rect.y - 4.0
            && y <= rect.y + rect.height + 4.0;
        if inside {
            return;
        }
    }

    // Always advance dismiss_epoch so any in-flight main-thread `show_toolbar_near`
    // queued just before this click/scroll/keypress is discarded.
    rt.dismiss_epoch.fetch_add(1, Ordering::SeqCst);

    if !rect.visible {
        return;
    }

    let app = rt.app.clone();
    let app_cb = app.clone();
    let _ = app.run_on_main_thread(move || {
        hide_toolbar_window(&app_cb);
    });
}

fn hide_toolbar_window(app: &AppHandle) {
    if let Some(rt) = RUNTIME.get() {
        rt.toolbar_rect.write().visible = false;
    }
    if let Some(win) = app.get_webview_window("toolbar") {
        #[cfg(target_os = "windows")]
        {
            windows::hide_window(&win);
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = win.hide();
        }
    }
}

pub fn hide_toolbar(app: &AppHandle) {
    if let Some(rt) = RUNTIME.get() {
        rt.dismiss_epoch.fetch_add(1, Ordering::SeqCst);
    }
    hide_toolbar_window(app);
}

fn show_toolbar_near(app: &AppHandle, payload: SelectionPayload) {
    let Some(toolbar) = app.get_webview_window("toolbar") else {
        return;
    };

    let (pos_x, pos_y, hit_x, hit_y, hit_w, hit_h) = compute_toolbar_position(&toolbar, payload.x, payload.y);

    #[cfg(target_os = "macos")]
    {
        let _ = toolbar.set_position(LogicalPosition::new(pos_x, pos_y));
        macos::show_inactive(&toolbar);
    }
    #[cfg(target_os = "windows")]
    {
        let _ = toolbar.set_position(PhysicalPosition::new(pos_x as i32, pos_y as i32));
        windows::show_inactive(&toolbar);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = toolbar.set_position(LogicalPosition::new(pos_x, pos_y));
        let _ = toolbar.show();
    }

    if let Some(rt) = RUNTIME.get() {
        let mut rect = rt.toolbar_rect.write();
        rect.x = hit_x;
        rect.y = hit_y;
        rect.width = hit_w;
        rect.height = hit_h;
        rect.visible = true;
    }

    let _ = app.emit_to("toolbar", "selection:update", payload);
}

/// Computes clamped toolbar position above (or below if near top edge) the cursor.
/// Returns `(win_x, win_y, hit_x, hit_y, hit_w, hit_h)` in the platform's hook coordinate space.
fn compute_toolbar_position(
    toolbar: &WebviewWindow,
    cursor_x: f64,
    cursor_y: f64,
) -> (f64, f64, f64, f64, f64, f64) {
    let monitors = toolbar.available_monitors().unwrap_or_default();

    #[cfg(target_os = "macos")]
    {
        // On macOS, CGEventGetLocation returns logical points (top-left origin).
        let mut target_area: Option<(f64, f64, f64, f64)> = None;
        for m in monitors {
            let scale = m.scale_factor();
            let mx = m.position().x as f64 / scale;
            let my = m.position().y as f64 / scale;
            let mw = m.size().width as f64 / scale;
            let mh = m.size().height as f64 / scale;
            if cursor_x >= mx && cursor_x <= mx + mw && cursor_y >= my && cursor_y <= my + mh {
                target_area = Some((mx, my, mw, mh));
                break;
            }
        }
        let (mx, my, mw, mh) = target_area.unwrap_or((0.0, 28.0, 1440.0, 900.0));

        let mut x = cursor_x - TOOLBAR_WIDTH / 2.0;
        let mut y = cursor_y - TOOLBAR_HEIGHT - 12.0;

        // If too close to the top (menu bar), flip below cursor
        if y < my + 32.0 {
            y = cursor_y + 18.0;
        }
        x = x.clamp(mx + 8.0, mx + mw - TOOLBAR_WIDTH - 8.0);
        y = y.clamp(my + 8.0, my + mh - TOOLBAR_HEIGHT - 8.0);

        (x, y, x, y, TOOLBAR_WIDTH, TOOLBAR_HEIGHT)
    }

    #[cfg(target_os = "windows")]
    {
        // On Windows, WH_MOUSE_LL returns physical screen pixels.
        let mut target_area: Option<(f64, f64, f64, f64, f64)> = None;
        for m in monitors {
            let scale = m.scale_factor();
            let mx = m.position().x as f64;
            let my = m.position().y as f64;
            let mw = m.size().width as f64;
            let mh = m.size().height as f64;
            if cursor_x >= mx && cursor_x <= mx + mw && cursor_y >= my && cursor_y <= my + mh {
                target_area = Some((mx, my, mw, mh, scale));
                break;
            }
        }
        let (mx, my, mw, mh, scale) = target_area.unwrap_or((0.0, 0.0, 1920.0, 1080.0, 1.0));
        let w = TOOLBAR_WIDTH * scale;
        let h = TOOLBAR_HEIGHT * scale;

        let mut x = cursor_x - w / 2.0;
        let mut y = cursor_y - h - 14.0 * scale;
        if y < my + 8.0 * scale {
            y = cursor_y + 20.0 * scale;
        }
        x = x.clamp(mx + 8.0 * scale, mx + mw - w - 8.0 * scale);
        y = y.clamp(my + 8.0 * scale, my + mh - h - 8.0 * scale);

        (x, y, x, y, w, h)
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = monitors;
        let x = (cursor_x - TOOLBAR_WIDTH / 2.0).max(8.0);
        let y = (cursor_y - TOOLBAR_HEIGHT - 12.0).max(8.0);
        (x, y, x, y, TOOLBAR_WIDTH, TOOLBAR_HEIGHT)
    }
}

pub fn position_action_window_near_last_selection(action_win: &WebviewWindow) {
    let Some(sel) = get_last_selection() else {
        let _ = action_win.center();
        return;
    };

    let monitors = action_win.available_monitors().unwrap_or_default();

    #[cfg(target_os = "macos")]
    {
        let mut area = (0.0, 28.0, 1440.0, 900.0);
        for m in monitors {
            let scale = m.scale_factor();
            let mx = m.position().x as f64 / scale;
            let my = m.position().y as f64 / scale;
            let mw = m.size().width as f64 / scale;
            let mh = m.size().height as f64 / scale;
            if sel.x >= mx && sel.x <= mx + mw && sel.y >= my && sel.y <= my + mh {
                area = (mx, my, mw, mh);
                break;
            }
        }
        let (mx, my, mw, mh) = area;
        let mut x = sel.x - ACTION_WIDTH / 2.0;
        let mut y = sel.y + 14.0;
        if y + ACTION_HEIGHT > my + mh - 12.0 {
            y = sel.y - ACTION_HEIGHT - 14.0;
        }
        x = x.clamp(mx + 12.0, mx + mw - ACTION_WIDTH - 12.0);
        y = y.clamp(my + 32.0, my + mh - ACTION_HEIGHT - 12.0);
        let _ = action_win.set_position(LogicalPosition::new(x, y));
    }

    #[cfg(target_os = "windows")]
    {
        let mut area = (0.0, 0.0, 1920.0, 1080.0, 1.0);
        for m in monitors {
            let scale = m.scale_factor();
            let mx = m.position().x as f64;
            let my = m.position().y as f64;
            let mw = m.size().width as f64;
            let mh = m.size().height as f64;
            if sel.x >= mx && sel.x <= mx + mw && sel.y >= my && sel.y <= my + mh {
                area = (mx, my, mw, mh, scale);
                break;
            }
        }
        let (mx, my, mw, mh, scale) = area;
        let w = ACTION_WIDTH * scale;
        let h = ACTION_HEIGHT * scale;
        let mut x = sel.x - w / 2.0;
        let mut y = sel.y + 14.0 * scale;
        if y + h > my + mh - 12.0 * scale {
            y = sel.y - h - 14.0 * scale;
        }
        x = x.clamp(mx + 12.0 * scale, mx + mw - w - 12.0 * scale);
        y = y.clamp(my + 12.0 * scale, my + mh - h - 12.0 * scale);
        let _ = action_win.set_position(PhysicalPosition::new(x as i32, y as i32));
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = monitors;
        let _ = action_win.center();
    }
}
