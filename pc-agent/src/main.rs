//! PC-side debug agent.
//!
//! This binary runs on the PC to be debugged. It exposes a small HTTP API
//! on `0.0.0.0:8765` that the Android tablet app talks to. The same
//! protocol can also be reached over USB (see `usb` module, TODO).
//!
//! Endpoints (all under `/v1`):
//!   GET  /v1/ping                -> "pong"
//!   GET  /v1/system              -> JSON system info
//!   GET  /v1/screenshot          -> raw PNG bytes
//!   GET  /v1/processes?top=N    -> JSON array of processes
//!   GET  /v1/log?path=X&tail=N  -> JSON {path, lines}
//!   GET  /v1/file?path=X        -> raw file bytes
//!
//! Auth: optional bearer token via `--token <secret>` or env `PC_AGENT_TOKEN`.
//! Send as `Authorization: Bearer <secret>`. Disabled if no token configured.

mod capture;
mod handlers;
mod proc;

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
struct Cli {
    /// Port to listen on.
    #[arg(long, env = "PC_AGENT_PORT", default_value_t = 8765)]
    port: u16,

    /// Interface to bind. 0.0.0.0 by default (all interfaces).
    #[arg(long, env = "PC_AGENT_BIND", default_value = "0.0.0.0")]
    bind: String,

    /// Bearer token. If set, requests must include `Authorization: Bearer <token>`.
    /// Can also be set via `PC_AGENT_TOKEN` env var.
    #[arg(long, env = "PC_AGENT_TOKEN")]
    token: Option<String>,

    /// Allowed directories for `GET /v1/file` and `/v1/log`. Comma-separated.
    /// Default: empty (all paths allowed — only for trusted LAN setups).
    #[arg(long, env = "PC_AGENT_ROOTS", value_delimiter = ',')]
    roots: Vec<PathBuf>,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logging: PC_AGENT_LOG=info,debug, etc. Default = info.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cli = Cli::parse();

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

    let app = Router::new()
        .route("/v1/ping", get(handlers::ping))
        .route("/v1/system", get(handlers::system))
        .route("/v1/screenshot", get(handlers::screenshot))
        .route("/v1/processes", get(handlers::processes))
        .route("/v1/log", get(handlers::log_tail))
        .route("/v1/file", get(handlers::file_read))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive()) // LAN-only, tablet is the only client
        .with_state(state);

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
