use md5::{Digest, Md5};
use serde_json::Value;
use url::Url;
use uuid::Uuid;

use super::{
    SUBSONIC_API_VERSION, SUBSONIC_CLIENT_NAME, SubsonicAlbumDto, SubsonicError, SubsonicSongDto,
};
use crate::opds_client::{parse_filename_from_content_disposition, sanitize_filename_component};

pub fn build_subsonic_url(
    base_url: &str,
    endpoint: &str,
    username: &str,
    password: &str,
    extra_params: &[(&str, String)],
) -> Result<String, SubsonicError> {
    let mut parsed = Url::parse(base_url.trim())
        .map_err(|e| SubsonicError::BadRequest(format!("Invalid Subsonic URL: {e}")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(SubsonicError::BadRequest(format!(
            "Unsupported Subsonic URL scheme '{}'",
            parsed.scheme()
        )));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(SubsonicError::BadRequest(
            "Subsonic base URL must not contain embedded credentials".to_string(),
        ));
    }
    parsed.set_fragment(None);

    let endpoint_clean = endpoint.trim_start_matches('/');
    {
        let mut segments = parsed.path_segments_mut().map_err(|()| {
            SubsonicError::BadRequest("Subsonic base URL cannot be a base".to_string())
        })?;
        segments.pop_if_empty();
    }
    let ends_with_rest = parsed
        .path_segments()
        .and_then(|mut s| s.next_back())
        .is_some_and(|seg| seg.eq_ignore_ascii_case("rest"));

    {
        let mut segments = parsed.path_segments_mut().map_err(|()| {
            SubsonicError::BadRequest("Subsonic base URL cannot be a base".to_string())
        })?;
        if !ends_with_rest {
            segments.push("rest");
        }
        for seg in endpoint_clean.split('/').filter(|s| !s.is_empty()) {
            segments.push(seg);
        }
    }

    let preserved_query: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| {
            !matches!(
                k.as_ref(),
                "u" | "t" | "s" | "p" | "apiKey" | "v" | "c" | "f"
            ) && !extra_params.iter().any(|(ek, _)| ek == k)
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let salt = Uuid::new_v4().simple().to_string();
    let mut pairs = parsed.query_pairs_mut();
    pairs.clear();
    for (k, v) in preserved_query {
        pairs.append_pair(&k, &v);
    }

    let user = username.trim();
    if !user.is_empty() {
        let mut md5 = Md5::new();
        md5.update(password.as_bytes());
        md5.update(salt.as_bytes());
        let token = hex::encode(md5.finalize());
        pairs.append_pair("u", user);
        pairs.append_pair("t", &token);
        pairs.append_pair("s", &salt);
    } else if !password.is_empty() {
        pairs.append_pair("apiKey", password);
    }

    pairs.append_pair("v", SUBSONIC_API_VERSION);
    pairs.append_pair("c", SUBSONIC_CLIENT_NAME);
    pairs.append_pair("f", "json");
    for (k, v) in extra_params {
        pairs.append_pair(k, v);
    }
    drop(pairs);

    Ok(parsed.to_string())
}

pub(super) fn json_value_items(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(arr) => arr.iter().collect(),
        Value::Object(_) => vec![value],
        _ => Vec::new(),
    }
}

pub(super) fn extract_subsonic_ok(root: &Value) -> Result<&Value, SubsonicError> {
    let inner = root
        .get("subsonic-response")
        .ok_or_else(|| SubsonicError::Upstream("Missing 'subsonic-response' envelope".into()))?;
    let status = inner.get("status").and_then(Value::as_str).unwrap_or("");
    if status.eq_ignore_ascii_case("ok") {
        Ok(inner)
    } else {
        let code = inner
            .pointer("/error/code")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let msg = inner
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("Subsonic request failed");
        Err(SubsonicError::Upstream(format!(
            "Subsonic error {code}: {msg}"
        )))
    }
}

/// RFC 3986 path-segment percent-encoding (encodes spaces as `%20`, never `+`).
pub(super) fn encode_segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for &b in raw.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub(super) fn parse_album_dto(source_id: &str, val: &Value) -> Option<SubsonicAlbumDto> {
    let id = val.get("id").and_then(value_to_string)?;
    let name = val
        .get("name")
        .or_else(|| val.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("Unknown Album")
        .to_string();
    let artist = val
        .get("artist")
        .and_then(Value::as_str)
        .unwrap_or("Unknown Artist")
        .to_string();
    let year = val
        .get("year")
        .and_then(Value::as_i64)
        .and_then(|y| i32::try_from(y).ok());
    let song_count = val
        .get("songCount")
        .and_then(Value::as_u64)
        .and_then(|c| u32::try_from(c).ok());
    let duration = val.get("duration").and_then(Value::as_u64);
    let cover_url = val
        .get("coverArt")
        .and_then(value_to_string)
        .filter(|s| !s.is_empty())
        .map(|cid| format!("/_app/subsonic/{source_id}/cover/{}", encode_segment(&cid)));

    Some(SubsonicAlbumDto {
        id,
        name,
        artist,
        year,
        song_count,
        duration,
        cover_url,
    })
}

pub(super) fn parse_song_dto(source_id: &str, val: &Value) -> Option<SubsonicSongDto> {
    let id = val.get("id").and_then(value_to_string)?;
    let title = val
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Unknown Track")
        .to_string();
    let artist = val
        .get("artist")
        .and_then(Value::as_str)
        .unwrap_or("Unknown Artist")
        .to_string();
    let album = val
        .get("album")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let track = val
        .get("track")
        .and_then(Value::as_u64)
        .and_then(|t| u32::try_from(t).ok());
    let duration = val.get("duration").and_then(Value::as_u64);
    let suffix = val
        .get("suffix")
        .and_then(Value::as_str)
        .map(str::to_string);
    let bit_rate = val
        .get("bitRate")
        .and_then(Value::as_u64)
        .and_then(|b| u32::try_from(b).ok());
    let size = val.get("size").and_then(Value::as_u64);
    let cover_url = val
        .get("coverArt")
        .and_then(value_to_string)
        .filter(|s| !s.is_empty())
        .map(|cid| format!("/_app/subsonic/{source_id}/cover/{}", encode_segment(&cid)));
    let stream_url = format!("/_app/subsonic/{source_id}/stream/{}", encode_segment(&id));

    Some(SubsonicSongDto {
        id,
        title,
        artist,
        album,
        track,
        duration,
        suffix,
        bit_rate,
        size,
        cover_url,
        stream_url,
    })
}

fn audio_ext_from_mime(content_type: &str) -> Option<&'static str> {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "audio/mpeg" | "audio/mp3" => Some("mp3"),
        "audio/flac" | "audio/x-flac" => Some("flac"),
        "audio/wav" | "audio/x-wav" | "audio/wave" => Some("wav"),
        "audio/ogg" | "application/ogg" => Some("ogg"),
        "audio/opus" => Some("opus"),
        "audio/mp4" | "audio/x-m4a" | "audio/aac" => Some("m4a"),
        _ => None,
    }
}

