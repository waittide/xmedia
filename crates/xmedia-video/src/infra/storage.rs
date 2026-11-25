use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use bytes::Bytes;
use quick_xml::Reader;
use quick_xml::events::Event;
use walkdir::WalkDir;
use xmedia_core::application::ports::{BinaryPayload, ByteRangeSpec, ByteRangeStream};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, LibraryKind};
use xmedia_core::infra::fingerprint::{
    fast_bytes_fingerprint, fast_content_fingerprint, max_mtime_secs,
};
use xmedia_core::infra::stream::{move_or_copy_file, open_file_range_stream_spec, sanitize_join};
use xmedia_core::infra::thumbnail::ThumbnailStore;

use crate::application::ports::{
    ParsedNfoMetadata, ScannedVideoEntry, StoredManagedVideo, VideoScannerPort, VideoStoragePort,
};
use crate::domain::filename_parser::parse_sidecar_subtitle_lang_and_label;
use crate::domain::id::VideoId;
use crate::domain::video::{
    SidecarSubtitle, VideoContainer, VideoItem, VideoSource, is_subtitle_extension,
};

#[derive(Clone)]
pub struct LocalVideoStorage {
    uploads_dir: PathBuf,
    subtitles_dir: PathBuf,
    posters: ThumbnailStore,
}

impl LocalVideoStorage {
    pub fn new(data_dir: impl AsRef<Path>) -> DomainResult<Self> {
        let base = data_dir.as_ref().join("video");
        let uploads_dir = base.join("uploads");
        let subtitles_dir = base.join("subtitles");
        for dir in [&uploads_dir, &subtitles_dir] {
            std::fs::create_dir_all(dir).map_err(|e| {
                DomainError::Storage(format!("failed to create video dir {}: {e}", dir.display()))
            })?;
        }
        let posters = ThumbnailStore::new(&base)?;
        Ok(Self {
            uploads_dir,
            subtitles_dir,
            posters,
        })
    }

    fn resolve_video_path(&self, library: &Library, video: &VideoItem) -> DomainResult<PathBuf> {
        match (&library.kind, &video.source) {
            (LibraryKind::Managed, VideoSource::Managed { storage_key, .. }) => {
                sanitize_join(&self.uploads_dir, storage_key)
            }
            (LibraryKind::Managed, VideoSource::OnlineTracked(info)) => {
                if info.cached
                    && let Some(ref key) = info.storage_key
                {
                    sanitize_join(&self.uploads_dir, key)
                } else {
                    Err(DomainError::NotFound(
                        "online-tracked video is not cached yet".into(),
                    ))
                }
            }
            (
                LibraryKind::LocalMount { root_path },
                VideoSource::LocalPath { relative_path, .. },
            ) => sanitize_join(Path::new(root_path), relative_path),
            _ => Err(DomainError::Validation(
                "mismatched library kind and video source".into(),
            )),
        }
    }
}

#[async_trait]
impl VideoStoragePort for LocalVideoStorage {
    async fn save_managed_video(
        &self,
        video_id: VideoId,
        container: VideoContainer,
        bytes: Bytes,
    ) -> DomainResult<StoredManagedVideo> {
        let storage_key = format!("{}.{}", video_id.0, container.extension());
        let dest = self.uploads_dir.join(&storage_key);
        let file_hash = fast_bytes_fingerprint(&bytes);
        let file_size = bytes.len() as u64;

        tokio::fs::write(&dest, &bytes)
            .await
            .map_err(|e| DomainError::Storage(format!("failed saving managed video: {e}")))?;

        Ok(StoredManagedVideo {
            storage_key,
            absolute_path: dest.to_string_lossy().into_owned(),
            file_size,
            file_hash,
        })
    }

    async fn save_managed_video_from_path(
        &self,
        video_id: VideoId,
        container: VideoContainer,
        src_path: &Path,
    ) -> DomainResult<StoredManagedVideo> {
        let storage_key = format!("{}.{}", video_id.0, container.extension());
        let dest = self.uploads_dir.join(&storage_key);
        let file_size = move_or_copy_file(src_path, &dest).await?;
        let dest_for_hash = dest.clone();
        let (_, file_hash) =
            tokio::task::spawn_blocking(move || fast_content_fingerprint(&dest_for_hash))
                .await
                .map_err(|e| {
                    DomainError::Storage(format!("video fingerprint task failed: {e}"))
                })??;

        Ok(StoredManagedVideo {
            storage_key,
            absolute_path: dest.to_string_lossy().into_owned(),
            file_size,
            file_hash,
        })
    }

