//! YOLOX-Nano wrapper (Apache 2.0).
//!
//! Loads a YOLOX-Nano ONNX model via `tract` (pure-Rust ONNX runtime,
//! chosen because ort ships no prebuilt for the gnu toolchain). The
//! model takes a 640x640 letterboxed RGB tensor (NCHW, float32,
//! normalized to [0,1]) and emits an `output` tensor shaped
//! `(1, 8400, 85)` where the 85 columns are
//! `[cx, cy, w, h, obj_conf, class0_conf, ..., class79_conf]`.
//!
//! We do the standard YOLOX preprocessing (letterbox, no mean/std
//! subtraction), sigmoid on the confidences, top-K + class-agnostic NMS,
//! and un-letterbox back to original image coordinates.

use std::path::Path;

use anyhow::{Context, Result};
use tract_ndarray::Array4;
use tract_onnx::prelude::*;

use super::coco;
use super::Detection;

const INPUT_SIZE: u32 = 640;

/// Default confidence threshold. YOLOX-Nano is well-calibrated; 0.3 is
/// the standard value from the official demo. We expose it in
/// `detect_with_conf` for callers that want to tweak.
const DEFAULT_CONF: f32 = 0.30;

/// Class-agnostic NMS IoU threshold. Boxes sharing more than this fraction
/// of their area are merged.
const NMS_IOU: f32 = 0.45;

/// Max boxes kept after NMS. 300 is plenty for any single 640x640 frame.
const MAX_DETECTIONS: usize = 300;

/// The tract ONNX runnable. We type-erase via `Box<dyn TypedOp>` to
/// avoid the deeply nested type signature of a `SimplePlan` in a struct
/// field.
type YoloXRunnable = SimplePlan<TypedFact, Box<dyn TypedOp>, TypedModel>;

pub struct YoloX {
    model: YoloXRunnable,
}

impl YoloX {
    pub fn load(onnx_path: &Path) -> Result<Self> {
        let model = tract_onnx::onnx()
            .model_for_path(onnx_path)
            .with_context(|| format!("tract: chargement {}", onnx_path.display()))?
            // YOLOX-Nano takes (1, 3, 640, 640) float32.
            .with_input_fact(
                0,
                InferenceFact::dt_shape(f32::datum_type(), tvec![1, 3, INPUT_SIZE as usize, INPUT_SIZE as usize]),
            )
            .context("tract: déclaration input fact")?
            .into_optimized()
            .context("tract: optimisation")?
            .into_runnable()
            .context("tract: into_runnable")?;
        Ok(Self { model })
    }

    /// Detect objects in an RGB image. Returns boxes in original image
    /// coordinates, filtered by `DEFAULT_CONF` and NMS.
    pub fn detect(&self, rgb: &[u8], width: u32, height: u32) -> Result<Vec<Detection>> {
        self.detect_with_conf(rgb, width, height, DEFAULT_CONF)
    }

