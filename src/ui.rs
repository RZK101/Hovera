use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreatePen, CreateRoundRectRgn, CreateSolidBrush, DeleteObject,
    DrawTextW, Ellipse, EndPaint, FillRect, InvalidateRect, LineTo, MoveToEx, SelectObject,
    SetBkMode, SetTextColor, SetWindowRgn, CLEARTYPE_QUALITY, DEFAULT_CHARSET, DEFAULT_PITCH,
    DT_CENTER, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_BOLD, FW_SEMIBOLD, OUT_DEFAULT_PRECIS,
    PAINTSTRUCT, PS_SOLID, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW,
    FindWindowW, GetClientRect, GetCursorPos, GetMessageW, GetSystemMetrics, KillTimer, LoadIconW,
    LoadImageW, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW, SendMessageW,
    SetForegroundWindow, SetLayeredWindowAttributes, SetTimer, SetWindowPos, ShowWindow,
    TrackPopupMenu, TranslateMessage, HWND_TOPMOST, ICON_BIG, ICON_SMALL, IDI_APPLICATION,
    IMAGE_ICON, LWA_ALPHA, LWA_COLORKEY, MB_ICONINFORMATION, MB_OK, MF_STRING, MSG, SM_CXSCREEN,
    SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, TPM_BOTTOMALIGN,
    TPM_LEFTALIGN, WM_COMMAND, WM_DESTROY, WM_PAINT, WM_RBUTTONUP, WM_SETICON, WM_TIMER, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const WM_APP_SHOW_HUD: u32 = 0x8001;
const WM_APP_TRAY: u32 = 0x8002;
const WM_APP_RADIAL_UPDATE: u32 = 0x8003;
const WM_APP_DRAW_UPDATE: u32 = 0x8004;
const TIMER_HIDE_HUD: usize = 1;

const MENU_TOGGLE_CAMERA: usize = 1001;
const MENU_TOGGLE_TRACKING: usize = 1002;
const MENU_TOGGLE_DRAW: usize = 1005;
const MENU_CLEAR_DRAW: usize = 1006;
const MENU_QUIT: usize = 1003;
const MENU_HELP: usize = 1004;

const RADIAL_SIZE: i32 = 260;
const COLOR_KEY_TRANS: u32 = 0x00010101;

#[derive(Clone, Copy)]
struct RadialData {
    visible: bool,
    cx: i32,
    cy: i32,
    selected: Option<usize>, // 0: Atas, 1: Kanan, 2: Bawah, 3: Kiri
}

