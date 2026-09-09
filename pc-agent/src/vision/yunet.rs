//! YuNet (OpenCV Zoo, MIT) — face detection wrapper.
//!
//! **Status: scaffold only.** The model loads via tract, but the
//! multi-stride decode is non-trivial (YuNet emits 3 tensors with
//! anchor-free, multi-stride regression and 5 keypoints per detection).
//! Decoding is on the follow-up:
//!
//! - https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet
//!
//! For now, [`YuNet::detect`] returns `Ok(None)` so callers see a
//! well-typed "no face" answer and the rest of the engine (YOLOX +
//! ByteTrack) keeps working. The HTTP handler will surface this as
//! `faces: [null, null, ...]` for any `person` detection in the frame.

use std::path::Path;

use anyhow::{Context, Result};
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
    // The runnable is loaded eagerly so that `detect()` stays infallible
    // apart from the actual decode. We keep it private and unused until
    // the multi-stride decode lands.
    _model: YuNetRunnable,
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
        Ok(Self { _model: model })
    }

    /// Detect faces in an RGB image. Returns `Ok(None)` until the
    /// multi-stride decode is implemented. See module docs.
    pub fn detect(&self, _rgb: &[u8], _width: u32, _height: u32) -> Result<Option<FaceDetection>> {
        Ok(None)
    }
}
