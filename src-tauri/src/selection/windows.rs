use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::WebviewWindow;

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, MAX_PATH, WPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, OpenClipboard,
    SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    VK_CONTROL,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowLongPtrW,
    GetWindowThreadProcessId, SetWindowLongPtrW, SetWindowPos, SetWindowsHookExW, ShowWindow,
    TranslateMessage, GWL_EXSTYLE, HHOOK, HWND_TOPMOST, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNOACTIVATE, WH_KEYBOARD_LL, WH_MOUSE_LL,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

static ENABLED: AtomicBool = AtomicBool::new(true);
static STARTED: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU64 = AtomicU64::new(0);

// Hook thread state (only mutated inside low-level hook callback on the hook thread)
struct MouseGestureState {
    mouse_down: bool,
    down_x: i32,
    down_y: i32,
    down_at: Option<Instant>,
    last_up_x: i32,
    last_up_y: i32,
    last_up_at: Option<Instant>,
    click_streak: u32,
}

static mut GESTURE: MouseGestureState = MouseGestureState {
    mouse_down: false,
    down_x: 0,
    down_y: 0,
    down_at: None,
    last_up_x: 0,
    last_up_y: 0,
    last_up_at: None,
    click_streak: 0,
};

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn start_monitor() -> bool {
    if STARTED.swap(true, Ordering::SeqCst) {
        return true;
    }

    std::thread::Builder::new()
        .name("liteask-win-hook".to_string())
        .spawn(|| unsafe {
            let mouse_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), None, 0);
            let kb_hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0);

            if mouse_hook.is_err() && kb_hook.is_err() {
                STARTED.store(false, Ordering::SeqCst);
                return;
            }

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .is_ok()
}

pub fn configure_floating_panel(window: &WebviewWindow, non_activating: bool) {
    if let Ok(hwnd_raw) = window.hwnd() {
        let hwnd = HWND(hwnd_raw.0);
        unsafe {
            let mut ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            ex_style |= (WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0) as isize;
            if non_activating {
                ex_style |= WS_EX_NOACTIVATE.0 as isize;
            }
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style);
        }
    }
}

pub fn show_inactive(window: &WebviewWindow) {
    if let Ok(hwnd_raw) = window.hwnd() {
        let hwnd = HWND(hwnd_raw.0);
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    } else {
        let _ = window.show();
    }
}

unsafe extern "system" fn mouse_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && ENABLED.load(Ordering::Relaxed) {
        let info = *(lparam.0 as *const MSLLHOOKSTRUCT);
        let x = info.pt.x;
        let y = info.pt.y;
        let msg = wparam.0 as u32;

        match msg {
            WM_LBUTTONDOWN => {
                GESTURE.mouse_down = true;
                GESTURE.down_x = x;
                GESTURE.down_y = y;
                GESTURE.down_at = Some(Instant::now());
                GENERATION.fetch_add(1, Ordering::SeqCst);
                super::handle_dismiss_signal(x as f64, y as f64, true);
            }
            WM_LBUTTONUP => {
                if GESTURE.mouse_down {
                    GESTURE.mouse_down = false;
                    let now = Instant::now();
                    let dx = (x - GESTURE.down_x) as f64;
                    let dy = (y - GESTURE.down_y) as f64;
                    let drag_dist = (dx * dx + dy * dy).sqrt();

                    let is_multi_click = match GESTURE.last_up_at {
                        Some(prev) => {
                            let dt = now.duration_since(prev);
                            let pdx = (x - GESTURE.last_up_x) as f64;
                            let pdy = (y - GESTURE.last_up_y) as f64;
                            dt <= Duration::from_millis(500) && (pdx * pdx + pdy * pdy).sqrt() <= 8.0
                        }
                        None => false,
                    };

                    if is_multi_click {
                        GESTURE.click_streak += 1;
                    } else {
                        GESTURE.click_streak = 1;
                    }
                    GESTURE.last_up_at = Some(now);
                    GESTURE.last_up_x = x;
                    GESTURE.last_up_y = y;

                    let drag_ok = drag_dist >= 6.0
                        && GESTURE
                            .down_at
                            .map(|t| now.duration_since(t) < Duration::from_secs(10))
                            .unwrap_or(true);
                    let click_ok = GESTURE.click_streak >= 2 && drag_dist < 6.0;

                    if drag_ok || click_ok {
                        let gen = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
                        std::thread::spawn(move || {
                            extract_and_emit(x as f64, y as f64, gen);
                        });
                    }
                }
            }
            WM_RBUTTONDOWN | WM_MOUSEWHEEL => {
                GENERATION.fetch_add(1, Ordering::SeqCst);
                super::handle_dismiss_signal(x as f64, y as f64, false);
            }
            _ => {}
        }
    }
    CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam)
}

