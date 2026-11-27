use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_formats::decode_text_smart;
use xmedia_formats::video::SubtitleFormatExt;
pub use xmedia_formats::video::convert_subtitle_bytes_to_vtt;

use crate::application::ops::{
    ExtractSubtitleReportDto, JellyfinArchiveInput, ProcessSubtitleInput, ProcessSubtitleReportDto,
    VideoSubtitleInfoDto, VideoToolCommand, VideoToolOutput,
};
use crate::domain::zh_text::{ZhConvertDirection, convert_zh_plain};
use crate::infra::atomic_fs::{
    atomic_write_bytes, cleanup_empty_parent_dirs, find_companion_sidecars,
};
use crate::infra::inspectors::video::{
    find_existing_nfo, find_show_nfo_for_episode, is_season_dir, read_base_video_meta,
    render_season_nfo_xml_preserving, render_tvshow_nfo_xml_preserving,
    render_video_nfo_xml_preserving, write_image_replacing_opposite_ext,
};

pub fn run_video_tool(cmd: VideoToolCommand) -> DomainResult<VideoToolOutput> {
    match cmd {
        VideoToolCommand::ListSubtitles { path } => {
            list_video_subtitles(&path).map(VideoToolOutput::Subtitles)
        }
        VideoToolCommand::ProcessSubtitle { path, input } => {
            process_video_subtitle(&path, &input).map(VideoToolOutput::ProcessedSubtitle)
        }
        VideoToolCommand::ExtractEmbeddedSubtitle {
            path,
            stream_index,
            language_tag,
            output_ext,
            ffmpeg_bin,
        } => extract_embedded_subtitle(
            &path,
            stream_index,
            language_tag.as_deref(),
            &output_ext,
            &ffmpeg_bin,
        )
        .map(VideoToolOutput::ExtractedSubtitle),
        VideoToolCommand::ReadBaseMeta { path } => Ok(VideoToolOutput::BaseMeta(Box::new(
            read_base_video_meta(&path),
        ))),
        VideoToolCommand::WriteSeasonNfo {
            season_dir,
            season_num,
            year,
            tmdb_id,
            bangumi_id,
        } => {
            let season_nfo_path = season_dir.join("season.nfo");
            let prev_season = std::fs::read(&season_nfo_path).ok();
            let nfo = render_season_nfo_xml_preserving(
                season_num,
                None,
                year,
                None,
                tmdb_id.as_deref(),
                bangumi_id.as_deref(),
                prev_season.as_deref(),
            );
            atomic_write_bytes(&season_nfo_path, nfo.as_bytes())?;
            Ok(VideoToolOutput::Unit)
        }
        VideoToolCommand::ArchiveJellyfin(input) => {
            archive_video_jellyfin(*input)?;
            Ok(VideoToolOutput::Unit)
        }
    }
}

