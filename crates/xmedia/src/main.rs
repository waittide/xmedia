use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;
use xmedia::{XMediaApp, XMediaConfig};
use xmedia_core::domain::library::MediaKind;

#[derive(Debug, Parser)]
#[command(
    name = "xmedia",
    version,
    about = "Self-hosted unified multi-user media server (Books, Comics, Music & Video)"
)]
struct Cli {
    /// Socket address to listen on
    #[arg(long, env = "XMEDIA_BIND", default_value = "0.0.0.0:8090")]
    bind: SocketAddr,

    /// Data directory for managed uploads, covers, thumbnails, and default SQLite DB
    #[arg(long, env = "XMEDIA_DATA_DIR", default_value = "./data")]
    data_dir: PathBuf,

    /// Database connection URL (`sqlite:...`, `postgresql://...`, or `mysql://...`).
    /// Defaults to `sqlite:<data_dir>/xmedia.db` when omitted.
    #[arg(long, env = "XMEDIA_DATABASE_URL")]
    database_url: Option<String>,

    /// Master secret for Web JWT signing and OpenSubsonic credential encryption
    #[arg(
        long,
        env = "XMEDIA_SECRET",
        default_value = "xmedia-change-this-secret-in-production"
    )]
    secret: String,

    /// JWT expiration duration in seconds (default: 7 days)
    #[arg(long, env = "XMEDIA_JWT_TTL_SECS", default_value_t = 604_800)]
    jwt_ttl_secs: u64,

    /// Path or binary name for FFmpeg (used by Music & Video transcoding and probing)
    #[arg(long, env = "XMEDIA_FFMPEG_BIN", default_value = "ffmpeg")]
    ffmpeg_bin: String,

    /// Optional external frontend build directory (`web/dist`) overriding embedded assets
    #[arg(long, env = "XMEDIA_WEB_DIR")]
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
    let database_url = cli.database_url.unwrap_or_else(|| {
        let sqlite_path = cli.data_dir.join("xmedia.db");
        format!("sqlite:{}", sqlite_path.display())
    });

    info!("connecting database ({})", redact_db_url(&database_url));
    let app = XMediaApp::connect_and_init(XMediaConfig {
        data_dir: cli.data_dir,
        database_url,
        secret: cli.secret,
        jwt_ttl_secs: cli.jwt_ttl_secs,
        ffmpeg_bin: cli.ffmpeg_bin,
        external_web_dir: cli.web_dir,
    })
    .await?;

    let listener = TcpListener::bind(cli.bind)
        .await
        .with_context(|| format!("failed to bind {}", cli.bind))?;

    info!("xmedia listening on http://{}", cli.bind);
    if app.enabled_modules.contains(&MediaKind::Book) {
        info!(
            "Book OPDS 1.2 / 2.0: http://{}/opds/book/v1.2 | http://{}/opds/book/v2",
            cli.bind, cli.bind
        );
    }
    if app.enabled_modules.contains(&MediaKind::Comic) {
        info!("Comic OPDS 1.2 + PSE: http://{}/opds/comic/v1.2", cli.bind);
    }
    if app.enabled_modules.contains(&MediaKind::Music) {
        info!("Music OpenSubsonic API: http://{}/rest", cli.bind);
    }

    axum::serve(listener, app.router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("http server error")?;

    Ok(())
}

fn redact_db_url(url: &str) -> String {
    if let Some((scheme_and_creds, host_part)) = url.rsplit_once('@')
        && let Some((scheme, _)) = scheme_and_creds.split_once("://")
    {
        return format!("{scheme}://***@{host_part}");
    }
    url.to_string()
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
    info!("shutdown signal received, stopping xmedia server");
}
