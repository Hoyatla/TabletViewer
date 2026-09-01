//! HTTP handlers for the debug agent.

use std::path::PathBuf;

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde::{Deserialize, Serialize};

use crate::capture::ScreenshotError;
use crate::proc::ProcessInfo;
use crate::AppState;

// ----------------------------------------------------------------------------
// Common
// ----------------------------------------------------------------------------

/// Returns 401 Unauthorized if the configured token doesn't match.
/// If no token is configured, always returns Ok.
pub fn check_auth(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), Response> {
    let Some(expected) = state.token.as_ref() else {
        return Ok(());
    };
    let supplied = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    match supplied {
        Some(t) if t == expected.as_str() => Ok(()),
        _ => Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "invalid or missing token" })),
        )
            .into_response()),
    }
}

fn path_allowed(state: &AppState, path: &PathBuf) -> bool {
    if state.allowed_roots.is_empty() {
        return true;
    }
    let canonical = match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(_) => return false,
    };
    state
        .allowed_roots
        .iter()
        .any(|root| canonical.starts_with(root))
}

// ----------------------------------------------------------------------------
// GET /v1/ping
// ----------------------------------------------------------------------------

pub async fn ping() -> &'static str {
    "pong"
}

// ----------------------------------------------------------------------------
// GET /v1/system
// ----------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SystemInfo {
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub kernel: Option<String>,
    pub os_version: Option<String>,
    pub cpu_brand: String,
    pub cpu_count: usize,
    pub mem_total_kb: u64,
    pub mem_avail_kb: u64,
    pub swap_total_kb: u64,
    pub swap_free_kb: u64,
    pub uptime_s: u64,
    pub boot_time_s: u64,
    pub loadavg: [f64; 3],
}

pub async fn system(
    State(state): State<AppState>,
) -> Json<SystemInfo> {
    let mut sys = state.sys.lock();
    sys.refresh_all();

    // Extract values from the lock guard BEFORE returning, because the
    // lock guard's lifetime is tied to this scope and we want to
    // return data that doesn't depend on the guard.
    let loadavg = sysinfo::System::load_average();
    let hostname = sysinfo::System::host_name().unwrap_or_default();
    let kernel = sysinfo::System::kernel_version();
    let os_version = sysinfo::System::os_version();
    let cpu_brand = sys
        .cpus()
        .first()
        .map(|c| c.brand().to_string())
        .unwrap_or_default();
    let cpu_count = sys.cpus().len();
    let mem_total_kb = sys.total_memory();
    let mem_avail_kb = sys.available_memory();
    let swap_total_kb = sys.total_swap();
    let swap_free_kb = sys.free_swap();
    let uptime_s = sysinfo::System::uptime();
    let boot_time_s = sysinfo::System::boot_time();

    Json(SystemInfo {
        hostname,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        kernel,
        os_version,
        cpu_brand,
        cpu_count,
        mem_total_kb,
        mem_avail_kb,
        swap_total_kb,
        swap_free_kb,
        uptime_s,
        boot_time_s,
        loadavg: [loadavg.one, loadavg.five, loadavg.fifteen],
    })
}

// ----------------------------------------------------------------------------
// GET /v1/screenshot
// ----------------------------------------------------------------------------

pub async fn screenshot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }

    let cap = state.capturer.clone();
    // Screenshots may take ~50ms; offload from the runtime worker pool.
    let img = match tokio::task::spawn_blocking(move || cap.grab_primary()).await {
        Ok(Ok(img)) => img,
        Ok(Err(ScreenshotError::NoDisplay)) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "no display available (headless?)"})),
            )
                .into_response();
        }
        Ok(Err(e)) => {
            tracing::error!("screenshot error: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("screenshot task join error: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "screenshot task failed"})),
            )
                .into_response();
        }
    };

    let (w, h) = (img.width(), img.height());

    // Encode to PNG into a Cursor<Vec<u8>> because image::write_to needs
    // a Write + Seek. After encoding, take the inner Vec.
    let mut cursor = std::io::Cursor::new(Vec::with_capacity((w as usize) * (h as usize)));
    if let Err(e) = img.write_to(&mut cursor, image::ImageOutputFormat::Png) {
        tracing::error!("png encode error: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("png encode failed: {e}")})),
        )
            .into_response();
    }
    let buf = cursor.into_inner();
    tracing::debug!("screenshot: {}x{} {} bytes", w, h, buf.len());

    let mut resp = Response::new(Body::from(buf));
    *resp.status_mut() = StatusCode::OK;
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/png"),
    );
    resp.headers_mut().insert("X-Screenshot-Width", HeaderValue::from(w));
    resp.headers_mut().insert("X-Screenshot-Height", HeaderValue::from(h));
    resp
}

// ----------------------------------------------------------------------------
// GET /v1/processes?top=N&sort=cpu|mem
// ----------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ProcessesQuery {
    pub top: Option<usize>,
    pub sort: Option<String>, // "cpu" or "mem"
}

pub async fn processes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ProcessesQuery>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    let mut sys = state.sys.lock();
    sys.refresh_processes();
    let infos = ProcessInfo::collect(&sys);

    let sort = q.sort.as_deref().unwrap_or("cpu");
    let mut sorted = infos;
    match sort {
        "mem" => sorted.sort_by(|a, b| b.mem_kb.cmp(&a.mem_kb)),
        _ => sorted.sort_by(|a, b| b.cpu_pct.partial_cmp(&a.cpu_pct).unwrap_or(std::cmp::Ordering::Equal)),
    }
    let n = q.top.unwrap_or(20).min(sorted.len());
    let truncated: Vec<ProcessInfo> = sorted.into_iter().take(n).collect();

    (StatusCode::OK, Json(truncated)).into_response()
}

// ----------------------------------------------------------------------------
// GET /v1/log?path=X&tail=N
// ----------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct LogQuery {
    pub path: String,
    pub tail: Option<usize>,
}

#[derive(Serialize)]
pub struct LogResponse {
    pub path: String,
    pub lines: Vec<String>,
    pub truncated: bool,
}

pub async fn log_tail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<LogQuery>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    let path = PathBuf::from(&q.path);
    if !path_allowed(&state, &path) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "path not in allowed roots"})),
        )
            .into_response();
    }

    let n = q.tail.unwrap_or(100);
    let (lines, truncated) = match tail_file(&path, n) {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };
    (
        StatusCode::OK,
        Json(LogResponse {
            path: q.path,
            lines,
            truncated,
        }),
    )
        .into_response()
}

fn tail_file(path: &std::path::Path, n: usize) -> std::io::Result<(Vec<String>, bool)> {
    use std::io::{BufReader, Read};
    let f = std::fs::File::open(path)?;
    let mut reader = BufReader::new(f);
    let mut buf = String::new();
    reader.read_to_string(&mut buf)?;
    let all: Vec<&str> = buf.lines().collect();
    let truncated = all.len() > n;
    let start = all.len().saturating_sub(n);
    let lines: Vec<String> = all[start..].iter().map(|s| s.to_string()).collect();
    Ok((lines, truncated))
}

// ----------------------------------------------------------------------------
// GET /v1/file?path=X
// ----------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct FileQuery {
    pub path: String,
}

pub async fn file_read(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<FileQuery>,
) -> Response {
    if let Err(r) = check_auth(&state, &headers) {
        return r;
    }
    let path = PathBuf::from(&q.path);
    if !path_allowed(&state, &path) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "path not in allowed roots"})),
        )
            .into_response();
    }
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };
    let mut resp = Response::new(Body::from(data));
    *resp.status_mut() = StatusCode::OK;
    // Best-guess content type. The client can override.
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    resp
}
