//! `isle-server`: serves the web client and the game protocol.
//!
//! ```text
//! isle-server [--addr 0.0.0.0:8080] [--static-dir web/dist] [--data-dir data]
//!             [--bot-delay 1.0] [--autopilot-secs 30]
//! ```
//! `PORT` and `ISLE_DATA_DIR` environment variables are honoured too.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use isle_server::{AppState, ServerConfig, app};
use tracing_subscriber::EnvFilter;

fn usage(err: &str) -> ! {
    if !err.is_empty() {
        eprintln!("error: {err}\n");
    }
    eprintln!(
        "usage: isle-server [--addr HOST:PORT] [--static-dir DIR] [--data-dir DIR]\n\
         \x20                  [--bot-delay SCALE] [--autopilot-secs N]"
    );
    std::process::exit(if err.is_empty() { 0 } else { 2 });
}

fn parse_args() -> (SocketAddr, ServerConfig) {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8080);
    let mut addr: SocketAddr = ([0, 0, 0, 0], port).into();
    let mut config = ServerConfig {
        data_dir: std::env::var_os("ISLE_DATA_DIR").map(PathBuf::from),
        static_dir: Some(PathBuf::from("web/dist")).filter(|d| d.join("index.html").exists()),
        ..ServerConfig::default()
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut val = || {
            it.next()
                .unwrap_or_else(|| usage(&format!("missing value for {flag}")))
        };
        match flag.as_str() {
            "--addr" => addr = val().parse().unwrap_or_else(|_| usage("bad --addr")),
            "--static-dir" => config.static_dir = Some(PathBuf::from(val())),
            "--data-dir" => config.data_dir = Some(PathBuf::from(val())),
            "--bot-delay" => {
                config.bot_delay_scale = val().parse().unwrap_or_else(|_| usage("bad --bot-delay"))
            }
            "--autopilot-secs" => {
                config.autopilot_after = Duration::from_secs(
                    val()
                        .parse()
                        .unwrap_or_else(|_| usage("bad --autopilot-secs")),
                )
            }
            "-h" | "--help" => usage(""),
            other => usage(&format!("unknown flag {other}")),
        }
    }
    (addr, config)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let (addr, config) = parse_args();
    match &config.static_dir {
        Some(dir) => tracing::info!("serving web client from {}", dir.display()),
        None => {
            tracing::warn!("no web client found (build it with `pnpm build` or pass --static-dir)")
        }
    }
    if let Some(dir) = &config.data_dir {
        tracing::info!("persisting rooms in {}", dir.display());
    }
    let state = AppState::new(config);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("cannot listen on {addr}: {e}");
            std::process::exit(1);
        });
    tracing::info!("listening on http://{addr}");
    if let Err(e) = axum::serve(listener, app(state.clone()))
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        tracing::error!("server error: {e}");
    }
    state.registry.shutdown().await;
    tracing::info!("bye");
}
