//! Vision pipeline for SenSÉ.
//!
//! Three small ONNX models + one tracking algorithm, glued together
//! to turn a stream of frames into events:
//!
//! - [`yolox::YoloX`] — YOLOX-Nano (Apache 2.0). 80 COCO classes, including
//!   all common domestic/wild animals. ~3.7 MB, ~30 ms/frame CPU at 640x640.
//! - [`yunet::YuNet`] — OpenCV Zoo (MIT). Face detection with 5 landmarks.
//!   137 KB, ~2 ms/frame. Lazy-loaded on the first `person` detection.
//! - [`track::ByteTracker`] — multi-object tracker (MIT). Associates
//!   detections across frames with stable `track_id`s, so the same
//!   dog stays "the same dog" instead of becoming a new event every frame.
//!
//! ## Why this stack
//!
//! YOLOv5/v8/v11 (Ultralytics) are AGPL-3.0 — incompatible with SenSÉ MIT.
//! YOLOv3 is the only MIT YOLO but is obsolete. YOLOX-Nano + YuNet + ByteTrack
//! is the smallest, cleanest Apache/MIT stack that covers the use case
//! (animal counting, presence detection, person + face ID) in < 5 MB total.
//!
//! ## Layout
//!
//! Models live next to the binary in `models/vision/<id>/model.onnx` plus a
//! `modele.json` manifest that mirrors SenSÉ's `Outils/Modeles/<racine>/` format.
//! They are downloaded on first use and cached. See [`manifest`].
//!
//! ## Endpoints (wired in `crate::build_router`)
//!
//! - `POST /v1/detect/frame` — single-frame detection. Returns detections
//!   with optional face landmarks. Tracking IDs are NOT stable across calls
//!   (use the video/stream endpoint for that — added in a follow-up).
//! - `POST /v1/detect/video` — multi-frame detection with ByteTrack
//!   active. Returns tracking events (`appeared`, `continued`, `disappeared`).
//!   (Implemented in a follow-up; the engine is ready.)

pub mod coco;
pub mod manifest;
pub mod track;
pub mod yolox;
pub mod yunet;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use self::yolox::YoloX;

/// What we return for one detected object. Box is in original image coords.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub class_id: usize,
    pub class_name: String,
    pub score: f32,
    /// Top-left corner, original image pixels.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Face detected inside a person box, with 5 landmarks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Face {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub score: f32,
    /// Right eye, left eye, nose, right mouth corner, left mouth corner.
    pub landmarks: [[f32; 2]; 5],
}

/// All detections + optional faces for one frame.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FrameResult {
    pub detections: Vec<Detection>,
    /// `detections[i]` is a person, and a face was found inside → here.
    pub faces: Vec<Option<Face>>,
    /// Wall-clock time spent in detection (excluding capture / encode).
    pub elapsed_ms: u64,
}

/// The vision engine, shared across HTTP requests.
///
/// Heavy models are loaded lazily and cached. Once loaded, inference is
/// ~30 ms per frame (YOLOX) + ~2 ms per person (YuNet). Keep this in
/// an `Arc<VisionEngine>` — it's cheap to clone.
pub struct VisionEngine {
    root: PathBuf,
    /// `None` until first call to `detect_frame` (YOLOX is the most-used
    /// model, we keep the engine snappy by not loading it at boot).
    yolox: Mutex<Option<Arc<YoloX>>>,
    /// Loaded on the first frame that contains at least one `person`.
    yunet: Mutex<Option<Arc<yunet::YuNet>>>,
}

impl VisionEngine {
    pub fn new(models_root: impl Into<PathBuf>) -> Self {
        Self {
            root: models_root.into(),
            yolox: Mutex::new(None),
            yunet: Mutex::new(None),
        }
    }

