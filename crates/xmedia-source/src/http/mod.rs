mod client;
pub mod proxy;
pub mod ssrf;
pub mod template;
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
    get_or_build_blocking_client, is_forbidden_custom_header, lookup_cookie_in_jar,
};
pub use proxy::{
    filter_header_map_for_target_origin, filter_headers_for_target_origin, has_origin_affinity,
    is_sensitive_origin_header, registrable_domain, strip_cross_origin_headers,
};
pub use ssrf::{
    SsrfSafeResolver, has_ssrf_io_error_in_chain, is_restricted_hostname, is_restricted_ip,
    ssrf_blocking_redirect_policy, ssrf_redirect_policy, validate_target_url,
    validate_target_url_async, validate_target_url_static,
};
pub use template::{
    JsonSigToken, count_comma_open_brace, eval_json_options_template, has_suspicious_option_chars,
    json_escape_str_inner, json_raw_structure_signature, json_structure_signature,
    resolve_literal_http_url, resolve_relative_url_preserving_options,
    split_legado_url_and_raw_options, split_legado_url_template_and_options,
    substitute_legado_options_vars, substitute_legado_url_template_vars, substitute_url_part_vars,
    url_encode_component, verify_js_pipeline_url_options,
};
pub use url_cookie::{
    HttpClientConfig, HttpFetchResult, LegadoUrlOptions, ParsedLegadoUrl, merge_cookie_string,
    normalize_cookie_key, parse_legado_url, resolve_relative_url, try_resolve_data_url,
};
