//! YuNet (OpenCV Zoo, MIT) — face detection wrapper.
//!
//! Loads a YuNet ONNX model via `tract` and decodes its three outputs:
//!
//! - `cls`     (1, N, 2) — per-anchor classification [non-face, face]
//! - `bbox`    (1, N, 4) — per-anchor box as (x, y, w, h) in **input**
//!                         pixel coords (320×240 by default)
//! - `kps`     (1, N, 10) — 5 landmarks (right eye, left eye, nose,
//!                          right mouth corner, left mouth corner), each
//!                          as (x, y) in input pixel coords
//!
//! Where N is the total number of anchors across strides 8/16/32
//! (≈1580 for the standard YuNet 2023mar export).
//!
//! **Preprocessing**: YuNet was trained on BGR images, normalized to
//! `[-1, 1]` via `(pixel - 127.5) / 128`. We swap R and B in the input
//! buffer before feeding the model.
//!
//! **Reference**: <https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet>

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use tract_ndarray::Array4;
use tract_onnx::prelude::*;

pub const INPUT_W: u32 = 320;
pub const INPUT_H: u32 = 240;

/// One detected face. Landmark order: right eye, left eye, nose,
/// right mouth corner, left mouth corner.
#[derive(Debug, Clone)]
pub struct FaceDetection {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub score: f32,
    pub landmarks: [[f32; 2]; 5],
}

type YuNetRunnable = SimplePlan<TypedFact, Box<dyn TypedOp>, TypedModel>;

pub struct YuNet {
    model: YuNetRunnable,
}

impl YuNet {
    pub fn load(onnx_path: &Path) -> Result<Self> {
        let model = tract_onnx::onnx()
            .model_for_path(onnx_path)
            .with_context(|| format!("tract: chargement {}", onnx_path.display()))?
            .with_input_fact(
                0,
                InferenceFact::dt_shape(f32::datum_type(), tvec![1, 3, INPUT_H as usize, INPUT_W as usize]),
            )
            .context("tract: déclaration input fact")?
            .into_optimized()
            .context("tract: optimisation")?
            .into_runnable()
            .context("tract: into_runnable")?;
        Ok(Self { model })
    }