    async fn save_managed_subtitle(
        &self,
        video_id: VideoId,
        sub_id: &str,
        ext: &str,
        bytes: Bytes,
    ) -> DomainResult<String> {
        let clean_ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
        let key = format!("{}_{sub_id}.{clean_ext}", video_id.0);
        let dest = self.subtitles_dir.join(&key);
        tokio::fs::write(&dest, &bytes)
            .await
            .map_err(|e| DomainError::Storage(format!("failed saving managed subtitle: {e}")))?;
        Ok(key)
    }

    async fn save_managed_subtitle_from_path(
        &self,
        video_id: VideoId,
        sub_id: &str,
        ext: &str,
        src_path: &Path,
    ) -> DomainResult<String> {
        let clean_ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
        let key = format!("{}_{sub_id}.{clean_ext}", video_id.0);
        let dest = self.subtitles_dir.join(&key);
        tokio::fs::copy(src_path, &dest).await.map_err(|e| {
            DomainError::Storage(format!(
                "failed saving managed subtitle from temp file: {e}"
            ))
        })?;
        Ok(key)
    }

    async fn delete_managed_file(&self, key: &str) -> DomainResult<()> {
        for base in [&self.uploads_dir, &self.subtitles_dir] {
            if let Ok(path) = sanitize_join(base, key)
                && path.exists()
            {
                let _ = tokio::fs::remove_file(path).await;
            }
        }
        Ok(())
    }

    async fn save_poster_bytes(&self, bytes: &[u8]) -> DomainResult<String> {
        self.posters.store_cover_bytes_async(bytes.to_vec()).await
    }

    async fn save_generated_poster_bytes(&self, bytes: &[u8]) -> DomainResult<String> {
        if xmedia_core::infra::thumbnail::sniff_image_mime(bytes) == "image/svg+xml" {
            return self
                .posters
                .store_trusted_svg_bytes_async(bytes.to_vec())
                .await;
        }
        self.posters.store_cover_bytes_async(bytes.to_vec()).await
    }

    async fn read_poster(&self, key: &str, max_size: Option<u32>) -> DomainResult<BinaryPayload> {
        self.posters
            .load_cover_async(key.to_string(), max_size)
            .await
    }

    async fn resolve_video_fs_path(
        &self,
        library: &Library,
        video: &VideoItem,
    ) -> DomainResult<String> {
        let p = self.resolve_video_path(library, video)?;
        Ok(p.to_string_lossy().into_owned())
    }

    async fn read_sidecar_subtitle_bytes(
        &self,
        library: &Library,
        video: &VideoItem,
        sub_id: &str,
    ) -> DomainResult<(String, Vec<u8>)> {
        let sub = video
            .source
            .sidecar_subtitles()
            .iter()
            .find(|s| s.id == sub_id)
            .ok_or_else(|| DomainError::NotFound("subtitle track not found".into()))?;

        let (root_dir, path) = match (&library.kind, &video.source) {
            (LibraryKind::Managed, VideoSource::Managed { .. } | VideoSource::OnlineTracked(_)) => {
                (
                    self.subtitles_dir.clone(),
                    sanitize_join(&self.subtitles_dir, &sub.path_or_key)?,
                )
            }
            (LibraryKind::LocalMount { root_path }, VideoSource::LocalPath { .. }) => (
                PathBuf::from(root_path),
                sanitize_join(Path::new(root_path), &sub.path_or_key)?,
            ),
            _ => {
                return Err(DomainError::Validation(
                    "mismatched library kind and video source".into(),
                ));
            }
        };

        let bytes = read_regular_bounded_async(&path, Some(&root_dir), MAX_SIDECAR_SUBTITLE_BYTES)
            .await?
            .ok_or_else(|| DomainError::NotFound("subtitle file not found or rejected".into()))?;
        Ok((sub.format.clone(), bytes))
    }

    async fn open_video_range_stream(
        &self,
        library: &Library,
        video: &VideoItem,
        spec: ByteRangeSpec,
    ) -> DomainResult<ByteRangeStream> {
        let path = self.resolve_video_path(library, video)?;
        open_file_range_stream_spec(&path, spec).await
    }
}

#[derive(Default)]
pub struct LocalVideoScanner;