struct UiState {
    hud_text: Mutex<String>,
    hwnd_hud: Mutex<usize>,
    hwnd_radial: Mutex<usize>,
    hwnd_draw: Mutex<usize>,
    radial: Mutex<RadialData>,
    strokes: Mutex<Vec<Vec<(i32, i32)>>>,
    is_running: Arc<AtomicBool>,
    tracking_enabled: Arc<AtomicBool>,
    show_camera: Arc<AtomicBool>,
    draw_mode: Arc<AtomicBool>,
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
            if let Ok(hwnd_val) = state.hwnd_hud.lock() {
                if *hwnd_val != 0 {
                    unsafe {
                        PostMessageW(*hwnd_val as HWND, WM_APP_SHOW_HUD, 0, 0);
                    }
                }
            }
        }
    }

    pub fn update_radial(&self, visible: bool, cx: i32, cy: i32, selected: Option<usize>) {
        if let Some(state) = UI_STATE.get() {
            if let Ok(mut guard) = state.radial.lock() {
                guard.visible = visible;
                guard.cx = cx;
                guard.cy = cy;
                guard.selected = selected;
            }
            if let Ok(hwnd_val) = state.hwnd_radial.lock() {
                if *hwnd_val != 0 {
                    unsafe {
                        PostMessageW(*hwnd_val as HWND, WM_APP_RADIAL_UPDATE, 0, 0);
                    }
                }
            }
        }
    }

    pub fn draw_start_stroke(&self, x: i32, y: i32) {
        if let Some(state) = UI_STATE.get() {
            if let Ok(mut strokes) = state.strokes.lock() {
                strokes.push(vec![(x, y)]);
            }
            self.trigger_draw_refresh();
        }
    }

    pub fn draw_add_point(&self, x: i32, y: i32) {
        if let Some(state) = UI_STATE.get() {
            if let Ok(mut strokes) = state.strokes.lock() {
                if let Some(last) = strokes.last_mut() {
                    if let Some(&(px, py)) = last.last() {
                        if (x - px).abs() + (y - py).abs() >= 3 {
                            last.push((x, y));
                        }
                    } else {
                        last.push((x, y));
                    }
                } else {
                    strokes.push(vec![(x, y)]);
                }
            }
            self.trigger_draw_refresh();
        }
    }

    pub fn draw_clear(&self) {
        if let Some(state) = UI_STATE.get() {
            if let Ok(mut strokes) = state.strokes.lock() {
                strokes.clear();
            }
            self.trigger_draw_refresh();
        }
    }

    fn trigger_draw_refresh(&self) {
        if let Some(state) = UI_STATE.get() {
            if let Ok(hwnd_val) = state.hwnd_draw.lock() {
                if *hwnd_val != 0 {
                    unsafe {
                        PostMessageW(*hwnd_val as HWND, WM_APP_DRAW_UPDATE, 0, 0);
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
    draw_mode: Arc<AtomicBool>,
) -> UiHandle {
    let _ = UI_STATE.set(UiState {
        hud_text: Mutex::new(String::new()),
        hwnd_hud: Mutex::new(0),
        hwnd_radial: Mutex::new(0),
        hwnd_draw: Mutex::new(0),
        radial: Mutex::new(RadialData {
            visible: false,
            cx: 0,
            cy: 0,
            selected: None,
        }),
        strokes: Mutex::new(Vec::new()),
        is_running,
        tracking_enabled,
        show_camera,
        draw_mode,
    });

    thread::spawn(|| unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);

        // 1. Jendela HUD + Tray
        let hud_class = to_wide("JediMouseHudClass");
        let mut wc_hud: WNDCLASSW = std::mem::zeroed();
        wc_hud.lpfnWndProc = Some(wnd_proc_hud);
        wc_hud.hInstance = hinstance;
        wc_hud.lpszClassName = hud_class.as_ptr();
        RegisterClassW(&wc_hud);

        let hud_w = 360;
        let hud_h = 46;
        let hwnd_hud = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            hud_class.as_ptr(),
            to_wide("Jedi Mouse HUD").as_ptr(),
            WS_POPUP,
            (screen_w - hud_w) / 2,
            28,
            hud_w,
            hud_h,
            0 as _,
            0 as _,
            hinstance,
            std::ptr::null(),
        );

        let rgn = CreateRoundRectRgn(0, 0, hud_w, hud_h, 24, 24);
        SetWindowRgn(hwnd_hud, rgn, 1);
        SetLayeredWindowAttributes(hwnd_hud, 0, 225, LWA_ALPHA);

        // 2. Jendela Radial Menu
        let radial_class = to_wide("JediMouseRadialClass");
        let mut wc_radial: WNDCLASSW = std::mem::zeroed();
        wc_radial.lpfnWndProc = Some(wnd_proc_radial);
        wc_radial.hInstance = hinstance;
        wc_radial.lpszClassName = radial_class.as_ptr();
        RegisterClassW(&wc_radial);

        let hwnd_radial = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            radial_class.as_ptr(),
            to_wide("Jedi Radial Menu").as_ptr(),
            WS_POPUP,
            (screen_w - RADIAL_SIZE) / 2,
            (screen_h - RADIAL_SIZE) / 2,
            RADIAL_SIZE,
            RADIAL_SIZE,
            0 as _,
            0 as _,
            hinstance,
            std::ptr::null(),
        );
        SetLayeredWindowAttributes(hwnd_radial, COLOR_KEY_TRANS, 235, LWA_COLORKEY | LWA_ALPHA);

        // 3. Jendela Air Annotation (Full Screen Transparent Overlay)
        let draw_class = to_wide("JediMouseDrawClass");
        let mut wc_draw: WNDCLASSW = std::mem::zeroed();
        wc_draw.lpfnWndProc = Some(wnd_proc_draw);
        wc_draw.hInstance = hinstance;
        wc_draw.lpszClassName = draw_class.as_ptr();
        RegisterClassW(&wc_draw);

        let hwnd_draw = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            draw_class.as_ptr(),
            to_wide("Jedi Air Annotation").as_ptr(),
            WS_POPUP,
            0,
            0,
            screen_w,
            screen_h,
            0 as _,
            0 as _,
            hinstance,
            std::ptr::null(),
        );
        SetLayeredWindowAttributes(hwnd_draw, COLOR_KEY_TRANS, 255, LWA_COLORKEY);

        if let Some(state) = UI_STATE.get() {
            if let Ok(mut g) = state.hwnd_hud.lock() {
                *g = hwnd_hud as usize;
            }
            if let Ok(mut g) = state.hwnd_radial.lock() {
                *g = hwnd_radial as usize;
            }
            if let Ok(mut g) = state.hwnd_draw.lock() {
                *g = hwnd_draw as usize;
            }
        }

        // Daftarkan ikon System Tray
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd_hud;
        nid.uID = 1;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_APP_TRAY;

        let custom_icon = LoadImageW(hinstance, 1 as *const u16, IMAGE_ICON, 16, 16, 0);
        nid.hIcon = if custom_icon != 0 as _ {
            custom_icon
        } else {
            LoadIconW(0 as _, IDI_APPLICATION)
        };

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

unsafe extern "system" fn wnd_proc_radial(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_RADIAL_UPDATE => {
            if let Some(state) = UI_STATE.get() {
                if let Ok(r) = state.radial.lock() {
                    if r.visible {
                        let x = r.cx - RADIAL_SIZE / 2;
                        let y = r.cy - RADIAL_SIZE / 2;
                        SetWindowPos(
                            hwnd,
                            HWND_TOPMOST,
                            x,
                            y,
                            0,
                            0,
                            SWP_NOSIZE | SWP_NOACTIVATE,
                        );
                        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                        InvalidateRect(hwnd, std::ptr::null(), 1);
                    } else {
                        ShowWindow(hwnd, SW_HIDE);
                    }
                }
            }
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect);

            // Bersihkan latar dengan warna transparan (ColorKey)
            let key_brush = CreateSolidBrush(COLOR_KEY_TRANS);
            FillRect(hdc, &rect, key_brush);
            DeleteObject(key_brush as _);

            let selected = UI_STATE
                .get()
                .and_then(|s| s.radial.lock().ok().and_then(|r| r.selected));

            // Lingkaran luar gelap
            let bg_brush = CreateSolidBrush(0x00242424);
            let border_pen = CreatePen(PS_SOLID as i32, 3, 0x00FFAA00);
            let old_brush = SelectObject(hdc, bg_brush as _);
            let old_pen = SelectObject(hdc, border_pen as _);
            Ellipse(hdc, 6, 6, RADIAL_SIZE - 6, RADIAL_SIZE - 6);
            SelectObject(hdc, old_brush);
            SelectObject(hdc, old_pen);
            DeleteObject(bg_brush as _);
            DeleteObject(border_pen as _);

            // 4 Tombol Sektor: Atas(0), Kanan(1), Bawah(2), Kiri(3)
            let items: [(&str, i32, i32); 4] = [
                ("SCREENSHOT", RADIAL_SIZE / 2, 48),
                ("TASK VIEW", RADIAL_SIZE - 54, RADIAL_SIZE / 2),
                ("AIR DRAW", RADIAL_SIZE / 2, RADIAL_SIZE - 48),
                ("CLIPBOARD", 54, RADIAL_SIZE / 2),
            ];

            let font = CreateFontW(
                15,
                0,
                0,
                0,
                FW_BOLD as i32,
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

            for (idx, (label, cx, cy)) in items.iter().enumerate() {
                let is_sel = selected == Some(idx);
                let fill_color = if is_sel { 0x00D87A00 } else { 0x00383838 };
                let b = CreateSolidBrush(fill_color);
                let p = CreatePen(PS_SOLID as i32, 2, if is_sel { 0x00FFFFFF } else { 0x00555555 });
                let ob = SelectObject(hdc, b as _);
                let op = SelectObject(hdc, p as _);
                Ellipse(hdc, cx - 42, cy - 26, cx + 42, cy + 26);
                SelectObject(hdc, ob);
                SelectObject(hdc, op);
                DeleteObject(b as _);
                DeleteObject(p as _);

                SetTextColor(hdc, 0x00FFFFFF);
                let mut tr = RECT {
                    left: cx - 44,
                    top: cy - 16,
                    right: cx + 44,
                    bottom: cy + 16,
                };
                let w = to_wide(label);
                DrawTextW(hdc, w.as_ptr(), -1, &mut tr, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            }

            SelectObject(hdc, old_font);
            DeleteObject(font as _);
            EndPaint(hwnd, &ps);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn wnd_proc_draw(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_DRAW_UPDATE => {
            if let Some(state) = UI_STATE.get() {
                let has_strokes = state
                    .strokes
                    .lock()
                    .map(|s| !s.is_empty())
                    .unwrap_or(false);
                if has_strokes || state.draw_mode.load(Ordering::SeqCst) {
                    ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                    InvalidateRect(hwnd, std::ptr::null(), 1);
                } else {
                    ShowWindow(hwnd, SW_HIDE);
                }
            }
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect);

            let key_brush = CreateSolidBrush(COLOR_KEY_TRANS);
            FillRect(hdc, &rect, key_brush);
            DeleteObject(key_brush as _);

            if let Some(state) = UI_STATE.get() {
                if let Ok(strokes) = state.strokes.lock() {
                    // Pena merah-oranye neon tebal (BGR: 0x003355FF)
                    let pen = CreatePen(PS_SOLID as i32, 6, 0x003355FF);
                    let old_pen = SelectObject(hdc, pen as _);

                    for stroke in strokes.iter() {
                        if let Some(&(x0, y0)) = stroke.first() {
                            MoveToEx(hdc, x0, y0, std::ptr::null_mut());
                            for &(x, y) in stroke.iter().skip(1) {
                                LineTo(hdc, x, y);
                            }
                        }
                    }

                    SelectObject(hdc, old_pen);
                    DeleteObject(pen as _);
                }
            }

            EndPaint(hwnd, &ps);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn wnd_proc_hud(
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
                    let draw_label = if state.draw_mode.load(Ordering::SeqCst) {
                        "Matikan Air Draw"
                    } else {
                        "Aktifkan Air Draw (Corat-Coret)"
                    };

                    AppendMenuW(hmenu, MF_STRING, MENU_TOGGLE_CAMERA, to_wide(cam_label).as_ptr());
                    AppendMenuW(hmenu, MF_STRING, MENU_TOGGLE_TRACKING, to_wide(track_label).as_ptr());
                    AppendMenuW(hmenu, MF_STRING, MENU_TOGGLE_DRAW, to_wide(draw_label).as_ptr());
                    AppendMenuW(hmenu, MF_STRING, MENU_CLEAR_DRAW, to_wide("Bersihkan Coretan").as_ptr());
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
                    MENU_TOGGLE_DRAW => {
                        let next = !state.draw_mode.load(Ordering::SeqCst);
                        state.draw_mode.store(next, Ordering::SeqCst);
                        if !next {
                            UiHandle.draw_clear();
                        }
                        UiHandle.show_hud(if next { "Air Draw: AKTIF" } else { "Air Draw: NONAKTIF" });
                    }
                    MENU_CLEAR_DRAW => {
                        UiHandle.draw_clear();
                        UiHandle.show_hud("Coretan Dibersihkan");
                    }
                    MENU_HELP => {
                        thread::spawn(|| unsafe {
                            let help_text = concat!(
                                "=== PERINTAH SUARA ===\n",
                                "• 'start tracking' / 'stop tracking' : Aktif/Pause tracking\n",
                                "• 'show camera' / 'hide camera'      : Tampil/Sembunyi kamera\n",
                                "• 'start typing' / 'stop typing'     : Mode dikte lengkap\n",
                                "• 'volume up' / 'volume down' / 'mute volume'\n",
                                "• 'switch window' / 'desktop left/right'\n\n",
                                "=== GESTUR TANGAN ===\n",
                                "• Tangan Kanan Cubit 1x / 2x  : Klik Kiri / Klik Kanan\n",
                                "• Tangan Kanan Tahan Cubit    : Drag & Drop / Gambar (Air Draw)\n",
                                "• 2 Tangan Terbuka            : Scroll Vertikal\n",
                                "• Tangan Kiri Cubit + Geser   : Air-Knob (Volume Naik/Turun)\n",
                                "• Tangan Kiri Kepal (Fist)    : Buka Radial Menu"
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