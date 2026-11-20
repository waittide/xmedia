pub mod config;
pub mod proxy;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use tauri::Manager;
use tracing_subscriber::EnvFilter;

use crate::config::AppServerStore;
use crate::proxy::RunningGateway;

pub struct GatewayUrlState(pub String);

#[tauri::command]
fn get_local_gateway_url(state: tauri::State<'_, GatewayUrlState>) -> String {
    state.0.clone()
}

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
/// so automated E2E tests can start the exact same local loopback gateway without a GUI display.
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
    let data_dir = std::env::var_os("XMEDIA_APP_DATA_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("xmedia-app-headless"));
    let store = AppServerStore::open_shared(data_dir.join("servers.json"))?;
    let dist_dir = external_dist_dir_from_env();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(proxy::run_gateway_forever(bind_addr, store, dist_dir))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();

    if let Some(headless_addr) = parse_headless_addr_from_env_and_args() {
        if let Err(err) = run_headless_gateway(&headless_addr) {
            eprintln!("Failed to run headless XMedia gateway: {err}");
            std::process::exit(1);
        }
        return;
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_local_gateway_url])
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("./data"));
            let store = AppServerStore::open_shared(data_dir.join("servers.json"))?;
            let dist_dir = external_dist_dir_from_env();

            let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
            let gateway: RunningGateway =
                tauri::async_runtime::block_on(proxy::start_gateway(bind_addr, store, dist_dir))?;
            let gateway_url = gateway.url.clone();
            tracing::info!(url = %gateway_url, "started Tauri local loopback gateway");

            app.manage(GatewayUrlState(gateway_url.clone()));
            app.manage(gateway);

            let external_url: tauri::Url = gateway_url.parse()?;
            if let Some(window) = app.get_webview_window("main") {
                window.navigate(external_url)?;
            } else {
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

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running XMedia Tauri application");
}
