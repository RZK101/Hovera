use std::ffi::c_void;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, SetPriorityClass, HIGH_PRIORITY_CLASS,
};
use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    keybd_event, mouse_event, GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD,
    KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, VK_A,
    VK_BACK, VK_C, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_F4, VK_LEFT, VK_LWIN, VK_MEDIA_PLAY_PAUSE,
    VK_MENU, VK_RETURN, VK_RIGHT, VK_S, VK_SHIFT, VK_TAB, VK_V, VK_VOLUME_DOWN, VK_VOLUME_MUTE,
    VK_VOLUME_UP, VK_Y, VK_Z,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SetCursorPos, SetWindowPos, HWND_TOPMOST, SM_CXSCREEN, SM_CYSCREEN,
    SWP_NOMOVE, SWP_NOSIZE,
};

fn tap_combo(keys: &[u16]) {
    unsafe {
        for &vk in keys {
            let flags = if vk == VK_LWIN || vk == VK_LEFT || vk == VK_RIGHT || vk == VK_DOWN {
                KEYEVENTF_EXTENDEDKEY
            } else {
                0
            };
            keybd_event(vk as u8, 0, flags, 0);
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
        for &vk in keys.iter().rev() {
            let flags = if vk == VK_LWIN || vk == VK_LEFT || vk == VK_RIGHT || vk == VK_DOWN {
                KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP
            } else {
                KEYEVENTF_KEYUP
            };
            keybd_event(vk as u8, 0, flags, 0);
        }
    }
}

/// Mengetik teks Unicode langsung ke jendela yang sedang aktif
pub fn type_text(text: &str) {
    let mut inputs: Vec<INPUT> = Vec::new();
    for code_unit in text.encode_utf16() {
        let down = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: code_unit,
                    dwFlags: KEYEVENTF_UNICODE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let up = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: code_unit,
                    dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        inputs.push(down);
        inputs.push(up);
    }

    if !inputs.is_empty() {
        unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            );
        }
    }
}

// --- PERINTAH EDIT TEKS (MODE DIKTE) ---
pub fn press_enter() {
    tap_combo(&[VK_RETURN]);
}

pub fn press_tab() {
    tap_combo(&[VK_TAB]);
}

pub fn press_backspace() {
    tap_combo(&[VK_BACK]);
}

/// Ctrl + Backspace (Hapus 1 kata terakhir)
pub fn delete_word() {
    tap_combo(&[VK_CONTROL, VK_BACK]);
}

/// Ctrl + A (Pilih semua teks)
pub fn select_all() {
    tap_combo(&[VK_CONTROL, VK_A]);
}

/// Ctrl + Z (Undo)
pub fn undo_action() {
    tap_combo(&[VK_CONTROL, VK_Z]);
}

/// Ctrl + Y (Redo)
pub fn redo_action() {
    tap_combo(&[VK_CONTROL, VK_Y]);
}

// --- KONTROL VOLUME (AIR-KNOB & VOICE) ---
pub fn volume_up() {
    tap_combo(&[VK_VOLUME_UP]);
}

pub fn volume_down() {
    tap_combo(&[VK_VOLUME_DOWN]);
}

pub fn volume_mute() {
    tap_combo(&[VK_VOLUME_MUTE]);
}

// --- SHORTCUT RADIAL MENU ---
/// Win + Shift + S (Snipping Tool / Screenshot)
pub fn take_screenshot() {
    tap_combo(&[VK_LWIN, VK_SHIFT, VK_S]);
}

/// Win + V (Clipboard History)
pub fn open_clipboard_history() {
    tap_combo(&[VK_LWIN, VK_V]);
}

/// Win + Tab (Task View)
pub fn open_task_view() {
    tap_combo(&[VK_LWIN, VK_TAB]);
}

/// Win + D (Show Desktop)
pub fn show_desktop() {
    tap_combo(&[VK_LWIN, 0x44]); // VK_D
}

pub fn mute_volume() {
    tap_combo(&[VK_VOLUME_MUTE]);
}

