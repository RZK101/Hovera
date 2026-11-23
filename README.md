<div align="center">

<img src="assets/icon.ico" alt="Hovera Logo" width="96" height="96" />

# Hovera

**Kontrol PC Tanpa Sentuhan — Hand Tracking 240Hz & Offline Voice Assistant berbasis Rust**

[![Rust](https://img.shields.io/badge/Rust-2021%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%2F11-blue?logo=windows)](https://www.microsoft.com/windows)
[![ONNX Runtime](https://img.shields.io/badge/Inference-ONNX%20Runtime-lightgrey)](https://onnxruntime.ai/)
[![Offline Voice](https://img.shields.io/badge/Voice-Vosk%20Offline-green)](https://alphacephei.com/vosk/)

</div>

---

## Tentang Hovera

**Hovera** adalah aplikasi utilitas desktop berbasis **Rust** yang memungkinkan kamu mengendalikan kursor, melakukan gestur multi-aksi, menjalankan perintah sistem, hingga mendikte teks secara *hands-free* menggunakan webcam dan mikrofon standar—**100% lokal dan offline** tanpa koneksi internet.

Dibangun dengan arsitektur *multi-threaded* yang memisahkan *pipeline* inferensi AI (30 FPS) dari *loop* interpolasi kursor (240 Hz), Hovera menghadirkan gerakan kursor yang sangat mulus tanpa jeda (*lag-free*) sekaligus berjalan ringan di latar belakang melalui **System Tray** dan **Floating HUD**.

---

## Fitur Utama

* **240 Hz Smooth Glide Cursor**: Kombinasi *One Euro Filter* dan algoritma *SmoothDamp* di *thread* khusus 240 Hz menjembatani interval kamera 30 FPS menjadi pergerakan kursor yang presisi dan bebas getar (*jitter-free*).
* **Hand Tracking Bertenaga ONNX**: Deteksi telapak tangan dan 21 titik *landmark* jari secara *real-time* menggunakan model ONNX dengan pemrosesan *letterbox* akurat[cite: 5].
* **Offline Voice Commands & Dikte Suara (Vosk)**: Mendukung kontrol sistem berbasis suara (*grammar-constrained* agar akurat dan minim salah deteksi) serta mode dikte teks (*Speech-to-Text*) langsung ke aplikasi aktif mana pun[cite: 8, 9].
* **Floating HUD (*Dynamic Island*)**: Kapsul notifikasi semi-transparan di bagian atas layar yang tembus klik (*click-through*) untuk memberikan umpan balik instan setiap kali perintah dijalankan[cite: 11].
* **Native Windows Experience**: Berjalan tanpa jendela konsol hitam di mode rilis, dilengkapi integrasi **System Tray**, menu panduan perintah interaktif, serta pratinjau kamera *Always-on-Top*[cite: 11, 13].

---

## Panduan Gestur Tangan

| Gestur | Aksi | Keterangan |
| :--- | :--- | :--- |
| **Gerakkan Jari Tengah** | Gerakkan Kursor | Pemetaan area aktif kamera ke resolusi layar penuh (DPI-aware). |
| **Cubit (Telunjuk + Jempol) 1x** | Klik Kiri | Kursor otomatis terkunci sesaat ketika jari mendekat agar klik tetap akurat[cite: 13]. |
| **Cubit 2x Cepat (`< 0.5 dtk`)** | Klik Kanan | Memicu *context menu* Windows[cite: 13]. |
| **Tahan Cubitan (`> 0.4 dtk`)** | Drag & Drop | Menahan klik kiri untuk memindahkan file/jendela hingga cubitan dilepas[cite: 13]. |
| **2 Tangan Terbuka + Gerak Naik/Turun** | Scroll Vertikal | Menggulir halaman web atau dokumen dengan halus[cite: 13]. |

---

## Daftar Perintah Suara & Shortcut

### Perintah Suara (Bahasa Inggris)
* **Kontrol Utama:**
  * `"start tracking"` / `"enable hand"` — Mengaktifkan deteksi gerakan tangan[cite: 8, 13]
  * `"stop tracking"` / `"disable hand"` — Menjeda deteksi gerakan tangan[cite: 8, 13]
  * `"show camera"` / `"hide camera"` — Menampilkan atau menyembunyikan jendela pratinjau kamera[cite: 8, 13]
* **Mode Dikte (*Voice-to-Text*):**
  * `"start typing"` — Masuk ke mode dikte (ucapan langsung diketik ke kursor aktif)[cite: 8, 13]
  * `"stop typing"` — Keluar dari mode dikte dan kembali ke mode perintah[cite: 8, 13]
* **Navigasi & Manajemen Jendela:**
  * `"switch window"` / `"next window"` — Berpindah jendela (`Alt + Tab`)[cite: 8, 9, 13]
  * `"desktop left"` / `"desktop right"` — Berpindah Virtual Desktop (`Win + Ctrl + Left/Right`)[cite: 8, 9, 13]
  * `"go back"` / `"go forward"` — Navigasi mundur/maju (`Alt + Left/Right`)[cite: 8, 9, 13]
  * `"close window"` / `"minimize window"` — Menutup (`Alt + F4`) atau mengecilkan jendela (`Win + Down`)[cite: 8, 9, 13]
* **Utilitas & Media:**
  * `"copy that"` / `"paste that"` — Menyalin (`Ctrl + C`) atau menempel (`Ctrl + V`)[cite: 8, 9, 13]
  * `"play pause"` — Kontrol Play/Pause media[cite: 8, 9, 13]
  * `"escape"` / `"press escape"` — Menekan tombol `Esc`[cite: 8, 9, 13]

### Shortcut Keyboard Global
* **`Ctrl + Alt + V`** — Tampilkan / Sembunyikan jendela pratinjau kamera[cite: 9, 13]
* **`Ctrl + Alt + Q`** — Keluar dari aplikasi sepenuhnya[cite: 9, 13]

---

## Struktur Proyek

```text
hovera/
├── assets/
│   └── icon.ico                # Ikon kustom aplikasi, tray, & window
├── models/
│   ├── palm_detection.onnx     # Model ONNX deteksi telapak tangan
│   ├── hand_landmark.onnx      # Model ONNX 21 titik landmark tangan
│   └── vosk-model-en/          # Model offline speech recognition Vosk
├── src/
│   ├── main.rs                 # Loop utama, filter kursor 240Hz, & manajemen window kamera
│   ├── hand_tracker.rs         # Inferensi ONNX Runtime & rendering skeleton tangan
│   ├── camera.rs               # Capture webcam multi-thread (nokhwa)
│   ├── voice.rs                # Audio stream (cpal) & voice recognition (vosk)
│   ├── ui.rs                   # Floating HUD, System Tray, & Win32 UI helpers
│   └── win_api.rs              # Wrapper Win32 API (input mouse/keyboard, DPI, timer)
├── build.rs                    # Script penyematan resource ikon Windows (winres)
└── Cargo.toml
