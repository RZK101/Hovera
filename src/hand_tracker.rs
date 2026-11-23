use image::{Rgb, RgbImage};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;
use std::f32::consts::PI;
use std::path::Path;

#[derive(Clone, Copy, Debug)]
pub struct Landmark {
    pub x: f32, // Normalisasi 0.0..1.0
    pub y: f32, // Normalisasi 0.0..1.0
}

#[derive(Clone, Debug)]
pub struct HandResult {
    pub landmarks: [Landmark; 21],
    pub is_left_label: bool,
}

#[derive(Clone, Copy)]
struct Anchor {
    x_center: f32,
    y_center: f32,
}

#[derive(Clone, Copy)]
struct PalmBox {
    cx: f32,
    cy: f32,
    w: f32,
    h: f32,
    rotation: f32,
    score: f32,
}

#[derive(Clone, Copy)]
struct LetterboxInfo {
    pad_x: f32,
    pad_y: f32,
    scale_x: f32,
    scale_y: f32,
}

pub const HAND_CONNECTIONS: [(usize, usize); 21] = [
    (0, 1), (1, 2), (2, 3), (3, 4),        // Ibu jari
    (0, 5), (5, 6), (6, 7), (7, 8),        // Telunjuk
    (5, 9), (9, 10), (10, 11), (11, 12),   // Jari tengah
    (9, 13), (13, 14), (14, 15), (15, 16), // Jari manis
    (13, 17), (17, 18), (18, 19), (19, 20),// Kelingking
    (0, 17),                               // Telapak
];

pub struct HandTracker {
    palm_session: Session,
    landmark_session: Session,
    palm_nchw: bool,
    landmark_nchw: bool,
    anchors: Vec<Anchor>,
}

impl HandTracker {
    pub fn new<P: AsRef<Path>>(palm_model: P, landmark_model: P) -> Result<Self, String> {
        let env = ort::init()
            .with_name("jedi_mouse")
            .build()
            .map_err(|e| format!("Gagal inisialisasi ONNX Runtime: {e}"))?;

        let load_model = |path: &Path| -> Result<(Session, bool), String> {
            let session = Session::builder(&env)
                .map_err(|e| format!("Gagal membuat SessionBuilder: {e}"))?
                .with_optimization_level(GraphOptimizationLevel::Level3)
                .map_err(|e| format!("Gagal mengatur optimasi: {e}"))?
                .with_intra_threads(2)
                .map_err(|e| format!("Gagal mengatur thread: {e}"))?
                .commit_from_file(path)
                .map_err(|e| format!("Gagal memuat model {}: {e}", path.display()))?;

            let is_nchw = session
                .inputs()
                .first()
                .and_then(|inp| inp.dtype().tensor_shape())
                .map(|shape| shape.len() == 4 && shape[1] == 3)
                .unwrap_or(false);

            Ok((session, is_nchw))
        };

        let (palm_session, palm_nchw) = load_model(palm_model.as_ref())?;
        let (landmark_session, landmark_nchw) = load_model(landmark_model.as_ref())?;

        Ok(Self {
            palm_session,
            landmark_session,
            palm_nchw,
            landmark_nchw,
            anchors: generate_ssd_anchors(),
        })
    }

    pub fn process(&mut self, frame: &RgbImage, max_hands: usize) -> Vec<HandResult> {
        let palms = self.detect_palms(frame, max_hands);
        let mut results = Vec::with_capacity(palms.len());

        for palm in palms {
            if let Some(hand) = self.detect_landmarks(frame, &palm) {
                results.push(hand);
            }
        }
        results
    }

