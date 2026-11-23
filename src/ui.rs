use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, DrawTextW,
    EndPaint, FillRect, InvalidateRect, SelectObject, SetBkMode, SetTextColor, SetWindowRgn,
    CLEARTYPE_QUALITY, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER, DT_SINGLELINE, DT_VCENTER,
    FF_DONTCARE, FW_SEMIBOLD, OUT_DEFAULT_PRECIS, PAINTSTRUCT, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW,
    FindWindowW, GetClientRect, GetCursorPos, GetMessageW, GetSystemMetrics, KillTimer, LoadIconW,
    LoadImageW, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW, SendMessageW,
    SetForegroundWindow, SetLayeredWindowAttributes, SetTimer, ShowWindow, TrackPopupMenu,
    TranslateMessage, ICON_BIG, ICON_SMALL, IDI_APPLICATION, IMAGE_ICON, LWA_ALPHA,
    MB_ICONINFORMATION, MB_OK, MF_STRING, MSG, SM_CXSCREEN, SW_HIDE, SW_SHOWNOACTIVATE,
    TPM_BOTTOMALIGN, TPM_LEFTALIGN, WM_COMMAND, WM_DESTROY, WM_PAINT, WM_RBUTTONUP, WM_SETICON,
    WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

const WM_APP_SHOW_HUD: u32 = 0x8001;
const WM_APP_TRAY: u32 = 0x8002;
const TIMER_HIDE_HUD: usize = 1;

const MENU_TOGGLE_CAMERA: usize = 1001;
const MENU_TOGGLE_TRACKING: usize = 1002;
const MENU_QUIT: usize = 1003;
const MENU_HELP: usize = 1004;

struct UiState {
    hud_text: Mutex<String>,
    hwnd: Mutex<usize>,
    is_running: Arc<AtomicBool>,
    tracking_enabled: Arc<AtomicBool>,
    show_camera: Arc<AtomicBool>,
}

static UI_STATE: OnceLock<UiState> = OnceLock::new();

#[derive(Clone)]
pub struct UiHandle;

impl UiHandle {
    pub fn show_hud(&self, text: &str) {
        if let Some(state) = UI_STATE.get() {
            if let Ok(mut guard) = state.hud_text.lock() {
                *guard = text.to_string();
            }
            if let Ok(hwnd_val) = state.hwnd.lock() {
                if *hwnd_val != 0 {
                    unsafe {
                        PostMessageW(*hwnd_val as HWND, WM_APP_SHOW_HUD, 0, 0);
                    }
                }
            }
        }
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn set_window_icon(window_title: &str) {
    let title_wide = to_wide(window_title);
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title_wide.as_ptr());
        if hwnd != 0 as _ {
            let hinstance = GetModuleHandleW(std::ptr::null());
            let icon_small = LoadImageW(hinstance, 1 as *const u16, IMAGE_ICON, 16, 16, 0);
            let icon_big = LoadImageW(hinstance, 1 as *const u16, IMAGE_ICON, 32, 32, 0);

            if icon_small != 0 as _ {
                SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, icon_small as isize);
            }
            if icon_big != 0 as _ {
                SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, icon_big as isize);
            }
        }
    }
}