#[async_trait]
impl VideoScannerPort for LocalVideoScanner {
    async fn scan_video_entries(&self, root_path: &str) -> DomainResult<Vec<ScannedVideoEntry>> {
        let root = PathBuf::from(root_path);
        tokio::task::spawn_blocking(move || -> DomainResult<Vec<ScannedVideoEntry>> {
            if !root.is_dir() {
                return Err(DomainError::Storage(format!(
                    "video mount root missing: {}",
                    root.display()
                )));
            }

            let mut dir_posters: HashMap<PathBuf, Option<String>> = HashMap::new();
            let mut single_video_dirs: HashMap<PathBuf, bool> = HashMap::new();
            let mut dir_subtitles: HashMap<PathBuf, Vec<CachedSubtitleFile>> = HashMap::new();
            let mut shared_nfo_cache: HashMap<PathBuf, Option<ParsedNfoMetadata>> = HashMap::new();
            let mut out = Vec::new();

            for entry in WalkDir::new(&root)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| {
                    e.depth() == 0
                        || !e
                            .file_name()
                            .to_str()
                            .is_some_and(|name| name.starts_with('.'))
                })
            {
                let Ok(entry) = entry else { continue };
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Some(container) = VideoContainer::from_extension(ext) else {
                    continue;
                };
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                let Ok(rel) = path.strip_prefix(&root) else {
                    continue;
                };
                let relative_path = path_to_slash(rel);
                let parent_dir = path.parent().unwrap_or(&root).to_path_buf();
                let video_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("video");
                let single_video_dir = *single_video_dirs
                    .entry(parent_dir.clone())
                    .or_insert_with(|| dir_has_single_video(&parent_dir));

                let sidecar_subtitles =
                    discover_sidecar_subtitles(&root, &parent_dir, video_stem, &mut dir_subtitles);
                let directory_poster_path = discover_video_poster(
                    &root,
                    &parent_dir,
                    video_stem,
                    single_video_dir,
                    &mut dir_posters,
                );
                let nfo = discover_and_parse_nfo(
                    &root,
                    &parent_dir,
                    video_stem,
                    single_video_dir,
                    &mut shared_nfo_cache,
                );

                let mut tracked_sidecars: Vec<PathBuf> =
                    nfo_candidate_paths(&root, &parent_dir, video_stem, single_video_dir);
                if let Some(ref poster_p) = directory_poster_path {
                    tracked_sidecars.push(PathBuf::from(poster_p));
                }
                for sub in &sidecar_subtitles {
                    tracked_sidecars.push(root.join(&sub.path_or_key));
                }
                let file_mtime = max_mtime_secs(path, &tracked_sidecars);

                out.push(ScannedVideoEntry {
                    absolute_path: path.to_string_lossy().into_owned(),
                    relative_path,
                    container,
                    file_size: meta.len(),
                    file_mtime,
                    sidecar_subtitles,
                    directory_poster_path,
                    nfo,
                });
            }

            out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
            Ok(out)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("video scan task panicked: {e}")))?
    }

    async fn read_external_image(&self, path: &str) -> DomainResult<Option<Vec<u8>>> {
        read_regular_bounded_async(Path::new(path), None, MAX_EXTERNAL_POSTER_BYTES).await
    }

    async fn compute_file_fingerprint(&self, path: &Path) -> DomainResult<String> {
        let fp_path = path.to_path_buf();
        let (_, hash) = tokio::task::spawn_blocking(move || fast_content_fingerprint(&fp_path))
            .await
            .map_err(|e| DomainError::Storage(format!("video fingerprint task failed: {e}")))??;
        Ok(hash)
    }
}

struct CachedSubtitleFile {
    file_name: String,
    stem_lower: String,
    ext_lower: String,
    rel_slash_path: String,
}

fn path_to_slash(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

const MAX_SIDECAR_SUBTITLE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_EXTERNAL_POSTER_BYTES: u64 = 32 * 1024 * 1024;
const MAX_NFO_BYTES: u64 = 2 * 1024 * 1024;

fn is_regular_non_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|m| !m.file_type().is_symlink() && m.is_file())
        .unwrap_or(false)
}

