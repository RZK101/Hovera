use crate::ui::UiHandle;
use crate::win_api::{
    close_window, copy_selection, media_play_pause, minimize_window, navigate_back,
    navigate_forward, paste_selection, press_escape, switch_desktop_left, switch_desktop_right,
    switch_window, type_text,
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use vosk::{DecodingState, LogLevel, Model, Recognizer};

const VOICE_GRAMMAR: &[&str] = &[
    "switch window",
    "next window",
    "desktop right",
    "desktop left",
    "desktop one",
    "desktop two",
    "go back",
    "go forward",
    "start tracking",
    "stop tracking",
    "enable hand",
    "disable hand",
    "escape",
    "press escape",
    "show camera",
    "hide camera",
    "close window",
    "minimize window",
    "copy that",
    "paste that",
    "play pause",
    "start typing",
    "[unk]",
];

pub struct VoiceController {
    handle: Option<JoinHandle<()>>,
}

impl VoiceController {
    pub fn start(
        model_path: &str,
        is_running: Arc<AtomicBool>,
        tracking_enabled: Arc<AtomicBool>,
        show_camera: Arc<AtomicBool>,
        ui: UiHandle,
    ) -> Result<Self, String> {
        let model_path = model_path.to_string();
        let (init_tx, init_rx) = mpsc::channel::<Result<(), String>>();

        let handle = thread::spawn(move || {
            vosk::set_log_level(LogLevel::Error);

            let Some(model) = Model::new(&model_path) else {
                let _ = init_tx.send(Err(format!(
                    "Gagal memuat model Vosk dari folder '{model_path}'"
                )));
                return;
            };

            let host = cpal::default_host();
            let Some(device) = host.default_input_device() else {
                let _ = init_tx.send(Err("Mikrofon default tidak ditemukan.".to_string()));
                return;
            };

            let config = match device.default_input_config() {
                Ok(c) => c,
                Err(e) => {
                    let _ = init_tx.send(Err(format!("Gagal membaca konfigurasi mikrofon: {e}")));
                    return;
                }
            };

            let sample_rate = config.sample_rate() as f32;
            let channels = config.channels() as usize;
            let sample_format = config.sample_format();

            let Some(mut cmd_recognizer) =
                Recognizer::new_with_grammar(&model, sample_rate, VOICE_GRAMMAR)
            else {
                let _ = init_tx.send(Err("Gagal menginisialisasi Command Recognizer.".to_string()));
                return;
            };

            let Some(mut dictation_recognizer) = Recognizer::new(&model, sample_rate) else {
                let _ = init_tx.send(Err("Gagal menginisialisasi Dictation Recognizer.".to_string()));
                return;
            };

            let (audio_tx, audio_rx) = mpsc::channel::<Vec<i16>>();
            fn err_fn<E: std::fmt::Display>(err: E) {
                let msg = err.to_string();
                if !msg.contains("buffer underrun or overrun") {
                    eprintln!("[VOICE ERROR] Stream mikrofon bermasalah: {msg}");
                }
            }

            let stream_result = match sample_format {
                SampleFormat::F32 => {
                    let tx = audio_tx.clone();
                    device.build_input_stream(
                        config.into(),
                        move |data: &[f32], _| {
                            let mono: Vec<i16> = data
                                .chunks(channels)
                                .map(|ch| (ch[0].clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                                .collect();
                            let _ = tx.send(mono);
                        },
                        err_fn,
                        None,
                    )
                }
                SampleFormat::I16 => {
                    let tx = audio_tx.clone();
                    device.build_input_stream(
                        config.into(),
                        move |data: &[i16], _| {
                            let mono: Vec<i16> = data.chunks(channels).map(|ch| ch[0]).collect();
                            let _ = tx.send(mono);
                        },
                        err_fn,
                        None,
                    )
                }
                SampleFormat::U16 => {
                    let tx = audio_tx.clone();
                    device.build_input_stream(
                        config.into(),
                        move |data: &[u16], _| {
                            let mono: Vec<i16> = data
                                .chunks(channels)
                                .map(|ch| (ch[0] as i32 - 32768) as i16)
                                .collect();
                            let _ = tx.send(mono);
                        },
                        err_fn,
                        None,
                    )
                }
                other => {
                    let _ = init_tx.send(Err(format!("Format audio tidak didukung: {other:?}")));
                    return;
                }
            };

            let stream = match stream_result {
                Ok(s) => s,
                Err(e) => {
                    let _ = init_tx.send(Err(format!("Gagal membuka stream mikrofon: {e}")));
                    return;
                }
            };

            if let Err(e) = stream.play() {
                let _ = init_tx.send(Err(format!("Gagal memulai rekaman mikrofon: {e}")));
                return;
            }

            let _ = init_tx.send(Ok(()));

            let mut dictation_mode = false;
            let mut last_trigger = Instant::now() - Duration::from_secs(2);
            let cooldown = Duration::from_millis(1200);

            while is_running.load(Ordering::SeqCst) {
                match audio_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(samples) => {
                        if dictation_mode {
                            if let Ok(DecodingState::Finalized) =
                                dictation_recognizer.accept_waveform(&samples)
                            {
                                if let Some(res) = dictation_recognizer.result().single() {
                                    let text = res.text.trim();
                                    if !text.is_empty() {
                                        if text.contains("stop typing") {
                                            dictation_mode = false;
                                            cmd_recognizer.reset();
                                            ui.show_hud("Mode Perintah AKTIF");
                                        } else {
                                            type_text(&format!("{text} "));
                                            ui.show_hud(&format!("Ketik: {text}"));
                                        }
                                    }
                                }
                            }
                        } else if let Ok(DecodingState::Finalized) =
                            cmd_recognizer.accept_waveform(&samples)
                        {
                            if let Some(res) = cmd_recognizer.result().single() {
                                let text = res.text.trim();
                                if !text.is_empty()
                                    && text != "[unk]"
                                    && last_trigger.elapsed() >= cooldown
                                {
                                    if text.contains("start typing") {
                                        dictation_mode = true;
                                        dictation_recognizer.reset();
                                        last_trigger = Instant::now();
                                        ui.show_hud("Mode Dikte AKTIF (ucap 'stop typing')");
                                    } else if execute_command(
                                        text,
                                        &tracking_enabled,
                                        &show_camera,
                                        &ui,
                                    ) {
                                        last_trigger = Instant::now();
                                    }
                                }
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }

            drop(stream);
        });

        match init_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                handle: Some(handle),
            }),
            Ok(Err(e)) => {
                let _ = handle.join();
                Err(e)
            }
            Err(_) => {
                let _ = handle.join();
                Err("Thread voice berhenti secara tak terduga.".to_string())
            }
        }
    }

    pub fn stop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn execute_command(
    text: &str,
    tracking_enabled: &AtomicBool,
    show_camera: &AtomicBool,
    ui: &UiHandle,
) -> bool {
    if text.contains("switch window") || text.contains("next window") {
        switch_window();
        ui.show_hud("Alt + Tab (Switch Window)");
        true
    } else if text.contains("desktop right") || text.contains("desktop two") {
        switch_desktop_right();
        ui.show_hud("Desktop Kanan");
        true
    } else if text.contains("desktop left") || text.contains("desktop one") {
        switch_desktop_left();
        ui.show_hud("Desktop Kiri");
        true
    } else if text.contains("go back") {
        navigate_back();
        ui.show_hud("Back");
        true
    } else if text.contains("go forward") {
        navigate_forward();
        ui.show_hud("Forward");
        true
    } else if text.contains("start tracking") || text.contains("enable hand") {
        tracking_enabled.store(true, Ordering::SeqCst);
        ui.show_hud("Hand Tracking: AKTIF");
        true
    } else if text.contains("stop tracking") || text.contains("disable hand") {
        tracking_enabled.store(false, Ordering::SeqCst);
        ui.show_hud("Hand Tracking: NONAKTIF");
        true
    } else if text.contains("escape") {
        press_escape();
        ui.show_hud("Escape");
        true
    } else if text.contains("show camera") {
        show_camera.store(true, Ordering::SeqCst);
        ui.show_hud("Kamera: TAMPIL");
        true
    } else if text.contains("hide camera") {
        show_camera.store(false, Ordering::SeqCst);
        ui.show_hud("Kamera: TERSEMBUNYI");
        true
    } else if text.contains("close window") {
        close_window();
        ui.show_hud("Tutup Jendela (Alt+F4)");
        true
    } else if text.contains("minimize window") {
        minimize_window();
        ui.show_hud("Minimize Jendela");
        true
    } else if text.contains("copy that") {
        copy_selection();
        ui.show_hud("Copy (Ctrl+C)");
        true
    } else if text.contains("paste that") {
        paste_selection();
        ui.show_hud("Paste (Ctrl+V)");
        true
    } else if text.contains("play pause") {
        media_play_pause();
        ui.show_hud("Play / Pause");
        true
    } else {
        false
    }
}