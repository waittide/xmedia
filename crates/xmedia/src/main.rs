use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Parser;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;
use xmedia::{
    DEFAULT_DEV_SECRET, XMediaApp, XMediaConfig, XMediaInitOptions,
    resolve_allow_private_ips_for_testing,
};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::infra::security::generate_random_secret_hex;

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

    /// Master secret for Web JWT signing and OpenSubsonic credential encryption.
    /// When omitted, a cryptographically random 256-bit secret is generated and persisted in `<data_dir>/.secret`.
    #[arg(long, env = "XMEDIA_SECRET")]
    secret: Option<String>,

    /// JWT expiration duration in seconds (default: 7 days)
    #[arg(long, env = "XMEDIA_JWT_TTL_SECS", default_value_t = 604_800)]
    jwt_ttl_secs: u64,

    /// Explicit allowed CORS origins (repeatable or comma-separated). Cross-origin requests are denied by default.
    #[arg(
        long = "cors-allow-origin",
        env = "XMEDIA_CORS_ALLOW_ORIGINS",
        value_delimiter = ','
    )]
    cors_allow_origins: Vec<String>,

    /// Path or binary name for FFmpeg (used by Music & Video transcoding and probing)
    #[arg(long, env = "XMEDIA_FFMPEG_BIN", default_value = "ffmpeg")]
    ffmpeg_bin: String,

    /// Optional external frontend build directory (`web/dist`) overriding embedded assets
    #[arg(long, env = "XMEDIA_WEB_DIR")]
    web_dir: Option<PathBuf>,

    /// Seed curated online sources for enabled domains on startup (insert-if-missing)
    #[arg(
        long,
        env = "XMEDIA_SEED_SOURCES",
        default_value_t = false,
        default_missing_value = "true",
        num_args = 0..=1,
        require_equals = true,
        value_parser = parse_boolish_flag
    )]
    seed_sources: bool,
}

fn parse_boolish_flag(raw: &str) -> Result<bool, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "" | "0" | "false" | "no" | "off" => Ok(false),
        other => Err(format!(
            "invalid boolean value '{other}' (expected one of: true, false, 1, 0, yes, no, on, off)"
        )),
    }
}