fn archive_video_jellyfin(input: JellyfinArchiveInput) -> DomainResult<()> {
    let JellyfinArchiveInput {
        root_path,
        src_abs,
        dst_abs,
        target_stem,
        meta,
        scraped,
        downloaded_images,
        skip_poster,
        skip_fanart,
        skip_thumb,
        only_fill_missing,
    } = input;

    let dst_dir = dst_abs
        .parent()
        .ok_or_else(|| DomainError::Validation("invalid target video folder".into()))?
        .to_path_buf();
    let show_dir = if meta.is_episode
        && is_season_dir(&dst_dir)
        && dst_dir != root_path
        && let Some(parent) = dst_dir.parent()
        && parent != root_path
        && parent.starts_with(&root_path)
    {
        parent.to_path_buf()
    } else {
        dst_dir.clone()
    };

    let will_replace_poster = !skip_poster
        && scraped
            .cover_url
            .as_deref()
            .map(str::trim)
            .is_some_and(|u| downloaded_images.contains_key(u));
    let will_replace_fanart = !skip_fanart
        && scraped
            .backdrop_url
            .as_deref()
            .map(str::trim)
            .is_some_and(|u| downloaded_images.contains_key(u));
    let will_replace_thumb = !skip_thumb
        && scraped
            .episode_still_url
            .as_deref()
            .map(str::trim)
            .is_some_and(|u| downloaded_images.contains_key(u));

    std::fs::create_dir_all(&dst_dir)
        .map_err(|e| DomainError::Storage(format!("failed creating Jellyfin dir: {e}")))?;
    if show_dir != dst_dir {
        std::fs::create_dir_all(&show_dir)
            .map_err(|e| DomainError::Storage(format!("failed creating Jellyfin show dir: {e}")))?;
    }

    let old_parent = src_abs.parent().map(Path::to_path_buf);
    let existing_video_nfo_bytes = find_existing_nfo(&src_abs, meta.is_episode)
        .or_else(|| find_existing_nfo(&src_abs, !meta.is_episode))
        .and_then(|p| std::fs::read(p).ok());
    let existing_show_nfo_bytes =
        find_show_nfo_for_episode(&src_abs).and_then(|p| std::fs::read(p).ok());
    let existing_old_season_nfo_bytes = old_parent
        .as_ref()
        .map(|p| p.join("season.nfo"))
        .filter(|p| p.is_file())
        .and_then(|p| std::fs::read(p).ok());
    let companions = find_companion_sidecars(&src_abs);

    if dst_abs != src_abs {
        std::fs::rename(&src_abs, &dst_abs).map_err(|e| {
            DomainError::Storage(format!(
                "failed archiving video into '{}': {e}",
                dst_abs.display()
            ))
        })?;
    }

    for (comp_abs, suffix) in companions {
        let lower = suffix.to_ascii_lowercase();
        let is_sub = lower.ends_with(".srt")
            || lower.ends_with(".ass")
            || lower.ends_with(".ssa")
            || lower.ends_with(".vtt");
        if is_sub {
            let sub_dst = dst_dir.join(format!("{target_stem}{suffix}"));
            if sub_dst != comp_abs && !sub_dst.exists() {
                let _ = std::fs::rename(&comp_abs, &sub_dst);
            }
        } else if dst_abs != src_abs {
            let image_ext = |p_lower: &str| -> &'static str {
                if p_lower.ends_with(".png") {
                    "png"
                } else if p_lower.ends_with(".webp") {
                    "webp"
                } else {
                    "jpg"
                }
            };
            let mut should_remove_old = true;
            if lower.contains("-poster.") && !will_replace_poster {
                let target_poster_dir = if meta.is_episode { &show_dir } else { &dst_dir };
                let migrated = target_poster_dir.join(format!("poster.{}", image_ext(&lower)));
                if !migrated.exists() && std::fs::copy(&comp_abs, &migrated).is_err() {
                    should_remove_old = false;
                }
            } else if lower.contains("-fanart.") && !will_replace_fanart {
                let target_fan_dir = if meta.is_episode { &show_dir } else { &dst_dir };
                let migrated = target_fan_dir.join(format!("fanart.{}", image_ext(&lower)));
                if !migrated.exists() && std::fs::copy(&comp_abs, &migrated).is_err() {
                    should_remove_old = false;
                }
            } else if lower.contains("-thumb.") && !will_replace_thumb {
                let migrated = dst_dir.join(format!("{target_stem}-thumb.{}", image_ext(&lower)));
                if !migrated.exists() && std::fs::copy(&comp_abs, &migrated).is_err() {
                    should_remove_old = false;
                }
            }
            if should_remove_old {
                let _ = std::fs::remove_file(&comp_abs);
            }
        }
    }

    if !meta.is_episode {
        let movie_nfo = dst_dir.join("movie.nfo");
        let prev_movie_nfo = std::fs::read(&movie_nfo).ok().or(existing_video_nfo_bytes);
        let xml = render_video_nfo_xml_preserving(&meta, prev_movie_nfo.as_deref());
        atomic_write_bytes(&movie_nfo, xml.as_bytes())?;

        let same_stem_nfo = dst_dir.join(format!("{target_stem}.nfo"));
        if same_stem_nfo.is_file() && same_stem_nfo != movie_nfo {
            let _ = std::fs::remove_file(same_stem_nfo);
        }

        if !skip_poster
            && let Some(u) = scraped.cover_url.as_deref().map(str::trim)
            && let Some((bytes, img_ext)) = downloaded_images.get(u)
        {
            write_image_replacing_opposite_ext(&dst_dir, "poster", img_ext, bytes)?;
        }
        if !skip_fanart
            && let Some(u) = scraped.backdrop_url.as_deref().map(str::trim)
            && let Some((bytes, img_ext)) = downloaded_images.get(u)
        {
            write_image_replacing_opposite_ext(&dst_dir, "fanart", img_ext, bytes)?;
        }
    } else {
        let tvshow_nfo_path = show_dir.join("tvshow.nfo");
        if !only_fill_missing || !tvshow_nfo_path.is_file() {
            let prev_tvshow = std::fs::read(&tvshow_nfo_path)
                .ok()
                .or(existing_show_nfo_bytes);
            let show_plot = scraped.show_summary.as_deref().or(meta.summary.as_deref());
            let tvshow_xml =
                render_tvshow_nfo_xml_preserving(&meta, show_plot, prev_tvshow.as_deref());
            atomic_write_bytes(&tvshow_nfo_path, tvshow_xml.as_bytes())?;
        }

        if !skip_poster
            && let Some(u) = scraped.cover_url.as_deref().map(str::trim)
            && let Some((bytes, img_ext)) = downloaded_images.get(u)
        {
            write_image_replacing_opposite_ext(&show_dir, "poster", img_ext, bytes)?;
        }
        if !skip_fanart
            && let Some(u) = scraped.backdrop_url.as_deref().map(str::trim)
            && let Some((bytes, img_ext)) = downloaded_images.get(u)
        {
            write_image_replacing_opposite_ext(&show_dir, "fanart", img_ext, bytes)?;
        }

        let season_num = meta.season_number.unwrap_or(1);
        let season_nfo_path = dst_dir.join("season.nfo");
        if !only_fill_missing || !season_nfo_path.is_file() {
            let prev_season = std::fs::read(&season_nfo_path)
                .ok()
                .or(existing_old_season_nfo_bytes);
            let season_plot = scraped
                .season_summary
                .as_deref()
                .or(scraped.show_summary.as_deref())
                .or(meta.summary.as_deref());
            let season_xml = render_season_nfo_xml_preserving(
                season_num,
                scraped.season_title.as_deref(),
                meta.year,
                season_plot,
                meta.tmdb_id.as_deref(),
                meta.bangumi_id.as_deref(),
                prev_season.as_deref(),
            );
            atomic_write_bytes(&season_nfo_path, season_xml.as_bytes())?;
        }

        if dst_dir != show_dir {
            let season_poster_u = scraped
                .season_poster_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .or_else(|| scraped.cover_url.as_deref().map(str::trim));
            if !skip_poster
                && let Some(u) = season_poster_u
                && let Some((bytes, img_ext)) = downloaded_images.get(u)
            {
                write_image_replacing_opposite_ext(&dst_dir, "poster", img_ext, bytes)?;
            }
            if !skip_fanart
                && let Some(u) = scraped.backdrop_url.as_deref().map(str::trim)
                && let Some((bytes, img_ext)) = downloaded_images.get(u)
            {
                write_image_replacing_opposite_ext(&dst_dir, "fanart", img_ext, bytes)?;
            }
        }

        let ep_nfo = dst_dir.join(format!("{target_stem}.nfo"));
        let prev_ep_nfo = std::fs::read(&ep_nfo).ok().or(existing_video_nfo_bytes);
        let ep_xml = render_video_nfo_xml_preserving(&meta, prev_ep_nfo.as_deref());
        atomic_write_bytes(&ep_nfo, ep_xml.as_bytes())?;

        if !skip_thumb
            && let Some(u) = scraped.episode_still_url.as_deref().map(str::trim)
            && let Some((bytes, img_ext)) = downloaded_images.get(u)
        {
            let thumb_base = format!("{target_stem}-thumb");
            write_image_replacing_opposite_ext(&dst_dir, &thumb_base, img_ext, bytes)?;
        }
    }

    if let Some(ref prev_dir) = old_parent {
        cleanup_empty_parent_dirs(prev_dir, &root_path);
    }

    Ok(())
}

