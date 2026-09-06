//! HTTP handlers for the UI Automation (UIA) endpoints.
//!
//! These are wrapped versions of the `uia` module functions. The 5 routes
//! let the SenSÉ assistant find, identify, and act on Windows desktop
//! controls without going through the mouse/keyboard.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::handlers::check_auth;
use crate::uia;
use crate::AppState;

// ---------------------------------------------------------------------------
// GET /v1/uia/dump
// ---------------------------------------------------------------------------

pub async fn uia_dump(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    match uia::dump_active_window() {
        Ok(v) => (StatusCode::OK, Json(json!({ "ok": true, "data": v }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/uia/invoke
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct InvokeBody {
    pub automation_id: String,
}

pub async fn uia_invoke(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<InvokeBody>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    match uia::invoke_by_id(&body.automation_id) {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/uia/set_text
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SetTextBody {
    pub automation_id: String,
    pub value: String,
}

pub async fn uia_set_text(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SetTextBody>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    match uia::set_text_by_id(&body.automation_id, &body.value) {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/uia/select
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SelectBody {
    pub automation_id: String,
    pub value: String,
}

pub async fn uia_select(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SelectBody>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    match uia::select_by_id(&body.automation_id, &body.value) {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/uia/press
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct PressBody {
    pub keys: String,
}

pub async fn uia_press(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PressBody>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    match uia::press_keys(&body.keys) {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e })),
        )
            .into_response(),
    }
}

// Note for the request body extractor: axum 0.7 requires `Json` in the
// signature for POST endpoints that expect a body. We import it above.
const _: fn() = || {
    fn assert_json<T: serde::de::DeserializeOwned>() {}
    assert_json::<InvokeBody>();
    assert_json::<SetTextBody>();
    assert_json::<SelectBody>();
    assert_json::<PressBody>();
    // Touch the import so the compiler doesn't drop it if all callers
    // change their patterns.
    let _: Option<Json<Value>> = None;
};