fn load_or_generate_secret(data_dir: &Path) -> anyhow::Result<String> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("failed to create data directory {}", data_dir.display()))?;
    let secret_path = data_dir.join(".secret");
    if secret_path.is_file() {
        let existing = std::fs::read_to_string(&secret_path)
            .with_context(|| format!("failed to read {}", secret_path.display()))?;
        let trimmed = existing.trim();
        if trimmed.len() >= 32 {
            return Ok(trimmed.to_string());
        }
    }

    let generated = generate_random_secret_hex();
    let tmp_path = data_dir.join(format!(".secret.{}.tmp", std::process::id()));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    {
        let mut f = opts
            .open(&tmp_path)
            .with_context(|| format!("failed to create {}", tmp_path.display()))?;
        f.write_all(generated.as_bytes())?;
        f.sync_all()?;
    }
    if let Err(err) = std::fs::rename(&tmp_path, &secret_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err)
            .with_context(|| format!("failed to persist secret file {}", secret_path.display()));
    }
    info!(
        "generated and persisted new 256-bit master secret at {}",
        secret_path.display()
    );
    Ok(generated)
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
    let secret = match cli
        .secret
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(provided) => {
            if !cli.bind.ip().is_loopback()
                && (provided == DEFAULT_DEV_SECRET || provided.len() < 32)
            {
                anyhow::bail!(
                    "refusing to bind non-loopback address {} with a weak or default XMEDIA_SECRET (<32 bytes); omit XMEDIA_SECRET to auto-generate a secure secret or supply a strong >=32-byte value",
                    cli.bind
                );
            }
            provided
        }
        None => load_or_generate_secret(&cli.data_dir)?,
    };

    let database_url = cli.database_url.unwrap_or_else(|| {
        let sqlite_path = cli.data_dir.join("xmedia.db");
        format!("sqlite:{}", sqlite_path.display())
    });

    let allow_private_ips = resolve_allow_private_ips_for_testing(
        cli.bind,
        std::env::var("XMEDIA_ALLOW_PRIVATE_IPS_FOR_TESTING")
            .ok()
            .as_deref(),
    );

    info!("connecting database ({})", redact_db_url(&database_url));
    let app = XMediaApp::connect_and_init_with_options(
        XMediaConfig {
            data_dir: cli.data_dir,
            database_url,
            secret,
            jwt_ttl_secs: cli.jwt_ttl_secs,
            ffmpeg_bin: cli.ffmpeg_bin,
            external_web_dir: cli.web_dir,
        },
        XMediaInitOptions {
            allow_permissive_cors: false,
            cors_allow_origins: cli.cors_allow_origins,
            allow_private_ips,
            seed_sources: cli.seed_sources,
        },
    )
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
    let without_query = url.split('?').next().unwrap_or(url);
    if let Some((scheme_and_creds, host_part)) = without_query.rsplit_once('@')
        && let Some((scheme, _)) = scheme_and_creds.split_once("://")
    {
        return format!("{scheme}://***@{host_part}");
    }
    without_query.to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    struct SeedEnvGuard {
        original: Option<std::ffi::OsString>,
    }

    impl SeedEnvGuard {
        fn capture() -> Self {
            Self {
                original: std::env::var_os("XMEDIA_SEED_SOURCES"),
            }
        }

        fn set(&self, val: Option<&str>) {
            // SAFETY: Only this single test in the `xmedia` binary mutates `XMEDIA_SEED_SOURCES`,
            // and `Drop` restores the original value.
            unsafe {
                match val {
                    Some(v) => std::env::set_var("XMEDIA_SEED_SOURCES", v),
                    None => std::env::remove_var("XMEDIA_SEED_SOURCES"),
                }
            }
        }
    }

    impl Drop for SeedEnvGuard {
        fn drop(&mut self) {
            // SAFETY: Restores `XMEDIA_SEED_SOURCES` after the single env-mutating test completes.
            unsafe {
                match &self.original {
                    Some(v) => std::env::set_var("XMEDIA_SEED_SOURCES", v),
                    None => std::env::remove_var("XMEDIA_SEED_SOURCES"),
                }
            }
        }
    }

    #[test]
    fn cli_seed_sources_parses_flags_and_boolish_env_values() {
        let guard = SeedEnvGuard::capture();

        // 1. No flag, no env -> false
        guard.set(None);
        let cli = Cli::try_parse_from(["xmedia"]).unwrap();
        assert!(!cli.seed_sources);

        // 2. CLI `--seed-sources` (no value) -> true, and does not swallow subsequent flags
        let cli =
            Cli::try_parse_from(["xmedia", "--seed-sources", "--bind", "127.0.0.1:9000"]).unwrap();
        assert!(cli.seed_sources);
        assert_eq!(cli.bind, "127.0.0.1:9000".parse::<SocketAddr>().unwrap());

        // 3. CLI `--seed-sources=<val>` explicit values
        for truthy in ["1", "true", "TRUE", "yes", "on"] {
            let arg = format!("--seed-sources={truthy}");
            let cli = Cli::try_parse_from(["xmedia", &arg]).unwrap();
            assert!(cli.seed_sources, "expected true for CLI {arg}");
        }
        for falsy in ["0", "false", "FALSE", "no", "off", ""] {
            let arg = format!("--seed-sources={falsy}");
            let cli = Cli::try_parse_from(["xmedia", &arg]).unwrap();
            assert!(!cli.seed_sources, "expected false for CLI {arg}");
        }
        assert!(Cli::try_parse_from(["xmedia", "--seed-sources=maybe"]).is_err());

        // 4. `XMEDIA_SEED_SOURCES` env var truthy values
        for truthy in ["1", "true", "True", "yes", "YES", "on", " ON ", " 1 "] {
            guard.set(Some(truthy));
            let cli = Cli::try_parse_from(["xmedia"]).unwrap();
            assert!(
                cli.seed_sources,
                "expected true for XMEDIA_SEED_SOURCES={truthy:?}"
            );
        }

        // 5. `XMEDIA_SEED_SOURCES` env var falsy and empty values
        for falsy in ["0", "false", "False", "no", "NO", "off", " OFF ", "", "   "] {
            guard.set(Some(falsy));
            let cli = Cli::try_parse_from(["xmedia"]).unwrap();
            assert!(
                !cli.seed_sources,
                "expected false for XMEDIA_SEED_SOURCES={falsy:?}"
            );
        }

        // 6. Invalid env value -> clean clap validation error (not silently true)
        for invalid in ["maybe", "2", "enable", "disabled"] {
            guard.set(Some(invalid));
            let err = Cli::try_parse_from(["xmedia"]).unwrap_err();
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::ValueValidation,
                "expected ValueValidation for XMEDIA_SEED_SOURCES={invalid:?}"
            );
        }

        // 7. CLI overrides env
        guard.set(Some("0"));
        let cli = Cli::try_parse_from(["xmedia", "--seed-sources"]).unwrap();
        assert!(cli.seed_sources);

        guard.set(Some("1"));
        let cli = Cli::try_parse_from(["xmedia", "--seed-sources=false"]).unwrap();
        assert!(!cli.seed_sources);
    }
}
