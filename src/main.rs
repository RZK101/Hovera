#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod camera;
mod hand_tracker;
mod ui;
mod voice;
mod win_api;

use camera::CameraStream;
use hand_tracker::{
    draw_filled_circle, draw_hand_skeleton, draw_rect_outline, HandTracker, Landmark,
};
use image::Rgb;
use minifb::{Window, WindowOptions};
use std::f32::consts::PI;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use voice::VoiceController;
use win_api::{
    disable_high_res_timer, enable_dpi_awareness, enable_high_res_timer, get_screen_size,
    emoji_panel, left_click, media_play_pause, mouse_down, mouse_up, move_mouse, mute_volume,
    open_clipboard_history, open_task_view, right_click, scroll_vertical, set_high_priority,
    set_window_topmost, show_desktop, take_screenshot, volume_down, volume_up, HotkeyWatcher,
};

const RADIAL_PINCH_THRESHOLD: f32 = 38.0;
const RADIAL_RELEASE_THRESHOLD: f32 = 46.0;
const RADIAL_HOLD_MS: u64 = 150;

struct OneEuroFilter {
    min_cutoff: f32,
    beta: f32,
    d_cutoff: f32,
    x_prev: Option<f32>,
    dx_prev: f32,
    t_prev: Option<Instant>,
}

impl OneEuroFilter {
    fn new(min_cutoff: f32, beta: f32, d_cutoff: f32) -> Self {
        Self {
            min_cutoff,
            beta,
            d_cutoff,
            x_prev: None,
            dx_prev: 0.0,
            t_prev: None,
        }
    }

    fn reset(&mut self) {
        self.x_prev = None;
        self.dx_prev = 0.0;
        self.t_prev = None;
    }

    fn smoothing_factor(dt: f32, cutoff: f32) -> f32 {
        let r = 2.0 * PI * cutoff * dt;
        r / (r + 1.0)
    }

    fn filter(&mut self, x: f32, now: Instant) -> f32 {
        let (Some(x_prev), Some(t_prev)) = (self.x_prev, self.t_prev) else {
            self.x_prev = Some(x);
            self.dx_prev = 0.0;
            self.t_prev = Some(now);
            return x;
        };

        let dt = now.duration_since(t_prev).as_secs_f32().max(1e-4);

        let a_d = Self::smoothing_factor(dt, self.d_cutoff);
        let dx = (x - x_prev) / dt;
        let dx_hat = self.dx_prev + a_d * (dx - self.dx_prev);

        let cutoff = self.min_cutoff + self.beta * dx_hat.abs();
        let a = Self::smoothing_factor(dt, cutoff);
        let x_hat = x_prev + a * (x - x_prev);

        self.x_prev = Some(x_hat);
        self.dx_prev = dx_hat;
        self.t_prev = Some(now);

        x_hat
    }
}

struct SmoothDamp {
    vel_x: f32,
    vel_y: f32,
    smooth_time: f32,
}

impl SmoothDamp {
    fn new(smooth_time: f32) -> Self {
        Self {
            vel_x: 0.0,
            vel_y: 0.0,
            smooth_time,
        }
    }

    fn reset(&mut self) {
        self.vel_x = 0.0;
        self.vel_y = 0.0;
    }

    fn update(
        &mut self,
        cur_x: f32,
        cur_y: f32,
        target_x: f32,
        target_y: f32,
        dt: f32,
    ) -> (f32, f32) {
        let st = self.smooth_time.max(0.0001);
        let omega = 2.0 / st;
        let x = omega * dt;
        let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);

        let change_x = cur_x - target_x;
        let change_y = cur_y - target_y;

        let temp_x = (self.vel_x + omega * change_x) * dt;
        let temp_y = (self.vel_y + omega * change_y) * dt;

        self.vel_x = (self.vel_x - omega * temp_x) * exp;
        self.vel_y = (self.vel_y - omega * temp_y) * exp;