async fn read_regular_bounded_async(
    path: &Path,
    expected_root: Option<&Path>,
    max_bytes: u64,
) -> DomainResult<Option<Vec<u8>>> {
    let path_owned = path.to_path_buf();
    let root_owned = expected_root.map(Path::to_path_buf);
    tokio::task::spawn_blocking(move || {
        let Ok(meta) = std::fs::symlink_metadata(&path_owned) else {
            return Ok(None);
        };
        if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > max_bytes {
            return Ok(None);
        }
        let target = if let Some(ref root) = root_owned {
            let Ok(canon_root) = std::fs::canonicalize(root) else {
                return Ok(None);
            };
            let Ok(canon_file) = std::fs::canonicalize(&path_owned) else {
                return Ok(None);
            };
            if !canon_file.starts_with(&canon_root) {
                return Ok(None);
            }
            canon_file
        } else {
            path_owned
        };
        let bytes = std::fs::read(&target)
            .map_err(|e| DomainError::Storage(format!("failed reading file: {e}")))?;
        if (bytes.len() as u64) > max_bytes {
            return Ok(None);
        }
        Ok(Some(bytes))
    })
    .await
    .map_err(|e| DomainError::Storage(format!("file read task failed: {e}")))?
}

fn scan_dir_subtitles(root: &Path, parent_dir: &Path) -> Vec<CachedSubtitleFile> {
    let Ok(read_dir) = std::fs::read_dir(parent_dir) else {
        return Vec::new();
    };
    let mut files: Vec<CachedSubtitleFile> = Vec::new();
    for item in read_dir.flatten() {
        let p = item.path();
        if !is_regular_non_symlink(&p) {
            continue;
        }
        let Some(ext) = p.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        if !is_subtitle_extension(ext) {
            continue;
        }
        let Some(fname) = p.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(sub_stem) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(rel) = p.strip_prefix(root) else {
            continue;
        };
        files.push(CachedSubtitleFile {
            file_name: fname.to_string(),
            stem_lower: sub_stem.to_lowercase(),
            ext_lower: ext.to_ascii_lowercase(),
            rel_slash_path: path_to_slash(rel),
        });
    }
    files.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    files
}

fn discover_sidecar_subtitles(
    root: &Path,
    parent_dir: &Path,
    video_stem: &str,
    dir_subtitles_cache: &mut HashMap<PathBuf, Vec<CachedSubtitleFile>>,
) -> Vec<SidecarSubtitle> {
    let cached = dir_subtitles_cache
        .entry(parent_dir.to_path_buf())
        .or_insert_with(|| scan_dir_subtitles(root, parent_dir));

    let stem_lower = video_stem.to_lowercase();
    let prefix_dot = format!("{stem_lower}.");

    cached
        .iter()
        .filter(|sub| sub.stem_lower == stem_lower || sub.stem_lower.starts_with(&prefix_dot))
        .enumerate()
        .map(|(idx, sub)| {
            let (language, label) =
                parse_sidecar_subtitle_lang_and_label(video_stem, &sub.file_name);
            SidecarSubtitle {
                id: format!("s{idx}"),
                format: sub.ext_lower.clone(),
                language,
                label,
                path_or_key: sub.rel_slash_path.clone(),
            }
        })
        .collect()
}

fn dir_has_single_video(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut count = 0usize;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file()
            && p.extension()
                .and_then(|s| s.to_str())
                .and_then(VideoContainer::from_extension)
                .is_some()
        {
            count += 1;
            if count > 1 {
                return false;
            }
        }
    }
    count == 1
}

fn is_season_dir_name(dir: &Path) -> bool {
    let Some(name) = dir.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    let l = name.trim().to_ascii_lowercase();
    l == "specials"
        || l.starts_with("season")
        || (l.starts_with('s') && l.len() <= 4 && l[1..].chars().all(|c| c.is_ascii_digit()))
        || (l.starts_with('第') && l.ends_with('季'))
}

