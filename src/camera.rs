use image::{imageops, RgbImage};
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{
    CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution,
};
use nokhwa::Camera;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub struct CameraStream {
    latest_frame: Arc<Mutex<Option<RgbImage>>>,
    is_running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl CameraStream {
    pub fn new(camera_idx: u32) -> Result<Self, String> {
        let latest_frame = Arc::new(Mutex::new(None));
        let is_running = Arc::new(AtomicBool::new(true));

        let frame_clone = Arc::clone(&latest_frame);
        let running_clone = Arc::clone(&is_running);

        let (init_tx, init_rx) = mpsc::channel::<Result<(), String>>();

        let handle = thread::spawn(move || {
            let index = CameraIndex::Index(camera_idx);
            let preferred = CameraFormat::new(Resolution::new(640, 480), FrameFormat::MJPEG, 30);
            let req_640 = RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(preferred));

            let mut camera = match Camera::new(index.clone(), req_640) {
                Ok(cam) => cam,
                Err(_) => {
                    let req_fallback = RequestedFormat::new::<RgbFormat>(RequestedFormatType::None);
                    match Camera::new(index, req_fallback) {
                        Ok(cam) => cam,
                        Err(e) => {
                            let err_msg = format!("Gagal membuka kamera: {e}");
                            running_clone.store(false, Ordering::SeqCst);
                            let _ = init_tx.send(Err(err_msg));
                            return;
                        }
                    }
                }
            };

            if let Err(e) = camera.open_stream() {
                let err_msg = format!("Gagal memulai stream kamera: {e}");
                running_clone.store(false, Ordering::SeqCst);
                let _ = init_tx.send(Err(err_msg));
                return;
            }

            println!("[INFO] Stream kamera aktif ({})", camera.resolution());
            let _ = init_tx.send(Ok(()));

            while running_clone.load(Ordering::SeqCst) {
                if let Ok(frame) = camera.frame() {
                    if let Ok(rgb) = frame.decode_image::<RgbFormat>() {
                        let flipped = imageops::flip_horizontal(&rgb);
                        if let Ok(mut slot) = frame_clone.lock() {
                            *slot = Some(flipped);
                        }
                    }
                } else {
                    thread::sleep(Duration::from_millis(5));
                }
            }

            let _ = camera.stop_stream();
        });

        match init_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                latest_frame,
                is_running,
                handle: Some(handle),
            }),
            Ok(Err(e)) => {
                let _ = handle.join();
                Err(e)
            }
            Err(_) => {
                let _ = handle.join();
                Err("Thread kamera berhenti secara tak terduga saat inisialisasi.".to_string())
            }
        }
    }

    pub fn read(&self) -> Option<RgbImage> {
        self.latest_frame.lock().ok()?.take()
    }

    pub fn release(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for CameraStream {
    fn drop(&mut self) {
        self.release();
    }
}