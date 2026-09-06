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
// POST /v1/uia/screenshot-window
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ScreenshotWindowBody {
    pub title: String,
}

pub async fn uia_screenshot_window(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ScreenshotWindowBody>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    match uia::screenshot_window_by_title(&body.title) {
        Ok(v) => (StatusCode::OK, Json(json!({ "ok": true, "data": v }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/uia/list-windows
// ---------------------------------------------------------------------------

pub async fn uia_list_windows(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    match uia::list_visible_windows() {
        Ok(windows_list) => {
            // Serialize as JSON array of {hwnd, title} objects.
            let arr: Vec<Value> = windows_list
                .into_iter()
                .map(|(hwnd, title)| json!({ "hwnd": hwnd.0 as u64, "title": title }))
                .collect();
            (
                StatusCode::OK,
                Json(json!({ "ok": true, "data": { "windows": arr, "count": arr.len() } })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/uia/dump-window
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct DumpWindowBody {
    pub title: String,
}

pub async fn uia_dump_window(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DumpWindowBody>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    match uia::dump_window_by_title(&body.title) {
        Ok(v) => (StatusCode::OK, Json(json!({ "ok": true, "data": v }))).into_response(),
        Err(e) => {
            // 404 when the window doesn't exist, 500 for everything else.
            let status = if e.starts_with("fenetre non trouvee")
                || e.starts_with("no window with title")
            {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(json!({ "ok": false, "error": e }))).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// POST /v1/uia/focus-window
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct FocusWindowBody {
    /// HWND as decimal or hex (`"0xABCD"`). We accept both for ergonomic
    /// reasons — callers usually copy hex from dump, the assistant often
    /// copies the decimal from the JSON output.
    pub hwnd: String,
}

fn parse_hwnd(s: &str) -> Result<windows::Win32::Foundation::HWND, String> {
    let s = s.trim();
    let parsed = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|e| format!("bad hex hwnd: {e}"))
    } else {
        s.parse::<u64>().map_err(|e| format!("bad decimal hwnd: {e}"))
    }?;
    Ok(windows::Win32::Foundation::HWND(parsed as *mut _))
}

pub async fn uia_focus_window(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<FocusWindowBody>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    let hwnd = match parse_hwnd(&body.hwnd) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "error": e })),
            )
                .into_response()
        }
    };
    match uia::focus_window(hwnd) {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/uia/find-main-edit
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct FindMainEditBody {
    /// Optional. If present, dump the named window and pick the best
    /// candidate. If absent, dump the foreground window.
    pub title: Option<String>,
}

pub async fn uia_find_main_edit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<FindMainEditBody>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    let automation = match uia::get_automation() {
        Ok(a) => a,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "ok": false, "error": e })),
            )
                .into_response()
        }
    };
    // Resolve the root element: from title if given, else foreground/root.
    let root: windows::Win32::UI::Accessibility::IUIAutomationElement = if let Some(title) = body.title.as_deref() {
        let hwnd_res = uia::find_hwnd_by_title(title);
        let hwnd = match hwnd_res {
            Ok(h) => h,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "ok": false, "error": e })),
                )
                    .into_response();
            }
        };
        match unsafe { automation.ElementFromHandle(hwnd) } {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "ok": false, "error": format!("ElementFromHandle({title}): {e}") })),
                )
                    .into_response();
            }
        }
    } else {
        match unsafe { automation.GetFocusedElement() }
            .or_else(|_| unsafe { automation.GetRootElement() })
        {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "ok": false, "error": format!("GetFocused/Root: {e}") })),
                )
                    .into_response();
            }
        }
    };
    match uia::find_main_edit(&automation, &root) {
        Ok(Some(v)) => (StatusCode::OK, Json(json!({ "ok": true, "data": v }))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": "no Document/Pane/Edit candidate found" })),
        )
            .into_response(),
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
    assert_json::<ScreenshotWindowBody>();
    assert_json::<DumpWindowBody>();
    // Touch the import so the compiler doesn't drop it if all callers
    // change their patterns.
    let _: Option<Json<Value>> = None;
};