    /// Detect faces in an RGB image. Returns up to N faces after
    /// score filtering and class-agnostic NMS. Boxes are mapped back
    /// to the original image's coordinate space.
    pub fn detect(
        &self,
        rgb: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<FaceDetection>> {
        // 1) Preprocess: RGB → BGR, resize to 320×240, normalize to [-1, 1].
        let input: Array4<f32> = preprocess_bgr_normalized(rgb, width, height, INPUT_W, INPUT_H);

        // 2) Run the model.
        let result = self
            .model
            .run(tvec![input.into_tensor().into()])
            .context("tract: run")?;

        // 3) Pull the three outputs. The exact order (cls, bbox, kps) is
        //    the standard for the OpenCV Zoo 2023mar export. If a
        //    different export reverses them or has 4 outputs (loc, conf,
        //    iou, kps), the shape check below will fail loudly.
        if result.len() != 3 {
            return Err(anyhow!(
                "yunet: {} sorties attendues, 3 trouvées. Export ONNX non supporté \
                 (besoin cls/bbox/kps).",
                result.len()
            ));
        }
        let cls = result[0]
            .to_array_view::<f32>()
            .context("yunet: extract cls")?;
        let bbox = result[1]
            .to_array_view::<f32>()
            .context("yunet: extract bbox")?;
        let kps = result[2]
            .to_array_view::<f32>()
            .context("yunet: extract kps")?;

        let cls_shape = cls.shape().to_vec();
        if cls_shape.len() != 3 || cls_shape[0] != 1 || cls_shape[2] != 2 {
            return Err(anyhow!(
                "yunet: forme cls inattendue {:?} (attendue (1, N, 2))",
                cls_shape
            ));
        }
        let num_anchors = cls_shape[1];
        if bbox.shape() != [1, num_anchors, 4] || kps.shape() != [1, num_anchors, 10] {
            return Err(anyhow!(
                "yunet: formes incohérentes — cls {:?}, bbox {:?}, kps {:?}",
                cls.shape(),
                bbox.shape(),
                kps.shape()
            ));
        }

        // 4) Decode every anchor. Score threshold at 0.5 — YuNet is
        //    well-calibrated and below this confidence faces are usually
        //    noise.
        const SCORE_THRESH: f32 = 0.5;
        const NMS_IOU: f32 = 0.3;
        const MAX_KEPT: usize = 50;

        struct Cand {
            x: f32,
            y: f32,
            w: f32,
            h: f32,
            score: f32,
            landmarks: [[f32; 2]; 5],
        }
        let mut cands: Vec<Cand> = Vec::new();
        for i in 0..num_anchors {
            let face_score = cls[[0, i, 1]];
            if face_score < SCORE_THRESH {
                continue;
            }
            let x = bbox[[0, i, 0]];
            let y = bbox[[0, i, 1]];
            let w = bbox[[0, i, 2]];
            let h = bbox[[0, i, 3]];

            let mut landmarks = [[0.0f32; 2]; 5];
            for k in 0..5 {
                landmarks[k] = [kps[[0, i, k * 2]], kps[[0, i, k * 2 + 1]]];
            }
            cands.push(Cand { x, y, w, h, score: face_score, landmarks });
        }

        // 5) Class-agnostic NMS.
        cands.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        let mut kept: Vec<Cand> = Vec::new();
        for c in cands {
            if kept.len() >= MAX_KEPT {
                break;
            }
            let mut overlaps = false;
            for k in &kept {
                if iou_xywh(c.x, c.y, c.w, c.h, k.x, k.y, k.w, k.h) > NMS_IOU {
                    overlaps = true;
                    break;
                }
            }
            if overlaps {
                continue;
            }
            kept.push(c);
        }

        // 6) Map boxes from 320×240 to original image coords.
        let sx = width as f32 / INPUT_W as f32;
        let sy = height as f32 / INPUT_H as f32;
        let out: Vec<FaceDetection> = kept
            .into_iter()
            .map(|c| FaceDetection {
                x: c.x * sx,
                y: c.y * sy,
                width: c.w * sx,
                height: c.h * sy,
                score: c.score,
                landmarks: c.landmarks.map(|lm| [lm[0] * sx, lm[1] * sy]),
            })
            .collect();
        Ok(out)
    }
}

/// RGB → BGR, resize to `target_w × target_h` (bilinear), normalize
/// to `[-1, 1]` via `(pixel - 127.5) / 128.0`. Returns an NCHW f32
/// tensor of shape (1, 3, target_h, target_w).
fn preprocess_bgr_normalized(
    rgb: &[u8],
    width: u32,
    height: u32,
    target_w: u32,
    target_h: u32,
) -> Array4<f32> {
    // Build an RGB image from the raw buffer, resize with the `image`
    // crate, then re-iterate pixel-by-pixel to swap R/B and normalize.
    let src = image::RgbImage::from_raw(width, height, rgb.to_vec())
        .expect("rgb buffer must be w*h*3 bytes");
    let resized = image::imageops::resize(
        &src,
        target_w,
        target_h,
        image::imageops::FilterType::Triangle,
    );
    let raw = resized.into_raw(); // RGB
    let plane = (target_w as usize) * (target_h as usize);
    let mut b = Vec::with_capacity(plane);
    let mut g = Vec::with_capacity(plane);
    let mut r = Vec::with_capacity(plane);
    for px in raw.chunks_exact(3) {
        // raw is RGB; we want BGR.
        b.push((px[2] as f32 - 127.5) / 128.0);
        g.push((px[1] as f32 - 127.5) / 128.0);
        r.push((px[0] as f32 - 127.5) / 128.0);
    }
    let mut data = Vec::with_capacity(plane * 3);
    data.extend(b);
    data.extend(g);
    data.extend(r);
    Array4::from_shape_vec((1, 3, target_h as usize, target_w as usize), data)
        .expect("shape (1,3,H,W) fits buffer")
}

fn iou_xywh(ax: f32, ay: f32, aw: f32, ah: f32, bx: f32, by: f32, bw: f32, bh: f32) -> f32 {
    let ax2 = ax + aw;
    let ay2 = ay + ah;
    let bx2 = bx + bw;
    let by2 = by + bh;
    let ix0 = ax.max(bx);
    let iy0 = ay.max(by);
    let ix1 = ax2.min(bx2);
    let iy1 = ay2.min(by2);
    let iw = (ix1 - ix0).max(0.0);
    let ih = (iy1 - iy0).max(0.0);
    let inter = iw * ih;
    if inter <= 0.0 {
        return 0.0;
    }
    let area_a = aw * ah;
    let area_b = bw * bh;
    inter / (area_a + area_b - inter)
}