unsafe extern "system" fn keyboard_hook_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 && ENABLED.load(Ordering::Relaxed) && wparam.0 as u32 == WM_KEYDOWN {
        let kb = *(lparam.0 as *const KBDLLHOOKSTRUCT);
        // Ignore injected Ctrl+C events from our own SendInput fallback
        let is_injected = (kb.flags.0 & 0x10) != 0;
        if !is_injected {
            super::handle_dismiss_signal(0.0, 0.0, false);
        }
    }
    CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam)
}

fn extract_and_emit(x: f64, y: f64, expected_gen: u64) {
    std::thread::sleep(Duration::from_millis(60));
    if !ENABLED.load(Ordering::Relaxed) || GENERATION.load(Ordering::SeqCst) != expected_gen {
        return;
    }

    let (pid, exe_name) = get_foreground_process();
    if pid == unsafe { GetCurrentProcessId() } {
        return;
    }
    if super::is_app_blacklisted(&exe_name) {
        return;
    }

    // Stage 1: UI Automation (zero clipboard touch)
    let mut selected = get_selected_text_via_uia().unwrap_or_default();

    // Stage 2: Safe clipboard backup + Ctrl+C + restore fallback
    if selected.trim().is_empty() {
        selected = get_selected_text_via_clipboard(expected_gen).unwrap_or_default();
    }

    if GENERATION.load(Ordering::SeqCst) != expected_gen {
        return;
    }

    let trimmed = selected.trim();
    if trimmed.is_empty() {
        return;
    }

    let bounded: String = trimmed.chars().take(15000).collect();
    super::handle_text_selected(bounded, x, y, exe_name);
}

fn get_foreground_process() -> (u32, String) {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return (0, String::new());
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return (0, String::new());
        }

        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return (pid, String::new());
        };

        let mut buf = [0u16; MAX_PATH as usize];
        let mut size = buf.len() as u32;
        let name = if QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
        .is_ok()
        {
            let full = OsString::from_wide(&buf[..size as usize]);
            Path::new(&full)
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        } else {
            String::new()
        };

        let _ = CloseHandle(handle);
        (pid, name)
    }
}

fn get_selected_text_via_uia() -> Option<String> {
    unsafe {
        let com_ok = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let result = (|| -> Option<String> {
            let uia: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let elem = uia.GetFocusedElement().ok()?;
            let pattern: IUIAutomationTextPattern =
                elem.GetCurrentPatternAs(UIA_TextPatternId).ok()?;
            let ranges = pattern.GetSelection().ok()?;
            let count = ranges.Length().ok()?;
            if count <= 0 {
                return None;
            }
            let first = ranges.GetElement(0).ok()?;
            let text = first.GetText(15000).ok()?;
            let s = text.to_string();
            if s.trim().is_empty() {
                None
            } else {
                Some(s)
            }
        })();

        if com_ok {
            CoUninitialize();
        }
        result
    }
}

fn read_clipboard_unicode() -> Option<String> {
    unsafe {
        if OpenClipboard(None).is_err() {
            return None;
        }
        let result = (|| -> Option<String> {
            let handle = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
            let ptr = GlobalLock(windows::Win32::Foundation::HGLOBAL(handle.0)) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let mut len = 0usize;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(ptr, len);
            let s = String::from_utf16_lossy(slice);
            let _ = GlobalUnlock(windows::Win32::Foundation::HGLOBAL(handle.0));
            Some(s)
        })();
        let _ = CloseClipboard();
        result
    }
}

fn write_clipboard_unicode(text: &str) {
    unsafe {
        if OpenClipboard(None).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = utf16.len() * std::mem::size_of::<u16>();
        if let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, bytes) {
            let dst = GlobalLock(hmem) as *mut u16;
            if !dst.is_null() {
                std::ptr::copy_nonoverlapping(utf16.as_ptr(), dst, utf16.len());
                let _ = GlobalUnlock(hmem);
                let _ = SetClipboardData(
                    CF_UNICODETEXT.0 as u32,
                    Some(windows::Win32::Foundation::HANDLE(hmem.0)),
                );
            }
        }
        let _ = CloseClipboard();
    }
}

fn send_ctrl_c() {
    unsafe {
        let make_key = |vk: VIRTUAL_KEY, up: bool| -> INPUT {
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk,
                        wScan: 0,
                        dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() },
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            }
        };

        let inputs = [
            make_key(VK_CONTROL, false),
            make_key(VIRTUAL_KEY(0x43), false), // 'C'
            make_key(VIRTUAL_KEY(0x43), true),
            make_key(VK_CONTROL, true),
        ];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

fn get_selected_text_via_clipboard(expected_gen: u64) -> Option<String> {
    let backup = read_clipboard_unicode();
    let seq_before = unsafe { GetClipboardSequenceNumber() };

    send_ctrl_c();

    let mut changed = false;
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(15));
        if GENERATION.load(Ordering::SeqCst) != expected_gen {
            break;
        }
        if unsafe { GetClipboardSequenceNumber() } != seq_before {
            changed = true;
            break;
        }
    }

    if !changed {
        return None;
    }

    let copied = read_clipboard_unicode();

    if let Some(prev) = backup {
        write_clipboard_unicode(&prev);
    }

    copied
}
