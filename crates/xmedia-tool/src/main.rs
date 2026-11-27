use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;
use xmedia_tool::plugin_cli::{PluginCommand, run_plugin_cli};
use xmedia_tool::{ToolInitOptions, XMediaToolApp, XMediaToolConfig};

#[derive(Debug, Subcommand)]
enum ToolSubcommand {
    /// Validate, debug, and test XMedia Plugin v2 (.js) and JSON rule plugins locally
    Plugin {
        #[command(subcommand)]
        action: PluginCommand,
    },
}

#[derive(Debug, Parser)]
#[command(
    name = "xmedia-tool",
    version,
    about = "Standalone Web workbench and plugin debugger for XMedia"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<ToolSubcommand>,

    /// Socket address to listen on (defaults to loopback for local safety)
    #[arg(long, env = "XMEDIA_TOOL_BIND", default_value = "127.0.0.1:8091")]
    bind: SocketAddr,

    /// Data directory for tool-config.json (managed roots, scraper keys & rename templates)
    #[arg(long, env = "XMEDIA_TOOL_DATA_DIR", default_value = "./data-tool")]
    data_dir: PathBuf,

    /// Whitelisted media root directory to expose in the workbench (repeatable: `--root /media/music --root /media/video`)
    #[arg(long = "root", value_name = "DIR")]
    roots: Vec<PathBuf>,

    /// Lock managed roots against runtime additions or removals via HTTP API
    #[arg(long, env = "XMEDIA_TOOL_LOCK_ROOTS")]
    lock_roots: bool,

    /// Optional access password protecting the Web workbench (required when binding a non-loopback address)
    #[arg(long, env = "XMEDIA_TOOL_PASSWORD")]
    password: Option<String>,

    /// Explicit allowed CORS origins (repeatable or comma-separated). Cross-origin requests are denied by default.
    #[arg(
        long = "cors-allow-origin",
        env = "XMEDIA_TOOL_CORS_ALLOW_ORIGINS",
        value_delimiter = ','
    )]
    cors_allow_origins: Vec<String>,

    /// Path or binary name for FFmpeg/FFprobe (used for Video stream inspection)
    #[arg(long, env = "XMEDIA_TOOL_FFMPEG_BIN", default_value = "ffmpeg")]
    ffmpeg_bin: String,

    /// Optional external frontend build directory (`web/dist-tool`) overriding embedded assets
    #[arg(long, env = "XMEDIA_TOOL_WEB_DIR")]
    web_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if let Some(ToolSubcommand::Plugin { action }) = cli.command {
        return tokio::task::spawn_blocking(move || run_plugin_cli(action))
            .await
            .context("plugin CLI worker panicked")?;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info")),
        )
        .init();

    let password_enabled = cli
        .password
        .as_deref()
        .is_some_and(|p| !p.trim().is_empty());

    if !cli.bind.ip().is_loopback() && !password_enabled {
        anyhow::bail!(
            "refusing to bind xmedia-tool on non-loopback address {} without --password / XMEDIA_TOOL_PASSWORD",
            cli.bind
        );
    }

    let listener = TcpListener::bind(cli.bind)
        .await
        .with_context(|| format!("failed to bind {}", cli.bind))?;
    let bound_addr = listener
        .local_addr()
        .context("failed resolving bound socket address")?;

    let app = XMediaToolApp::init_with_options(
        XMediaToolConfig {
            data_dir: cli.data_dir,
            initial_roots: cli.roots,
            password: cli.password,
            ffmpeg_bin: cli.ffmpeg_bin,
            external_web_dir: cli.web_dir,
        },
        ToolInitOptions {
            providers: None,
            bind_addr: Some(bound_addr),
            cors_allow_origins: cli.cors_allow_origins,
            lock_roots: cli.lock_roots,
        },
    )
    .await?;

    info!(
        "xmedia-tool listening on http://{} (password protection: {})",
        bound_addr,
        if password_enabled { "enabled" } else { "open" }
    );

    axum::serve(
        listener,
        app.router
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
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