    /// Same as [`Self::detect`] but with a caller-controlled confidence threshold.
    pub fn detect_with_conf(
        &self,
        rgb: &[u8],
        width: u32,
        height: u32,
        conf: f32,
    ) -> Result<Vec<Detection>> {
        // 1) Letterbox: scale so the longest side is 640, pad the rest.
        let (letter, scale, pad_x, pad_y) = letterbox(rgb, width, height, INPUT_SIZE);

        // 2) HWC u8 [0,255] -> CHW f32 [0,1].
        let input: Array4<f32> = hwc_u8_to_chw_f32(&letter, INPUT_SIZE, INPUT_SIZE);

        // 3) Run the model.
        let result = self
            .model
            .run(tvec![input.into_tensor().into()])
            .context("tract: run")?;

        // 4) Extract the (1, 8400, 85) output. YOLOX-Nano has one output.
        let output_view = result[0]
            .to_array_view::<f32>()
            .context("tract: extract output f32 view")?;
        let shape = output_view.shape().to_vec();
        if shape.len() != 3 || shape[0] != 1 || shape[2] != 85 {
            anyhow::bail!(
                "tract: forme de sortie YOLOX inattendue {:?} (attendue (1, 8400, 85))",
                shape
            );
        }
        let num_anchors = shape[1];

        // 5) Decode per-anchor: cx,cy,w,h,obj + 80 class confidences.
        let mut candidates: Vec<(f32, Detection)> = Vec::with_capacity(num_anchors);
        for anchor_idx in 0..num_anchors {
            let row = output_view.slice(tract_ndarray::s![0, anchor_idx, ..]);
            let a = row.as_slice().context("tract: anchor row as_slice")?;
            let cx = a[0];
            let cy = a[1];
            let w = a[2];
            let h = a[3];
            let obj = sigmoid(a[4]);
            if obj < 0.001 {
                continue;
            }
            // Best class.
            let mut best_cls = 0usize;
            let mut best_score = 0f32;
            for c in 0..80 {
                let s = sigmoid(a[5 + c]);
                if s > best_score {
                    best_score = s;
                    best_cls = c;
                }
            }
            let score = obj * best_score;
            if score < conf {
                continue;
            }
            // Convert from letterboxed coords back to original image coords.
            let x0 = ((cx - w / 2.0) - pad_x) / scale;
            let y0 = ((cy - h / 2.0) - pad_y) / scale;
            let bw = w / scale;
            let bh = h / scale;
            candidates.push((
                score,
                Detection {
                    class_id: best_cls,
                    class_name: coco::name(best_cls).to_string(),
                    score,
                    x: x0.max(0.0),
                    y: y0.max(0.0),
                    width: bw,
                    height: bh,
                },
            ));
        }

        // 6) Class-agnostic NMS.
        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut kept: Vec<Detection> = Vec::with_capacity(MAX_DETECTIONS);
        for (_, det) in candidates {
            if kept.len() >= MAX_DETECTIONS {
                break;
            }
            if overlaps_existing(&det, &kept) {
                continue;
            }
            kept.push(det);
        }
        Ok(kept)
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

fn overlaps_existing(det: &Detection, kept: &[Detection]) -> bool {
    for k in kept {
        if k.class_id != det.class_id {
            continue;
        }
        let iou = iou(det, k);
        if iou > NMS_IOU {
            return true;
        }
    }
    false
}

fn iou(a: &Detection, b: &Detection) -> f32 {
    let ax2 = a.x + a.width;
    let ay2 = a.y + a.height;
    let bx2 = b.x + b.width;
    let by2 = b.y + b.height;
    let inter_x0 = a.x.max(b.x);
    let inter_y0 = a.y.max(b.y);
    let inter_x1 = ax2.min(bx2);
    let inter_y1 = ay2.min(by2);
    let iw = (inter_x1 - inter_x0).max(0.0);
    let ih = (inter_y1 - inter_y0).max(0.0);
    let inter = iw * ih;
    if inter <= 0.0 {
        return 0.0;
    }
    let area_a = a.width * a.height;
    let area_b = b.width * b.height;
    inter / (area_a + area_b - inter)
}

/// Letterbox: scale image to fit in `target` and pad with zeros (114, 114, 114)
/// per YOLOX convention. Returns (rgb, scale, pad_x, pad_y).
fn letterbox(rgb: &[u8], w: u32, h: u32, target: u32) -> (Vec<u8>, f32, f32, f32) {
    let scale = (target as f32 / w as f32).min(target as f32 / h as f32);
    let new_w = (w as f32 * scale).round() as u32;
    let new_h = (h as f32 * scale).round() as u32;
    let pad_x = (target as f32 - new_w as f32) / 2.0;
    let pad_y = (target as f32 - new_h as f32) / 2.0;

    let src = image::RgbImage::from_raw(w, h, rgb.to_vec())
        .expect("rgb buffer must be w*h*3 bytes");
    let resized = image::imageops::resize(&src, new_w, new_h, image::imageops::FilterType::Triangle);

    let mut canvas = vec![114u8; (target * target * 3) as usize];
    let x_off = pad_x as u32;
    let y_off = pad_y as u32;
    for y in 0..new_h {
        let src_start = (y * new_w * 3) as usize;
        let dst_start = ((y + y_off) * target * 3 + x_off * 3) as usize;
        canvas[dst_start..dst_start + (new_w * 3) as usize]
            .copy_from_slice(&resized.as_raw()[src_start..src_start + (new_w * 3) as usize]);
    }
    (canvas, scale, pad_x, pad_y)
}

/// HWC u8 [0,255] -> CHW f32 [0,1] tensor of shape (1, 3, H, W).
fn hwc_u8_to_chw_f32(rgb: &[u8], w: u32, h: u32) -> Array4<f32> {
    let plane = (w as usize) * (h as usize);
    let mut r = Vec::with_capacity(plane);
    let mut g = Vec::with_capacity(plane);
    let mut b = Vec::with_capacity(plane);
    for px in rgb.chunks_exact(3) {
        r.push(px[0] as f32 / 255.0);
        g.push(px[1] as f32 / 255.0);
        b.push(px[2] as f32 / 255.0);
    }
    let mut data = Vec::with_capacity(plane * 3);
    data.extend(r);
    data.extend(g);
    data.extend(b);
    Array4::from_shape_vec((1, 3, h as usize, w as usize), data)
        .expect("shape (1,3,H,W) fits buffer")
}