fn is_supported_audio_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mp3" | "flac" | "wav" | "ogg" | "opus" | "m4a" | "aac" | "alac" | "aiff" | "wma" | "wv"
    )
}

fn sanitize_base_name(raw: &str) -> String {
    sanitize_filename_component(raw)
}

pub(super) fn resolve_audio_filename(
    filename_hint: Option<&str>,
    content_disposition: Option<&str>,
    content_type: &str,
    meta_title: Option<&str>,
    meta_suffix: Option<&str>,
    song_id: &str,
) -> String {
    let cd_name = content_disposition.and_then(parse_filename_from_content_disposition);

    let base = filename_hint
        .map(sanitize_base_name)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            cd_name
                .map(|s| sanitize_base_name(&s))
                .filter(|s| !s.is_empty())
        })
        .or_else(|| meta_title.map(sanitize_base_name).filter(|s| !s.is_empty()))
        .unwrap_or_else(|| {
            let safe_id = sanitize_base_name(song_id);
            if safe_id.is_empty() {
                "track".to_string()
            } else {
                format!("track-{safe_id}")
            }
        });

    if let Some(ext) = std::path::Path::new(&base)
        .extension()
        .and_then(|s| s.to_str())
        && is_supported_audio_ext(ext)
    {
        return base;
    }

    let ext = meta_suffix
        .filter(|s| is_supported_audio_ext(s))
        .map(str::to_string)
        .or_else(|| audio_ext_from_mime(content_type).map(str::to_string))
        .unwrap_or_else(|| "mp3".to_string());

    format!("{base}.{ext}")
}
