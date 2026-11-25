use std::collections::{HashMap, HashSet};

use xmedia_source::http::{
    LegadoUrlOptions, is_forbidden_custom_header, parse_legado_url, resolve_relative_url,
    split_legado_url_and_raw_options,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct CanonicalRequestKey {
    url_no_frag: String,
    method: String,
    body: Option<String>,
    headers: Vec<(String, String)>,
    charset: Option<String>,
}

impl CanonicalRequestKey {
    fn from_raw(raw_url: &str, source_url: &str) -> Self {
        let parsed = parse_legado_url(raw_url);
        let resolved = resolve_relative_url(&parsed.url, source_url);
        let url_no_frag = strip_fragment(&resolved).to_string();
        let method = parsed
            .options
            .method
            .as_deref()
            .unwrap_or("GET")
            .trim()
            .to_ascii_uppercase();
        let body = if method != "GET" && method != "HEAD" {
            match parsed.options.body {
                Some(serde_json::Value::String(s)) => Some(s),
                Some(serde_json::Value::Null) | None => None,
                Some(other) => Some(other.to_string()),
            }
        } else {
            None
        };
        let mut headers = Vec::new();
        if let Some(serde_json::Value::Object(map)) = parsed.options.headers {
            for (k, v) in map {
                if is_forbidden_custom_header(&k) {
                    continue;
                }
                let val_str = match v {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                };
                headers.push((k.to_ascii_lowercase(), val_str));
            }
            headers.sort();
        }
        let charset = parsed
            .options
            .charset
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty());
        Self {
            url_no_frag,
            method,
            body,
            headers,
            charset,
        }
    }

    fn plain_get(url_no_frag: &str) -> Self {
        Self {
            url_no_frag: url_no_frag.to_string(),
            method: "GET".to_string(),
            body: None,
            headers: Vec::new(),
            charset: None,
        }
    }

    fn is_plain_get_or_head(&self) -> bool {
        matches!(self.method.as_str(), "GET" | "HEAD")
            && self.body.is_none()
            && self.headers.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RedirectTargetKey {
    final_url: String,
    method: String,
    body: Option<String>,
    headers: Vec<(String, String)>,
}

#[derive(Default)]
pub(super) struct PaginationCycleGuard {
    visited_requests: HashSet<CanonicalRequestKey>,
    visited_get_bodies: HashMap<String, u64>,
    redirected_post_final_urls: HashSet<String>,
    visited_non_get_bodies: HashMap<String, HashSet<u64>>,
    visited_redirect_targets: HashSet<RedirectTargetKey>,
}

fn hash_response_body(body: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    body.hash(&mut hasher);
    hasher.finish()
}

impl PaginationCycleGuard {
    pub(super) fn try_begin(
        &mut self,
        page_url: &str,
        source_url: &str,
    ) -> Option<CanonicalRequestKey> {
        let key = CanonicalRequestKey::from_raw(page_url, source_url);
        self.visited_requests.insert(key.clone()).then_some(key)
    }

    pub(super) fn record_fetched(
        &mut self,
        req_key: &CanonicalRequestKey,
        final_url: &str,
        body: &str,
    ) -> bool {
        let final_no_frag = strip_fragment(final_url).to_string();
        let body_hash = hash_response_body(body);
        if req_key.is_plain_get_or_head() {
            if self.visited_get_bodies.contains_key(&final_no_frag)
                || self
                    .visited_non_get_bodies
                    .get(&final_no_frag)
                    .is_some_and(|hashes| hashes.contains(&body_hash))
            {
                return false;
            }
            self.visited_get_bodies
                .insert(final_no_frag.clone(), body_hash);
            if final_no_frag != req_key.url_no_frag {
                self.visited_requests
                    .insert(CanonicalRequestKey::plain_get(&final_no_frag));
            }
            true
        } else {
            let target_key = RedirectTargetKey {
                final_url: final_no_frag.clone(),
                method: req_key.method.clone(),
                body: req_key.body.clone(),
                headers: req_key.headers.clone(),
            };
            if !self.visited_redirect_targets.insert(target_key) {
                return false;
            }
            if self.visited_get_bodies.get(&final_no_frag) == Some(&body_hash) {
                return false;
            }
            if self
                .visited_non_get_bodies
                .get(&final_no_frag)
                .is_some_and(|hashes| hashes.contains(&body_hash))
            {
                return false;
            }
            if final_no_frag != req_key.url_no_frag {
                self.redirected_post_final_urls
                    .insert(final_no_frag.clone());
            }
            self.visited_non_get_bodies
                .entry(final_no_frag)
                .or_default()
                .insert(body_hash);
            true
        }
    }

    pub(super) fn can_enqueue(&self, next_url: &str, source_url: &str) -> bool {
        let next_key = CanonicalRequestKey::from_raw(next_url, source_url);
        if self.visited_requests.contains(&next_key) {
            return false;
        }
        if next_key.is_plain_get_or_head() {
            if self.visited_get_bodies.contains_key(&next_key.url_no_frag)
                || self
                    .redirected_post_final_urls
                    .contains(&next_key.url_no_frag)
            {
                return false;
            }
        } else if self.visited_redirect_targets.contains(&RedirectTargetKey {
            final_url: next_key.url_no_frag.clone(),
            method: next_key.method.clone(),
            body: next_key.body.clone(),
            headers: next_key.headers.clone(),
        }) {
            return false;
        }
        true
    }
}

fn strip_fragment(url: &str) -> &str {
    url.split('#').next().unwrap_or(url)
}

pub(super) fn is_placeholder_or_fragment_url(raw_url: &str, final_url: &str) -> bool {
    let t = raw_url.trim();
    if t.is_empty() {
        return true;
    }
    let (url_part, has_valid_opts) = if let (part, Some(opt_json)) =
        split_legado_url_and_raw_options(t)
        && serde_json::from_str::<LegadoUrlOptions>(opt_json).is_ok()
    {
        (part.trim(), true)
    } else {
        (t, false)
    };
    if url_part.is_empty() {
        return !has_valid_opts;
    }
    if url_part == "#" || url_part.starts_with('#') {
        return true;
    }
    let lower = url_part.to_ascii_lowercase();
    if lower.starts_with("javascript:") || lower.starts_with("about:") {
        return true;
    }
    let final_no_frag = strip_fragment(final_url);
    url_part
        .strip_prefix(final_no_frag)
        .is_some_and(|rest| rest.starts_with('#'))
}
