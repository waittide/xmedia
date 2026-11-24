pub mod app_icon;
pub mod gateway;
pub mod local_engine;
pub mod opds_client;
pub mod remote_sources;
pub mod subsonic_client;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use tauri::Manager;
use tracing_subscriber::EnvFilter;

use crate::gateway::RunningGateway;

struct GatewayLifecycleState(Mutex<Option<RunningGateway>>);

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init();
}

fn external_dist_dir_from_env() -> Option<PathBuf> {
    std::env::var_os("XMEDIA_APP_DIST_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Checks `--headless-gateway <addr>` CLI argument or `XMEDIA_APP_HEADLESS_ADDR` env var
/// so automated E2E tests can start the exact same standalone local gateway without a GUI display.
pub fn parse_headless_addr_from_env_and_args() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--headless-gateway" {
            return Some(args.next().unwrap_or_else(|| "127.0.0.1:0".to_string()));
        }
        if let Some(val) = arg.strip_prefix("--headless-gateway=") {
            return Some(if val.is_empty() {
                "127.0.0.1:0".to_string()
            } else {
                val.to_string()
            });
        }
    }
    std::env::var("XMEDIA_APP_HEADLESS_ADDR")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn run_headless_gateway(raw_addr: &str) -> anyhow::Result<()> {
    let bind_addr: SocketAddr = raw_addr.parse()?;
    if !bind_addr.ip().is_loopback() {
        anyhow::bail!(
            "headless standalone gateway must bind to a loopback address (got {})",
            bind_addr.ip()
        );
    }
    let data_dir = std::env::var_os("XMEDIA_APP_DATA_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("xmedia-app-headless"));
    let dist_dir = external_dist_dir_from_env();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(gateway::run_gateway_forever(bind_addr, data_dir, dist_dir))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();

    if let Some(headless_addr) = parse_headless_addr_from_env_and_args() {
        if let Err(err) = run_headless_gateway(&headless_addr) {
            eprintln!("Failed to run headless XMedia standalone gateway: {err}");
            std::process::exit(1);
        }
        return;
    }

    let app = tauri::Builder::default()
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("./data"));
            let dist_dir = external_dist_dir_from_env();

            let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
            #[cfg(desktop)]
            let icon_applier: Option<app_icon::IconApplier> = {
                let handle = app.handle().clone();
                Some(std::sync::Arc::new(move |variant| {
                    app_icon::apply_desktop_window_icon(&handle, variant);
                }))
            };
            #[cfg(not(desktop))]
            let icon_applier: Option<app_icon::IconApplier> = None;

            let gateway: RunningGateway =
                tauri::async_runtime::block_on(gateway::start_gateway_with_icon_applier(
                    bind_addr,
                    data_dir,
                    dist_dir,
                    icon_applier,
                ))?;
            let gateway_url = gateway.url.clone();
            #[cfg(desktop)]
            let initial_icon = gateway.current_app_icon();
            tracing::info!(url = %gateway_url, "started Tauri standalone local loopback gateway");

            app.manage(GatewayLifecycleState(Mutex::new(Some(gateway))));

            let external_url: tauri::Url = gateway_url.parse()?;
            if let Some(window) = app.get_webview_window("main") {
                window.navigate(external_url)?;
            } else {
                #[allow(unused_mut)]
                let mut builder = tauri::WebviewWindowBuilder::new(
                    app,
                    "main",
                    tauri::WebviewUrl::External(external_url),
                );
                #[cfg(desktop)]
                {
                    builder = builder
                        .title("XMedia")
                        .inner_size(1280.0, 820.0)
                        .min_inner_size(390.0, 640.0)
                        .resizable(true)
                        .center();
                }
                builder.build()?;
            }

            #[cfg(desktop)]
            if initial_icon != app_icon::AppIconVariant::default() {
                app_icon::apply_desktop_window_icon(app.handle(), initial_icon);
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building XMedia Tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event
            && let Some(lifecycle) = app_handle.try_state::<GatewayLifecycleState>()
            && let Ok(mut guard) = lifecycle.0.lock()
            && let Some(mut gateway) = guard.take()
        {
            tauri::async_runtime::block_on(gateway.shutdown_gracefully(Duration::from_secs(5)));
        }
    });
}
