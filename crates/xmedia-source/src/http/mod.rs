mod client;
pub mod ssrf;
mod url_cookie;

#[cfg(test)]
mod tests;

pub const DEFAULT_WEBVIEW_UA: &str = "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";
pub const MAX_RESPONSE_BODY_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_REDIRECT_HOPS: usize = 6;
pub const MAX_COOKIE_ENTRIES: usize = 256;
pub const MAX_COOKIE_TOTAL_BYTES: usize = 256 * 1024;

pub use client::{
    LegadoHttpClient, build_sanitized_headers, decode_response_bytes, get_or_build_async_client,
    get_or_build_blocking_client, is_forbidden_custom_header,
};
pub use ssrf::{
    SsrfSafeResolver, is_restricted_hostname, is_restricted_ip, ssrf_redirect_policy,
    validate_target_url, validate_target_url_static,
};
pub use url_cookie::{
    HttpClientConfig, HttpFetchResult, LegadoUrlOptions, ParsedLegadoUrl, merge_cookie_string,
    normalize_cookie_key, parse_legado_url, resolve_relative_url, try_resolve_data_url,
};