pub fn start_ui(
    is_running: Arc<AtomicBool>,
    tracking_enabled: Arc<AtomicBool>,
    show_camera: Arc<AtomicBool>,
) -> UiHandle {
    let _ = UI_STATE.set(UiState {
        hud_text: Mutex::new(String::new()),
        hwnd: Mutex::new(0),
        is_running,
        tracking_enabled,
        show_camera,
    });

    thread::spawn(|| unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class_name = to_wide("JediMouseHudClass");

        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(wnd_proc);
        wc.hInstance = hinstance;
        wc.lpszClassName = class_name.as_ptr();
        RegisterClassW(&wc);

        let hud_w = 340;
        let hud_h = 46;
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let pos_x = (screen_w - hud_w) / 2;
        let pos_y = 28;

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name.as_ptr(),
            to_wide("Jedi Mouse HUD").as_ptr(),
            WS_POPUP,
            pos_x,
            pos_y,
            hud_w,
            hud_h,
            0 as _,
            0 as _,
            hinstance,
            std::ptr::null(),
        );

        if let Some(state) = UI_STATE.get() {
            if let Ok(mut guard) = state.hwnd.lock() {
                *guard = hwnd as usize;
            }
        }

        // Bentuk kapsul (Dynamic Island) & semi-transparan
        let rgn = CreateRoundRectRgn(0, 0, hud_w, hud_h, 24, 24);
        SetWindowRgn(hwnd, rgn, 1);
        SetLayeredWindowAttributes(hwnd, 0, 225, LWA_ALPHA);

        // Daftarkan ikon System Tray
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_APP_TRAY;

        // Muat ikon kustom dari resource ID 1 (16x16 untuk tray)
        let custom_icon = LoadImageW(
            hinstance,
            1 as *const u16,
            IMAGE_ICON,
            16,
            16,
            0,
        );
        let final_icon = if custom_icon != 0 as _ {
            custom_icon
        } else {
            LoadIconW(0 as _, IDI_APPLICATION)
        };
        nid.hIcon = final_icon;

        let tip = to_wide("Jedi Mouse (Klik kanan untuk menu)");
        for (i, &c) in tip.iter().take(nid.szTip.len() - 1).enumerate() {
            nid.szTip[i] = c;
        }
        Shell_NotifyIconW(NIM_ADD, &nid);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, 0 as _, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        Shell_NotifyIconW(NIM_DELETE, &nid);
    });

    UiHandle
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_SHOW_HUD => {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            InvalidateRect(hwnd, std::ptr::null(), 1);
            SetTimer(hwnd, TIMER_HIDE_HUD, 1800, None);
            0
        }
        WM_TIMER => {
            if wparam == TIMER_HIDE_HUD {
                KillTimer(hwnd, TIMER_HIDE_HUD);
                ShowWindow(hwnd, SW_HIDE);
            }
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect);

            let bg_brush = CreateSolidBrush(0x00222222);
            FillRect(hdc, &rect, bg_brush);
            DeleteObject(bg_brush as _);

            let font = CreateFontW(
                20,
                0,
                0,
                0,
                FW_SEMIBOLD as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                0,
                CLEARTYPE_QUALITY as u32,
                (DEFAULT_PITCH | FF_DONTCARE) as u32,
                to_wide("Segoe UI").as_ptr(),
            );
            let old_font = SelectObject(hdc, font as _);
            SetBkMode(hdc, TRANSPARENT as i32);
            SetTextColor(hdc, 0x00FFFFFF);

            let text = UI_STATE
                .get()
                .and_then(|s| s.hud_text.lock().ok().map(|g| g.clone()))
                .unwrap_or_default();
            let wide = to_wide(&text);
            DrawTextW(
                hdc,
                wide.as_ptr(),
                -1,
                &mut rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );

            SelectObject(hdc, old_font);
            DeleteObject(font as _);
            EndPaint(hwnd, &ps);
            0
        }
        WM_APP_TRAY => {
            if (lparam as u32) == WM_RBUTTONUP {
                if let Some(state) = UI_STATE.get() {
                    let hmenu = CreatePopupMenu();
                    let cam_label = if state.show_camera.load(Ordering::SeqCst) {
                        "Sembunyikan Kamera"
                    } else {
                        "Tampilkan Kamera"
                    };
                    let track_label = if state.tracking_enabled.load(Ordering::SeqCst) {
                        "Pause Hand Tracking"
                    } else {
                        "Aktifkan Hand Tracking"
                    };

                    AppendMenuW(hmenu, MF_STRING, MENU_TOGGLE_CAMERA, to_wide(cam_label).as_ptr());
                    AppendMenuW(hmenu, MF_STRING, MENU_TOGGLE_TRACKING, to_wide(track_label).as_ptr());
                    AppendMenuW(hmenu, MF_STRING, MENU_HELP, to_wide("Daftar Perintah").as_ptr());
                    AppendMenuW(hmenu, MF_STRING, MENU_QUIT, to_wide("Keluar").as_ptr());

                    let mut pt: POINT = std::mem::zeroed();
                    GetCursorPos(&mut pt);
                    SetForegroundWindow(hwnd);
                    TrackPopupMenu(
                        hmenu,
                        TPM_BOTTOMALIGN | TPM_LEFTALIGN,
                        pt.x,
                        pt.y,
                        0,
                        hwnd,
                        std::ptr::null(),
                    );
                    DestroyMenu(hmenu);
                }
            }
            0
        }
        WM_COMMAND => {
            let cmd = wparam & 0xFFFF;
            if let Some(state) = UI_STATE.get() {
                match cmd {
                    MENU_TOGGLE_CAMERA => {
                        let next = !state.show_camera.load(Ordering::SeqCst);
                        state.show_camera.store(next, Ordering::SeqCst);
                        UiHandle.show_hud(if next { "Kamera: TAMPIL" } else { "Kamera: TERSEMBUNYI" });
                    }
                    MENU_TOGGLE_TRACKING => {
                        let next = !state.tracking_enabled.load(Ordering::SeqCst);
                        state.tracking_enabled.store(next, Ordering::SeqCst);
                        UiHandle.show_hud(if next { "Tracking: AKTIF" } else { "Tracking: PAUSE" });
                    }
                    MENU_HELP => {
                        thread::spawn(|| unsafe {
                            // Tunggu sebentar hingga MessageBox muncul, lalu ganti ikonnya
                            thread::spawn(|| {
                                for _ in 0..20 {
                                    thread::sleep(std::time::Duration::from_millis(25));
                                    let title = to_wide("Daftar Perintah - Jedi Mouse");
                                    if FindWindowW(std::ptr::null(), title.as_ptr()) != 0 as _ {
                                        set_window_icon("Daftar Perintah - Jedi Mouse");
                                        break;
                                    }
                                }
                            });

                            let help_text = concat!(
                                "=== PERINTAH SUARA ===\n",
                                "• 'start tracking' / 'enable hand'  : Aktifkan tracking\n",
                                "• 'stop tracking' / 'disable hand'  : Pause tracking\n",
                                "• 'show camera' / 'hide camera'     : Tampil/Sembunyi kamera\n",
                                "• 'start typing' / 'stop typing'    : Mode dikte suara\n",
                                "• 'switch window' / 'next window'   : Alt + Tab\n",
                                "• 'desktop left' / 'desktop right'  : Pindah Virtual Desktop\n",
                                "• 'go back' / 'go forward'          : Navigasi Back/Forward\n",
                                "• 'close window' / 'minimize window': Tutup / Minimize jendela\n",
                                "• 'copy that' / 'paste that'        : Ctrl+C / Ctrl+V\n",
                                "• 'play pause' / 'escape'           : Media Play/Pause / Esc\n\n",
                                "=== GESTUR & SHORTCUT ===\n",
                                "• Cubit (Pinch) 1x      : Klik Kiri\n",
                                "• Cubit 2x cepat        : Klik Kanan\n",
                                "• Tahan cubit (>0.4 dtk): Drag & Drop\n",
                                "• 2 Tangan terbuka      : Scroll vertikal\n",
                                "• Ctrl + Alt + V        : Tampil/Sembunyi kamera\n",
                                "• Ctrl + Alt + Q        : Keluar aplikasi"
                            );
                            MessageBoxW(
                                0 as _,
                                to_wide(help_text).as_ptr(),
                                to_wide("Daftar Perintah - Jedi Mouse").as_ptr(),
                                MB_OK | MB_ICONINFORMATION,
                            );
                        });
                    }
                    MENU_QUIT => {
                        state.is_running.store(false, Ordering::SeqCst);
                        PostQuitMessage(0);
                    }
                    _ => {}
                }
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}