fn discover_video_poster(
    root: &Path,
    parent_dir: &Path,
    video_stem: &str,
    single_video_dir: bool,
    dir_cache: &mut HashMap<PathBuf, Option<String>>,
) -> Option<String> {
    for ext in ["jpg", "jpeg", "png", "webp"] {
        for candidate_name in [
            format!("{video_stem}-poster.{ext}"),
            format!("{video_stem}-thumb.{ext}"),
            format!("{video_stem}.{ext}"),
        ] {
            let p = parent_dir.join(candidate_name);
            if is_regular_non_symlink(&p) {
                return Some(p.to_string_lossy().into_owned());
            }
        }
    }

    let poster_names = [
        "poster.jpg",
        "poster.jpeg",
        "poster.png",
        "poster.webp",
        "folder.jpg",
        "folder.png",
        "folder.webp",
        "cover.jpg",
        "cover.png",
        "cover.webp",
    ];

    let allow_dir_poster = single_video_dir
        || is_regular_non_symlink(&parent_dir.join("tvshow.nfo"))
        || is_season_dir_name(parent_dir);
    if allow_dir_poster {
        let found_in_dir = dir_cache
            .entry(parent_dir.to_path_buf())
            .or_insert_with(|| {
                for name in poster_names {
                    let p = parent_dir.join(name);
                    if is_regular_non_symlink(&p) {
                        return Some(p.to_string_lossy().into_owned());
                    }
                }
                None
            })
            .clone();
        if found_in_dir.is_some() {
            return found_in_dir;
        }
    }

    // Jellyfin `Show/Season XX/` fallback: look up series-level `poster.*` in parent show directory
    if (is_season_dir_name(parent_dir) || is_regular_non_symlink(&parent_dir.join("season.nfo")))
        && parent_dir != root
        && let Some(show_dir) = parent_dir.parent()
        && show_dir != root
        && show_dir.starts_with(root)
    {
        return dir_cache
            .entry(show_dir.to_path_buf())
            .or_insert_with(|| {
                for name in poster_names {
                    let p = show_dir.join(name);
                    if is_regular_non_symlink(&p) {
                        return Some(p.to_string_lossy().into_owned());
                    }
                }
                None
            })
            .clone();
    }

    None
}

fn nfo_candidate_paths(
    root: &Path,
    parent_dir: &Path,
    video_stem: &str,
    single_video_dir: bool,
) -> Vec<PathBuf> {
    let mut candidates = vec![parent_dir.join(format!("{video_stem}.nfo"))];
    if single_video_dir {
        candidates.push(parent_dir.join("movie.nfo"));
    }
    candidates.push(parent_dir.join("season.nfo"));
    candidates.push(parent_dir.join("tvshow.nfo"));
    if parent_dir != root
        && let Some(show_dir) = parent_dir.parent()
        && show_dir != root
        && show_dir.starts_with(root)
    {
        candidates.push(show_dir.join("tvshow.nfo"));
    }
    candidates
}

fn read_and_parse_nfo(path: &Path) -> Option<ParsedNfoMetadata> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_NFO_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let (xml, _) = xmedia_core::infra::stream::decode_text_smart(&bytes);
    parse_nfo_xml(&xml)
}

fn discover_and_parse_nfo(
    root: &Path,
    parent_dir: &Path,
    video_stem: &str,
    single_video_dir: bool,
    shared_nfo_cache: &mut HashMap<PathBuf, Option<ParsedNfoMetadata>>,
) -> Option<ParsedNfoMetadata> {
    let candidates = nfo_candidate_paths(root, parent_dir, video_stem, single_video_dir);

    let mut primary: Option<ParsedNfoMetadata> = None;
    for (idx, p) in candidates.into_iter().enumerate() {
        // Index 0 is `{video_stem}.nfo` (video-specific); subsequent entries are shared
        // directory-level NFOs (`movie.nfo`, `season.nfo`, `tvshow.nfo`) cached per path.
        let parsed = if idx == 0 {
            read_and_parse_nfo(&p)
        } else {
            shared_nfo_cache
                .entry(p)
                .or_insert_with_key(|path| read_and_parse_nfo(path))
                .clone()
        };
        if let Some(parsed) = parsed {
            if let Some(ref mut base) = primary {
                if base.show_title.is_none() {
                    base.show_title = parsed.show_title.or(parsed.title);
                }
                if base.year.is_none() {
                    base.year = parsed.year;
                }
                if base.season.is_none() {
                    base.season = parsed.season;
                }
                if base.genres.is_empty() {
                    base.genres = parsed.genres;
                }
                if base.tags.is_empty() {
                    base.tags = parsed.tags;
                }
            } else {
                primary = Some(parsed);
            }
        }
    }
    primary
}

fn append_xml_ref(out: &mut String, r: &quick_xml::events::BytesRef<'_>) {
    if let Ok(Some(ch)) = r.resolve_char_ref() {
        out.push(ch);
        return;
    }
    if let Some(resolved) = quick_xml::escape::resolve_xml_entity(r.as_ref()) {
        out.push_str(resolved);
    } else {
        out.push('&');
        out.push_str(r.as_ref());
        out.push(';');
    }
}