        let out_x = target_x + (change_x + temp_x) * exp;
        let out_y = target_y + (change_y + temp_y) * exp;
        (out_x, out_y)
    }
}

#[derive(Clone, Copy)]
struct CursorState {
    target_x: f32,
    target_y: f32,
    active: bool,
    moving: bool,
    snap: bool,
}

fn log_step(msg: &str) {
    println!("{msg}");
    let _ = io::stdout().flush();
}

fn get_distance(p1: (f32, f32), p2: (f32, f32)) -> f32 {
    (p2.0 - p1.0).hypot(p2.1 - p1.1)
}

fn map_value(value: f32, in_min: f32, in_max: f32, out_min: f32, out_max: f32) -> f32 {
    let clamped = value.clamp(in_min, in_max);
    (clamped - in_min) * (out_max - out_min) / (in_max - in_min) + out_min
}

fn is_hand_open(lms: &[Landmark; 21]) -> bool {
    let tips = [8, 12, 16, 20];
    let pips = [6, 10, 14, 18];
    let mut open_fingers = 0;
    for (tip, pip) in tips.iter().zip(pips.iter()) {
        if lms[*tip].y < lms[*pip].y {
            open_fingers += 1;
        }
    }
    open_fingers >= 3
}

fn main() {
    enable_dpi_awareness();
    enable_high_res_timer();
    set_high_priority();

    let (screen_w, screen_h) = get_screen_size();

    log_step("[1/3] Memuat model ONNX...");
    let mut tracker = match HandTracker::new(
        "models/palm_detection.onnx",
        "models/hand_landmark.onnx",
    ) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[ERROR] {e}");
            disable_high_res_timer();
            return;
        }
    };

    log_step("[2/3] Menginisialisasi kamera...");
    let mut cap = match CameraStream::new(0) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[ERROR] {e}");
            disable_high_res_timer();
            return;
        }
    };

    let is_running = Arc::new(AtomicBool::new(true));
    let tracking_enabled = Arc::new(AtomicBool::new(false));
    let show_camera = Arc::new(AtomicBool::new(true));
    let draw_mode = Arc::new(AtomicBool::new(false));

    let ui_handle = ui::start_ui(
        Arc::clone(&is_running),
        Arc::clone(&tracking_enabled),
        Arc::clone(&show_camera),
        Arc::clone(&draw_mode),
    );
    ui_handle.show_hud("Jedi Mouse Siap!");

    log_step("[3/3] Mengaktifkan Voice Command (Vosk EN)...");
    let voice_ctrl = match VoiceController::start(
        "models/vosk-model-en",
        Arc::clone(&is_running),
        Arc::clone(&tracking_enabled),
        Arc::clone(&show_camera),
        ui_handle.clone(),
    ) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("[WARN] Voice Command nonaktif: {e}");
            ui_handle.show_hud("Voice Nonaktif (Hand Tracking Aktif)");
            tracking_enabled.store(true, Ordering::SeqCst);
            None
        }
    };

    let cursor_state = Arc::new(Mutex::new(CursorState {
        target_x: screen_w * 0.5,
        target_y: screen_h * 0.5,
        active: false,
        moving: false,
        snap: false,
    }));

    let running_cursor = Arc::clone(&is_running);
    let state_cursor = Arc::clone(&cursor_state);
    let cursor_thread = thread::spawn(move || {
        let mut damper = SmoothDamp::new(0.042);
        let mut cur_x = screen_w * 0.5;
        let mut cur_y = screen_h * 0.5;
        let mut last_sent_x = cur_x;
        let mut last_sent_y = cur_y;
        let mut last_tick = Instant::now();

        while running_cursor.load(Ordering::SeqCst) {
            let now = Instant::now();
            let dt = now.duration_since(last_tick).as_secs_f32().clamp(0.001, 0.05);
            last_tick = now;

            let snapshot = if let Ok(mut st) = state_cursor.lock() {
                let copy = *st;
                if st.snap {
                    st.snap = false;
                }
                Some(copy)
            } else {
                None
            };

            if let Some(st) = snapshot {
                if st.snap {
                    cur_x = st.target_x;
                    cur_y = st.target_y;
                    damper.reset();
                    move_mouse(cur_x, cur_y);
                    last_sent_x = cur_x;
                    last_sent_y = cur_y;
                } else if st.active && st.moving {
                    let (nx, ny) = damper.update(cur_x, cur_y, st.target_x, st.target_y, dt);
                    cur_x = nx;
                    cur_y = ny;

                    if (cur_x - last_sent_x).hypot(cur_y - last_sent_y) >= 0.25 {
                        move_mouse(cur_x, cur_y);
                        last_sent_x = cur_x;
                        last_sent_y = cur_y;
                    }
                } else {
                    damper.reset();
                }
            }

            thread::sleep(Duration::from_millis(4));
        }
    });

    let mut hotkeys = HotkeyWatcher::new();

    let mut is_pinching = false;
    let mut pinch_start_time = Instant::now();
    let mut last_click_time: Option<Instant> = None;
    let mut is_dragging = false;
    let mut right_hand_prev_y: Option<f32> = None;

    // State untuk Air-Knob (Volume Tangan Kiri)
    let mut left_knob_ref_y: Option<f32> = None;

    // State untuk Radial Menu (Hold Pinch Jempol + Kelingking -> Lepas untuk Pilih)
    let mut radial_open = false;
    let mut radial_center = (screen_w * 0.5, screen_h * 0.5);
    let mut radial_selected: Option<usize> = None;
    let mut radial_pinch_start: Option<Instant> = None;

    let frame_r: f32 = 120.0;
    let mut filter_x = OneEuroFilter::new(1.1, 0.015, 1.0);
    let mut filter_y = OneEuroFilter::new(1.1, 0.015, 1.0);
    let mut hand_was_active = false;

    let mut preview_window: Option<Window> = None;
    let mut pixel_buffer: Vec<u32> = Vec::new();

    while is_running.load(Ordering::SeqCst) {
        let (toggle_v, quit_q) = hotkeys.poll();
        if toggle_v {
            let next = !show_camera.load(Ordering::SeqCst);
            show_camera.store(next, Ordering::SeqCst);
            ui_handle.show_hud(if next { "Kamera: TAMPIL" } else { "Kamera: TERSEMBUNYI" });
        }
        if quit_q {
            break;
        }

        let Some(mut frame) = cap.read() else {
            thread::sleep(Duration::from_millis(2));
            continue;
        };

        let now = Instant::now();
        let w = frame.width() as f32;
        let h = frame.height() as f32;
        let cam_visible = show_camera.load(Ordering::SeqCst);

        if tracking_enabled.load(Ordering::SeqCst) {
            let detected_hands = tracker.process(&frame, 2);

            let mut left_hand_lms: Option<[Landmark; 21]> = None;
            let mut right_hand_lms: Option<[Landmark; 21]> = None;

            if cam_visible {
                draw_rect_outline(
                    &mut frame,
                    frame_r as i32,
                    frame_r as i32,
                    (w - frame_r) as i32,
                    (h - frame_r) as i32,
                    Rgb([255, 0, 255]),
                );
            }

            for hand in &detected_hands {
                if cam_visible {
                    draw_hand_skeleton(&mut frame, &hand.landmarks);
                }
                if hand.is_left_label {
                    right_hand_lms = Some(hand.landmarks);
                } else {
                    left_hand_lms = Some(hand.landmarks);
                }
            }

            // --- 1. GESTUR TANGAN KIRI: AIR-KNOB (VOLUME) ---
            if let Some(left_lms) = left_hand_lms {
                let l_index = (left_lms[8].x * w, left_lms[8].y * h);
                let l_thumb = (left_lms[4].x * w, left_lms[4].y * h);
                let l_pinch = get_distance(l_index, l_thumb) < 38.0;

                if l_pinch {
                    let cur_y = l_index.1;
                    if let Some(ref_y) = left_knob_ref_y {
                        let dy = ref_y - cur_y; // Positif jika naik
                        if dy > 16.0 {
                            volume_up();
                            left_knob_ref_y = Some(cur_y);
                            ui_handle.show_hud("Air-Knob: Volume Naik ▲");
                        } else if dy < -16.0 {
                            volume_down();
                            left_knob_ref_y = Some(cur_y);
                            ui_handle.show_hud("Air-Knob: Volume Turun ▼");
                        }
                    } else {
                        left_knob_ref_y = Some(cur_y);
                    }
                    if cam_visible {
                        draw_filled_circle(
                            &mut frame,
                            l_index.0 as i32,
                            l_index.1 as i32,
                            14,
                            Rgb([0, 200, 255]),
                        );
                    }
                } else {
                    left_knob_ref_y = None;
                }
            } else {
                left_knob_ref_y = None;
            }

            // --- 2. GESTUR TANGAN KANAN: KURSOR, SCROLL, RADIAL MENU, AIR DRAW ---
            if let Some(lms) = right_hand_lms {
                let middle_tip_raw = (lms[12].x * w, lms[12].y * h);
                let index_tip = (lms[8].x * w, lms[8].y * h);
                let thumb_tip = (lms[4].x * w, lms[4].y * h);
                let pinky_tip = (lms[20].x * w, lms[20].y * h);

                let pinch_dist = get_distance(index_tip, thumb_tip);
                let pinky_pinch_dist = get_distance(thumb_tip, pinky_tip);

                // --- TRIGGER RADIAL MENU (Hold Pinch Jempol + Kelingking -> Lepas untuk Pilih) ---
                if pinky_pinch_dist < RADIAL_PINCH_THRESHOLD {
                    let start = *radial_pinch_start.get_or_insert(now);
                    if !radial_open
                        && now.duration_since(start) >= Duration::from_millis(RADIAL_HOLD_MS)
                    {
                        radial_open = true;
                        radial_selected = None;
                        if let Ok(st) = cursor_state.lock() {
                            radial_center = (
                                st.target_x.clamp(160.0, screen_w - 160.0),
                                st.target_y.clamp(160.0, screen_h - 160.0),
                            );
                        }
                        ui_handle.update_radial(
                            true,
                            radial_center.0 as i32,
                            radial_center.1 as i32,
                            None,
                        );
                    }

                    if cam_visible {
                        draw_filled_circle(
                            &mut frame,
                            pinky_tip.0 as i32,
                            pinky_tip.1 as i32,
                            12,
                            Rgb([255, 165, 0]),
                        );
                    }
                } else if pinky_pinch_dist > RADIAL_RELEASE_THRESHOLD {
                    radial_pinch_start = None;
                    if radial_open {
                        radial_open = false;
                        ui_handle.update_radial(false, 0, 0, None);
                        if let Some(sel) = radial_selected.take() {
                            match sel {
                                0 => {
                                    take_screenshot();
                                    ui_handle.show_hud("Radial: Screenshot");
                                }
                                1 => {
                                    show_desktop();
                                    ui_handle.show_hud("Radial: Show Desktop");
                                }
                                2 => {
                                    open_task_view();
                                    ui_handle.show_hud("Radial: Task View");
                                }
                                3 => {
                                    media_play_pause();
                                    ui_handle.show_hud("Radial: Play / Pause");
                                }
                                4 => {
                                    let next = !draw_mode.load(Ordering::SeqCst);
                                    draw_mode.store(next, Ordering::SeqCst);
                                    if !next {
                                        ui_handle.draw_clear();
                                    }
                                    ui_handle.show_hud(if next {
                                        "Air Draw: AKTIF (Cubit kanan untuk gambar)"
                                    } else {
                                        "Air Draw: NONAKTIF"
                                    });
                                }
                                5 => {
                                    mute_volume();
                                    ui_handle.show_hud("Radial: Mute / Unmute");
                                }
                                6 => {
                                    open_clipboard_history();
                                    ui_handle.show_hud("Radial: Clipboard");
                                }
                                7 => {
                                    emoji_panel();
                                    ui_handle.show_hud("Radial: Emoji Panel");
                                }
                                _ => {}
                            }
                        }
                    }
                }

                let raw_screen_x = map_value(middle_tip_raw.0, frame_r, w - frame_r, 0.0, screen_w);
                let raw_screen_y = map_value(middle_tip_raw.1, frame_r, h - frame_r, 0.0, screen_h);

                let mut is_scrolling = false;
                if !radial_open {
                    if let Some(left_lms) = left_hand_lms {
                        if is_hand_open(&left_lms) && is_hand_open(&lms) {
                            is_scrolling = true;
                        }
                    }
                }

                if is_scrolling {
                    if let Ok(mut st) = cursor_state.lock() {
                        st.active = true;
                        st.moving = false;
                    }
                    if let Some(prev_y) = right_hand_prev_y {
                        let dy = middle_tip_raw.1 - prev_y;
                        if dy.abs() > 1.0 {
                            scroll_vertical((-dy * 35.0) as i32);
                        }
                    }
                    right_hand_prev_y = Some(middle_tip_raw.1);
                    if cam_visible {
                        draw_filled_circle(&mut frame, 30, 30, 14, Rgb([255, 255, 0]));
                    }
                } else {
                    right_hand_prev_y = None;
                    let is_drawing_mode = draw_mode.load(Ordering::SeqCst);
                    let freeze_for_click =
                        !is_dragging && !is_drawing_mode && !radial_open && pinch_dist < 52.0;

                    let (tx, ty) = if !hand_was_active {
                        filter_x.reset();
                        filter_y.reset();
                        let x = filter_x.filter(raw_screen_x, now);
                        let y = filter_y.filter(raw_screen_y, now);
                        if let Ok(mut st) = cursor_state.lock() {
                            st.target_x = x;
                            st.target_y = y;
                            st.active = true;
                            st.moving = true;
                            st.snap = true;
                        }
                        hand_was_active = true;
                        (x, y)
                    } else if freeze_for_click {
                        if let Ok(mut st) = cursor_state.lock() {
                            st.moving = false;
                            (st.target_x, st.target_y)
                        } else {
                            (raw_screen_x, raw_screen_y)
                        }
                    } else {
                        let x = filter_x.filter(raw_screen_x, now);
                        let y = filter_y.filter(raw_screen_y, now);
                        if let Ok(mut st) = cursor_state.lock() {
                            st.target_x = x;
                            st.target_y = y;
                            st.active = true;
                            st.moving = true;
                        }
                        (x, y)
                    };

                    // Update pilihan sektor Radial Menu jika sedang terbuka
                    if radial_open {
                        let dx = tx - radial_center.0;
                        let dy = ty - radial_center.1;
                        let dist_sq = dx * dx + dy * dy;
                        let new_sel = if dist_sq > 2000.0 {
                            let deg = dy.atan2(dx).to_degrees();
                            let shifted = (deg + 90.0 + 22.5).rem_euclid(360.0);
                            Some((shifted / 45.0).floor() as usize)
                        } else {
                            None
                        };
                        if new_sel != radial_selected {
                            radial_selected = new_sel;
                            ui_handle.update_radial(
                                true,
                                radial_center.0 as i32,
                                radial_center.1 as i32,
                                radial_selected,
                            );
                        }
                    } else if is_drawing_mode {
                        // Mode Air Annotation: cubit kanan untuk menggambar
                        if pinch_dist < 42.0 {
                            if !is_pinching {
                                is_pinching = true;
                                ui_handle.draw_start_stroke(tx as i32, ty as i32);
                            } else {
                                ui_handle.draw_add_point(tx as i32, ty as i32);
                            }
                        } else {
                            is_pinching = false;
                        }
                    } else {
                        // Mode Mouse Normal: Klik Kiri, Klik Kanan, Drag & Drop
                        if pinch_dist < 40.0 {
                            if !is_pinching {
                                is_pinching = true;
                                pinch_start_time = now;
                            } else if now.duration_since(pinch_start_time).as_secs_f32() > 0.4
                                && !is_dragging
                            {
                                mouse_down();
                                is_dragging = true;
                                if cam_visible {
                                    draw_filled_circle(
                                        &mut frame,
                                        index_tip.0 as i32,
                                        index_tip.1 as i32,
                                        15,
                                        Rgb([0, 255, 0]),
                                    );
                                }
                            }
                        } else if is_pinching {
                            is_pinching = false;
                            if is_dragging {
                                mouse_up();
                                is_dragging = false;
                            } else {
                                let is_double = last_click_time
                                    .map(|t| now.duration_since(t).as_secs_f32() < 0.5)
                                    .unwrap_or(false);

                                if is_double {
                                    right_click();
                                    last_click_time = None;
                                } else {
                                    left_click();
                                    last_click_time = Some(now);
                                }
                            }
                        }
                    }
                }
            } else {
                if is_dragging {
                    mouse_up();
                    is_dragging = false;
                }
                is_pinching = false;
                radial_pinch_start = None;
                if radial_open {
                    radial_open = false;
                    radial_selected = None;
                    ui_handle.update_radial(false, 0, 0, None);
                }
                hand_was_active = false;
                right_hand_prev_y = None;
                if let Ok(mut st) = cursor_state.lock() {
                    st.active = false;
                    st.moving = false;
                }
            }
        } else {
            if is_dragging {
                mouse_up();
                is_dragging = false;
            }
            is_pinching = false;
            radial_pinch_start = None;
            hand_was_active = false;
            right_hand_prev_y = None;
            left_knob_ref_y = None;
            if radial_open {
                radial_open = false;
                radial_selected = None;
                ui_handle.update_radial(false, 0, 0, None);
            }
            if let Ok(mut st) = cursor_state.lock() {
                st.active = false;
                st.moving = false;
            }
            if cam_visible {
                draw_filled_circle(&mut frame, 30, 30, 12, Rgb([255, 50, 50]));
            }
        }

        // --- JENDELA PREVIEW KAMERA ---
        if cam_visible {
            let fw = frame.width() as usize;
            let fh = frame.height() as usize;

            if preview_window.is_none() {
                if let Ok(win) = Window::new(
                    "Hovera",
                    fw,
                    fh,
                    WindowOptions {
                        resize: true,
                        ..WindowOptions::default()
                    },
                ) {
                    set_window_topmost(win.get_window_handle());
                    ui::set_window_icon("Hovera");
                    preview_window = Some(win);
                }
            }

            if let Some(win) = preview_window.as_mut() {
                if !win.is_open() {
                    show_camera.store(false, Ordering::SeqCst);
                    preview_window = None;
                } else {
                    pixel_buffer.resize(fw * fh, 0);
                    for (dst, px) in pixel_buffer.iter_mut().zip(frame.pixels()) {
                        *dst = ((px[0] as u32) << 16) | ((px[1] as u32) << 8) | (px[2] as u32);
                    }
                    let _ = win.update_with_buffer(&pixel_buffer, fw, fh);
                }
            }
        } else if preview_window.is_some() {
            preview_window = None;
        }
    }

    if is_dragging {
        mouse_up();
    }
    is_running.store(false, Ordering::SeqCst);
    if let Some(mut v) = voice_ctrl {
        v.stop();
    }
    let _ = cursor_thread.join();
    cap.release();
    disable_high_res_timer();
}