static RE_SRT_TIMING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:(\d{1,2}):)?(\d{2}):(\d{2})[,.](\d{3})\s*-->\s*(?:(\d{1,2}):)?(\d{2}):(\d{2})[,.](\d{3})(.*)$",
    )
    .expect("valid srt timing regex")
});

static RE_ASS_DIALOGUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(Dialogue:\s*[^,]*,)(\d+):(\d{2}):(\d{2})\.(\d{2}),(\d+):(\d{2}):(\d{2})\.(\d{2}),([^,]*,[^,]*,[^,]*,[^,]*,[^,]*,[^,]*,)(.*)$",
    )
    .expect("valid ass dialogue regex")
});

fn detect_encoding_and_decode(bytes: &[u8]) -> (String, String, bool) {
    let stripped = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    if let Ok(s) = std::str::from_utf8(stripped) {
        return ("UTF-8".to_string(), s.to_string(), true);
    }
    let (decoded, enc_label) = decode_text_smart(bytes);
    (
        enc_label.to_string(),
        decoded.trim_start_matches('\u{feff}').to_string(),
        false,
    )
}

fn parse_subtitle_lang_and_forced(video_stem: &str, sub_filename: &str) -> (Option<String>, bool) {
    let without_ext = Path::new(sub_filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(sub_filename);
    let suffix = if without_ext
        .get(..video_stem.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(video_stem))
    {
        without_ext[video_stem.len()..].trim_start_matches(['.', '-', '_', ' '])
    } else {
        ""
    };
    if suffix.is_empty() {
        return (None, false);
    }
    let parts: Vec<&str> = suffix
        .split(['.', '_', ' '])
        .filter(|s| !s.is_empty())
        .collect();
    let sub_tokens: Vec<String> = suffix
        .split(['.', '_', '-', ' '])
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let forced = sub_tokens.iter().any(|p| p == "forced");
    let has_token = |candidates: &[&str]| {
        parts
            .iter()
            .any(|p| candidates.iter().any(|c| p.eq_ignore_ascii_case(c)))
            || sub_tokens.iter().any(|p| candidates.iter().any(|c| p == c))
    };

    let lang = if has_token(&["zh-cn", "zh-hans", "chs", "sc", "gb", "简体"]) {
        Some("zh-CN".to_string())
    } else if has_token(&["zh-tw", "zh-hk", "zh-hant", "cht", "tc", "big5", "繁体"]) {
        Some("zh-TW".to_string())
    } else if has_token(&["zh", "chi", "zho"]) {
        Some("zh-CN".to_string())
    } else if has_token(&["en", "eng", "en-us", "en-gb"]) {
        Some("en".to_string())
    } else if has_token(&["ja", "jpn", "jp"]) {
        Some("ja".to_string())
    } else {
        parts
            .first()
            .filter(|p| !p.eq_ignore_ascii_case("forced"))
            .map(|p| (*p).to_string())
    };
    (lang, forced)
}

pub fn list_video_subtitles(video_path: &Path) -> DomainResult<Vec<VideoSubtitleInfoDto>> {
    let parent = video_path
        .parent()
        .ok_or_else(|| DomainError::Validation("video path has no parent dir".into()))?;
    let stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let stem_lower = stem.to_ascii_lowercase();

    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(parent) else {
        return Ok(out);
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let ext = p
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if SubtitleFormatExt::from_extension(&ext).is_none() {
            continue;
        }
        let Some(fname) = p.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let fname_lower = fname.to_ascii_lowercase();
        if !fname_lower.starts_with(&stem_lower) {
            continue;
        }
        let remainder = &fname_lower[stem_lower.len()..];
        if !remainder.starts_with('.') && !remainder.starts_with('_') && !remainder.starts_with('-')
        {
            continue;
        }
        let bytes = std::fs::read(&p).unwrap_or_default();
        let (enc, _, is_utf8) = detect_encoding_and_decode(&bytes);
        let (lang, forced) = parse_subtitle_lang_and_forced(stem, fname);
        out.push(VideoSubtitleInfoDto {
            filename: fname.to_string(),
            extension: ext,
            file_size: bytes.len() as u64,
            detected_encoding: enc,
            is_utf8,
            language_tag: lang,
            forced,
        });
    }
    out.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(out)
}

fn format_srt_ms(total_ms: i64, dot_sep: bool) -> String {
    let clamped = total_ms.max(0);
    let h = clamped / 3_600_000;
    let m = (clamped % 3_600_000) / 60_000;
    let s = (clamped % 60_000) / 1_000;
    let ms = clamped % 1_000;
    let sep = if dot_sep { '.' } else { ',' };
    format!("{h:02}:{m:02}:{s:02}{sep}{ms:03}")
}

fn format_ass_ms(total_ms: i64) -> String {
    let clamped = total_ms.max(0);
    let h = clamped / 3_600_000;
    let m = (clamped % 3_600_000) / 60_000;
    let s = (clamped % 60_000) / 1_000;
    let cs = (clamped % 1_000) / 10;
    format!("{h}:{m:02}:{s:02}.{cs:02}")
}

/// Converts Zh text in ASS dialogue while skipping `{...}` override tags and `\N`/`\n` escapes.
fn convert_ass_dialogue_text(text: &str, dir: ZhConvertDirection) -> String {
    let mut out = String::with_capacity(text.len());
    let mut buf = String::new();
    let mut in_brace = false;

    for ch in text.chars() {
        if in_brace {
            out.push(ch);
            if ch == '}' {
                in_brace = false;
            }
        } else if ch == '{' {
            if !buf.is_empty() {
                out.push_str(&convert_zh_plain(&buf, dir));
                buf.clear();
            }
            in_brace = true;
            out.push(ch);
        } else {
            buf.push(ch);
        }
    }
    if !buf.is_empty() {
        out.push_str(&convert_zh_plain(&buf, dir));
    }
    out
}

pub fn transform_subtitle_content(
    raw: &str,
    ext: &str,
    offset_ms: i64,
    zh_direction: Option<ZhConvertDirection>,
) -> (String, usize) {
    let is_ass = matches!(ext, "ass" | "ssa");
    let is_vtt = ext == "vtt";
    let mut out_lines = Vec::new();
    let mut shifted_cues = 0usize;

    for raw_line in raw.replace("\r\n", "\n").lines() {
        if is_ass {
            if let Some(caps) = RE_ASS_DIALOGUE.captures(raw_line) {
                let mut start_ms = caps[2].parse::<i64>().unwrap_or(0) * 3_600_000
                    + caps[3].parse::<i64>().unwrap_or(0) * 60_000
                    + caps[4].parse::<i64>().unwrap_or(0) * 1_000
                    + caps[5].parse::<i64>().unwrap_or(0) * 10;
                let mut end_ms = caps[6].parse::<i64>().unwrap_or(0) * 3_600_000
                    + caps[7].parse::<i64>().unwrap_or(0) * 60_000
                    + caps[8].parse::<i64>().unwrap_or(0) * 1_000
                    + caps[9].parse::<i64>().unwrap_or(0) * 10;
                if offset_ms != 0 {
                    start_ms += offset_ms;
                    end_ms += offset_ms;
                    shifted_cues += 1;
                }
                let body = if let Some(dir) = zh_direction {
                    convert_ass_dialogue_text(&caps[11], dir)
                } else {
                    caps[11].to_string()
                };
                out_lines.push(format!(
                    "{}{},{},{}{}",
                    &caps[1],
                    format_ass_ms(start_ms),
                    format_ass_ms(end_ms),
                    &caps[10],
                    body
                ));
            } else {
                out_lines.push(raw_line.to_string());
            }
        } else if let Some(caps) = RE_SRT_TIMING.captures(raw_line) {
            if offset_ms != 0 {
                let start_h = caps
                    .get(1)
                    .and_then(|m| m.as_str().parse::<i64>().ok())
                    .unwrap_or(0);
                let end_h = caps
                    .get(5)
                    .and_then(|m| m.as_str().parse::<i64>().ok())
                    .unwrap_or(0);
                let start_ms = start_h * 3_600_000
                    + caps[2].parse::<i64>().unwrap_or(0) * 60_000
                    + caps[3].parse::<i64>().unwrap_or(0) * 1_000
                    + caps[4].parse::<i64>().unwrap_or(0)
                    + offset_ms;
                let end_ms = end_h * 3_600_000
                    + caps[6].parse::<i64>().unwrap_or(0) * 60_000
                    + caps[7].parse::<i64>().unwrap_or(0) * 1_000
                    + caps[8].parse::<i64>().unwrap_or(0)
                    + offset_ms;
                shifted_cues += 1;
                out_lines.push(format!(
                    "{} --> {}{}",
                    format_srt_ms(start_ms, is_vtt),
                    format_srt_ms(end_ms, is_vtt),
                    &caps[9]
                ));
            } else {
                out_lines.push(raw_line.to_string());
            }
        } else if let Some(dir) = zh_direction {
            if raw_line.trim().chars().all(|c| c.is_ascii_digit()) {
                out_lines.push(raw_line.to_string());
            } else {
                out_lines.push(convert_zh_plain(raw_line, dir));
            }
        } else {
            out_lines.push(raw_line.to_string());
        }
    }

    (out_lines.join("\n"), shifted_cues)
}

fn validate_subtitle_lang_tag(raw: &str) -> DomainResult<&str> {
    let lang = raw.trim();
    if lang.is_empty()
        || lang.len() > 64
        || lang.contains('/')
        || lang.contains('\\')
        || lang.contains("..")
        || lang.starts_with('.')
        || lang.ends_with('.')
        || lang
            .chars()
            .any(|c| c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Err(DomainError::Validation(format!(
            "invalid subtitle language tag '{lang}'"
        )));
    }
    Ok(lang)
}

pub fn process_video_subtitle(
    video_path: &Path,
    input: &ProcessSubtitleInput,
) -> DomainResult<ProcessSubtitleReportDto> {
    let parent = video_path
        .parent()
        .ok_or_else(|| DomainError::Validation("video path has no parent dir".into()))?;
    if input.subtitle_filename.is_empty()
        || input.subtitle_filename.contains('/')
        || input.subtitle_filename.contains('\\')
        || input.subtitle_filename.contains("..")
        || input.subtitle_filename.chars().any(char::is_control)
    {
        return Err(DomainError::Validation("invalid subtitle filename".into()));
    }
    let sub_path = parent.join(&input.subtitle_filename);
    if sub_path.parent() != Some(parent) {
        return Err(DomainError::Validation("invalid subtitle filename".into()));
    }
    if !sub_path.is_file() {
        return Err(DomainError::NotFound(format!(
            "subtitle '{}' not found",
            input.subtitle_filename
        )));
    }
    let ext = sub_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "srt".to_string());

    let raw_bytes = std::fs::read(&sub_path)
        .map_err(|e| DomainError::Storage(format!("read subtitle failed: {e}")))?;
    let (orig_encoding, decoded, was_utf8) = detect_encoding_and_decode(&raw_bytes);

    let (transformed, shifted_cues) =
        transform_subtitle_content(&decoded, &ext, input.offset_ms, input.zh_direction);

    let should_rewrite =
        input.convert_utf8 || input.offset_ms != 0 || input.zh_direction.is_some() || !was_utf8;

    let mut target_path: PathBuf = sub_path.clone();
    let mut final_filename = input.subtitle_filename.clone();
    if let Some(ref lang_raw) = input.rename_language {
        let trimmed = lang_raw.trim();
        if !trimmed.is_empty() {
            let lang = validate_subtitle_lang_tag(trimmed)?;
            let video_stem = video_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("video");
            let forced_part = if input.forced { ".forced" } else { "" };
            final_filename = format!("{video_stem}.{lang}{forced_part}.{ext}");
            target_path = parent.join(&final_filename);
            if target_path.parent() != Some(parent) {
                return Err(DomainError::Validation(
                    "subtitle rename target escapes parent directory".into(),
                ));
            }
        }
    }

    if should_rewrite {
        atomic_write_bytes(&target_path, transformed.as_bytes())?;
        if target_path != sub_path {
            let _ = std::fs::remove_file(&sub_path);
        }
    } else if target_path != sub_path {
        std::fs::rename(&sub_path, &target_path)
            .map_err(|e| DomainError::Storage(format!("rename subtitle failed: {e}")))?;
    }

    Ok(ProcessSubtitleReportDto {
        original_filename: input.subtitle_filename.clone(),
        final_filename,
        original_encoding: orig_encoding,
        shifted_cues,
        converted_utf8: should_rewrite && !was_utf8,
    })
}

