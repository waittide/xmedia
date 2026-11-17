use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use async_trait::async_trait;
use bytes::Bytes;
use quick_xml::Reader;
use quick_xml::events::Event;
use walkdir::WalkDir;
use xmedia_core::application::ports::{BinaryPayload, ByteRangeSpec, ByteRangeStream};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, LibraryKind};
use xmedia_core::infra::stream::{
    decode_text_smart, open_file_range_stream_spec, sanitize_join, sha256_hex,
};
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
        let file_hash = sha256_hex(&bytes);
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
        self.posters.store_cover_bytes(bytes)
    }

    async fn read_poster(&self, key: &str, max_size: Option<u32>) -> DomainResult<BinaryPayload> {
        self.posters.load_cover(key, max_size)
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

        let path = match (&library.kind, &video.source) {
            (LibraryKind::Managed, VideoSource::Managed { .. }) => {
                sanitize_join(&self.subtitles_dir, &sub.path_or_key)?
            }
            (LibraryKind::LocalMount { root_path }, VideoSource::LocalPath { .. }) => {
                sanitize_join(Path::new(root_path), &sub.path_or_key)?
            }
            _ => {
                return Err(DomainError::Validation(
                    "mismatched library kind and video source".into(),
                ));
            }
        };

        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|e| DomainError::Storage(format!("failed to read subtitle: {e}")))?;
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
                let file_mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| i64::try_from(d.as_secs()).unwrap_or(0))
                    .unwrap_or(0);

                let parent_dir = path.parent().unwrap_or(&root).to_path_buf();
                let video_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("video");

                let sidecar_subtitles = discover_sidecar_subtitles(&root, &parent_dir, video_stem);
                let directory_poster_path =
                    discover_video_poster(&root, &parent_dir, video_stem, &mut dir_posters);
                let nfo = discover_and_parse_nfo(&root, &parent_dir, video_stem);

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
        let p = Path::new(path);
        if !p.is_file() {
            return Ok(None);
        }
        let bytes = tokio::fs::read(p)
            .await
            .map_err(|e| DomainError::Storage(format!("failed reading poster {path}: {e}")))?;
        Ok(Some(bytes))
    }
}

fn path_to_slash(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn discover_sidecar_subtitles(
    root: &Path,
    parent_dir: &Path,
    video_stem: &str,
) -> Vec<SidecarSubtitle> {
    let Ok(read_dir) = std::fs::read_dir(parent_dir) else {
        return Vec::new();
    };
    let stem_lower = video_stem.to_lowercase();
    let prefix_dot = format!("{stem_lower}.");
    let mut matches: Vec<(String, String, PathBuf)> = Vec::new();

    for item in read_dir.flatten() {
        let p = item.path();
        if !p.is_file() {
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
        let sub_lower = sub_stem.to_lowercase();
        if sub_lower == stem_lower || sub_lower.starts_with(&prefix_dot) {
            matches.push((fname.to_string(), ext.to_ascii_lowercase(), p));
        }
    }

    matches.sort_by(|a, b| a.0.cmp(&b.0));
    matches
        .into_iter()
        .enumerate()
        .filter_map(|(idx, (fname, fmt, full_path))| {
            let rel = full_path.strip_prefix(root).ok()?;
            let (language, label) = parse_sidecar_subtitle_lang_and_label(video_stem, &fname);
            Some(SidecarSubtitle {
                id: format!("s{idx}"),
                format: fmt,
                language,
                label,
                path_or_key: path_to_slash(rel),
            })
        })
        .collect()
}

fn discover_video_poster(
    root: &Path,
    parent_dir: &Path,
    video_stem: &str,
    dir_cache: &mut HashMap<PathBuf, Option<String>>,
) -> Option<String> {
    for ext in ["jpg", "jpeg", "png", "webp"] {
        for candidate_name in [
            format!("{video_stem}-poster.{ext}"),
            format!("{video_stem}-thumb.{ext}"),
            format!("{video_stem}.{ext}"),
        ] {
            let p = parent_dir.join(candidate_name);
            if p.is_file() {
                return Some(p.to_string_lossy().into_owned());
            }
        }
    }

    let poster_names = [
        "poster.jpg",
        "poster.png",
        "poster.webp",
        "folder.jpg",
        "folder.png",
        "cover.jpg",
        "cover.png",
    ];

    let found_in_dir = dir_cache
        .entry(parent_dir.to_path_buf())
        .or_insert_with(|| {
            for name in poster_names {
                let p = parent_dir.join(name);
                if p.is_file() {
                    return Some(p.to_string_lossy().into_owned());
                }
            }
            None
        })
        .clone();
    if found_in_dir.is_some() {
        return found_in_dir;
    }

    // Jellyfin `Show/Season XX/` fallback: look up series-level `poster.jpg` in parent show directory
    if parent_dir != root
        && let Some(show_dir) = parent_dir.parent()
        && show_dir != root
        && show_dir.starts_with(root)
    {
        return dir_cache
            .entry(show_dir.to_path_buf())
            .or_insert_with(|| {
                for name in poster_names {
                    let p = show_dir.join(name);
                    if p.is_file() {
                        return Some(p.to_string_lossy().into_owned());
                    }
                }
                None
            })
            .clone();
    }

    None
}

fn discover_and_parse_nfo(
    root: &Path,
    parent_dir: &Path,
    video_stem: &str,
) -> Option<ParsedNfoMetadata> {
    let mut candidates = vec![
        parent_dir.join(format!("{video_stem}.nfo")),
        parent_dir.join("movie.nfo"),
        parent_dir.join("tvshow.nfo"),
    ];
    if parent_dir != root
        && let Some(show_dir) = parent_dir.parent()
        && show_dir != root
        && show_dir.starts_with(root)
    {
        candidates.push(show_dir.join("tvshow.nfo"));
    }

    let mut primary: Option<ParsedNfoMetadata> = None;
    for p in candidates {
        if p.is_file()
            && let Ok(bytes) = std::fs::read(&p)
        {
            let (xml, _) = decode_text_smart(&bytes);
            if let Some(parsed) = parse_nfo_xml(&xml) {
                if let Some(ref mut base) = primary {
                    // Merge missing series-level genres/tags/year from `tvshow.nfo`
                    if base.show_title.is_none() {
                        base.show_title = parsed.show_title.or(parsed.title);
                    }
                    if base.year.is_none() {
                        base.year = parsed.year;
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
    }
    primary
}

fn parse_nfo_xml(xml: &str) -> Option<ParsedNfoMetadata> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut meta = ParsedNfoMetadata::default();
    let mut current_tag = String::new();
    let mut any_found = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                current_tag = e.local_name().as_ref().to_ascii_lowercase();
            }
            Ok(Event::End(_)) => {
                current_tag.clear();
            }
            Ok(Event::Text(t)) => {
                let val = t.xml10_content().trim().to_string();
                if val.is_empty() {
                    continue;
                }
                match current_tag.as_str() {
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
                    "season" if meta.season.is_none() => {
                        meta.season = val.parse::<u32>().ok();
                    }
                    "episode" if meta.episode.is_none() => {
                        meta.episode = val.parse::<u32>().ok();
                    }
                    "genre" if !meta.genres.contains(&val) => {
                        meta.genres.push(val);
                        any_found = true;
                    }
                    "tag" if !meta.tags.contains(&val) => {
                        meta.tags.push(val);
                        any_found = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    if any_found { Some(meta) } else { None }
}