    fn detect_palms(&mut self, frame: &RgbImage, max_hands: usize) -> Vec<PalmBox> {
        const IN_SIZE: usize = 192;
        let (shape, data, lb) = prepare_letterbox_buffer(frame, IN_SIZE, self.palm_nchw);
        let Ok(input_tensor) = Tensor::from_array((shape, data)) else {
            return Vec::new();
        };
        let inputs = ort::inputs![input_tensor];
        let Ok(outputs) = self.palm_session.run(inputs) else {
            return Vec::new();
        };
        if outputs.len() < 2 {
            return Vec::new();
        }

        let Ok((_, s0)) = outputs[0].try_extract_tensor::<f32>() else {
            return Vec::new();
        };
        let Ok((_, s1)) = outputs[1].try_extract_tensor::<f32>() else {
            return Vec::new();
        };

        let (reg_slice, score_slice) = if s0.len() > s1.len() {
            (s0, s1)
        } else {
            (s1, s0)
        };

        let mut candidates = Vec::new();
        let num_anchors = self.anchors.len().min(score_slice.len());

        for i in 0..num_anchors {
            let score = sigmoid(score_slice[i]);
            if score < 0.7 {
                continue;
            }
            let base = i * 18;
            if base + 17 >= reg_slice.len() {
                break;
            }
            let anchor = self.anchors[i];
            let cx = reg_slice[base] / IN_SIZE as f32 + anchor.x_center;
            let cy = reg_slice[base + 1] / IN_SIZE as f32 + anchor.y_center;
            let w = reg_slice[base + 2] / IN_SIZE as f32;
            let h = reg_slice[base + 3] / IN_SIZE as f32;

            let kp0_x = reg_slice[base + 4] / IN_SIZE as f32 + anchor.x_center;
            let kp0_y = reg_slice[base + 5] / IN_SIZE as f32 + anchor.y_center;
            let kp2_x = reg_slice[base + 8] / IN_SIZE as f32 + anchor.x_center;
            let kp2_y = reg_slice[base + 9] / IN_SIZE as f32 + anchor.y_center;

            let target_angle = PI * 0.5;
            let angle = target_angle - (-(kp2_y - kp0_y)).atan2(kp2_x - kp0_x);

            let shift_x = 0.5 * h * angle.sin();
            let shift_y = -0.5 * h * angle.cos();
            let box_size = w.max(h) * 2.6;

            candidates.push(PalmBox {
                cx: cx + shift_x,
                cy: cy + shift_y,
                w: box_size,
                h: box_size,
                rotation: angle,
                score,
            });
        }

        // Lakukan NMS di ruang letterbox persegi (geometri akurat), lalu konversi ke koordinat frame asli
        let mut kept = nms_palms(candidates, 0.3, max_hands);
        for palm in &mut kept {
            palm.cx = (palm.cx - lb.pad_x) / lb.scale_x;
            palm.cy = (palm.cy - lb.pad_y) / lb.scale_y;
        }
        kept
    }