pub fn extract_embedded_subtitle(
    video_path: &Path,
    stream_index: u32,
    language_tag: Option<&str>,
    output_ext: &str,
    ffmpeg_bin: &str,
) -> DomainResult<ExtractSubtitleReportDto> {
    let parent = video_path
        .parent()
        .ok_or_else(|| DomainError::Validation("video path has no parent dir".into()))?;
    let stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    let ext = match output_ext.to_ascii_lowercase().as_str() {
        "ass" => "ass",
        "vtt" => "vtt",
        _ => "srt",
    };
    let lang = match language_tag.map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => validate_subtitle_lang_tag(raw)?,
        None => "zh-CN",
    };
    let created_filename = format!("{stem}.{lang}.{ext}");
    let out_path = parent.join(&created_filename);
    if out_path.parent() != Some(parent) {
        return Err(DomainError::Validation(
            "subtitle output path escapes parent directory".into(),
        ));
    }

    let map_arg = format!("0:{stream_index}");
    let video_str = video_path.to_string_lossy();
    let out_str = out_path.to_string_lossy();
    let out = crate::infra::inspectors::process::run_command_bounded(
        ffmpeg_bin,
        &[
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            &video_str,
            "-map",
            &map_arg,
            &out_str,
        ],
        std::time::Duration::from_secs(180),
        2 * 1024 * 1024,
    )?;

    if !out.status.success() || !out_path.is_file() {
        return Err(DomainError::Storage(format!(
            "ffmpeg failed extracting subtitle stream 0:{stream_index}"
        )));
    }

    let file_size = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    Ok(ExtractSubtitleReportDto {
        created_filename,
        file_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_and_ass_transforms_shift_time_and_preserve_ass_override_tags() {
        let srt = "1\n00:00:05,500 --> 00:00:08,000\n軟體開發測試\n";
        let (out_srt, cues) =
            transform_subtitle_content(srt, "srt", 1500, Some(ZhConvertDirection::T2s));
        assert_eq!(cues, 1);
        assert!(out_srt.contains("00:00:07,000 --> 00:00:09,500"));
        assert!(out_srt.contains("软件开发测试"));

        let ass = "Dialogue: 0,0:00:10.50,0:00:13.20,Default,,0,0,0,,{\\fn黑體\\fs24}這是一段測試";
        let (out_ass, ass_cues) =
            transform_subtitle_content(ass, "ass", -500, Some(ZhConvertDirection::T2s));
        assert_eq!(ass_cues, 1);
        assert!(out_ass.contains("0:00:10.00,0:00:12.70"));
        // Style tag inside {...} must stay untouched while dialogue text converts to simplified
        assert!(out_ass.contains("{\\fn黑體\\fs24}这是一段测试"));
    }

    #[test]
    fn subtitle_gbk_to_utf8_and_jellyfin_rename_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let vid = tmp.path().join("Frieren - S01E01.mkv");
        std::fs::write(&vid, b"dummy").unwrap();
        let sub = tmp.path().join("Frieren - S01E01.chs.srt");
        let (gbk_bytes, _, _) =
            encoding_rs::GB18030.encode("1\n00:00:01,000 --> 00:00:03,000\n你好世界\n");
        std::fs::write(&sub, gbk_bytes).unwrap();

        let listed = list_video_subtitles(&vid).unwrap();
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].is_utf8);
        assert_eq!(listed[0].language_tag.as_deref(), Some("zh-CN"));

        let rep = process_video_subtitle(
            &vid,
            &ProcessSubtitleInput {
                subtitle_filename: "Frieren - S01E01.chs.srt".into(),
                convert_utf8: true,
                offset_ms: 500,
                zh_direction: None,
                rename_language: Some("zh-CN".into()),
                forced: false,
            },
        )
        .unwrap();
        assert_eq!(rep.final_filename, "Frieren - S01E01.zh-CN.srt");
        assert!(rep.converted_utf8);
        let final_text = std::fs::read_to_string(tmp.path().join(&rep.final_filename)).unwrap();
        assert!(final_text.contains("00:00:01,500 --> 00:00:03,500"));
        assert!(final_text.contains("你好世界"));
    }

    #[test]
    fn subtitle_language_tag_blocks_path_traversal_and_control_chars() {
        let tmp = tempfile::tempdir().unwrap();
        let vid = tmp.path().join("Movie.mkv");
        std::fs::write(&vid, b"dummy").unwrap();
        let sub = tmp.path().join("Movie.chs.srt");
        std::fs::write(&sub, b"1\n00:00:01,000 --> 00:00:02,000\nHi\n").unwrap();

        for bad_lang in [
            "../../pwn",
            "zh/../../x",
            "zh\\..\\x",
            "zh..cn",
            "zh\x00cn",
            "zh\ncn",
        ] {
            let err = process_video_subtitle(
                &vid,
                &ProcessSubtitleInput {
                    subtitle_filename: "Movie.chs.srt".into(),
                    rename_language: Some(bad_lang.into()),
                    ..Default::default()
                },
            )
            .unwrap_err();
            assert!(matches!(err, DomainError::Validation(_)));

            let ext_err =
                extract_embedded_subtitle(&vid, 0, Some(bad_lang), "srt", "ffmpeg").unwrap_err();
            assert!(matches!(ext_err, DomainError::Validation(_)));
        }
    }
}