fn parse_nfo_xml(xml: &str) -> Option<ParsedNfoMetadata> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut meta = ParsedNfoMetadata::default();
    let mut root_tag: Option<String> = None;
    let mut tag_stack: Vec<String> = Vec::new();
    let mut text_acc = String::new();
    let mut any_found = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let tag = e.local_name().as_ref().to_ascii_lowercase();
                if root_tag.is_none() {
                    root_tag = Some(tag.clone());
                }
                tag_stack.push(tag);
                text_acc.clear();
            }
            Ok(Event::Text(t)) => {
                if !tag_stack.is_empty() {
                    text_acc.push_str(&t.xml10_content());
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if !tag_stack.is_empty() {
                    append_xml_ref(&mut text_acc, &r);
                }
            }
            Ok(Event::CData(c)) => {
                if !tag_stack.is_empty() {
                    text_acc.push_str(c.as_ref());
                }
            }
            Ok(Event::End(_)) => {
                if let Some(tag) = tag_stack.pop() {
                    let trimmed = text_acc.trim();
                    if !trimmed.is_empty() && tag_stack.len() == 1 {
                        let val = trimmed.to_string();
                        let is_tvshow = root_tag.as_deref() == Some("tvshow");
                        let is_season = root_tag.as_deref() == Some("season");
                        match tag.as_str() {
                            "title" if is_tvshow => {
                                if meta.show_title.is_none() {
                                    meta.show_title = Some(val);
                                    any_found = true;
                                }
                            }
                            "title" if is_season => {}
                            "title" if meta.title.is_none() => {
                                meta.title = Some(val);
                                any_found = true;
                            }
                            "showtitle" if meta.show_title.is_none() => {
                                meta.show_title = Some(val);
                                any_found = true;
                            }
                            "plot" | "outline" if meta.summary.is_none() => {
                                meta.summary = Some(val);
                                any_found = true;
                            }
                            "year" | "premiered" if meta.year.is_none() => {
                                if let Some(y) = val.get(0..4).and_then(|s| s.parse::<i32>().ok()) {
                                    meta.year = Some(y);
                                    any_found = true;
                                }
                            }
                            "season" | "seasonnumber" if meta.season.is_none() => {
                                if let Ok(s) = val.parse::<u32>() {
                                    meta.season = Some(s);
                                    any_found = true;
                                }
                            }
                            "episode" if meta.episode.is_none() => {
                                if let Ok(ep) = val.parse::<u32>() {
                                    meta.episode = Some(ep);
                                    any_found = true;
                                }
                            }
                            "genre"
                                if !meta.genres.iter().any(|g| g.eq_ignore_ascii_case(&val)) =>
                            {
                                meta.genres.push(val);
                                any_found = true;
                            }
                            "tag" if !meta.tags.iter().any(|t| t.eq_ignore_ascii_case(&val)) => {
                                meta.tags.push(val);
                                any_found = true;
                            }
                            _ => {}
                        }
                    }
                }
                text_acc.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    if any_found { Some(meta) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_nfo_xml_handles_entities_char_refs_and_cdata() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<movie>
  <title>Tom &amp; Jerry: &quot;Classic&quot; &lt;Remastered&gt; &#x2605;</title>
  <plot><![CDATA[Hero & <Villain> face off in "Round 1"!]]></plot>
  <year>2021</year>
  <genre>Action &amp; Adventure</genre>
  <genre>action &amp; adventure</genre>
  <tag>Cat &amp; Mouse</tag>
  <actor>
    <name>Nested Actor</name>
    <title>Should Not Override Title</title>
  </actor>
</movie>"#;

        let meta = parse_nfo_xml(xml).expect("should parse movie NFO");
        assert_eq!(
            meta.title.as_deref(),
            Some("Tom & Jerry: \"Classic\" <Remastered> ★")
        );
        assert_eq!(
            meta.summary.as_deref(),
            Some("Hero & <Villain> face off in \"Round 1\"!")
        );
        assert_eq!(meta.year, Some(2021));
        assert_eq!(meta.genres, vec!["Action & Adventure"]);
        assert_eq!(meta.tags, vec!["Cat & Mouse"]);
    }

    #[test]
    fn parse_nfo_xml_handles_mixed_text_entity_cdata_and_tvshow_title() {
        let xml = r#"<tvshow>
  <title>Dungeons &amp; <![CDATA[Dragons: <Honor>]]></title>
  <plot>Intro &amp; <![CDATA[<details>]]> &apos;end&apos;</plot>
  <premiered>2023-09-29</premiered>
  <genre>Fantasy</genre>
  <tag>RPG &amp; Magic</tag>
</tvshow>"#;

        let meta = parse_nfo_xml(xml).expect("should parse tvshow NFO");
        assert_eq!(meta.title, None);
        assert_eq!(
            meta.show_title.as_deref(),
            Some("Dungeons & Dragons: <Honor>")
        );
        assert_eq!(meta.summary.as_deref(), Some("Intro & <details> 'end'"));
        assert_eq!(meta.year, Some(2023));
        assert_eq!(meta.genres, vec!["Fantasy"]);
        assert_eq!(meta.tags, vec!["RPG & Magic"]);
    }

    #[tokio::test]
    async fn scanner_caches_tvshow_nfo_and_dir_subtitles_across_episodes() {
        let tmp = TempDir::new().unwrap();
        let show_dir = tmp.path().join("Spice & Wolf");
        let season_dir = show_dir.join("Season 01");
        std::fs::create_dir_all(&season_dir).unwrap();

        std::fs::write(
            show_dir.join("tvshow.nfo"),
            "<tvshow><title>Spice &amp; Wolf</title><year>2024</year><genre>Fantasy &amp; Romance</genre></tvshow>",
        )
        .unwrap();

        for ep in 1..=3 {
            std::fs::write(season_dir.join(format!("0{ep} - Ep{ep}.mp4")), b"fake-mp4").unwrap();
            std::fs::write(
                season_dir.join(format!("0{ep} - Ep{ep}.zh.srt")),
                b"1\n00:00:01,000 --> 00:00:02,000\nHi\n",
            )
            .unwrap();
        }

        let scanner = LocalVideoScanner;
        let entries = scanner
            .scan_video_entries(tmp.path().to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(entries.len(), 3);
        for (idx, entry) in entries.iter().enumerate() {
            let nfo = entry.nfo.as_ref().expect("tvshow.nfo merged");
            assert_eq!(nfo.show_title.as_deref(), Some("Spice & Wolf"));
            assert_eq!(nfo.year, Some(2024));
            assert_eq!(nfo.genres, vec!["Fantasy & Romance"]);
            assert_eq!(entry.sidecar_subtitles.len(), 1);
            assert!(entry.sidecar_subtitles[0].path_or_key.ends_with(&format!(
                "0{} - Ep{}.zh.srt",
                idx + 1,
                idx + 1
            )));
        }
    }

    #[tokio::test]
    async fn parse_nfo_xml_supports_seasonnumber_and_scanner_inherits_season_nfo() {
        let season_xml = "<season><title>Season 2</title><seasonnumber>2</seasonnumber></season>";
        let parsed_season = parse_nfo_xml(season_xml).expect("should parse season.nfo");
        assert_eq!(parsed_season.season, Some(2));
        assert_eq!(parsed_season.title, None);

        let tmp = TempDir::new().unwrap();
        let show_dir = tmp.path().join("Bocchi");
        let special_dir = show_dir.join("Arc Two");
        std::fs::create_dir_all(&special_dir).unwrap();
        std::fs::write(
            show_dir.join("tvshow.nfo"),
            "<tvshow><title>Bocchi the Rock!</title></tvshow>",
        )
        .unwrap();
        std::fs::write(special_dir.join("season.nfo"), season_xml).unwrap();
        std::fs::write(special_dir.join("01 - Opening.mp4"), b"fake-mp4").unwrap();
        std::fs::write(
            special_dir.join("01 - Opening.nfo"),
            "<episodedetails><title>Opening</title><episode>1</episode></episodedetails>",
        )
        .unwrap();

        let scanner = LocalVideoScanner;
        let entries = scanner
            .scan_video_entries(tmp.path().to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        let nfo = entries[0].nfo.as_ref().expect("merged NFO");
        assert_eq!(nfo.title.as_deref(), Some("Opening"));
        assert_eq!(nfo.show_title.as_deref(), Some("Bocchi the Rock!"));
        assert_eq!(nfo.season, Some(2));
        assert_eq!(nfo.episode, Some(1));
    }
}
