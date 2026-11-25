use std::path::{Path, PathBuf};

use url::Url;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};

const MAX_HLS_SEGMENTS: usize = 10_000;

pub(super) async fn open_exclusive_temp_file(path: &Path) -> std::io::Result<tokio::fs::File> {
    let mut opts = tokio::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    opts.mode(0o600);
    opts.open(path).await
}

pub(super) struct TempFileGuard {
    path: PathBuf,
}

impl TempFileGuard {
    pub(super) async fn create_exclusive(ext: &str) -> DomainResult<(Self, tokio::fs::File)> {
        let clean: String = ext
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(8)
            .collect();
        let suffix = if clean.is_empty() {
            "mp4".to_string()
        } else {
            clean
        };
        for _ in 0..8 {
            let path =
                std::env::temp_dir().join(format!("xmedia-video-{}.{suffix}", Uuid::now_v7()));
            match open_exclusive_temp_file(&path).await {
                Ok(file) => return Ok((Self { path }, file)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(DomainError::Storage(format!(
                        "创建临时视频缓存文件失败: {e}"
                    )));
                }
            }
        }
        Err(DomainError::Storage(
            "创建独占临时视频缓存文件失败: 重试次数耗尽".into(),
        ))
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(super) struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    pub(super) fn new() -> DomainResult<Self> {
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        for _ in 0..8 {
            let path = std::env::temp_dir().join(format!("xmedia-hls-{}", Uuid::now_v7()));
            match builder.create(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(DomainError::Storage(format!("创建 HLS 临时目录失败: {e}")));
                }
            }
        }
        Err(DomainError::Storage(
            "创建独占 HLS 临时目录失败: 重试次数耗尽".into(),
        ))
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn resolve_strict_http_hls_uri(raw_uri: &str, base_url: &Url) -> DomainResult<Url> {
    let clean = raw_uri.trim();
    if clean.is_empty() || clean.bytes().any(|b| b.is_ascii_control() || b == b'\\') {
        return Err(DomainError::Validation("无效的 HLS URI".into()));
    }
    let lower = clean.to_ascii_lowercase();
    if lower.starts_with("file:")
        || lower.starts_with("data:")
        || lower.starts_with("crypto:")
        || lower.starts_with("subfile:")
        || lower.starts_with("concat:")
        || lower.starts_with("pipe:")
        || lower.starts_with("fd:")
    {
        return Err(DomainError::Validation(
            "安全策略拒绝非 HTTP(S) 的 HLS URI 协议".into(),
        ));
    }
    let joined = base_url
        .join(clean)
        .map_err(|e| DomainError::Validation(format!("无效的 HLS URI: {e}")))?;
    if !matches!(joined.scheme(), "http" | "https")
        || joined.host_str().is_none_or(|h| h.is_empty())
    {
        return Err(DomainError::Validation(format!(
            "安全策略仅允许 HTTP/HTTPS HLS URI，已拒绝协议: {}",
            joined.scheme()
        )));
    }
    Ok(joined)
}

pub(super) async fn stage_local_hls_playlist_with<F, Fut>(
    playlist_text: &str,
    base_url: &Url,
    mut fetch_to_local: F,
) -> DomainResult<Vec<String>>
where
    F: FnMut(&Url, String, u64) -> Fut,
    Fut: std::future::Future<Output = DomainResult<u64>>,
{
    let mut total_bytes: u64 = 0;
    let mut seg_idx: usize = 0;
    let mut asset_idx: usize = 0;
    let mut local_lines = Vec::new();

    for raw_line in playlist_text.lines() {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            let Some(prefix4) = trimmed.get(..4) else {
                continue;
            };
            if !prefix4.eq_ignore_ascii_case("#EXT") {
                continue;
            }
            if trimmed
                .get(..13)
                .is_some_and(|p| p.eq_ignore_ascii_case("#EXT-X-DEFINE"))
            {
                return Err(DomainError::Validation(
                    "安全策略拒绝包含 #EXT-X-DEFINE 变量替换的 HLS 播放列表".into(),
                ));
            }
            if trimmed
                .get(..8)
                .is_some_and(|p| p.eq_ignore_ascii_case("#EXTINF:"))
            {
                let rest = &trimmed[8..];
                let dur = rest.split(',').next().unwrap_or(rest).trim();
                local_lines.push(format!("#EXTINF:{dur},"));
                continue;
            }

            let Some((tag_prefix, attr_str)) = trimmed.split_once(':') else {
                if trimmed.to_ascii_lowercase().contains("uri") {
                    return Err(DomainError::Validation(
                        "HLS 标签包含无法解析的 URI 属性".into(),
                    ));
                }
                local_lines.push(trimmed.to_string());
                continue;
            };

            if !attr_str.contains('=') && !attr_str.to_ascii_lowercase().contains("uri") {
                local_lines.push(trimmed.to_string());
                continue;
            }

            let bytes = attr_str.as_bytes();
            let len = bytes.len();
            let mut idx = 0usize;
            let mut out_attrs: Vec<String> = Vec::new();

            while idx < len {
                while idx < len && (bytes[idx].is_ascii_whitespace() || bytes[idx] == b',') {
                    idx += 1;
                }
                if idx >= len {
                    break;
                }
                let key_start = idx;
                while idx < len
                    && (bytes[idx].is_ascii_alphanumeric()
                        || bytes[idx] == b'-'
                        || bytes[idx] == b'_')
                {
                    idx += 1;
                }
                if idx == key_start {
                    return Err(DomainError::Validation("HLS 标签属性名格式无效".into()));
                }
                let key = &attr_str[key_start..idx];
                while idx < len && bytes[idx].is_ascii_whitespace() {
                    idx += 1;
                }
                if idx >= len || bytes[idx] != b'=' {
                    return Err(DomainError::Validation(format!(
                        "HLS 标签属性 {key} 缺少 '=' 号"
                    )));
                }
                idx += 1;
                while idx < len && bytes[idx].is_ascii_whitespace() {
                    idx += 1;
                }
                if idx >= len {
                    return Err(DomainError::Validation(format!(
                        "HLS 标签属性 {key} 值为空"
                    )));
                }

                let (raw_val, was_quoted) = if bytes[idx] == b'"' || bytes[idx] == b'\'' {
                    let quote = bytes[idx];
                    let val_start = idx + 1;
                    let Some(close_rel) = attr_str[val_start..].find(quote as char) else {
                        return Err(DomainError::Validation(format!(
                            "HLS 标签属性 {key} 引号未闭合"
                        )));
                    };
                    let val = &attr_str[val_start..val_start + close_rel];
                    idx = val_start + close_rel + 1;
                    while idx < len && bytes[idx].is_ascii_whitespace() {
                        idx += 1;
                    }
                    if idx < len && bytes[idx] != b',' {
                        return Err(DomainError::Validation(format!(
                            "HLS 标签属性 {key} 分隔符无效"
                        )));
                    }
                    (val, true)
                } else {
                    let val_start = idx;
                    while idx < len && bytes[idx] != b',' && !bytes[idx].is_ascii_whitespace() {
                        idx += 1;
                    }
                    let val = &attr_str[val_start..idx];
                    while idx < len && bytes[idx].is_ascii_whitespace() {
                        idx += 1;
                    }
                    if idx < len && bytes[idx] != b',' {
                        return Err(DomainError::Validation(format!(
                            "HLS 标签属性 {key} 未加引号的值格式无效"
                        )));
                    }
                    (val, false)
                };

                if key.eq_ignore_ascii_case("URI") {
                    let joined = resolve_strict_http_hls_uri(raw_val, base_url)?;
                    asset_idx += 1;
                    let fname = format!("asset_{asset_idx:05}.bin");
                    total_bytes = fetch_to_local(&joined, fname.clone(), total_bytes).await?;
                    out_attrs.push(format!("URI=\"{fname}\""));
                } else if was_quoted {
                    if raw_val.contains('"') || raw_val.bytes().any(|b| b.is_ascii_control()) {
                        return Err(DomainError::Validation(format!(
                            "HLS 标签属性 {key} 包含非法字符"
                        )));
                    }
                    out_attrs.push(format!("{key}=\"{raw_val}\""));
                } else {
                    if raw_val.is_empty()
                        || raw_val
                            .bytes()
                            .any(|b| b.is_ascii_control() || b == b'"' || b == b'\'')
                    {
                        return Err(DomainError::Validation(format!(
                            "HLS 标签属性 {key} 值无效"
                        )));
                    }
                    out_attrs.push(format!("{key}={raw_val}"));
                }
            }

            if out_attrs.is_empty() {
                return Err(DomainError::Validation("HLS 标签属性列表为空".into()));
            }
            local_lines.push(format!("{}:{}", tag_prefix.trim(), out_attrs.join(",")));
            continue;
        }

        let joined = resolve_strict_http_hls_uri(trimmed, base_url)?;
        seg_idx += 1;
        if seg_idx > MAX_HLS_SEGMENTS {
            return Err(DomainError::Validation("HLS 分片数量超过缓存上限".into()));
        }
        let ext = Path::new(joined.path())
            .extension()
            .and_then(|s| s.to_str())
            .map(|e| e.to_ascii_lowercase())
            .filter(|e| {
                matches!(
                    e.as_str(),
                    "ts" | "m4s" | "mp4" | "m4a" | "aac" | "mp3" | "ac3" | "eac3" | "vtt" | "bin"
                )
            })
            .unwrap_or_else(|| "ts".to_string());
        let fname = format!("seg_{seg_idx:06}.{ext}");
        total_bytes = fetch_to_local(&joined, fname.clone(), total_bytes).await?;
        local_lines.push(fname);
    }

    if seg_idx == 0 {
        return Err(DomainError::Validation(
            "HLS 播放列表未包含可缓存分片".into(),
        ));
    }

    Ok(local_lines)
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncWriteExt;

    use super::*;

    #[tokio::test]
    async fn stage_local_hls_playlist_rejects_file_data_and_malformed_uris() {
        let base = Url::parse("https://cdn.example.com/hls/index.m3u8").unwrap();
        let malicious_cases = [
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"file:///etc/passwd\"\n#EXTINF:4.0,\nseg0.ts\n",
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI='file:///etc/passwd'\n#EXTINF:4.0,\nseg0.ts\n",
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=file:///etc/passwd\n#EXTINF:4.0,\nseg0.ts\n",
            "#EXTM3U\n#EXT-X-MAP:URI=\"data:application/octet-stream;base64,AAAA\"\n#EXTINF:4.0,\nseg0.ts\n",
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"crypto:/etc/passwd\"\n#EXTINF:4.0,\nseg0.ts\n",
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"https://cdn.example.com/k.key\n#EXTINF:4.0,\nseg0.ts\n",
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\n#EXTINF:4.0,\nseg0.ts\n",
            "#EXTM3U\n#EXT-X-DEFINE:NAME=\"x\",VALUE=\"file:///etc/passwd\"\n#EXTINF:4.0,\nseg0.ts\n",
            "#EXTM3U\n#EXTINF:4.0,\nfile:///etc/passwd\n",
            "#EXTM3U\n#EXTINF:4.0,\ndata:video/mp2t;base64,AAAA\n",
        ];

        for playlist in malicious_cases {
            let res =
                stage_local_hls_playlist_with(playlist, &base, |_url, _fname, cur| async move {
                    Ok(cur + 16)
                })
                .await;
            assert!(
                matches!(res, Err(DomainError::Validation(_))),
                "expected Validation error for malicious playlist: {playlist}"
            );
        }
    }

    #[tokio::test]
    async fn stage_local_hls_playlist_rewrites_all_http_uri_forms() {
        let base = Url::parse("https://cdn.example.com/hls/index.m3u8").unwrap();
        let playlist = concat!(
            "#EXTM3U\n",
            "#EXT-X-VERSION:6\n",
            "#EXT-X-TARGETDURATION:6\n",
            "# arbitrary comment URI=file:///etc/passwd\n",
            "#EXT-X-KEY:METHOD=AES-128,URI='keys/enc.key',IV=0x00000000000000000000000000000001\n",
            "#EXT-X-MAP:URI=init.mp4\n",
            "#EXTINF:5.500,Episode Title\n",
            "https://cdn.example.com/hls/disguised_seg.png\n",
            "#EXT-X-ENDLIST\n",
        );

        let mut downloaded = Vec::new();
        let lines = stage_local_hls_playlist_with(playlist, &base, |url, fname, cur| {
            downloaded.push((url.to_string(), fname));
            async move { Ok(cur + 128) }
        })
        .await
        .expect("valid HTTP HLS playlist should succeed");

        assert_eq!(
            downloaded,
            vec![
                (
                    "https://cdn.example.com/hls/keys/enc.key".to_string(),
                    "asset_00001.bin".to_string()
                ),
                (
                    "https://cdn.example.com/hls/init.mp4".to_string(),
                    "asset_00002.bin".to_string()
                ),
                (
                    "https://cdn.example.com/hls/disguised_seg.png".to_string(),
                    "seg_000001.ts".to_string()
                ),
            ]
        );
        let joined = lines.join("\n");
        assert!(joined.contains("URI=\"asset_00001.bin\""));
        assert!(joined.contains("URI=\"asset_00002.bin\""));
        assert!(joined.contains("seg_000001.ts"));
        assert!(!joined.contains("/etc/passwd"));
    }

    #[tokio::test]
    async fn temp_file_and_dir_guards_use_exclusive_owner_only_permissions() {
        let (file_guard, mut file) = TempFileGuard::create_exclusive("mp4").await.unwrap();
        let file_path = file_guard.path().to_path_buf();
        file.write_all(b"video-data").await.unwrap();
        drop(file);
        assert!(file_path.is_file());

        let dup_err = open_exclusive_temp_file(&file_path).await.unwrap_err();
        assert_eq!(dup_err.kind(), std::io::ErrorKind::AlreadyExists);

        let dir_guard = TempDirGuard::new().unwrap();
        let dir_path = dir_guard.path().to_path_buf();
        assert!(dir_path.is_dir());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let f_mode = std::fs::metadata(&file_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(f_mode, 0o600);
            let d_mode = std::fs::metadata(&dir_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(d_mode, 0o700);
        }

        drop(file_guard);
        drop(dir_guard);
        assert!(!file_path.exists());
        assert!(!dir_path.exists());
    }
}
