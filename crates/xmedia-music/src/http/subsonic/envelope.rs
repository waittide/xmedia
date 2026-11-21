use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Number, Value};
use xmedia_core::domain::error::DomainError;
use xmedia_core::domain::xml::xml_escape;

pub const SUBSONIC_API_VERSION: &str = "1.16.1";
pub const SERVER_TYPE: &str = "xmedia";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_JSONP_CALLBACK_LEN: usize = 128;

/// Validates a JSONP callback identifier (`^[A-Za-z_$][0-9A-Za-z_$]*(\.[A-Za-z_$][0-9A-Za-z_$]*)*$`, max 128 bytes).
pub fn is_valid_jsonp_callback(callback: &str) -> bool {
    if callback.is_empty() || callback.len() > MAX_JSONP_CALLBACK_LEN {
        return false;
    }
    callback.split('.').all(|segment| {
        let mut bytes = segment.bytes();
        let Some(first) = bytes.next() else {
            return false;
        };
        let first_ok = first.is_ascii_alphabetic() || first == b'_' || first == b'$';
        first_ok && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseFormat {
    Xml,
    Json,
    Jsonp,
}

#[derive(Debug, Clone)]
pub struct SubsonicParams {
    pairs: Vec<(String, String)>,
}

impl SubsonicParams {
    pub fn from_query_and_form(query: Option<&str>, form_body: &[u8]) -> Self {
        let mut pairs = Vec::new();
        if let Some(q) = query {
            for (k, v) in form_urlencoded::parse(q.as_bytes()) {
                pairs.push((k.into_owned(), v.into_owned()));
            }
        }
        if !form_body.is_empty() {
            for (k, v) in form_urlencoded::parse(form_body) {
                pairs.push((k.into_owned(), v.into_owned()));
            }
        }
        Self { pairs }
    }

    pub fn first(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn all(&self, key: &str) -> Vec<String> {
        self.pairs
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .collect()
    }

    pub fn u32(&self, key: &str) -> Option<u32> {
        self.first(key)?.trim().parse::<u32>().ok()
    }

    pub fn u64(&self, key: &str) -> Option<u64> {
        self.first(key)?.trim().parse::<u64>().ok()
    }

    pub fn i32(&self, key: &str) -> Option<i32> {
        self.first(key)?.trim().parse::<i32>().ok()
    }

    pub fn bool_or(&self, key: &str, default: bool) -> bool {
        match self.first(key).map(|s| s.trim().to_ascii_lowercase()) {
            Some(ref s) if s == "true" || s == "1" => true,
            Some(ref s) if s == "false" || s == "0" => false,
            _ => default,
        }
    }

    pub fn response_format(&self) -> ResponseFormat {
        match self.first("f").map(|s| s.trim().to_ascii_lowercase()) {
            Some(ref f) if f == "json" => ResponseFormat::Json,
            Some(ref f) if f == "jsonp" => ResponseFormat::Jsonp,
            _ => ResponseFormat::Xml,
        }
    }

    pub fn jsonp_callback(&self) -> &str {
        self.first("callback").unwrap_or("callback")
    }
}

#[derive(Debug, Clone)]
pub enum SsVal {
    Str(String),
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
    IntList(Vec<i64>),
}

impl SsVal {
    fn to_xml_attr(&self) -> String {
        match self {
            Self::Str(s) => xml_escape(s),
            Self::Bool(b) => b.to_string(),
            Self::Int(i) => i.to_string(),
            Self::Uint(u) => u.to_string(),
            Self::Float(f) => format!("{f:.2}"),
            Self::IntList(list) => list
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        }
    }

    fn to_json_val(&self) -> Value {
        match self {
            Self::Str(s) => Value::String(s.clone()),
            Self::Bool(b) => Value::Bool(*b),
            Self::Int(i) => Value::Number((*i).into()),
            Self::Uint(u) => Value::Number((*u).into()),
            Self::Float(f) => Number::from_f64(*f)
                .map(Value::Number)
                .unwrap_or(Value::Null),
            Self::IntList(list) => {
                Value::Array(list.iter().map(|&n| Value::Number(n.into())).collect())
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SsNode {
    pub attrs: Vec<(&'static str, SsVal)>,
    pub text: Option<String>,
    pub xml_int_children: Vec<(&'static str, Vec<i64>)>,
    pub children_single: Vec<(&'static str, SsNode)>,
    pub children_lists: Vec<(&'static str, Vec<SsNode>)>,
}

impl SsNode {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn attr_str(mut self, key: &'static str, val: impl Into<String>) -> Self {
        self.attrs.push((key, SsVal::Str(val.into())));
        self
    }

    pub fn attr_opt_str(mut self, key: &'static str, val: Option<impl Into<String>>) -> Self {
        if let Some(v) = val {
            self.attrs.push((key, SsVal::Str(v.into())));
        }
        self
    }

    pub fn attr_bool(mut self, key: &'static str, val: bool) -> Self {
        self.attrs.push((key, SsVal::Bool(val)));
        self
    }

    pub fn attr_int(mut self, key: &'static str, val: i64) -> Self {
        self.attrs.push((key, SsVal::Int(val)));
        self
    }

    pub fn attr_opt_int(mut self, key: &'static str, val: Option<i64>) -> Self {
        if let Some(v) = val {
            self.attrs.push((key, SsVal::Int(v)));
        }
        self
    }

    pub fn attr_uint(mut self, key: &'static str, val: u64) -> Self {
        self.attrs.push((key, SsVal::Uint(val)));
        self
    }

    pub fn attr_opt_uint(mut self, key: &'static str, val: Option<u64>) -> Self {
        if let Some(v) = val {
            self.attrs.push((key, SsVal::Uint(v)));
        }
        self
    }

    pub fn attr_opt_float(mut self, key: &'static str, val: Option<f32>) -> Self {
        if let Some(v) = val {
            self.attrs.push((key, SsVal::Float(f64::from(v))));
        }
        self
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    pub fn child_single(mut self, tag: &'static str, node: SsNode) -> Self {
        self.children_single.push((tag, node));
        self
    }

    pub fn child_list(mut self, tag: &'static str, nodes: Vec<SsNode>) -> Self {
        self.children_lists.push((tag, nodes));
        self
    }

    pub fn child_int_list(mut self, tag: &'static str, values: Vec<i64>) -> Self {
        self.xml_int_children.push((tag, values));
        self
    }

    fn write_xml(&self, tag: &str, out: &mut String) {
        out.push('<');
        out.push_str(tag);
        for (k, v) in &self.attrs {
            out.push(' ');
            out.push_str(k);
            out.push_str("=\"");
            out.push_str(&v.to_xml_attr());
            out.push('"');
        }

        let has_body = self.text.is_some()
            || !self.xml_int_children.is_empty()
            || !self.children_single.is_empty()
            || self.children_lists.iter().any(|(_, l)| !l.is_empty());

        if !has_body {
            out.push_str("/>");
            return;
        }

        out.push('>');
        if let Some(ref txt) = self.text {
            out.push_str(&xml_escape(txt));
        }
        for (itag, nums) in &self.xml_int_children {
            for n in nums {
                out.push('<');
                out.push_str(itag);
                out.push('>');
                out.push_str(&n.to_string());
                out.push_str("</");
                out.push_str(itag);
                out.push('>');
            }
        }
        for (ctag, node) in &self.children_single {
            node.write_xml(ctag, out);
        }
        for (ltag, list) in &self.children_lists {
            for item in list {
                item.write_xml(ltag, out);
            }
        }
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
    }

    fn to_json_object(&self) -> Value {
        let mut map = Map::new();
        for (k, v) in &self.attrs {
            map.insert((*k).to_string(), v.to_json_val());
        }
        if let Some(ref txt) = self.text {
            map.insert("value".to_string(), Value::String(txt.clone()));
        }
        for (itag, nums) in &self.xml_int_children {
            map.insert(
                (*itag).to_string(),
                SsVal::IntList(nums.clone()).to_json_val(),
            );
        }
        for (ctag, node) in &self.children_single {
            map.insert((*ctag).to_string(), node.to_json_object());
        }
        for (ltag, list) in &self.children_lists {
            map.insert(
                (*ltag).to_string(),
                Value::Array(list.iter().map(Self::to_json_object).collect()),
            );
        }
        Value::Object(map)
    }
}

#[derive(Debug, Clone)]
pub struct SubsonicError {
    pub code: i64,
    pub message: String,
}

impl SubsonicError {
    pub fn missing_param(param: &str) -> Self {
        Self {
            code: 10,
            message: format!("Required parameter '{param}' is missing."),
        }
    }

    pub fn generic(msg: impl Into<String>) -> Self {
        Self {
            code: 0,
            message: msg.into(),
        }
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self {
            code: 70,
            message: msg.into(),
        }
    }
}

impl From<DomainError> for SubsonicError {
    fn from(err: DomainError) -> Self {
        match err {
            DomainError::Validation(m) => Self {
                code: 10,
                message: m,
            },
            DomainError::Unauthorized(m) => Self {
                code: 40,
                message: m,
            },
            DomainError::Forbidden(m) => Self {
                code: 50,
                message: m,
            },
            DomainError::NotFound(m) => Self {
                code: 70,
                message: m,
            },
            DomainError::Storage(detail) => {
                tracing::error!(detail = %detail, "subsonic internal storage error");
                Self {
                    code: 0,
                    message: "Internal storage error".to_string(),
                }
            }
            DomainError::Persistence(detail) => {
                tracing::error!(detail = %detail, "subsonic internal persistence error");
                Self {
                    code: 0,
                    message: "Internal database error".to_string(),
                }
            }
            DomainError::Conflict(m) | DomainError::UnsupportedFormat(m) => Self {
                code: 0,
                message: m,
            },
            other => Self {
                code: 0,
                message: other.to_string(),
            },
        }
    }
}

pub fn render_ok(params: &SubsonicParams, payload: Option<(&'static str, SsNode)>) -> Response {
    render_envelope(params, true, payload, None)
}

pub fn render_array_root(
    params: &SubsonicParams,
    root_key: &'static str,
    xml_child_tag: &'static str,
    items: Vec<SsNode>,
) -> Response {
    match params.response_format() {
        ResponseFormat::Xml => {
            let mut xml = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><subsonic-response xmlns=\"http://subsonic.org/restapi\" status=\"ok\" version=\"{SUBSONIC_API_VERSION}\" type=\"{SERVER_TYPE}\" serverVersion=\"{SERVER_VERSION}\" openSubsonic=\"true\">"
            );
            for item in &items {
                item.write_xml(xml_child_tag, &mut xml);
            }
            xml.push_str("</subsonic-response>");
            xml_http_response(xml)
        }
        fmt => {
            let mut root = base_json_envelope("ok");
            root.insert(
                root_key.to_string(),
                Value::Array(items.iter().map(SsNode::to_json_object).collect()),
            );
            json_http_response(fmt, params.jsonp_callback(), root)
        }
    }
}

pub fn render_err(params: &SubsonicParams, err: SubsonicError) -> Response {
    let err_node = SsNode::new()
        .attr_int("code", err.code)
        .attr_str("message", err.message);
    render_envelope(params, false, None, Some(err_node))
}

fn render_envelope(
    params: &SubsonicParams,
    ok: bool,
    payload: Option<(&'static str, SsNode)>,
    error: Option<SsNode>,
) -> Response {
    let status_str = if ok { "ok" } else { "failed" };
    match params.response_format() {
        ResponseFormat::Xml => {
            let mut xml = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><subsonic-response xmlns=\"http://subsonic.org/restapi\" status=\"{status_str}\" version=\"{SUBSONIC_API_VERSION}\" type=\"{SERVER_TYPE}\" serverVersion=\"{SERVER_VERSION}\" openSubsonic=\"true\""
            );
            if payload.is_none() && error.is_none() {
                xml.push_str("/>");
            } else {
                xml.push('>');
                if let Some((tag, node)) = payload {
                    node.write_xml(tag, &mut xml);
                }
                if let Some(err_node) = error {
                    err_node.write_xml("error", &mut xml);
                }
                xml.push_str("</subsonic-response>");
            }
            xml_http_response(xml)
        }
        fmt => {
            let mut root = base_json_envelope(status_str);
            if let Some((tag, node)) = payload {
                root.insert(tag.to_string(), node.to_json_object());
            }
            if let Some(err_node) = error {
                root.insert("error".to_string(), err_node.to_json_object());
            }
            json_http_response(fmt, params.jsonp_callback(), root)
        }
    }
}

fn base_json_envelope(status: &str) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("status".to_string(), Value::String(status.to_string()));
    map.insert(
        "version".to_string(),
        Value::String(SUBSONIC_API_VERSION.to_string()),
    );
    map.insert("type".to_string(), Value::String(SERVER_TYPE.to_string()));
    map.insert(
        "serverVersion".to_string(),
        Value::String(SERVER_VERSION.to_string()),
    );
    map.insert("openSubsonic".to_string(), Value::Bool(true));
    map
}

fn xml_http_response(xml: String) -> Response {
    let mut res = (StatusCode::OK, Body::from(xml)).into_response();
    let headers = res.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/xml; charset=utf-8"),
    );
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    res
}

fn json_http_response(fmt: ResponseFormat, callback: &str, root: Map<String, Value>) -> Response {
    let mut wrapper = Map::new();
    wrapper.insert("subsonic-response".to_string(), Value::Object(root));
    let json_text = serde_json::to_string(&Value::Object(wrapper)).unwrap_or_default();

    match fmt {
        ResponseFormat::Jsonp => {
            if !is_valid_jsonp_callback(callback) {
                let mut err_root = base_json_envelope("failed");
                let err_node = SsNode::new()
                    .attr_int("code", 10)
                    .attr_str("message", "Invalid JSONP callback parameter.");
                err_root.insert("error".to_string(), err_node.to_json_object());
                let mut err_wrapper = Map::new();
                err_wrapper.insert("subsonic-response".to_string(), Value::Object(err_root));
                let err_json =
                    serde_json::to_string(&Value::Object(err_wrapper)).unwrap_or_default();
                let mut res = (StatusCode::BAD_REQUEST, Body::from(err_json)).into_response();
                res.headers_mut().insert(
                    CONTENT_TYPE,
                    HeaderValue::from_static("application/json; charset=utf-8"),
                );
                res.headers_mut()
                    .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
                return res;
            }
            let body = format!("{callback}({json_text});");
            let mut res = (StatusCode::OK, Body::from(body)).into_response();
            res.headers_mut().insert(
                CONTENT_TYPE,
                HeaderValue::from_static("application/javascript; charset=utf-8"),
            );
            res.headers_mut()
                .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
            res
        }
        _ => {
            let mut res = (StatusCode::OK, Body::from(json_text)).into_response();
            let headers = res.headers_mut();
            headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_static("application/json; charset=utf-8"),
            );
            headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
            res
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[test]
    fn jsonp_callback_validation_blocks_xss_and_allows_valid_identifiers() {
        for valid in [
            "callback",
            "jQuery123456_789",
            "_cb",
            "$fn",
            "window.myApp.cb_1",
            &"a".repeat(MAX_JSONP_CALLBACK_LEN),
        ] {
            assert!(is_valid_jsonp_callback(valid), "should allow {valid}");
        }

        for invalid in [
            "",
            "alert(1)//",
            "cb;alert(1)",
            "<script>alert(1)</script>",
            "foo.bar()",
            "123abc",
            ".leadingDot",
            "trailingDot.",
            "a..b",
            "a.1b",
            "cb\nalert(1)",
            &"a".repeat(MAX_JSONP_CALLBACK_LEN + 1),
        ] {
            assert!(!is_valid_jsonp_callback(invalid), "should reject {invalid}");
        }
    }

    #[tokio::test]
    async fn render_ok_rejects_malicious_jsonp_callback_with_400_json() {
        let params = SubsonicParams::from_query_and_form(Some("f=jsonp&callback=alert(1)//"), &[]);
        let resp = render_ok(&params, None);
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            resp.headers().get(CONTENT_TYPE).unwrap(),
            "application/json; charset=utf-8"
        );
        assert_eq!(
            resp.headers().get(X_CONTENT_TYPE_OPTIONS).unwrap(),
            "nosniff"
        );
        let bytes = to_bytes(resp.into_body(), 1024 * 64).await.unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(!body.contains("alert(1)"));
        assert!(body.contains("\"status\":\"failed\""));
    }

    #[tokio::test]
    async fn render_ok_wraps_valid_jsonp_callback() {
        let params =
            SubsonicParams::from_query_and_form(Some("f=jsonp&callback=myApp.onData"), &[]);
        let resp = render_ok(&params, None);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(CONTENT_TYPE).unwrap(),
            "application/javascript; charset=utf-8"
        );
        let bytes = to_bytes(resp.into_body(), 1024 * 64).await.unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(body.starts_with("myApp.onData({"));
        assert!(body.ends_with("});"));
    }
}