    /// Default model root: `<exe_dir>/models/vision`. Can be overridden
    /// with `VISION_MODELS_DIR` env var.
    pub fn with_default_root() -> Self {
        let root = std::env::var("VISION_MODELS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
                exe.parent().unwrap_or(Path::new(".")).join("models").join("vision")
            });
        Self::new(root)
    }

    /// Where to put a model next to the binary. The download helper
    /// creates the dir if missing.
    pub fn model_path(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// Run detection on a single frame. RGB layout, original size given
    /// for box scaling. Tracking is NOT applied here — use [`Self::track_video`]
    /// for that. We still call this for a single-frame API.
    pub fn detect_frame(
        self: &Arc<Self>,
        rgb: &[u8],
        width: u32,
        height: u32,
        detect_faces: bool,
    ) -> Result<FrameResult> {
        let start = std::time::Instant::now();

        let yolox = self.get_or_load_yolox()?;
        let dets = yolox.detect(rgb, width, height)?;

        // Face detection on persons, only if requested and only if a model is present.
        let mut faces: Vec<Option<Face>> = vec![None; dets.len()];
        if detect_faces && dets.iter().any(|d| d.class_id == coco::PERSON) {
            let yunet = self.get_or_load_yunet()?;
            for (i, d) in dets.iter().enumerate() {
                if d.class_id != coco::PERSON { continue; }
                // YuNet works best on a crop with some padding.
                let crop = crop_with_padding(rgb, width, height, d, 0.15);
                if let Ok(Some(face)) = yunet.detect(&crop.rgb, crop.width, crop.height) {
                    // Map face box back to original image coords.
                    let fx = face.x + crop.offset_x as f32;
                    let fy = face.y + crop.offset_y as f32;
                    let mut landmarks = face.landmarks;
                    for lm in &mut landmarks {
                        lm[0] += crop.offset_x as f32;
                        lm[1] += crop.offset_y as f32;
                    }
                    faces[i] = Some(Face {
                        x: fx, y: fy, width: face.width, height: face.height,
                        score: face.score, landmarks,
                    });
                }
            }
        }

        Ok(FrameResult {
            detections: dets,
            faces,
            elapsed_ms: start.elapsed().as_millis() as u64,
        })
    }

    fn get_or_load_yolox(self: &Arc<Self>) -> Result<Arc<YoloX>> {
        let mut guard = self.yolox.lock();
        if let Some(y) = guard.as_ref() {
            return Ok(Arc::clone(y));
        }
        let dir = self.model_path("yolox-nano");
        let onnx = dir.join("model.onnx");
        if !onnx.exists() {
            return Err(anyhow!(
                "YOLOX-Nano non trouvé ({}) — exécute `pc-agent download-models` \
                 ou copie le .onnx manuellement",
                onnx.display()
            ));
        }
        info!("yolox: chargement {}", onnx.display());
        let y = Arc::new(YoloX::load(&onnx).context("chargement yolox-nano")?);
        *guard = Some(Arc::clone(&y));
        Ok(y)
    }

    fn get_or_load_yunet(self: &Arc<Self>) -> Result<Arc<yunet::YuNet>> {
        let mut guard = self.yunet.lock();
        if let Some(y) = guard.as_ref() {
            return Ok(Arc::clone(y));
        }
        let dir = self.model_path("yunet");
        let onnx = dir.join("model.onnx");
        if !onnx.exists() {
            warn!("yunet: pas de .onnx dans {} — détection de visages désactivée", dir.display());
            return Err(anyhow!("yunet non installé"));
        }
        info!("yunet: chargement {}", onnx.display());
        let y = Arc::new(yunet::YuNet::load(&onnx).context("chargement yunet")?);
        *guard = Some(Arc::clone(&y));
        Ok(y)
    }
}

/// Crop a region of an RGB image with relative padding. Returns the crop
/// and its offset in the original image.
struct Crop {
    rgb: Vec<u8>,
    width: u32,
    height: u32,
    offset_x: u32,
    offset_y: u32,
}

fn crop_with_padding(rgb: &[u8], width: u32, height: u32, d: &Detection, pad: f32) -> Crop {
    let pad_w = (d.width * pad) as i32;
    let pad_h = (d.height * pad) as i32;
    let x0 = (d.x as i32 - pad_w).max(0) as u32;
    let y0 = (d.y as i32 - pad_h).max(0) as u32;
    let x1 = ((d.x + d.width) as i32 + pad_w).min(width as i32) as u32;
    let y1 = ((d.y + d.height) as i32 + pad_h).min(height as i32) as u32;
    let cw = x1 - x0;
    let ch = y1 - y0;

    let mut out = Vec::with_capacity((cw * ch * 3) as usize);
    for y in y0..y1 {
        let start = (y * width * 3 + x0 * 3) as usize;
        out.extend_from_slice(&rgb[start..start + (cw * 3) as usize]);
    }
    Crop { rgb: out, width: cw, height: ch, offset_x: x0, offset_y: y0 }
}
