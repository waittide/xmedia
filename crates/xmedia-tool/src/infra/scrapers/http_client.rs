use std::time::Duration;

use reqwest::Client;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;

pub const DEFAULT_TOOL_USER_AGENT: &str =
    "xmedia-tool/0.1.0 (https://github.com/xmedia-org/xmedia)";

pub fn build_http_client(creds: &ProviderCredentials) -> DomainResult<Client> {
    let ua = creds
        .user_agent
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(DEFAULT_TOOL_USER_AGENT);

    let mut builder = Client::builder()
        .user_agent(ua)
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(10));

    if let Some(proxy_url) = creds
        .http_proxy
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let proxy = reqwest::Proxy::all(proxy_url).map_err(|e| {
            DomainError::Validation(format!("invalid proxy URL '{proxy_url}': {e}"))
        })?;
        builder = builder.proxy(proxy);
    }

    builder
        .build()
        .map_err(|e| DomainError::Storage(format!("failed building HTTP client: {e}")))
}
