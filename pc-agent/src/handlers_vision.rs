//! HTTP handlers for the vision pipeline.
//!
//! Endpoints (mounted in `crate::build_router`):
//!
//! - `POST /v1/detect/frame` — single-frame object detection (YOLOX-Nano)
//!   with optional face detection (YuNet) on detected persons.
//!   Body: `{ "image_b64": "...", "detect_faces": bool, "classes_filter": [str]? }`.
//!   Response: [`FrameResponse`].

use std::collections::HashSet;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::handlers::check_auth;
use crate::vision::{Detection, Face, FrameResult};
use crate::AppState;

// ----------------------------------------------------------------------------
// POST /v1/detect/frame
// ----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DetectFrameBody {
    /// Base64 of the image (any format `image` crate can decode:
    /// PNG, JPEG, BMP, WebP, GIF first frame, ...).
    pub image_b64: String,
    /// Run YuNet on each detected person. Default: false.
    #[serde(default)]
    pub detect_faces: bool,
    /// Optional whitelist of COCO class names. Default: all 80.
    /// Example: `["cat", "dog", "bird"]` to limit the response size.
    #[serde(default)]
    pub classes_filter: Option<Vec<String>>,
    /// Override YOLOX confidence threshold (0.0–1.0). Default: 0.30.
    #[serde(default)]
    pub conf: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct FrameResponse {
    pub image_width: u32,
    pub image_height: u32,
    pub detections: Vec<DetectionOut>,
    pub faces: Vec<Option<FaceOut>>,
    pub elapsed_ms: u64,
    pub elapsed_capture_ms: u64,
    pub elapsed_decode_ms: u64,
    pub elapsed_model_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct DetectionOut {
    pub class_id: usize,
    pub class_name: String,
    pub score: f32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl From<&Detection> for DetectionOut {
    fn from(d: &Detection) -> Self {
        Self {
            class_id: d.class_id,
            class_name: d.class_name.clone(),
            score: d.score,
            x: d.x, y: d.y, width: d.width, height: d.height,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FaceOut {
    pub x: f32, pub y: f32, pub width: f32, pub height: f32,
    pub score: f32,
    pub landmarks: [[f32; 2]; 5],
}

impl From<&Face> for FaceOut {
    fn from(f: &Face) -> Self {
        Self {
            x: f.x, y: f.y, width: f.width, height: f.height,
            score: f.score, landmarks: f.landmarks,
        }
    }
}

pub async fn detect_frame(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DetectFrameBody>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    let total_start = std::time::Instant::now();

    // 1) Decode base64 → image.
    let decode_start = std::time::Instant::now();
    let bytes = match B64.decode(body.image_b64.as_bytes()) {
        Ok(b) => b,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, format!("image_b64 invalide: {e}")),
    };
    let img = match image::load_from_memory(&bytes) {
        Ok(i) => i.to_rgb8(),
        Err(e) => return error_response(StatusCode::BAD_REQUEST, format!("image illisible: {e}")),
    };
    let (w, h) = (img.width(), img.height());
    let rgb = img.into_raw();
    let decode_ms = decode_start.elapsed().as_millis() as u64;

    // 2) Build class filter.
    let allowed: Option<HashSet<usize>> = body.classes_filter.as_ref().map(|names| {
        let mut s = HashSet::new();
        for n in names {
            if let Some((id, _)) = crate::vision::coco::CLASSES
                .iter().enumerate().find(|(_, c)| **c == n.as_str())
            {
                s.insert(id);
            } else {
                warn!("classes_filter: classe inconnue « {} »", n);
            }
        }
        s
    });

    // 3) Run YOLOX (and optionally YuNet) through the engine.
    let model_start = std::time::Instant::now();
    let result: FrameResult = match state.vision.detect_frame(&rgb, w, h, body.detect_faces) {
        Ok(r) => r,
        Err(e) => {
            // Surface the most useful "model not found" errors as 503,
            // other errors as 500.
            let msg = format!("{e:#}");
            let status = if msg.contains("non trouvé") || msg.contains("not installed") {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            return error_response(status, msg);
        }
    };
    let model_ms = model_start.elapsed().as_millis() as u64;

    // 4) Filter by class whitelist if requested.
    let (dets_out, faces_out) = match allowed {
        None => {
            let d: Vec<DetectionOut> = result.detections.iter().map(DetectionOut::from).collect();
            let f: Vec<Option<FaceOut>> = result.faces.iter().map(|o| o.as_ref().map(FaceOut::from)).collect();
            (d, f)
        }
        Some(allowed) => {
            let mut d = Vec::new();
            let mut f = Vec::new();
            for (i, det) in result.detections.iter().enumerate() {
                if allowed.contains(&det.class_id) {
                    d.push(DetectionOut::from(det));
                    f.push(result.faces[i].as_ref().map(FaceOut::from));
                }
            }
            (d, f)
        }
    };

    let resp = FrameResponse {
        image_width: w,
        image_height: h,
        detections: dets_out,
        faces: faces_out,
        elapsed_ms: result.elapsed_ms,
        elapsed_capture_ms: 0,
        elapsed_decode_ms: decode_ms,
        elapsed_model_ms: model_ms,
    };
    let total_ms = total_start.elapsed().as_millis() as u64;
    info!(
        "detect_frame: {}x{}, {} détections, {}/{} ms (total {})",
        w, h, resp.detections.len(), model_ms, total_ms, total_ms
    );
    (StatusCode::OK, Json(resp)).into_response()
}

fn error_response(status: StatusCode, msg: String) -> Response {
    (status, Json(serde_json::json!({ "error": msg }))).into_response()
}
