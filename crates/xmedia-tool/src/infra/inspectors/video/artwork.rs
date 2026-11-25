use std::path::Path;

use xmedia_core::domain::error::DomainResult;
pub(super) use xmedia_formats::video::{
    find_clearlogo_path, find_fanart_path, find_poster_path, find_thumb_path,
};

use crate::infra::atomic_fs::atomic_write_bytes;

/// Atomically writes `{stem_base}.{ext}` inside `dir` and removes any stale sibling with other
/// image extensions (`.jpg`/`.jpeg`/`.png`/`.webp`) so old artwork never shadows the updated file.
pub(crate) fn write_image_replacing_opposite_ext(
    dir: &Path,
    stem_base: &str,
    ext: &str,
    bytes: &[u8],
) -> DomainResult<()> {
    let target = dir.join(format!("{stem_base}.{ext}"));
    atomic_write_bytes(&target, bytes)?;
    for stale_ext in ["jpg", "jpeg", "png", "webp"] {
        if stale_ext != ext {
            let stale_path = dir.join(format!("{stem_base}.{stale_ext}"));
            if stale_path.is_file() {
                let _ = std::fs::remove_file(stale_path);
            }
        }
    }
    Ok(())
}
