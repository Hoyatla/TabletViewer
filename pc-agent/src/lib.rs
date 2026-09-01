//! PC-side debug agent — library.
//!
//! Exposes the HTTP router and `AppState` so integration tests in
//! `tests/integration.rs` can hit endpoints via `tower::ServiceExt::oneshot`
//! without a real socket.

pub mod capture;
pub mod handlers;
pub mod proc;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::routing::get;
use axum::Router;
use clap::Parser;
use sysinfo::System;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "pc-agent", version, about)]
pub struct Cli {
    /// Port to listen on.
    #[arg(long, env = "PC_AGENT_PORT", default_value_t = 8765)]
    pub port: u16,

    /// Interface to bind. 0.0.0.0 by default (all interfaces).
    #[arg(long, env = "PC_AGENT_BIND", default_value = "0.0.0.0")]
    pub bind: String,

    /// Bearer token. If set, requests must include `Authorization: Bearer <token>`.
    /// Can also be set via `PC_AGENT_TOKEN` env var.
    #[arg(long, env = "PC_AGENT_TOKEN")]
    pub token: Option<String>,

    /// Allowed directories for `GET /v1/file` and `/v1/log`. Comma-separated.
    /// Default: empty (all paths allowed — only for trusted LAN setups).
    #[arg(long, env = "PC_AGENT_ROOTS", value_delimiter = ',')]
    pub roots: Vec<PathBuf>,
}

#[derive(Clone)]
pub struct AppState {
    pub token: Option<Arc<String>>,
    pub allowed_roots: Arc<Vec<PathBuf>>,
    /// Reused sysinfo handle — kept across requests for efficiency.
    pub sys: Arc<parking_lot::Mutex<System>>,
    /// Cached screenshotter — initialized once.
    pub capturer: Arc<capture::Capturer>,
}

/// Build the HTTP router with all routes wired. Public so integration
/// tests in `tests/integration.rs` can call it with a fresh `AppState`.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/v1/ping", get(handlers::ping))
        .route("/v1/system", get(handlers::system))
        .route("/v1/screenshot", get(handlers::screenshot))
        .route("/v1/processes", get(handlers::processes))
        .route("/v1/log", get(handlers::log_tail))
        .route("/v1/file", get(handlers::file_read))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive()) // LAN-only, tablet is the only client
        .with_state(state)
}

/// Initialize tracing. Idempotent — safe to call from tests.
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .try_init();
}

/// Run the server. Used by `main.rs`.
pub async fn run(cli: Cli) -> anyhow::Result<()> {
    init_tracing();

    let token = cli.token.clone().map(Arc::new);
    if token.is_some() {
        tracing::info!("Auth enabled (bearer token)");
    } else {
        tracing::warn!("Auth disabled — agent is open to anyone who can reach port {}.", cli.port);
    }

    let allowed_roots = Arc::new(cli.roots.clone());
    if !allowed_roots.is_empty() {
        tracing::info!("File/log access restricted to: {:?}", allowed_roots);
    }

    let capturer = capture::Capturer::new().context("init screenshot capture")?;
    let sys = System::new_all();
    let state = AppState {
        token,
        allowed_roots,
        sys: Arc::new(parking_lot::Mutex::new(sys)),
        capturer: Arc::new(capturer),
    };
    let app = build_router(state);

    let addr: SocketAddr = format!("{}:{}", cli.bind, cli.port)
        .parse()
        .context("parse bind address")?;
    tracing::info!("pc-agent listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind to {}", addr))?;
    axum::serve(listener, app).await?;
    Ok(())
}