/// Win + . (Emoji Panel)
pub fn emoji_panel() {
    tap_combo(&[VK_LWIN, 0xBE]); // VK_OEM_PERIOD
}

// --- NAVIGASI & SISTEM ---
pub fn navigate_back() {
    tap_combo(&[VK_MENU, VK_LEFT]);
}

pub fn navigate_forward() {
    tap_combo(&[VK_MENU, VK_RIGHT]);
}

pub fn switch_window() {
    tap_combo(&[VK_MENU, VK_TAB]);
}

pub fn switch_desktop_right() {
    tap_combo(&[VK_LWIN, VK_CONTROL, VK_RIGHT]);
}

pub fn switch_desktop_left() {
    tap_combo(&[VK_LWIN, VK_CONTROL, VK_LEFT]);
}

pub fn press_escape() {
    tap_combo(&[VK_ESCAPE]);
}

pub fn close_window() {
    tap_combo(&[VK_MENU, VK_F4]);
}

pub fn minimize_window() {
    tap_combo(&[VK_LWIN, VK_DOWN]);
}

pub fn copy_selection() {
    tap_combo(&[VK_CONTROL, VK_C]);
}

pub fn paste_selection() {
    tap_combo(&[VK_CONTROL, VK_V]);
}

pub fn media_play_pause() {
    tap_combo(&[VK_MEDIA_PLAY_PAUSE]);
}

pub fn enable_dpi_awareness() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

pub fn enable_high_res_timer() {
    unsafe {
        timeBeginPeriod(1);
    }
}

pub fn disable_high_res_timer() {
    unsafe {
        timeEndPeriod(1);
    }
}

pub fn set_high_priority() {
    unsafe {
        let handle = GetCurrentProcess();
        SetPriorityClass(handle, HIGH_PRIORITY_CLASS);
    }
}

pub fn get_screen_size() -> (f32, f32) {
    unsafe {
        let w = GetSystemMetrics(SM_CXSCREEN) as f32;
        let h = GetSystemMetrics(SM_CYSCREEN) as f32;
        (w, h)
    }
}

pub fn move_mouse(x: f32, y: f32) {
    unsafe {
        SetCursorPos(x.round() as i32, y.round() as i32);
    }
}

pub fn mouse_down() {
    unsafe {
        mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
    }
}

pub fn mouse_up() {
    unsafe {
        mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
    }
}

pub fn left_click() {
    unsafe {
        mouse_event(MOUSEEVENTF_LEFTDOWN | MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
    }
}

pub fn right_click() {
    unsafe {
        mouse_event(MOUSEEVENTF_RIGHTDOWN | MOUSEEVENTF_RIGHTUP, 0, 0, 0, 0);
    }
}

pub fn scroll_vertical(amount: i32) {
    unsafe {
        mouse_event(MOUSEEVENTF_WHEEL, 0, 0, amount as _, 0);
    }
}

pub fn set_window_topmost(raw_hwnd: *mut c_void) {
    unsafe {
        let hwnd = raw_hwnd as HWND;
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE,
        );
    }
}

fn is_key_down(vkey: i32) -> bool {
    unsafe { (GetAsyncKeyState(vkey) as u16 & 0x8000) != 0 }
}

pub struct HotkeyWatcher {
    prev_v: bool,
    prev_q: bool,
}

impl HotkeyWatcher {
    pub fn new() -> Self {
        Self {
            prev_v: false,
            prev_q: false,
        }
    }

    pub fn poll(&mut self) -> (bool, bool) {
        let ctrl = is_key_down(VK_CONTROL as i32);
        let alt = is_key_down(VK_MENU as i32);
        let v_down = ctrl && alt && is_key_down('V' as i32);
        let q_down = ctrl && alt && is_key_down('Q' as i32);

        let toggle_v = v_down && !self.prev_v;
        let trigger_q = q_down && !self.prev_q;

        self.prev_v = v_down;
        self.prev_q = q_down;

        (toggle_v, trigger_q)
    }
}