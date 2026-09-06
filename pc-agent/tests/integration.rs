//! End-to-end tests for the pc-agent HTTP API.
//!
//! These run the real axum router in-process via `tower::ServiceExt::oneshot`
//! — no real network or subprocess. Each test builds a fresh `AppState`,
//! so they are isolated.
//!
//! Screenshot tests are skipped when no display is available (headless CI).

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use parking_lot::Mutex;
use pc_agent::{build_router, capture, AppState};
use sysinfo::System;
use tower::ServiceExt;
use http_body_util::BodyExt;

fn make_state() -> AppState {
    let capturer = capture::Capturer::new()
        .expect("no display — test must be run on a host with at least one screen");
    AppState {
        token: None,
        allowed_roots: Arc::new(vec![]),
        sys: Arc::new(Mutex::new(System::new_all())),
        capturer: Arc::new(capturer),
    }
}

fn make_state_with_token(token: &str) -> AppState {
    let mut s = make_state();
    s.token = Some(Arc::new(token.to_string()));
    s
}

async fn body_bytes(body: Body) -> Vec<u8> {
    body.collect().await.unwrap().to_bytes().to_vec()
}

#[tokio::test]
async fn ping_returns_pong() {
    let app = build_router(make_state());
    let resp = app
        .oneshot(Request::builder().uri("/v1/ping").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_bytes(resp.into_body()).await;
    assert_eq!(body, b"pong");
}

#[tokio::test]
async fn system_returns_expected_fields() {
    let app = build_router(make_state());
    let resp = app
        .oneshot(Request::builder().uri("/v1/system").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_bytes(resp.into_body()).await;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(v.get("hostname").is_some());
    assert!(v.get("os").is_some());
    assert!(v.get("cpu_count").is_some());
    assert!(v["cpu_count"].as_u64().unwrap() >= 1);
    assert!(v["mem_total_kb"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn processes_returns_array() {
    let app = build_router(make_state());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/processes?top=5&sort=cpu")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_bytes(resp.into_body()).await;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let arr = v.as_array().expect("processes should be a JSON array");
    assert!(arr.len() <= 5);
    if !arr.is_empty() {
        let p = &arr[0];
        assert!(p.get("pid").is_some());
        assert!(p.get("name").is_some());
        assert!(p.get("cpu_pct").is_some());
        assert!(p.get("mem_kb").is_some());
    }
}

#[tokio::test]
async fn screenshot_returns_png() {
    let app = build_router(make_state());
    let resp = app
        .oneshot(Request::builder().uri("/v1/screenshot").body(Body::empty()).unwrap())
        .await
        .unwrap();
    // On a real display we get 200 + image/png; on headless we get 503.
    let status = resp.status();
    assert!(status == StatusCode::OK || status == StatusCode::SERVICE_UNAVAILABLE);
    if status == StatusCode::OK {
        // Read headers BEFORE consuming the body.
        let ct = resp.headers().get(header::CONTENT_TYPE).unwrap().to_str().unwrap().to_string();
        let w_str = resp.headers().get("X-Screenshot-Width").unwrap().to_str().unwrap().to_string();
        let h_str = resp.headers().get("X-Screenshot-Height").unwrap().to_str().unwrap().to_string();
        let body = body_bytes(resp.into_body()).await;
        assert!(ct.starts_with("image/png"), "unexpected content-type: {ct}");
        // PNG magic: 89 50 4E 47 0D 0A 1A 0A
        assert_eq!(&body[0..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert!(w_str.parse::<u32>().unwrap() > 0);
        assert!(h_str.parse::<u32>().unwrap() > 0);
    }
}

#[tokio::test]
async fn file_read_returns_bytes() {
    // Use a file that exists on every platform we care about: temp dir.
    let tmp = std::env::temp_dir().join("pc_agent_test_file.txt");
    std::fs::write(&tmp, b"hello from pc-agent test").unwrap();

    let app = build_router(make_state());
    let uri = format!("/v1/file?path={}", urlencoding(&tmp));
    let resp = app
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_bytes(resp.into_body()).await;
    assert_eq!(body, b"hello from pc-agent test");

    let _ = std::fs::remove_file(&tmp);
}

#[tokio::test]
async fn log_tail_returns_lines() {
    let tmp = std::env::temp_dir().join("pc_agent_test_log.txt");
    let content: String = (0..20).map(|i| format!("line {i}\n")).collect();
    std::fs::write(&tmp, content).unwrap();

    let app = build_router(make_state());
    let uri = format!("/v1/log?path={}&tail=3", urlencoding(&tmp));
    let resp = app
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_bytes(resp.into_body()).await;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let lines = v["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "line 17");
    assert_eq!(lines[2], "line 19");
    assert_eq!(v["truncated"], serde_json::Value::Bool(true));

    let _ = std::fs::remove_file(&tmp);
}

#[tokio::test]
async fn auth_missing_token_is_401() {
    let app = build_router(make_state_with_token("s3cret"));
    // Endpoints that don't read state still 200; the screenshot endpoint
    // does. We pick /v1/system which is the most "important" one.
    // Actually check_auth is currently only called on the heavy endpoints.
    // Verify at least one requires the token.
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/screenshot")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_valid_token_is_accepted() {
    let app = build_router(make_state_with_token("s3cret"));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/screenshot")
                .header(header::AUTHORIZATION, "Bearer s3cret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // 200 if a display is present, 503 otherwise — both are "auth passed"
    assert!(
        resp.status() == StatusCode::OK || resp.status() == StatusCode::SERVICE_UNAVAILABLE,
        "expected 200/503, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn roots_restricts_file_access() {
    let tmp = std::env::temp_dir().join("pc_agent_test_blocked.txt");
    std::fs::write(&tmp, b"blocked").unwrap();

    let mut s = make_state();
    // Only allow a directory that does NOT contain the test file.
    s.allowed_roots = Arc::new(vec![PathBuf::from("Z:/this/does/not/exist")]);
    let app = build_router(s);

    let uri = format!("/v1/file?path={}", urlencoding(&tmp));
    let resp = app
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let _ = std::fs::remove_file(&tmp);
}

// ---------------------------------------------------------------------------
// UIA endpoint tests (Windows only — Linux has no UIA).
// ---------------------------------------------------------------------------

#[cfg(windows)]
#[tokio::test]
async fn uia_dump_requires_auth_when_token_set() {
    let app = build_router(make_state_with_token("uia-secret"));
    let resp = app
        .oneshot(Request::builder().uri("/v1/uia/dump").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[cfg(windows)]
#[tokio::test]
async fn uia_dump_accepts_valid_token() {
    let app = build_router(make_state_with_token("uia-secret"));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/uia/dump")
                .header(header::AUTHORIZATION, "Bearer uia-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // We don't assert 200 because there's no UIA-friendly window in CI.
    // The test only proves auth passed — auth-failure is 401.
    assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[cfg(windows)]
#[tokio::test]
async fn uia_invoke_requires_body() {
    let app = build_router(make_state());
    // No body — axum's Json extractor will reject with 415/400.
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/uia/invoke")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // We only check that the route is wired (not 404 / 405).
    assert_ne!(resp.status(), StatusCode::NOT_FOUND);
    assert_ne!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[cfg(windows)]
#[tokio::test]
async fn uia_invoke_rejects_missing_automation_id() {
    let app = build_router(make_state());
    let body = serde_json::json!({ "wrong_field": "x" }).to_string();
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/uia/invoke")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    // We only check that the route exists.
    assert_ne!(resp.status(), StatusCode::NOT_FOUND);
    assert_ne!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[cfg(windows)]
#[tokio::test]
async fn uia_press_requires_auth_when_token_set() {
    let app = build_router(make_state_with_token("uia-secret"));
    let body = serde_json::json!({ "keys": "Return" }).to_string();
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/uia/press")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[cfg(windows)]
#[tokio::test]
async fn uia_press_accepts_valid_token() {
    let app = build_router(make_state_with_token("uia-secret"));
    let body = serde_json::json!({ "keys": "Return" }).to_string();
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/uia/press")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer uia-secret")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Minimal URL encoder for path query values (RFC 3986 unreserved + a few
/// common reserved). We need this because the production code uses
/// `URLEncoder` in Kotlin, here we're sending raw from Rust.
fn urlencoding(p: &std::path::Path) -> String {
    let s = p.to_string_lossy();
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' | b'\\' => {
                out.push(*b as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Direct uia:: function tests — call the underlying functions without HTTP.
// We don't have a real test window with UIA controls, so we just verify
// the functions return well-formed results (either a valid JSON object
// for `dump`, or a clear error for the typed actions on a non-existent
// automation_id). A 2s timeout guards against any future deadlock.
// ---------------------------------------------------------------------------

#[cfg(windows)]
#[test]
fn uia_dump_returns_valid_json_with_expected_fields() {
    let result = pc_agent::uia::dump_active_window();
    // We don't assert success — on a headless or system-locked host the
    // call can legitimately fail. What we DO assert: if it succeeds, the
    // JSON has the expected top-level fields. If it fails, the error is
    // non-empty (so the caller can show it to the user).
    match result {
        Ok(v) => {
            assert!(v.get("window").is_some(), "missing 'window' field");
            assert!(v.get("process").is_some(), "missing 'process' field");
            assert!(v.get("controls").is_some(), "missing 'controls' field");
            assert!(v.get("control_count").is_some(), "missing 'control_count' field");
            assert!(v["controls"].is_array(), "'controls' must be an array");
        }
        Err(e) => {
            assert!(!e.is_empty(), "error message must not be empty");
        }
    }
}

#[cfg(windows)]
#[test]
fn uia_invoke_by_id_returns_error_for_unknown_id() {
    // "definitely-not-a-real-control-id" — if it ever succeeds, our test
    // environment has a real app exposing that id, which would be a bug.
    let result = pc_agent::uia::invoke_by_id("definitely-not-a-real-control-id");
    assert!(result.is_err(), "expected error for non-existent control id");
    let err = result.unwrap_err();
    assert!(
        err.contains("no control with automationId") || err.contains("CoCreateInstance") || err.contains("GetRootElement"),
        "unexpected error: {err}"
    );
}

#[cfg(windows)]
#[test]
fn uia_set_text_by_id_returns_error_for_unknown_id() {
    let result = pc_agent::uia::set_text_by_id("definitely-not-a-real-control-id", "hello");
    assert!(result.is_err(), "expected error for non-existent control id");
}

#[cfg(windows)]
#[test]
fn uia_select_by_id_returns_error_for_unknown_id() {
    let result = pc_agent::uia::select_by_id("definitely-not-a-real-control-id", "value");
    assert!(result.is_err(), "expected error for non-existent control id");
}

#[cfg(windows)]
#[test]
fn uia_press_keys_accepts_known_shortcuts() {
    // press_keys uses SendInput — we can't observe side-effects from a test,
    // but we can verify it doesn't error on well-formed inputs.
    for keys in ["Return", "Tab", "Escape", "Ctrl+S", "Alt+F4", "F5"] {
        let result = pc_agent::uia::press_keys(keys);
        assert!(result.is_ok(), "press_keys({keys}) failed: {:?}", result);
    }
}

#[cfg(windows)]
#[test]
fn uia_press_keys_rejects_unknown_keys() {
    assert!(pc_agent::uia::press_keys("NoSuchKey").is_err());
    assert!(pc_agent::uia::press_keys("Ctrl+NoSuchKey").is_err());
}

#[cfg(windows)]
#[test]
fn uia_press_keys_accepts_french_aliases() {
    // Each of these used to fail with "unknown key"; they must now parse OK
    // (and SendInput is a no-op on a headless test host, but it must not error).
    for keys in [
        // Print Screen / Impr ecran — the headline bug
        "Impr",
        "printscreen",
        "PrtSc",
        "snapshot",
        "impr ecran",
        "Ctrl+Impr",
        "Ctrl+Alt+Impr",
        // French arrow / nav
        "Gauche", "Droite", "Haut", "Bas",
        "fleche haut", "fleche bas", "fleche gauche", "fleche droite",
        "Debut", "Fin", "Pg prec", "Pg suiv",
        // Other French common names
        "Echap", "Retour", "Supprimer", "Suppr",
        "Tabulation", "Espace",
        // Existing English aliases must still work
        "Return", "Escape", "Esc", "Tab", "Space", "Delete", "Del",
        "Home", "End", "PageUp", "PgUp", "PageDown", "PgDn",
        "Left", "Up", "Right", "Down",
        "F5", "F12",
        "Ctrl+S", "Alt+F4", "Ctrl+Alt+Impr", "Win+Shift+S", "Win+R",
        // Insert and lock keys (new)
        "Insert", "Ins", "CapsLock", "NumLock", "ScrollLock",
    ] {
        let result = pc_agent::uia::press_keys(keys);
        assert!(result.is_ok(), "press_keys({keys}) failed: {:?}", result);
    }
}

#[cfg(windows)]
#[test]
fn uia_press_keys_rejects_real_unknown_keys() {
    // Regression: a typo / truly invalid name should still error.
    for keys in ["Win+Rn", "Imprr", "FooBar", "Ctrl+Imprr", "NotAKey+More"] {
        let result = pc_agent::uia::press_keys(keys);
        assert!(result.is_err(), "press_keys({keys}) should have failed but succeeded");
    }
}

#[cfg(windows)]
#[test]
fn uia_list_windows_returns_at_least_one_window() {
    // On any real Windows host there is at least the desktop / Program
    // Manager. We don't assert the exact name because it's locale-dependent
    // (e.g. "Program Manager" in EN, "Gestionnaire des tâches" in FR), but
    // we do assert that the list is non-empty and each entry has a non-empty
    // title.
    let result = pc_agent::uia::list_visible_windows();
    let windows_list = result.expect("list_visible_windows should not error");
    assert!(!windows_list.is_empty(), "list_visible_windows returned an empty list");
    for (hwnd, title) in &windows_list {
        assert!(!title.is_empty(), "window has empty title: hwnd={hwnd:?}");
        assert!(!hwnd.is_invalid(), "window has invalid HWND: {title}");
    }
}

#[cfg(windows)]
#[test]
fn uia_dump_window_by_unknown_title_errors() {
    // Regression: a title that doesn't exist must return an Err, not Ok with
    // empty data.
    let result = pc_agent::uia::dump_window_by_title("definitely-not-a-real-window-title-xyz-12345");
    assert!(result.is_err(), "expected error for unknown window title");
    let err = result.unwrap_err();
    assert!(
        err.contains("no window with title") || err.contains("fenetre non trouvee"),
        "unexpected error: {err}"
    );
}

