use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;
use xmedia_tool::{XMediaToolApp, XMediaToolConfig};

#[derive(Debug, Parser)]
#[command(
    name = "xmedia-tool",
    version,
    about = "Standalone Web workbench for scraping & editing raw Book, Comic, Music & Video source files"
)]
struct Cli {
    /// Socket address to listen on
    #[arg(long, env = "XMEDIA_TOOL_BIND", default_value = "0.0.0.0:8091")]
    bind: SocketAddr,

    /// Data directory for tool-config.json (managed roots, scraper keys & rename templates)
    #[arg(long, env = "XMEDIA_TOOL_DATA_DIR", default_value = "./data-tool")]
    data_dir: PathBuf,

    /// Whitelisted media root directory to expose in the workbench (repeatable: `--root /media/music --root /media/video`)
    #[arg(long = "root", value_name = "DIR")]
    roots: Vec<PathBuf>,

    /// Optional access password protecting the Web workbench
    #[arg(long, env = "XMEDIA_TOOL_PASSWORD")]
    password: Option<String>,

    /// Path or binary name for FFmpeg/FFprobe (used for Video stream inspection)
    #[arg(long, env = "XMEDIA_TOOL_FFMPEG_BIN", default_value = "ffmpeg")]
    ffmpeg_bin: String,

    /// Optional external frontend build directory (`web/dist-tool`) overriding embedded assets
    #[arg(long, env = "XMEDIA_TOOL_WEB_DIR")]
    web_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info")),
        )
        .init();

    let cli = Cli::parse();
    let password_enabled = cli
        .password
        .as_deref()
        .is_some_and(|p| !p.trim().is_empty());

    let app = XMediaToolApp::init(XMediaToolConfig {
        data_dir: cli.data_dir,
        initial_roots: cli.roots,
        password: cli.password,
        ffmpeg_bin: cli.ffmpeg_bin,
        external_web_dir: cli.web_dir,
    })
    .await?;

    let listener = TcpListener::bind(cli.bind)
        .await
        .with_context(|| format!("failed to bind {}", cli.bind))?;

    info!(
        "xmedia-tool listening on http://{} (password protection: {})",
        cli.bind,
        if password_enabled { "enabled" } else { "open" }
    );

    axum::serve(listener, app.router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("xmedia-tool http server error")?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    info!("shutdown signal received, stopping xmedia-tool server");
}