    fn detect_landmarks(&mut self, frame: &RgbImage, palm: &PalmBox) -> Option<HandResult> {
        const LM_SIZE: usize = 224;
        let fw = frame.width() as f32;
        let fh = frame.height() as f32;

        let shape = if self.landmark_nchw {
            [1, 3, LM_SIZE, LM_SIZE]
        } else {
            [1, LM_SIZE, LM_SIZE, 3]
        };
        let plane = LM_SIZE * LM_SIZE;
        let mut data = vec![0.0f32; 3 * plane];

        let cos_r = palm.rotation.cos();
        let sin_r = palm.rotation.sin();

        // ROI persegi berdasarkan sisi terpanjang frame agar tangan tidak gepeng
        let roi_size = palm.w * fw.max(fh);
        let roi_w = roi_size;
        let roi_h = roi_size;
        let center_x = palm.cx * fw;
        let center_y = palm.cy * fh;

        for y in 0..LM_SIZE {
            let ny = (y as f32 / LM_SIZE as f32) - 0.5;
            for x in 0..LM_SIZE {
                let nx = (x as f32 / LM_SIZE as f32) - 0.5;
                let lx = nx * roi_w;
                let ly = ny * roi_h;

                let src_x = (center_x + lx * cos_r - ly * sin_r).round() as i32;
                let src_y = (center_y + lx * sin_r + ly * cos_r).round() as i32;

                let (r, g, b) = if src_x >= 0
                    && src_x < frame.width() as i32
                    && src_y >= 0
                    && src_y < frame.height() as i32
                {
                    let p = frame.get_pixel(src_x as u32, src_y as u32);
                    (p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0)
                } else {
                    (0.0, 0.0, 0.0)
                };

                let idx = y * LM_SIZE + x;
                if self.landmark_nchw {
                    data[idx] = r;
                    data[plane + idx] = g;
                    data[2 * plane + idx] = b;
                } else {
                    let base = idx * 3;
                    data[base] = r;
                    data[base + 1] = g;
                    data[base + 2] = b;
                }
            }
        }

        let input_tensor = Tensor::from_array((shape, data)).ok()?;
        let inputs = ort::inputs![input_tensor];
        let outputs = self.landmark_session.run(inputs).ok()?;
        if outputs.len() < 3 {
            return None;
        }

        let (_, lm_slice) = outputs[0].try_extract_tensor::<f32>().ok()?;
        let (_, flag_slice) = outputs[1].try_extract_tensor::<f32>().ok()?;
        let (_, hand_slice) = outputs[2].try_extract_tensor::<f32>().ok()?;

        let presence = flag_slice.first().copied().unwrap_or(0.0);
        if presence < 0.7 {
            return None;
        }

        let handedness_val = hand_slice.first().copied().unwrap_or(0.5);
        let is_left_label = handedness_val < 0.5;

        if lm_slice.len() < 63 {
            return None;
        }

        let mut landmarks = [Landmark { x: 0.0, y: 0.0 }; 21];
        for i in 0..21 {
            let nx = (lm_slice[i * 3] / LM_SIZE as f32) - 0.5;
            let ny = (lm_slice[i * 3 + 1] / LM_SIZE as f32) - 0.5;
            let lx = nx * roi_w;
            let ly = ny * roi_h;

            let orig_x = (center_x + lx * cos_r - ly * sin_r) / fw;
            let orig_y = (center_y + lx * sin_r + ly * cos_r) / fh;

            landmarks[i] = Landmark {
                x: orig_x.clamp(0.0, 1.0),
                y: orig_y.clamp(0.0, 1.0),
            };
        }

        Some(HandResult {
            landmarks,
            is_left_label,
        })
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

fn generate_ssd_anchors() -> Vec<Anchor> {
    let mut anchors = Vec::with_capacity(2016);
    let specs = [(24usize, 2usize), (12usize, 6usize)];
    for (grid, num_per_cell) in specs {
        for y in 0..grid {
            for x in 0..grid {
                let x_center = (x as f32 + 0.5) / grid as f32;
                let y_center = (y as f32 + 0.5) / grid as f32;
                for _ in 0..num_per_cell {
                    anchors.push(Anchor { x_center, y_center });
                }
            }
        }
    }
    anchors
}

fn prepare_letterbox_buffer(
    frame: &RgbImage,
    size: usize,
    nchw: bool,
) -> ([usize; 4], Vec<f32>, LetterboxInfo) {
    let shape = if nchw {
        [1, 3, size, size]
    } else {
        [1, size, size, 3]
    };
    let plane = size * size;
    let mut data = vec![0.0f32; 3 * plane]; // Padding hitam (0.0)

    let fw = frame.width() as f32;
    let fh = frame.height() as f32;
    let s = size as f32;

    let scale = (s / fw).min(s / fh);
    let new_w = ((fw * scale).round() as usize).clamp(1, size);
    let new_h = ((fh * scale).round() as usize).clamp(1, size);
    let pad_x = (size - new_w) / 2;
    let pad_y = (size - new_h) / 2;

    for y in 0..new_h {
        let sy = ((y as f32 / scale) as u32).min(frame.height() - 1);
        for x in 0..new_w {
            let sx = ((x as f32 / scale) as u32).min(frame.width() - 1);
            let p = frame.get_pixel(sx, sy);
            let r = p[0] as f32 / 255.0;
            let g = p[1] as f32 / 255.0;
            let b = p[2] as f32 / 255.0;

            let idx = (y + pad_y) * size + (x + pad_x);
            if nchw {
                data[idx] = r;
                data[plane + idx] = g;
                data[2 * plane + idx] = b;
            } else {
                let base = idx * 3;
                data[base] = r;
                data[base + 1] = g;
                data[base + 2] = b;
            }
        }
    }

    let lb = LetterboxInfo {
        pad_x: pad_x as f32 / s,
        pad_y: pad_y as f32 / s,
        scale_x: new_w as f32 / s,
        scale_y: new_h as f32 / s,
    };

    (shape, data, lb)
}

fn compute_iou(a: &PalmBox, b: &PalmBox) -> f32 {
    let a_x1 = a.cx - a.w * 0.5;
    let a_y1 = a.cy - a.h * 0.5;
    let a_x2 = a.cx + a.w * 0.5;
    let a_y2 = a.cy + a.h * 0.5;

    let b_x1 = b.cx - b.w * 0.5;
    let b_y1 = b.cy - b.h * 0.5;
    let b_x2 = b.cx + b.w * 0.5;
    let b_y2 = b.cy + b.h * 0.5;

    let inter_x1 = a_x1.max(b_x1);
    let inter_y1 = a_y1.max(b_y1);
    let inter_x2 = a_x2.min(b_x2);
    let inter_y2 = a_y2.min(b_y2);

    let inter_w = (inter_x2 - inter_x1).max(0.0);
    let inter_h = (inter_y2 - inter_y1).max(0.0);
    let inter_area = inter_w * inter_h;

    let area_a = a.w * a.h;
    let area_b = b.w * b.h;
    let union_area = area_a + area_b - inter_area;

    if union_area <= 0.0 {
        0.0
    } else {
        inter_area / union_area
    }
}

fn nms_palms(mut boxes: Vec<PalmBox>, iou_thresh: f32, max_keep: usize) -> Vec<PalmBox> {
    boxes.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    let mut kept: Vec<PalmBox> = Vec::new();
    for b in boxes {
        if kept.len() >= max_keep {
            break;
        }
        let overlap = kept.iter().any(|k| compute_iou(k, &b) > iou_thresh);
        if !overlap {
            kept.push(b);
        }
    }
    kept
}

// === FUNGSI GAMBAR OVERLAY RINGAN (PENGGANTI cv2.rectangle, cv2.circle, mp_draw) ===

pub fn draw_rect_outline(img: &mut RgbImage, x1: i32, y1: i32, x2: i32, y2: i32, color: Rgb<u8>) {
    draw_line(img, x1, y1, x2, y1, color);
    draw_line(img, x2, y1, x2, y2, color);
    draw_line(img, x2, y2, x1, y2, color);
    draw_line(img, x1, y2, x1, y1, color);
}

pub fn draw_filled_circle(img: &mut RgbImage, cx: i32, cy: i32, radius: i32, color: Rgb<u8>) {
    let w = img.width() as i32;
    let h = img.height() as i32;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= radius * radius {
                let px = cx + dx;
                let py = cy + dy;
                if px >= 0 && px < w && py >= 0 && py < h {
                    img.put_pixel(px as u32, py as u32, color);
                }
            }
        }
    }
}

pub fn draw_hand_skeleton(img: &mut RgbImage, lms: &[Landmark; 21]) {
    let w = img.width() as f32;
    let h = img.height() as f32;

    for &(a, b) in &HAND_CONNECTIONS {
        let x1 = (lms[a].x * w) as i32;
        let y1 = (lms[a].y * h) as i32;
        let x2 = (lms[b].x * w) as i32;
        let y2 = (lms[b].y * h) as i32;
        draw_line(img, x1, y1, x2, y2, Rgb([0, 220, 180]));
    }

    for lm in lms {
        let cx = (lm.x * w) as i32;
        let cy = (lm.y * h) as i32;
        draw_filled_circle(img, cx, cy, 3, Rgb([255, 80, 80]));
    }
}

fn draw_line(img: &mut RgbImage, mut x0: i32, mut y0: i32, x1: i32, y1: i32, color: Rgb<u8>) {
    let w = img.width() as i32;
    let h = img.height() as i32;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        if x0 >= 0 && x0 < w && y0 >= 0 && y0 < h {
            img.put_pixel(x0 as u32, y0 as u32, color);
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}