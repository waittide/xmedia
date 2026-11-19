use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::Event;
use regex::Regex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::stream::decode_text_smart;
use xmedia_core::infra::thumbnail::sniff_image_mime;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::domain::media::BookFileMeta;
use crate::infra::atomic_fs::{atomic_write_bytes, atomic_write_via_temp};
use crate::infra::inspectors::append_xml_ref;
use crate::infra::inspectors::book::{
    attr_value, find_opf_path_in_container, find_sidecar_cover, normalize_zip_rel,
    read_book_meta_any, replace_opf_metadata_block, rewrite_epub_entries,
};
use crate::infra::inspectors::zh_text::{
    BookTextTransformOptions, apply_text_transforms_to_chapter, rewrite_css_vertical_to_horizontal,
};

static DEFAULT_VOLUME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(?:第[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+[卷部集册篇](?:[\s：:、—\-].*)?|卷[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+(?:[\s：:、—\-].*)?|(?:volume|vol\.?|book|part)\s+[0-9ivxlcdm]+(?:\b.*)?)$",
    )
    .expect("valid default volume heading regex")
});

static DEFAULT_CHAPTER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)^\s*(?:第[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+[\s章节回话折幕][^\n。．！？；]*|(?:chapter|episode)\s+[0-9ivxlcdm]+(?:\b[^\n,;"]*)?|(?:楔子|序[章言幕曲]?|引子|前言|番外|尾声|后记|终章|prologue|epilogue)(?:[\s\u{3000}：:、—\-].*)?)$"#,
    )
    .expect("valid default chapter heading regex")
});

static DEFAULT_PROSE_FALSE_CHAPTER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\s*第[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+[章节回话折幕][中内裏里上下前后後的了说說道云]",
    )
    .expect("valid prose false chapter regex")
});

const DEFAULT_EPUB_CSS: &str = r#"body {
  font-family: -apple-system, BlinkMacSystemFont, "PingFang SC", "Noto Serif CJK SC", "Microsoft YaHei", serif;
  line-height: 1.8;
  margin: 0;
  padding: 1.2em 1.4em;
  word-wrap: break-word;
}
h1, h2, h3 {
  line-height: 1.35;
  margin: 1.2em 0 0.8em;
  font-weight: 700;
}
p {
  margin: 0 0 0.85em;
  text-indent: 2em;
  text-align: justify;
}
.cover-wrap {
  text-align: center;
  margin: 0;
  padding: 0;
}
.cover-wrap img {
  max-width: 100%;
  max-height: 98vh;
  object-fit: contain;
}
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ChapterEditMode {
    #[default]
    Plain,
    Xhtml,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditableBookChapterDto {
    pub id: String,
    pub title: String,
    /// `0` = Volume / Top-level chapter, `1` = Sub-chapter nested under preceding `0`
    pub level: u8,
    pub plain_text: String,
    pub xhtml_body: String,
    pub has_rich_media: bool,
    pub source_href: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookContentDocumentDto {
    pub format: String,
    pub title: String,
    pub authors: Vec<String>,
    #[serde(default)]
    pub stylesheets: Vec<String>,
    pub chapters: Vec<EditableBookChapterDto>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveBookChapterInput {
    pub title: String,
    #[serde(default)]
    pub level: u8,
    #[serde(default)]
    pub mode: ChapterEditMode,
    #[serde(default)]
    pub plain_text: String,
    #[serde(default)]
    pub xhtml_body: String,
    #[serde(default)]
    pub source_href: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformBookContentResultDto {
    pub chapters: Vec<EditableBookChapterDto>,
    pub stylesheets: Vec<String>,
    pub changed_chapters: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsertedEpubImageDto {
    pub asset_path: String,
    pub chapter_rel_href: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolishBookReportDto {
    pub jacket_updated: bool,
    pub removed_orphan_files: usize,
}

pub fn transform_book_content_batch(
    mut chapters: Vec<EditableBookChapterDto>,
    mut stylesheets: Vec<String>,
    opts: &BookTextTransformOptions,
) -> DomainResult<TransformBookContentResultDto> {
    let mut compiled_custom = Vec::new();
    for pat in &opts.custom_strip_patterns {
        let trimmed = pat.trim();
        if trimmed.is_empty() {
            continue;
        }
        let re = Regex::new(trimmed).map_err(|e| {
            DomainError::Validation(format!("自定义净化正则表达式无效 '{trimmed}': {e}"))
        })?;
        compiled_custom.push(re);
    }

    let mut changed_chapters = 0usize;
    for ch in &mut chapters {
        let (new_title, new_plain, new_xhtml) = apply_text_transforms_to_chapter(
            &ch.title,
            &ch.plain_text,
            &ch.xhtml_body,
            opts,
            &compiled_custom,
        );
        if new_title != ch.title || new_plain != ch.plain_text || new_xhtml != ch.xhtml_body {
            changed_chapters += 1;
            ch.title = new_title;
            ch.plain_text = new_plain;
            ch.xhtml_body = new_xhtml;
        }
    }

    if opts.vertical_to_horizontal {
        for css in &mut stylesheets {
            *css = rewrite_css_vertical_to_horizontal(css);
        }
    }

    Ok(TransformBookContentResultDto {
        chapters,
        stylesheets,
        changed_chapters,
    })
}

pub fn read_book_content(path: &Path) -> DomainResult<BookContentDocumentDto> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let meta = read_book_meta_any(path)?;

    match ext.as_str() {
        "txt" => {
            let bytes = std::fs::read(path)
                .map_err(|e| DomainError::Storage(format!("failed reading txt file: {e}")))?;
            let (decoded, _) = decode_text_smart(&bytes);
            let chapters = segment_txt_chapters(&decoded, None, None)?;
            Ok(BookContentDocumentDto {
                format: "txt".to_string(),
                title: meta.title,
                authors: meta.authors,
                stylesheets: Vec::new(),
                chapters,
            })
        }
        "epub" => {
            let (chapters, stylesheets) = read_epub_chapters_and_styles(path)?;
            Ok(BookContentDocumentDto {
                format: "epub".to_string(),
                title: meta.title,
                authors: meta.authors,
                stylesheets,
                chapters,
            })
        }
        _ => Err(DomainError::UnsupportedFormat(format!(
            "book content editing only supports TXT and EPUB (got '.{ext}')"
        ))),
    }
}

pub fn resegment_txt_file(
    path: &Path,
    chapter_pattern: Option<&str>,
    volume_pattern: Option<&str>,
) -> DomainResult<Vec<EditableBookChapterDto>> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ext != "txt" {
        return Err(DomainError::Validation(
            "regex re-segmentation is only applicable to TXT files".into(),
        ));
    }
    let bytes = std::fs::read(path)
        .map_err(|e| DomainError::Storage(format!("failed reading txt file: {e}")))?;
    let (decoded, _) = decode_text_smart(&bytes);
    segment_txt_chapters(&decoded, chapter_pattern, volume_pattern)
}

pub fn segment_txt_chapters(
    raw_text: &str,
    custom_chapter_pattern: Option<&str>,
    custom_volume_pattern: Option<&str>,
) -> DomainResult<Vec<EditableBookChapterDto>> {
    let custom_chap = match custom_chapter_pattern
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(pat) => Some(
            Regex::new(pat)
                .map_err(|e| DomainError::Validation(format!("章节正则表达式无效: {e}")))?,
        ),
        None => None,
    };
    let custom_vol = match custom_volume_pattern
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(pat) => Some(
            Regex::new(pat)
                .map_err(|e| DomainError::Validation(format!("分卷正则表达式无效: {e}")))?,
        ),
        None => None,
    };

    let chap_re = custom_chap.as_ref().unwrap_or(&DEFAULT_CHAPTER_RE);
    let vol_re_opt = if custom_chap.is_some() && custom_vol.is_none() {
        None
    } else {
        Some(custom_vol.as_ref().unwrap_or(&DEFAULT_VOLUME_RE))
    };

    let normalized = raw_text.replace("\r\n", "\n").replace('\r', "\n");

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum HKind {
        Volume,
        Chapter,
    }

    struct RawSec {
        kind: HKind,
        title: String,
        under_volume: bool,
        body_start: usize,
        body_end: usize,
    }

    let mut sections: Vec<RawSec> = Vec::new();
    let mut seen_volume = false;
    let mut current: Option<(HKind, String, bool, usize)> = None;
    let mut cursor = 0usize;

    for raw_line in normalized.split_inclusive('\n') {
        let next_cursor = cursor + raw_line.len();
        let trimmed = raw_line.trim();
        let char_len = trimmed.chars().count();

        let matched_kind = if (1..=80).contains(&char_len) {
            if vol_re_opt.is_some_and(|re| re.is_match(trimmed)) {
                Some(HKind::Volume)
            } else if chap_re.is_match(trimmed)
                && (custom_chap.is_some() || !DEFAULT_PROSE_FALSE_CHAPTER_RE.is_match(trimmed))
            {
                Some(HKind::Chapter)
            } else {
                None
            }
        } else {
            None
        };

        if let Some(kind) = matched_kind {
            if let Some((pk, pt, p_under, p_start)) = current.take() {
                sections.push(RawSec {
                    kind: pk,
                    title: pt,
                    under_volume: p_under,
                    body_start: p_start,
                    body_end: cursor,
                });
            } else if cursor > 0 && !normalized[..cursor].trim().is_empty() {
                sections.push(RawSec {
                    kind: HKind::Chapter,
                    title: "前言 / 序".to_string(),
                    under_volume: false,
                    body_start: 0,
                    body_end: cursor,
                });
            }

            if kind == HKind::Volume {
                seen_volume = true;
                current = Some((HKind::Volume, trimmed.to_string(), false, next_cursor));
            } else {
                current = Some((
                    HKind::Chapter,
                    trimmed.to_string(),
                    seen_volume,
                    next_cursor,
                ));
            }
        }

        cursor = next_cursor;
    }

    if let Some((pk, pt, p_under, p_start)) = current {
        sections.push(RawSec {
            kind: pk,
            title: pt,
            under_volume: p_under,
            body_start: p_start,
            body_end: normalized.len(),
        });
    }

    if sections.is_empty() {
        let chunks = chunk_unstructured_text(&normalized, 6000);
        return Ok(chunks
            .into_iter()
            .enumerate()
            .map(|(idx, (title, plain))| {
                let xhtml = plain_to_xhtml_body(&title, &plain);
                EditableBookChapterDto {
                    id: format!("ch_{}", idx + 1),
                    title,
                    level: 0,
                    plain_text: plain,
                    xhtml_body: xhtml,
                    has_rich_media: false,
                    source_href: None,
                }
            })
            .collect());
    }

    let mut out = Vec::with_capacity(sections.len());
    for (idx, sec) in sections.into_iter().enumerate() {
        let plain = clean_plain_paragraphs(&normalized[sec.body_start..sec.body_end]);
        let level = if sec.kind == HKind::Volume || !sec.under_volume {
            0
        } else {
            1
        };
        let xhtml = plain_to_xhtml_body(&sec.title, &plain);
        out.push(EditableBookChapterDto {
            id: format!("ch_{}", idx + 1),
            title: sec.title,
            level,
            plain_text: plain,
            xhtml_body: xhtml,
            has_rich_media: false,
            source_href: None,
        });
    }
    Ok(out)
}

fn chunk_unstructured_text(text: &str, approx_chars: usize) -> Vec<(String, String)> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return vec![("第 1 章".to_string(), String::new())];
    }
    let mut out = Vec::new();
    let mut acc = String::new();
    let mut acc_chars = 0usize;

    for line in trimmed.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if !acc.is_empty() {
            acc.push('\n');
        }
        acc.push_str(t);
        acc_chars += t.chars().count();
        if acc_chars >= approx_chars {
            let idx = out.len() + 1;
            out.push((format!("第 {idx} 节"), std::mem::take(&mut acc)));
            acc_chars = 0;
        }
    }
    if !acc.is_empty() {
        let idx = out.len() + 1;
        out.push((format!("第 {idx} 节"), acc));
    }
    out
}

fn clean_plain_paragraphs(raw: &str) -> String {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if !t.is_empty() {
            lines.push(t);
        }
    }
    lines.join("\n")
}

pub fn plain_to_xhtml_body(title: &str, plain: &str) -> String {
    let mut buf = String::new();
    let clean_title = title.trim();
    if !clean_title.is_empty() {
        buf.push_str(&format!("  <h2>{}</h2>\n", escape(clean_title)));
    }
    for line in plain.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        buf.push_str(&format!("  <p>{}</p>\n", escape(t)));
    }
    buf.trim_end().to_string()
}

pub fn xhtml_body_to_plain(xhtml_body: &str, strip_heading: Option<&str>) -> String {
    let mut reader = Reader::from_str(xhtml_body);
    reader.config_mut().trim_text(false);

    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut skip_depth = 0usize;

    let flush_current = |cur: &mut String, out: &mut Vec<String>| {
        let collapsed: String = cur.split_whitespace().collect::<Vec<_>>().join(" ");
        cur.clear();
        if !collapsed.is_empty() {
            out.push(collapsed);
        }
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = e.local_name().as_ref().to_ascii_lowercase();
                if matches!(name.as_str(), "script" | "style" | "head") {
                    skip_depth += 1;
                } else if skip_depth == 0
                    && matches!(
                        name.as_str(),
                        "p" | "div"
                            | "h1"
                            | "h2"
                            | "h3"
                            | "h4"
                            | "h5"
                            | "h6"
                            | "li"
                            | "blockquote"
                            | "section"
                            | "article"
                            | "tr"
                            | "br"
                    )
                {
                    flush_current(&mut current, &mut paragraphs);
                }
            }
            Ok(Event::Empty(ref e)) => {
                if skip_depth == 0 && e.local_name().as_ref().eq_ignore_ascii_case("br") {
                    flush_current(&mut current, &mut paragraphs);
                }
            }
            Ok(Event::Text(ref t)) if skip_depth == 0 => {
                current.push_str(&t.xml_content(quick_xml::XmlVersion::Implicit1_0));
            }
            Ok(Event::GeneralRef(ref r)) if skip_depth == 0 => {
                append_xml_ref(&mut current, r);
            }
            Ok(Event::CData(ref c)) if skip_depth == 0 => {
                current.push_str(c.as_ref());
            }
            Ok(Event::End(ref e)) => {
                let name = e.local_name().as_ref().to_ascii_lowercase();
                if matches!(name.as_str(), "script" | "style" | "head") {
                    skip_depth = skip_depth.saturating_sub(1);
                } else if skip_depth == 0
                    && matches!(
                        name.as_str(),
                        "p" | "div"
                            | "h1"
                            | "h2"
                            | "h3"
                            | "h4"
                            | "h5"
                            | "h6"
                            | "li"
                            | "blockquote"
                            | "section"
                            | "article"
                            | "tr"
                    )
                {
                    flush_current(&mut current, &mut paragraphs);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    flush_current(&mut current, &mut paragraphs);

    if let Some(h) = strip_heading.map(str::trim).filter(|s| !s.is_empty())
        && paragraphs.first().is_some_and(|first| first.trim() == h)
    {
        paragraphs.remove(0);
    }

    paragraphs.join("\n")
}

fn detect_rich_media_in_xhtml(xhtml: &str) -> bool {
    static RICH_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)<\s*(?:img|svg|image|table|video|audio|figure|canvas)\b")
            .expect("valid rich media regex")
    });
    RICH_RE.is_match(xhtml)
}

fn split_xhtml_head_and_body(full_xhtml: &str) -> (Option<String>, String) {
    static HEAD_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<head\b[^>]*>.*?</head>").expect("valid head regex"));
    static BODY_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?is)<body\b[^>]*>(?P<inner>.*?)</body>").expect("valid body regex")
    });

    let head = HEAD_RE.find(full_xhtml).map(|m| m.as_str().to_string());
    let body_inner = BODY_RE
        .captures(full_xhtml)
        .and_then(|c| c.name("inner"))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_else(|| full_xhtml.trim().to_string());
    (head, body_inner)
}

fn extract_heading_from_xhtml(full_xhtml: &str) -> Option<String> {
    static H_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?is)<(?:h1|h2|h3|title)\b[^>]*>(?P<t>.*?)</(?:h1|h2|h3|title)>")
            .expect("valid heading regex")
    });
    for caps in H_RE.captures_iter(full_xhtml) {
        if let Some(m) = caps.name("t") {
            let plain = xhtml_body_to_plain(m.as_str(), None);
            let trimmed = plain.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn wrap_full_xhtml_document(
    title: &str,
    lang: &str,
    preserved_head: Option<&str>,
    css_rel_href: Option<&str>,
    body_inner: &str,
) -> String {
    let head_block = if let Some(h) = preserved_head {
        h.to_string()
    } else {
        let css_line = css_rel_href
            .map(|href| {
                format!(
                    "  <link rel=\"stylesheet\" type=\"text/css\" href=\"{}\"/>\n",
                    escape(href)
                )
            })
            .unwrap_or_default();
        format!(
            "<head>\n  <meta charset=\"utf-8\"/>\n  <title>{}</title>\n{}</head>",
            escape(if title.trim().is_empty() {
                "Chapter"
            } else {
                title.trim()
            }),
            css_line
        )
    };

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"{lang}\" lang=\"{lang}\">\n{head_block}\n<body>\n{body_inner}\n</body>\n</html>\n",
        lang = escape(if lang.trim().is_empty() {
            "zh-CN"
        } else {
            lang.trim()
        }),
    )
}

#[derive(Clone, Debug)]
struct ManifestNode {
    id: String,
    href: String,
    full_path: String,
    media_type: String,
    properties: Option<String>,
}

struct EpubStructureInfo {
    opf_path: String,
    opf_dir: PathBuf,
    raw_opf: String,
    package_version: String,
    unique_id: String,
    toc_ncx_attr: Option<String>,
    spine_page_progression: Option<String>,
    cover_meta_id: Option<String>,
    manifest: Vec<ManifestNode>,
    spine_idrefs: Vec<String>,
}

fn percent_decode_path(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push(((hi << 4) | lo) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| raw.to_string())
}

fn read_zip_bytes<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    entry_path: &str,
) -> Option<Vec<u8>> {
    if let Ok(mut f) = archive.by_name(entry_path) {
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).ok()?;
        return Some(buf);
    }
    if entry_path.contains('%') {
        let decoded = percent_decode_path(entry_path);
        if let Ok(mut f) = archive.by_name(&decoded) {
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).ok()?;
            return Some(buf);
        }
    }
    None
}

fn parse_epub_structure<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
) -> DomainResult<EpubStructureInfo> {
    let container_bytes = read_zip_bytes(archive, "META-INF/container.xml")
        .ok_or_else(|| DomainError::Validation("EPUB missing META-INF/container.xml".into()))?;
    let opf_path = find_opf_path_in_container(&container_bytes)
        .ok_or_else(|| DomainError::Validation("EPUB container.xml missing rootfile".into()))?;
    let opf_bytes = read_zip_bytes(archive, &opf_path)
        .ok_or_else(|| DomainError::Validation(format!("EPUB missing OPF '{opf_path}'")))?;
    let raw_opf = String::from_utf8_lossy(&opf_bytes).into_owned();
    let opf_dir = Path::new(&opf_path)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();

    let mut reader = Reader::from_str(&raw_opf);
    reader.config_mut().trim_text(true);

    let mut package_version = "3.0".to_string();
    let mut unique_id = "bookid".to_string();
    let mut toc_ncx_attr = None;
    let mut spine_page_progression = None;
    let mut cover_meta_id = None;
    let mut manifest = Vec::new();
    let mut spine_idrefs = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e) | Event::Empty(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                match local.as_str() {
                    "package" => {
                        if let Some(v) = attr_value(e, "version") {
                            package_version = v;
                        }
                        if let Some(uid) = attr_value(e, "unique-identifier") {
                            unique_id = uid;
                        }
                    }
                    "meta" => {
                        if attr_value(e, "name")
                            .is_some_and(|n| n.trim().eq_ignore_ascii_case("cover"))
                        {
                            cover_meta_id = attr_value(e, "content")
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty());
                        }
                    }
                    "item" => {
                        if let (Some(id), Some(href)) = (attr_value(e, "id"), attr_value(e, "href"))
                        {
                            let media_type = attr_value(e, "media-type").unwrap_or_default();
                            let properties = attr_value(e, "properties");
                            let full_path = normalize_zip_rel(&opf_dir, &href);
                            manifest.push(ManifestNode {
                                id,
                                href,
                                full_path,
                                media_type,
                                properties,
                            });
                        }
                    }
                    "spine" => {
                        toc_ncx_attr = attr_value(e, "toc");
                        spine_page_progression = attr_value(e, "page-progression-direction");
                    }
                    "itemref" => {
                        if let Some(idref) = attr_value(e, "idref") {
                            spine_idrefs.push(idref);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    Ok(EpubStructureInfo {
        opf_path,
        opf_dir,
        raw_opf,
        package_version,
        unique_id,
        toc_ncx_attr,
        spine_page_progression,
        cover_meta_id,
        manifest,
        spine_idrefs,
    })
}

fn parse_nav_toc_map(nav_bytes: &[u8], nav_dir: &Path) -> HashMap<String, (String, u8)> {
    let xml = String::from_utf8_lossy(nav_bytes);
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);

    let mut out: HashMap<String, (String, u8)> = HashMap::new();
    let mut in_toc_nav = false;
    let mut nav_depth = 0usize;
    let mut ol_depth = 0u8;
    let mut active_href: Option<String> = None;
    let mut text_buf = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                match local.as_str() {
                    "nav" => {
                        if nav_depth == 0 {
                            let is_toc = attr_value(e, "type")
                                .is_some_and(|v| v.split_whitespace().any(|t| t == "toc"))
                                || attr_value(e, "role")
                                    .is_some_and(|v| v.split_whitespace().any(|t| t == "doc-toc"));
                            in_toc_nav = is_toc || out.is_empty();
                            ol_depth = 0;
                        }
                        nav_depth += 1;
                    }
                    "ol" | "ul" if in_toc_nav => {
                        ol_depth = ol_depth.saturating_add(1);
                    }
                    "a" if in_toc_nav && ol_depth > 0 => {
                        active_href = attr_value(e, "href").map(|h| normalize_zip_rel(nav_dir, &h));
                        text_buf.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref t)) if active_href.is_some() => {
                text_buf.push_str(&t.xml_content(quick_xml::XmlVersion::Implicit1_0));
            }
            Ok(Event::GeneralRef(ref r)) if active_href.is_some() => {
                append_xml_ref(&mut text_buf, r);
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                match local.as_str() {
                    "a" if in_toc_nav => {
                        if let Some(full) = active_href.take() {
                            let title = text_buf.trim().to_string();
                            if !title.is_empty() {
                                out.entry(full)
                                    .or_insert((title, ol_depth.saturating_sub(1).min(1)));
                            }
                        }
                        text_buf.clear();
                    }
                    "ol" | "ul" if in_toc_nav => {
                        ol_depth = ol_depth.saturating_sub(1);
                    }
                    "nav" if nav_depth > 0 => {
                        nav_depth -= 1;
                        if nav_depth == 0 {
                            in_toc_nav = false;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    out
}

fn parse_ncx_toc_map(ncx_bytes: &[u8], ncx_dir: &Path) -> HashMap<String, (String, u8)> {
    let xml = String::from_utf8_lossy(ncx_bytes);
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);

    let mut out: HashMap<String, (String, u8)> = HashMap::new();
    let mut depth = 0u8;
    let mut in_text = false;
    let mut cur_title = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                match local.as_str() {
                    "navpoint" => {
                        depth = depth.saturating_add(1);
                        cur_title.clear();
                    }
                    "text" if depth > 0 => {
                        in_text = true;
                        cur_title.clear();
                    }
                    "content" if depth > 0 => {
                        if let Some(src) = attr_value(e, "src") {
                            let full = normalize_zip_rel(ncx_dir, &src);
                            let t = cur_title.trim().to_string();
                            if !t.is_empty() {
                                out.entry(full)
                                    .or_insert((t, depth.saturating_sub(1).min(1)));
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                if depth > 0
                    && e.local_name().as_ref().eq_ignore_ascii_case("content")
                    && let Some(src) = attr_value(e, "src")
                {
                    let full = normalize_zip_rel(ncx_dir, &src);
                    let t = cur_title.trim().to_string();
                    if !t.is_empty() {
                        out.entry(full)
                            .or_insert((t, depth.saturating_sub(1).min(1)));
                    }
                }
            }
            Ok(Event::Text(ref t)) if in_text => {
                cur_title.push_str(&t.xml_content(quick_xml::XmlVersion::Implicit1_0));
            }
            Ok(Event::GeneralRef(ref r)) if in_text => {
                append_xml_ref(&mut cur_title, r);
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                match local.as_str() {
                    "text" => in_text = false,
                    "navpoint" => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    out
}

fn is_nav_manifest_node(m: &ManifestNode) -> bool {
    m.properties
        .as_deref()
        .is_some_and(|p| p.split_whitespace().any(|tok| tok == "nav"))
}

fn is_cover_wrapper_doc(
    m: &ManifestNode,
    toc_map: &HashMap<String, (String, u8)>,
    plain: &str,
    body_xhtml: &str,
) -> bool {
    if toc_map.contains_key(&m.full_path) {
        return false;
    }
    let id_lower = m.id.to_ascii_lowercase();
    let href_lower = m.href.to_ascii_lowercase();
    let looks_cover_named = id_lower == "cover"
        || id_lower == "cover-page"
        || href_lower.ends_with("cover.xhtml")
        || href_lower.ends_with("cover.html")
        || href_lower.ends_with("titlepage.xhtml");
    looks_cover_named && plain.chars().count() < 20 && detect_rich_media_in_xhtml(body_xhtml)
}

pub fn read_epub_internal_asset(
    epub_path: &Path,
    raw_asset_path: &str,
) -> DomainResult<BinaryPayload> {
    let normalized = normalize_zip_rel(Path::new(""), raw_asset_path.trim_start_matches('/'));
    if normalized.is_empty() {
        return Err(DomainError::Validation("empty epub asset path".into()));
    }
    let f = File::open(epub_path)
        .map_err(|e| DomainError::Storage(format!("failed opening epub: {e}")))?;
    let mut archive = ZipArchive::new(BufReader::new(f))
        .map_err(|e| DomainError::Validation(format!("invalid epub archive: {e}")))?;
    let bytes = read_zip_bytes(&mut archive, &normalized).ok_or_else(|| {
        DomainError::NotFound(format!("epub internal asset '{normalized}' not found"))
    })?;
    let guessed = mime_guess::from_path(&normalized).first_raw();
    let mime_type = match guessed {
        Some(m) if !m.starts_with("image/") => m.to_string(),
        Some(m) => {
            let sniffed = sniff_image_mime(&bytes);
            if sniffed.starts_with("image/") {
                sniffed.to_string()
            } else {
                m.to_string()
            }
        }
        None => sniff_image_mime(&bytes).to_string(),
    };
    Ok(BinaryPayload { bytes, mime_type })
}

fn sanitize_xhtml_fragment(raw: &str) -> String {
    static VOID_TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)<(?P<tag>br|hr|img|meta|link|col|input)(?P<attrs>[^>]*?)(?P<slash>/?)>")
            .expect("valid void tag regex")
    });
    let replaced_entities = raw.replace("&nbsp;", "&#160;");
    VOID_TAG_RE
        .replace_all(&replaced_entities, |caps: &regex::Captures<'_>| {
            let tag = caps.name("tag").map(|m| m.as_str()).unwrap_or("br");
            let attrs = caps.name("attrs").map(|m| m.as_str()).unwrap_or("");
            let slash = caps.name("slash").map(|m| m.as_str()).unwrap_or("");
            if slash == "/" || attrs.trim_end().ends_with('/') {
                caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string()
            } else {
                format!("<{tag}{attrs}/>")
            }
        })
        .into_owned()
}

fn read_epub_chapters_and_styles(
    epub_path: &Path,
) -> DomainResult<(Vec<EditableBookChapterDto>, Vec<String>)> {
    let f = File::open(epub_path)
        .map_err(|e| DomainError::Storage(format!("failed opening epub: {e}")))?;
    let mut archive = ZipArchive::new(BufReader::new(f))
        .map_err(|e| DomainError::Validation(format!("invalid epub archive: {e}")))?;
    let info = parse_epub_structure(&mut archive)?;

    let mut toc_map: HashMap<String, (String, u8)> = HashMap::new();
    if let Some(nav_node) = info.manifest.iter().find(|m| is_nav_manifest_node(m))
        && let Some(nav_bytes) = read_zip_bytes(&mut archive, &nav_node.full_path)
    {
        let nav_dir = Path::new(&nav_node.full_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        toc_map = parse_nav_toc_map(&nav_bytes, nav_dir);
    }
    if toc_map.is_empty()
        && let Some(ncx_node) = info.manifest.iter().find(|m| {
            info.toc_ncx_attr.as_deref() == Some(m.id.as_str())
                || m.media_type
                    .eq_ignore_ascii_case("application/x-dtbncx+xml")
        })
        && let Some(ncx_bytes) = read_zip_bytes(&mut archive, &ncx_node.full_path)
    {
        let ncx_dir = Path::new(&ncx_node.full_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        toc_map = parse_ncx_toc_map(&ncx_bytes, ncx_dir);
    }

    let manifest_by_id: HashMap<&str, &ManifestNode> =
        info.manifest.iter().map(|m| (m.id.as_str(), m)).collect();

    let mut chapters = Vec::new();
    for idref in &info.spine_idrefs {
        let Some(node) = manifest_by_id.get(idref.as_str()).copied() else {
            continue;
        };
        if is_nav_manifest_node(node) {
            continue;
        }
        if !node.media_type.contains("html") && !node.media_type.contains("xml") {
            continue;
        }
        let Some(bytes) = read_zip_bytes(&mut archive, &node.full_path) else {
            continue;
        };
        let full_str = String::from_utf8_lossy(&bytes);
        let (_, body_inner) = split_xhtml_head_and_body(&full_str);

        let (toc_title_opt, level) = toc_map
            .get(&node.full_path)
            .map(|(t, l)| (Some(t.clone()), *l))
            .unwrap_or((None, 0));

        let preliminary_plain = xhtml_body_to_plain(&body_inner, None);
        if is_cover_wrapper_doc(node, &toc_map, &preliminary_plain, &body_inner) {
            continue;
        }

        let resolved_title = toc_title_opt
            .or_else(|| extract_heading_from_xhtml(&full_str))
            .unwrap_or_else(|| format!("第 {} 章", chapters.len() + 1));

        let plain_text = xhtml_body_to_plain(&body_inner, Some(&resolved_title));
        let has_rich_media = detect_rich_media_in_xhtml(&body_inner);

        chapters.push(EditableBookChapterDto {
            id: node.id.clone(),
            title: resolved_title,
            level,
            plain_text,
            xhtml_body: body_inner,
            has_rich_media,
            source_href: Some(node.full_path.clone()),
        });
    }

    let mut stylesheets = Vec::new();
    for m in &info.manifest {
        if m.media_type.eq_ignore_ascii_case("text/css")
            && let Some(css_bytes) = read_zip_bytes(&mut archive, &m.full_path)
        {
            let css_str = String::from_utf8_lossy(&css_bytes).trim().to_string();
            if !css_str.is_empty() {
                stylesheets.push(css_str);
            }
        }
    }

    Ok((chapters, stylesheets))
}

fn relative_href_from_dir(from_dir: &Path, target_full_path: &str) -> String {
    let from_comps: Vec<String> = from_dir
        .components()
        .filter_map(|c| match c {
            Component::Normal(p) => Some(p.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let target_comps: Vec<&str> = target_full_path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    let mut common = 0usize;
    while common < from_comps.len()
        && common < target_comps.len()
        && from_comps[common] == target_comps[common]
    {
        common += 1;
    }

    let mut rel_parts: Vec<String> = Vec::new();
    for _ in common..from_comps.len() {
        rel_parts.push("..".to_string());
    }
    for seg in &target_comps[common..] {
        rel_parts.push((*seg).to_string());
    }
    rel_parts.join("/")
}

/// Renders an EPUB 3 Navigation Document (`nav.xhtml`) supporting 2-level Volume -> Chapter nesting.
fn render_nav_xhtml(
    book_title: &str,
    lang: &str,
    items: &[(String, String, u8)],
    nav_dir: &Path,
) -> String {
    let mut body = String::new();
    body.push_str("  <nav epub:type=\"toc\" id=\"toc\" role=\"doc-toc\">\n");
    body.push_str(&format!("    <h1>{}</h1>\n", escape(book_title)));
    body.push_str("    <ol>\n");

    let mut idx = 0usize;
    while idx < items.len() {
        let (title, full_path, _lvl) = &items[idx];
        let rel_href = relative_href_from_dir(nav_dir, full_path);

        // Collect immediately following level=1 children if this item is level=0
        let is_top = items[idx].2 == 0 || idx == 0;
        let mut children = Vec::new();
        if is_top {
            let mut next = idx + 1;
            while next < items.len() && items[next].2 > 0 {
                children.push(&items[next]);
                next += 1;
            }
        }

        if children.is_empty() {
            body.push_str(&format!(
                "      <li><a href=\"{}\">{}</a></li>\n",
                escape(&rel_href),
                escape(title)
            ));
            idx += 1;
        } else {
            body.push_str(&format!(
                "      <li>\n        <a href=\"{}\">{}</a>\n        <ol>\n",
                escape(&rel_href),
                escape(title)
            ));
            for (ct, cp, _) in &children {
                let crel = relative_href_from_dir(nav_dir, cp);
                body.push_str(&format!(
                    "          <li><a href=\"{}\">{}</a></li>\n",
                    escape(&crel),
                    escape(ct)
                ));
            }
            body.push_str("        </ol>\n      </li>\n");
            idx += 1 + children.len();
        }
    }

    body.push_str("    </ol>\n  </nav>");

    let clean_lang = if lang.trim().is_empty() {
        "zh-CN"
    } else {
        lang.trim()
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{l}\" lang=\"{l}\">\n<head>\n  <meta charset=\"utf-8\"/>\n  <title>{t}</title>\n</head>\n<body>\n{body}\n</body>\n</html>\n",
        l = escape(clean_lang),
        t = escape(book_title),
    )
}

/// Renders an EPUB 2 NCX Table of Contents (`toc.ncx`) for universal reader compatibility.
fn render_toc_ncx(
    book_uid: &str,
    book_title: &str,
    items: &[(String, String, u8)],
    ncx_dir: &Path,
) -> String {
    let has_sub = items.iter().any(|(_, _, lvl)| *lvl > 0);
    let depth = if has_sub { 2 } else { 1 };
    let mut nav_map = String::new();
    let mut play_order = 1usize;

    let mut idx = 0usize;
    while idx < items.len() {
        let (title, full_path, _) = &items[idx];
        let rel_href = relative_href_from_dir(ncx_dir, full_path);
        let cur_order = play_order;
        play_order += 1;

        let is_top = items[idx].2 == 0 || idx == 0;
        let mut children = Vec::new();
        if is_top {
            let mut next = idx + 1;
            while next < items.len() && items[next].2 > 0 {
                children.push(&items[next]);
                next += 1;
            }
        }

        nav_map.push_str(&format!(
            "    <navPoint id=\"navPoint-{cur_order}\" playOrder=\"{cur_order}\">\n      <navLabel><text>{}</text></navLabel>\n      <content src=\"{}\"/>\n",
            escape(title),
            escape(&rel_href)
        ));

        for (ct, cp, _) in &children {
            let c_order = play_order;
            play_order += 1;
            let crel = relative_href_from_dir(ncx_dir, cp);
            nav_map.push_str(&format!(
                "      <navPoint id=\"navPoint-{c_order}\" playOrder=\"{c_order}\">\n        <navLabel><text>{}</text></navLabel>\n        <content src=\"{}\"/>\n      </navPoint>\n",
                escape(ct),
                escape(&crel)
            ));
        }

        nav_map.push_str("    </navPoint>\n");
        idx += 1 + children.len();
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n  <head>\n    <meta name=\"dtb:uid\" content=\"{uid}\"/>\n    <meta name=\"dtb:depth\" content=\"{depth}\"/>\n    <meta name=\"dtb:totalPageCount\" content=\"0\"/>\n    <meta name=\"dtb:maxPageNumber\" content=\"0\"/>\n  </head>\n  <docTitle><text>{title}</text></docTitle>\n  <navMap>\n{nav_map}  </navMap>\n</ncx>\n",
        uid = escape(book_uid),
        title = escape(book_title),
    )
}

pub fn save_book_content(
    path: &Path,
    chapters: &[SaveBookChapterInput],
    stylesheets: Option<&[String]>,
    force_horizontal_spine: bool,
) -> DomainResult<()> {
    if chapters.is_empty() {
        return Err(DomainError::Validation("图书至少需要保留一个章节".into()));
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match ext.as_str() {
        "txt" => save_txt_content(path, chapters),
        "epub" => save_epub_content(path, chapters, stylesheets, force_horizontal_spine),
        _ => Err(DomainError::UnsupportedFormat(format!(
            "book content saving only supports TXT and EPUB (got '.{ext}')"
        ))),
    }
}

fn save_txt_content(path: &Path, chapters: &[SaveBookChapterInput]) -> DomainResult<()> {
    let mut blocks = Vec::with_capacity(chapters.len());
    for (idx, ch) in chapters.iter().enumerate() {
        let title = if ch.title.trim().is_empty() {
            format!("第 {} 章", idx + 1)
        } else {
            ch.title.trim().to_string()
        };
        let body = match ch.mode {
            ChapterEditMode::Plain => clean_plain_paragraphs(&ch.plain_text),
            ChapterEditMode::Xhtml => xhtml_body_to_plain(&ch.xhtml_body, Some(&title)),
        };
        if body.is_empty() {
            blocks.push(title);
        } else {
            blocks.push(format!("{title}\n\n{body}"));
        }
    }
    let joined = format!("{}\n", blocks.join("\n\n\n"));
    atomic_write_bytes(path, joined.as_bytes())
}

fn save_epub_content(
    epub_path: &Path,
    chapters: &[SaveBookChapterInput],
    stylesheets: Option<&[String]>,
    force_horizontal_spine: bool,
) -> DomainResult<()> {
    let meta = read_book_meta_any(epub_path)?;
    let lang = meta.language.as_deref().unwrap_or("zh-CN");

    let f = File::open(epub_path)
        .map_err(|e| DomainError::Storage(format!("failed opening epub: {e}")))?;
    let mut archive = ZipArchive::new(BufReader::new(f))
        .map_err(|e| DomainError::Validation(format!("invalid epub archive: {e}")))?;
    let info = parse_epub_structure(&mut archive)?;

    // Build existing TOC map solely to detect preserved leading cover wrapper in spine
    let mut old_toc_map: HashMap<String, (String, u8)> = HashMap::new();
    if let Some(nav_node) = info.manifest.iter().find(|m| is_nav_manifest_node(m))
        && let Some(nav_bytes) = read_zip_bytes(&mut archive, &nav_node.full_path)
    {
        let nav_dir = Path::new(&nav_node.full_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        old_toc_map = parse_nav_toc_map(&nav_bytes, nav_dir);
    }

    let manifest_by_id: HashMap<&str, &ManifestNode> =
        info.manifest.iter().map(|m| (m.id.as_str(), m)).collect();
    let manifest_by_full: HashMap<&str, &ManifestNode> = info
        .manifest
        .iter()
        .map(|m| (m.full_path.as_str(), m))
        .collect();

    // Identify old editable chapter paths & optional preserved leading cover spine id
    let mut old_editable_paths: HashSet<String> = HashSet::new();
    let mut preserved_cover_spine_id: Option<String> = None;

    for (pos, idref) in info.spine_idrefs.iter().enumerate() {
        let Some(node) = manifest_by_id.get(idref.as_str()).copied() else {
            continue;
        };
        if is_nav_manifest_node(node) {
            continue;
        }
        if !node.media_type.contains("html") && !node.media_type.contains("xml") {
            continue;
        }
        if let Some(bytes) = read_zip_bytes(&mut archive, &node.full_path) {
            let full_str = String::from_utf8_lossy(&bytes);
            let (_, body_inner) = split_xhtml_head_and_body(&full_str);
            let plain = xhtml_body_to_plain(&body_inner, None);
            if pos == 0 && is_cover_wrapper_doc(node, &old_toc_map, &plain, &body_inner) {
                preserved_cover_spine_id = Some(node.id.clone());
                continue;
            }
        }
        old_editable_paths.insert(node.full_path.clone());
    }

    // Locate or create stylesheet for newly inserted chapters & persist edited CSS stylesheets
    let existing_css_nodes: Vec<&ManifestNode> = info
        .manifest
        .iter()
        .filter(|m| m.media_type.eq_ignore_ascii_case("text/css"))
        .collect();
    let had_no_existing_css = existing_css_nodes.is_empty();

    let mut replacements: Vec<(String, Vec<u8>)> = Vec::new();
    let mut extra_manifest_nodes: Vec<ManifestNode> = Vec::new();

    let effective_css_full = if let Some(first_css) = existing_css_nodes.first() {
        if let Some(edited_list) = stylesheets {
            for (idx, css_node) in existing_css_nodes.iter().enumerate() {
                if let Some(raw_css) = edited_list.get(idx) {
                    let final_css = if force_horizontal_spine {
                        rewrite_css_vertical_to_horizontal(raw_css)
                    } else {
                        raw_css.clone()
                    };
                    replacements.push((css_node.full_path.clone(), final_css.into_bytes()));
                } else if force_horizontal_spine
                    && let Some(orig_bytes) = read_zip_bytes(&mut archive, &css_node.full_path)
                {
                    let orig_str = String::from_utf8_lossy(&orig_bytes);
                    let rewritten = rewrite_css_vertical_to_horizontal(&orig_str);
                    replacements.push((css_node.full_path.clone(), rewritten.into_bytes()));
                }
            }
        } else if force_horizontal_spine {
            for css_node in &existing_css_nodes {
                if let Some(orig_bytes) = read_zip_bytes(&mut archive, &css_node.full_path) {
                    let orig_str = String::from_utf8_lossy(&orig_bytes);
                    let rewritten = rewrite_css_vertical_to_horizontal(&orig_str);
                    replacements.push((css_node.full_path.clone(), rewritten.into_bytes()));
                }
            }
        }
        first_css.full_path.clone()
    } else {
        let css_full = normalize_zip_rel(&info.opf_dir, "Styles/xmedia_style.css");
        let css_href = relative_href_from_dir(&info.opf_dir, &css_full);
        let initial_css = stylesheets
            .and_then(|s| s.first())
            .map(String::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(DEFAULT_EPUB_CSS);
        let final_css = if force_horizontal_spine {
            rewrite_css_vertical_to_horizontal(initial_css)
        } else {
            initial_css.to_string()
        };
        replacements.push((css_full.clone(), final_css.into_bytes()));
        extra_manifest_nodes.push(ManifestNode {
            id: "xmedia-default-css".to_string(),
            href: css_href,
            full_path: css_full.clone(),
            media_type: "text/css".to_string(),
            properties: None,
        });
        css_full
    };

    let mut used_source_paths: HashSet<String> = HashSet::new();
    let mut used_ids: HashSet<String> = info.manifest.iter().map(|m| m.id.clone()).collect();
    let mut final_chapter_items: Vec<(String, String, u8, String)> =
        Vec::with_capacity(chapters.len()); // (title, full_path, level, manifest_id)

    for (idx, ch) in chapters.iter().enumerate() {
        let title = if ch.title.trim().is_empty() {
            format!("第 {} 章", idx + 1)
        } else {
            ch.title.trim().to_string()
        };
        let level = ch.level.min(1);

        let reusable_source = ch
            .source_href
            .as_deref()
            .filter(|p| old_editable_paths.contains(*p) && !used_source_paths.contains(*p))
            .map(str::to_string);

        let (target_full_path, manifest_id, preserved_head) =
            if let Some(src_full) = reusable_source {
                used_source_paths.insert(src_full.clone());
                let head = read_zip_bytes(&mut archive, &src_full).and_then(|b| {
                    let s = String::from_utf8_lossy(&b);
                    split_xhtml_head_and_body(&s).0
                });
                let id = manifest_by_full
                    .get(src_full.as_str())
                    .map(|m| m.id.clone())
                    .unwrap_or_else(|| format!("ch-{}", idx + 1));
                (src_full, id, head)
            } else {
                let mut seq = idx + 1;
                let (new_full, new_id) = loop {
                    let cand_rel = format!("Text/chapter_edit_{seq:04}.xhtml");
                    let cand_full = normalize_zip_rel(&info.opf_dir, &cand_rel);
                    let cand_id = format!("xch-{seq:04}");
                    if !used_source_paths.contains(&cand_full)
                        && !manifest_by_full.contains_key(cand_full.as_str())
                        && !used_ids.contains(&cand_id)
                    {
                        used_ids.insert(cand_id.clone());
                        used_source_paths.insert(cand_full.clone());
                        break (cand_full, cand_id);
                    }
                    seq += 1;
                };
                let href = relative_href_from_dir(&info.opf_dir, &new_full);
                extra_manifest_nodes.push(ManifestNode {
                    id: new_id.clone(),
                    href,
                    full_path: new_full.clone(),
                    media_type: "application/xhtml+xml".to_string(),
                    properties: None,
                });
                (new_full, new_id, None)
            };

        let ch_dir = Path::new(&target_full_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let css_rel = relative_href_from_dir(ch_dir, &effective_css_full);

        let body_inner = match ch.mode {
            ChapterEditMode::Plain => plain_to_xhtml_body(&title, &ch.plain_text),
            ChapterEditMode::Xhtml => {
                if ch.xhtml_body.trim().is_empty() {
                    plain_to_xhtml_body(&title, &ch.plain_text)
                } else {
                    sanitize_xhtml_fragment(&ch.xhtml_body)
                }
            }
        };

        let effective_head = preserved_head.map(|mut h| {
            if force_horizontal_spine {
                h = rewrite_css_vertical_to_horizontal(&h);
            }
            if had_no_existing_css
                && stylesheets.is_some_and(|s| s.first().is_some_and(|c| !c.trim().is_empty()))
                && !h.to_ascii_lowercase().contains("stylesheet")
                && let Some(pos) = h.to_ascii_lowercase().rfind("</head>")
            {
                let link_tag = format!(
                    "  <link rel=\"stylesheet\" type=\"text/css\" href=\"{}\"/>\n",
                    escape(&css_rel)
                );
                h.insert_str(pos, &link_tag);
            }
            h
        });

        let full_doc = wrap_full_xhtml_document(
            &title,
            lang,
            effective_head.as_deref(),
            Some(&css_rel),
            &body_inner,
        );
        replacements.push((target_full_path.clone(), full_doc.into_bytes()));
        final_chapter_items.push((title, target_full_path, level, manifest_id));
    }
    drop(archive);

    // Deleted chapter files to strip from zip & manifest
    let removed_paths: Vec<String> = old_editable_paths
        .difference(&used_source_paths)
        .cloned()
        .collect();
    let removed_set: HashSet<&str> = removed_paths.iter().map(String::as_str).collect();

    // Ensure nav.xhtml and toc.ncx exist in manifest & replacements
    let toc_triples: Vec<(String, String, u8)> = final_chapter_items
        .iter()
        .map(|(t, p, l, _)| (t.clone(), p.clone(), *l))
        .collect();

    let nav_full_path =
        if let Some(existing_nav) = info.manifest.iter().find(|m| is_nav_manifest_node(m)) {
            existing_nav.full_path.clone()
        } else {
            let nf = normalize_zip_rel(&info.opf_dir, "nav.xhtml");
            let nh = relative_href_from_dir(&info.opf_dir, &nf);
            extra_manifest_nodes.push(ManifestNode {
                id: "xmedia-nav".to_string(),
                href: nh,
                full_path: nf.clone(),
                media_type: "application/xhtml+xml".to_string(),
                properties: Some("nav".to_string()),
            });
            nf
        };
    let nav_dir = Path::new(&nav_full_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let nav_xml = render_nav_xhtml(&meta.title, lang, &toc_triples, nav_dir);
    replacements.push((nav_full_path, nav_xml.into_bytes()));

    let (ncx_id, ncx_full_path) = if let Some(existing_ncx) = info.manifest.iter().find(|m| {
        info.toc_ncx_attr.as_deref() == Some(m.id.as_str())
            || m.media_type
                .eq_ignore_ascii_case("application/x-dtbncx+xml")
    }) {
        (existing_ncx.id.clone(), existing_ncx.full_path.clone())
    } else {
        let nf = normalize_zip_rel(&info.opf_dir, "toc.ncx");
        let nh = relative_href_from_dir(&info.opf_dir, &nf);
        let nid = "xmedia-ncx".to_string();
        extra_manifest_nodes.push(ManifestNode {
            id: nid.clone(),
            href: nh,
            full_path: nf.clone(),
            media_type: "application/x-dtbncx+xml".to_string(),
            properties: None,
        });
        (nid, nf)
    };
    let ncx_dir = Path::new(&ncx_full_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let ncx_xml = render_toc_ncx(&info.unique_id, &meta.title, &toc_triples, ncx_dir);
    replacements.push((ncx_full_path, ncx_xml.into_bytes()));

    // Rebuild <manifest> and <spine> inside OPF while preserving <metadata> and package version
    let mut merged_manifest: Vec<ManifestNode> = info
        .manifest
        .into_iter()
        .filter(|m| !removed_set.contains(m.full_path.as_str()))
        .collect();
    merged_manifest.extend(extra_manifest_nodes);

    let mut spine_ids = Vec::with_capacity(final_chapter_items.len() + 1);
    if let Some(cov_id) = preserved_cover_spine_id {
        spine_ids.push(cov_id);
    }
    for (_, _, _, mid) in &final_chapter_items {
        spine_ids.push(mid.clone());
    }

    let effective_spine_dir = if force_horizontal_spine {
        None
    } else {
        info.spine_page_progression.as_deref()
    };

    let updated_opf = rebuild_opf_manifest_and_spine(
        &info.raw_opf,
        &merged_manifest,
        &spine_ids,
        &ncx_id,
        effective_spine_dir,
        &meta,
        &info.unique_id,
        &info.package_version,
    );
    replacements.push((info.opf_path, updated_opf.into_bytes()));

    rewrite_epub_entries(epub_path, replacements, &removed_paths)
}

#[allow(clippy::too_many_arguments)]
fn rebuild_opf_manifest_and_spine(
    raw_opf: &str,
    manifest: &[ManifestNode],
    spine_ids: &[String],
    ncx_id: &str,
    spine_page_progression: Option<&str>,
    meta: &BookFileMeta,
    unique_id: &str,
    package_version: &str,
) -> String {
    static MANIFEST_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?is)<(?P<p>\w+:)?manifest\b[^>]*>.*?</(?:\w+:)?manifest>")
            .expect("valid manifest regex")
    });
    static SPINE_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?is)<(?P<p>\w+:)?spine\b[^>]*>.*?</(?:\w+:)?spine>")
            .expect("valid spine regex")
    });
    static PKG_VER_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(<(?:\w+:)?package\b[^>]*?\bversion\s*=\s*["'])[^"']+(["'])"#)
            .expect("valid package version regex")
    });

    // Upgrade package version to 3.0 when nav property is present so EPUB3 validators pass
    let upgraded_opf = PKG_VER_RE.replace(raw_opf, "${1}3.0${2}").into_owned();
    let effective_ver = if package_version.starts_with('3') {
        package_version
    } else {
        "3.0"
    };

    let mut manifest_xml = String::from("<manifest>\n");
    for item in manifest {
        let props_attr = item
            .properties
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|p| format!(" properties=\"{}\"", escape(p)))
            .unwrap_or_default();
        manifest_xml.push_str(&format!(
            "    <item id=\"{}\" href=\"{}\" media-type=\"{}\"{}/>\n",
            escape(&item.id),
            escape(&item.href),
            escape(&item.media_type),
            props_attr
        ));
    }
    manifest_xml.push_str("  </manifest>");

    let dir_attr = spine_page_progression
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|d| format!(" page-progression-direction=\"{}\"", escape(d)))
        .unwrap_or_default();
    let mut spine_xml = format!("<spine toc=\"{}\"{dir_attr}>\n", escape(ncx_id));
    for idref in spine_ids {
        spine_xml.push_str(&format!("    <itemref idref=\"{}\"/>\n", escape(idref)));
    }
    spine_xml.push_str("  </spine>");

    let with_manifest = MANIFEST_RE
        .replace(&upgraded_opf, manifest_xml.as_str())
        .into_owned();
    let with_spine = SPINE_RE
        .replace(&with_manifest, spine_xml.as_str())
        .into_owned();

    replace_opf_metadata_block(&with_spine, meta, None, unique_id, effective_ver)
}

pub fn convert_txt_to_epub(
    txt_path: &Path,
    chapters_override: Option<&[SaveBookChapterInput]>,
    stylesheets_override: Option<&[String]>,
    delete_source_txt: bool,
) -> DomainResult<PathBuf> {
    let ext = txt_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ext != "txt" {
        return Err(DomainError::Validation(
            "only TXT files can be converted to EPUB".into(),
        ));
    }

    let epub_path = txt_path.with_extension("epub");
    if epub_path.exists() {
        return Err(DomainError::Conflict(format!(
            "目标文件 '{}' 已存在，请先重命名或移除已有同名 EPUB",
            epub_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        )));
    }

    let meta = read_book_meta_any(txt_path)?;
    let lang = meta.language.as_deref().unwrap_or("zh-CN");

    let resolved_chapters: Vec<SaveBookChapterInput> = if let Some(list) = chapters_override {
        if list.is_empty() {
            return Err(DomainError::Validation(
                "至少需要包含一个章节才能生成 EPUB".into(),
            ));
        }
        list.to_vec()
    } else {
        let bytes = std::fs::read(txt_path)
            .map_err(|e| DomainError::Storage(format!("failed reading txt: {e}")))?;
        let (decoded, _) = decode_text_smart(&bytes);
        segment_txt_chapters(&decoded, None, None)?
            .into_iter()
            .map(|c| SaveBookChapterInput {
                title: c.title,
                level: c.level,
                mode: ChapterEditMode::Plain,
                plain_text: c.plain_text,
                xhtml_body: c.xhtml_body,
                source_href: None,
            })
            .collect()
    };

    let cover_payload: Option<(Vec<u8>, &'static str, &'static str)> = find_sidecar_cover(txt_path)
        .and_then(|cov_path| {
            let bytes = std::fs::read(cov_path).ok()?;
            if bytes.is_empty() {
                return None;
            }
            let mime = sniff_image_mime(&bytes);
            let ext = match mime {
                "image/png" => "png",
                "image/webp" => "webp",
                "image/gif" => "gif",
                _ => "jpg",
            };
            Some((bytes, mime, ext))
        });

    let book_uuid = format!("urn:uuid:{}", Uuid::now_v7());

    atomic_write_via_temp(&epub_path, |staging| {
        let out_file = File::create(staging)
            .map_err(|e| DomainError::Storage(format!("failed creating epub staging: {e}")))?;
        let mut writer = ZipWriter::new(BufWriter::new(out_file));

        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        // 1. OCF `mimetype` uncompressed first entry
        writer
            .start_file("mimetype", stored)
            .map_err(|e| DomainError::Storage(format!("epub mimetype start error: {e}")))?;
        writer
            .write_all(b"application/epub+zip")
            .map_err(|e| DomainError::Storage(format!("epub mimetype write error: {e}")))?;

        // 2. META-INF/container.xml
        let container_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#;
        writer
            .start_file("META-INF/container.xml", deflated)
            .map_err(|e| DomainError::Storage(format!("epub container error: {e}")))?;
        writer
            .write_all(container_xml.as_bytes())
            .map_err(|e| DomainError::Storage(format!("epub container write error: {e}")))?;

        // 3. Stylesheet
        let chosen_css = stylesheets_override
            .and_then(|s| s.first())
            .map(String::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(DEFAULT_EPUB_CSS);
        writer
            .start_file("OEBPS/Styles/style.css", deflated)
            .map_err(|e| DomainError::Storage(format!("epub css error: {e}")))?;
        writer
            .write_all(chosen_css.as_bytes())
            .map_err(|e| DomainError::Storage(format!("epub css write error: {e}")))?;

        let mut manifest_nodes = vec![
            ManifestNode {
                id: "style-css".to_string(),
                href: "Styles/style.css".to_string(),
                full_path: "OEBPS/Styles/style.css".to_string(),
                media_type: "text/css".to_string(),
                properties: None,
            },
            ManifestNode {
                id: "nav".to_string(),
                href: "nav.xhtml".to_string(),
                full_path: "OEBPS/nav.xhtml".to_string(),
                media_type: "application/xhtml+xml".to_string(),
                properties: Some("nav".to_string()),
            },
            ManifestNode {
                id: "ncx".to_string(),
                href: "toc.ncx".to_string(),
                full_path: "OEBPS/toc.ncx".to_string(),
                media_type: "application/x-dtbncx+xml".to_string(),
                properties: None,
            },
        ];
        let mut spine_ids = Vec::new();

        // 4. Optional Cover Image & Cover Page
        let mut cover_item_id: Option<&str> = None;
        if let Some((cov_bytes, cov_mime, cov_ext)) = &cover_payload {
            let img_rel = format!("Images/cover.{cov_ext}");
            let img_full = format!("OEBPS/{img_rel}");
            writer
                .start_file(&img_full, deflated)
                .map_err(|e| DomainError::Storage(format!("epub cover image error: {e}")))?;
            writer
                .write_all(cov_bytes)
                .map_err(|e| DomainError::Storage(format!("epub cover write error: {e}")))?;

            let cover_xhtml = wrap_full_xhtml_document(
                "封面",
                lang,
                None,
                Some("../Styles/style.css"),
                &format!(
                    "  <div class=\"cover-wrap\"><img src=\"../{img_rel}\" alt=\"Cover\"/></div>"
                ),
            );
            writer
                .start_file("OEBPS/Text/cover.xhtml", deflated)
                .map_err(|e| DomainError::Storage(format!("epub cover xhtml error: {e}")))?;
            writer
                .write_all(cover_xhtml.as_bytes())
                .map_err(|e| DomainError::Storage(format!("epub cover xhtml write error: {e}")))?;

            manifest_nodes.push(ManifestNode {
                id: "cover-image".to_string(),
                href: img_rel,
                full_path: img_full,
                media_type: (*cov_mime).to_string(),
                properties: Some("cover-image".to_string()),
            });
            manifest_nodes.push(ManifestNode {
                id: "cover-page".to_string(),
                href: "Text/cover.xhtml".to_string(),
                full_path: "OEBPS/Text/cover.xhtml".to_string(),
                media_type: "application/xhtml+xml".to_string(),
                properties: None,
            });
            spine_ids.push("cover-page".to_string());
            cover_item_id = Some("cover-image");
        }

        // 5. Chapters
        let mut toc_triples: Vec<(String, String, u8)> =
            Vec::with_capacity(resolved_chapters.len());
        for (idx, ch) in resolved_chapters.iter().enumerate() {
            let seq = idx + 1;
            let title = if ch.title.trim().is_empty() {
                format!("第 {seq} 章")
            } else {
                ch.title.trim().to_string()
            };
            let level = ch.level.min(1);
            let rel_href = format!("Text/chapter_{seq:04}.xhtml");
            let full_path = format!("OEBPS/{rel_href}");
            let item_id = format!("ch-{seq:04}");

            let body_inner = match ch.mode {
                ChapterEditMode::Plain => plain_to_xhtml_body(&title, &ch.plain_text),
                ChapterEditMode::Xhtml => {
                    if ch.xhtml_body.trim().is_empty() {
                        plain_to_xhtml_body(&title, &ch.plain_text)
                    } else {
                        sanitize_xhtml_fragment(&ch.xhtml_body)
                    }
                }
            };
            let doc = wrap_full_xhtml_document(
                &title,
                lang,
                None,
                Some("../Styles/style.css"),
                &body_inner,
            );
            writer
                .start_file(&full_path, deflated)
                .map_err(|e| DomainError::Storage(format!("epub chapter start error: {e}")))?;
            writer
                .write_all(doc.as_bytes())
                .map_err(|e| DomainError::Storage(format!("epub chapter write error: {e}")))?;

            manifest_nodes.push(ManifestNode {
                id: item_id.clone(),
                href: rel_href,
                full_path: full_path.clone(),
                media_type: "application/xhtml+xml".to_string(),
                properties: None,
            });
            spine_ids.push(item_id);
            toc_triples.push((title, full_path, level));
        }

        // 6. nav.xhtml + toc.ncx
        let oebps_dir = Path::new("OEBPS");
        let nav_xml = render_nav_xhtml(&meta.title, lang, &toc_triples, oebps_dir);
        writer
            .start_file("OEBPS/nav.xhtml", deflated)
            .map_err(|e| DomainError::Storage(format!("epub nav start error: {e}")))?;
        writer
            .write_all(nav_xml.as_bytes())
            .map_err(|e| DomainError::Storage(format!("epub nav write error: {e}")))?;

        let ncx_xml = render_toc_ncx(&book_uuid, &meta.title, &toc_triples, oebps_dir);
        writer
            .start_file("OEBPS/toc.ncx", deflated)
            .map_err(|e| DomainError::Storage(format!("epub ncx start error: {e}")))?;
        writer
            .write_all(ncx_xml.as_bytes())
            .map_err(|e| DomainError::Storage(format!("epub ncx write error: {e}")))?;

        // 7. OEBPS/content.opf
        let base_opf = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\">\n  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n    <dc:identifier id=\"bookid\">{}</dc:identifier>\n  </metadata>\n  <manifest></manifest>\n  <spine toc=\"ncx\"></spine>\n</package>\n",
            escape(&book_uuid)
        );
        let with_meta =
            replace_opf_metadata_block(&base_opf, &meta, cover_item_id, "bookid", "3.0");
        let final_opf = rebuild_opf_manifest_and_spine(
            &with_meta,
            &manifest_nodes,
            &spine_ids,
            "ncx",
            None,
            &meta,
            "bookid",
            "3.0",
        );

        writer
            .start_file("OEBPS/content.opf", deflated)
            .map_err(|e| DomainError::Storage(format!("epub opf start error: {e}")))?;
        writer
            .write_all(final_opf.as_bytes())
            .map_err(|e| DomainError::Storage(format!("epub opf write error: {e}")))?;

        writer
            .finish()
            .map_err(|e| DomainError::Storage(format!("epub zip finalize error: {e}")))?;
        Ok(())
    })?;

    if delete_source_txt {
        let _ = std::fs::remove_file(txt_path);
    }

    Ok(epub_path)
}

pub fn insert_epub_image(
    epub_path: &Path,
    chapter_source_href: Option<&str>,
    bytes: &[u8],
    mime_hint: Option<&str>,
) -> DomainResult<InsertedEpubImageDto> {
    if bytes.is_empty() {
        return Err(DomainError::Validation("上传的图片文件为空".into()));
    }
    let ext_check = epub_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ext_check != "epub" {
        return Err(DomainError::Validation(
            "仅 EPUB 图书支持章节内插入图片".into(),
        ));
    }

    let sniffed = sniff_image_mime(bytes);
    let mime_type = if sniffed.starts_with("image/") {
        sniffed.to_string()
    } else if let Some(hint) = mime_hint.filter(|m| m.starts_with("image/")) {
        hint.to_string()
    } else {
        return Err(DomainError::Validation(
            "不支持的图片格式，请上传 JPG/PNG/WebP/GIF/SVG".into(),
        ));
    };

    let img_ext = match mime_type.as_str() {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/svg+xml" => "svg",
        _ => "jpg",
    };

    let f = File::open(epub_path)
        .map_err(|e| DomainError::Storage(format!("failed opening epub: {e}")))?;
    let mut archive = ZipArchive::new(BufReader::new(f))
        .map_err(|e| DomainError::Validation(format!("invalid epub archive: {e}")))?;
    let info = parse_epub_structure(&mut archive)?;
    drop(archive);

    let short_id = &Uuid::now_v7().simple().to_string()[..12];
    let rel_img = format!("Images/ximg_{short_id}.{img_ext}");
    let full_img = normalize_zip_rel(&info.opf_dir, &rel_img);
    let href_in_opf = relative_href_from_dir(&info.opf_dir, &full_img);
    let item_id = format!("ximg-{short_id}");

    static CLOSE_MANIFEST_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)</(?:\w+:)?manifest>").expect("valid manifest end regex")
    });

    let item_line = format!(
        "    <item id=\"{}\" href=\"{}\" media-type=\"{}\"/>\n  ",
        escape(&item_id),
        escape(&href_in_opf),
        escape(&mime_type)
    );
    let updated_opf = if let Some(m) = CLOSE_MANIFEST_RE.find(&info.raw_opf) {
        let mut s = info.raw_opf.clone();
        s.insert_str(m.start(), &item_line);
        s
    } else {
        return Err(DomainError::Validation(
            "EPUB OPF 缺少 <manifest> 节点".into(),
        ));
    };

    rewrite_epub_entries(
        epub_path,
        vec![
            (full_img.clone(), bytes.to_vec()),
            (info.opf_path, updated_opf.into_bytes()),
        ],
        &[],
    )?;

    let fallback_ch_full = normalize_zip_rel(&info.opf_dir, "Text/chapter.xhtml");
    let ch_ref_full = chapter_source_href
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&fallback_ch_full);
    let ch_dir = Path::new(ch_ref_full)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let chapter_rel_href = relative_href_from_dir(ch_dir, &full_img);

    Ok(InsertedEpubImageDto {
        asset_path: full_img,
        chapter_rel_href,
        mime_type,
    })
}

fn render_book_jacket_xhtml(
    meta: &BookFileMeta,
    cover_rel_href: Option<&str>,
    css_rel_href: Option<&str>,
) -> String {
    let lang = meta
        .language
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("zh-CN");
    let css_link = css_rel_href
        .map(|h| {
            format!(
                "  <link rel=\"stylesheet\" type=\"text/css\" href=\"{}\"/>\n",
                escape(h)
            )
        })
        .unwrap_or_default();

    let jacket_css = r#"  <style type="text/css">
    .xmedia-jacket { max-width: 38em; margin: 1.2em auto; padding: 1.2em 1.4em; border: 1px solid rgba(128,128,128,0.28); border-radius: 10px; line-height: 1.75; }
    .xmedia-jacket-head { text-align: center; margin-bottom: 1.2em; }
    .xmedia-jacket-cover { margin: 0 auto 1em; text-align: center; }
    .xmedia-jacket-cover img { max-height: 14em; max-width: 65%; border-radius: 6px; box-shadow: 0 4px 14px rgba(0,0,0,0.16); }
    .xmedia-jacket-title { font-size: 1.45em; font-weight: 700; margin: 0.2em 0 0.35em; line-height: 1.3; text-indent: 0; }
    .xmedia-jacket-author { font-size: 1em; opacity: 0.85; margin: 0 0 0.5em; text-indent: 0; }
    .xmedia-jacket-series { display: inline-block; font-size: 0.88em; padding: 0.15em 0.7em; border-radius: 999px; background: rgba(128,128,128,0.14); margin-bottom: 0.5em; }
    .xmedia-jacket-meta { font-size: 0.88em; opacity: 0.82; margin: 0.8em 0; padding: 0.65em 0.9em; background: rgba(128,128,128,0.08); border-radius: 6px; }
    .xmedia-jacket-meta div { margin: 0.2em 0; text-indent: 0; }
    .xmedia-jacket-tags { margin: 0.7em 0; text-align: center; }
    .xmedia-jacket-tag { display: inline-block; font-size: 0.8em; padding: 0.12em 0.55em; margin: 0.15em; border: 1px solid rgba(128,128,128,0.3); border-radius: 4px; }
    .xmedia-jacket-hr { border: 0; border-top: 1px solid rgba(128,128,128,0.24); margin: 1em 0; }
    .xmedia-jacket-desc p { margin: 0 0 0.75em; text-indent: 2em; text-align: justify; }
  </style>"#;

    let mut body = String::from(
        "  <section class=\"xmedia-jacket\" epub:type=\"frontmatter\">\n    <div class=\"xmedia-jacket-head\">\n",
    );
    if let Some(cov) = cover_rel_href {
        body.push_str(&format!(
            "      <div class=\"xmedia-jacket-cover\"><img src=\"{}\" alt=\"封面\"/></div>\n",
            escape(cov)
        ));
    }
    body.push_str(&format!(
        "      <h1 class=\"xmedia-jacket-title\">{}</h1>\n",
        escape(meta.title.trim())
    ));
    if !meta.authors.is_empty() {
        let joined = meta.authors.join(" / ");
        body.push_str(&format!(
            "      <p class=\"xmedia-jacket-author\">{}</p>\n",
            escape(&joined)
        ));
    }
    if let Some(ser) = meta
        .series
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let mut badge = ser.to_string();
        if let Some(sub) = meta
            .subseries
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            badge.push_str(" · ");
            badge.push_str(sub);
        }
        if let Some(idx) = meta
            .series_index
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            badge.push_str(&format!(" #{idx}"));
        }
        body.push_str(&format!(
            "      <div class=\"xmedia-jacket-series\">{}</div>\n",
            escape(&badge)
        ));
    }
    body.push_str("    </div>\n");

    if !meta.tags.is_empty() {
        body.push_str("    <div class=\"xmedia-jacket-tags\">\n");
        for tag in &meta.tags {
            let t = tag.trim();
            if !t.is_empty() {
                body.push_str(&format!(
                    "      <span class=\"xmedia-jacket-tag\">{}</span>\n",
                    escape(t)
                ));
            }
        }
        body.push_str("    </div>\n");
    }

    let mut meta_lines = Vec::new();
    if let Some(p) = meta
        .publisher
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        meta_lines.push(format!("出版社：{}", escape(p)));
    }
    if let Some(d) = meta
        .published_at
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        meta_lines.push(format!("出版日期：{}", escape(d)));
    }
    if let Some(i) = meta
        .isbn
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        meta_lines.push(format!("ISBN：{}", escape(i)));
    }
    if !meta_lines.is_empty() {
        body.push_str("    <div class=\"xmedia-jacket-meta\">\n");
        for line in meta_lines {
            body.push_str(&format!("      <div>{line}</div>\n"));
        }
        body.push_str("    </div>\n");
    }

    if let Some(desc) = meta
        .description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        body.push_str(
            "    <hr class=\"xmedia-jacket-hr\"/>\n    <div class=\"xmedia-jacket-desc\">\n",
        );
        for para in desc.lines() {
            let t = para.trim();
            if !t.is_empty() {
                body.push_str(&format!("      <p>{}</p>\n", escape(t)));
            }
        }
        body.push_str("    </div>\n");
    }

    body.push_str("  </section>");

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{l}\" lang=\"{l}\">\n<head>\n  <meta charset=\"utf-8\"/>\n  <title>书籍简介 - {t}</title>\n{css_link}{jacket_css}\n</head>\n<body>\n{body}\n</body>\n</html>\n",
        l = escape(lang),
        t = escape(meta.title.trim()),
    )
}

pub fn polish_epub_book(epub_path: &Path) -> DomainResult<PolishBookReportDto> {
    let ext_check = epub_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ext_check != "epub" {
        return Err(DomainError::Validation("仅 EPUB 图书支持一键抛光".into()));
    }

    let meta = read_book_meta_any(epub_path)?;
    let lang = meta.language.as_deref().unwrap_or("zh-CN");

    let f = File::open(epub_path)
        .map_err(|e| DomainError::Storage(format!("failed opening epub: {e}")))?;
    let mut archive = ZipArchive::new(BufReader::new(f))
        .map_err(|e| DomainError::Validation(format!("invalid epub archive: {e}")))?;
    let info = parse_epub_structure(&mut archive)?;

    // Existing TOC map for preserving TOC hierarchy & titles
    let mut toc_map: HashMap<String, (String, u8)> = HashMap::new();
    if let Some(nav_node) = info.manifest.iter().find(|m| is_nav_manifest_node(m))
        && let Some(nav_bytes) = read_zip_bytes(&mut archive, &nav_node.full_path)
    {
        let nav_dir = Path::new(&nav_node.full_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        toc_map = parse_nav_toc_map(&nav_bytes, nav_dir);
    }
    if toc_map.is_empty()
        && let Some(ncx_node) = info.manifest.iter().find(|m| {
            info.toc_ncx_attr.as_deref() == Some(m.id.as_str())
                || m.media_type
                    .eq_ignore_ascii_case("application/x-dtbncx+xml")
        })
        && let Some(ncx_bytes) = read_zip_bytes(&mut archive, &ncx_node.full_path)
    {
        let ncx_dir = Path::new(&ncx_node.full_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        toc_map = parse_ncx_toc_map(&ncx_bytes, ncx_dir);
    }

    // Locate cover image in manifest if available
    let cover_img_node = info
        .manifest
        .iter()
        .find(|m| {
            m.media_type.starts_with("image/")
                && (m
                    .properties
                    .as_deref()
                    .is_some_and(|p| p.split_whitespace().any(|t| t == "cover-image"))
                    || info.cover_meta_id.as_deref() == Some(m.id.as_str()))
        })
        .or_else(|| {
            info.manifest.iter().find(|m| {
                m.media_type.starts_with("image/")
                    && (m.id.to_ascii_lowercase().contains("cover")
                        || m.href.to_ascii_lowercase().contains("cover"))
            })
        });
    let cover_img_id = cover_img_node.map(|m| m.id.clone());
    let cover_img_full = cover_img_node.map(|m| m.full_path.clone());

    let primary_css_full = info
        .manifest
        .iter()
        .find(|m| m.media_type.eq_ignore_ascii_case("text/css"))
        .map(|m| m.full_path.clone());

    let mut extra_manifest_nodes: Vec<ManifestNode> = Vec::new();
    let mut replacements: Vec<(String, Vec<u8>)> = Vec::new();

    // Create or update xmedia_jacket.xhtml
    let (jacket_id, jacket_full_path) = if let Some(existing_jacket) = info
        .manifest
        .iter()
        .find(|m| m.id == "xmedia-jacket" || m.href.ends_with("xmedia_jacket.xhtml"))
    {
        (
            existing_jacket.id.clone(),
            existing_jacket.full_path.clone(),
        )
    } else {
        let jf = normalize_zip_rel(&info.opf_dir, "Text/xmedia_jacket.xhtml");
        let jh = relative_href_from_dir(&info.opf_dir, &jf);
        let jid = "xmedia-jacket".to_string();
        extra_manifest_nodes.push(ManifestNode {
            id: jid.clone(),
            href: jh,
            full_path: jf.clone(),
            media_type: "application/xhtml+xml".to_string(),
            properties: None,
        });
        (jid, jf)
    };

    let jacket_dir = Path::new(&jacket_full_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let jacket_cover_rel = cover_img_full
        .as_deref()
        .map(|cf| relative_href_from_dir(jacket_dir, cf));
    let jacket_css_rel = primary_css_full
        .as_deref()
        .map(|sf| relative_href_from_dir(jacket_dir, sf));
    let jacket_xhtml = render_book_jacket_xhtml(
        &meta,
        jacket_cover_rel.as_deref(),
        jacket_css_rel.as_deref(),
    );
    replacements.push((jacket_full_path.clone(), jacket_xhtml.clone().into_bytes()));

    let manifest_by_id: HashMap<&str, &ManifestNode> =
        info.manifest.iter().map(|m| (m.id.as_str(), m)).collect();

    // Determine whether spine[0] is a cover wrapper page
    let mut leading_cover_wrapper_id: Option<String> = None;
    if let Some(first_id) = info.spine_idrefs.first()
        && let Some(first_node) = manifest_by_id.get(first_id.as_str()).copied()
        && first_node.id != jacket_id
        && let Some(bytes) = read_zip_bytes(&mut archive, &first_node.full_path)
    {
        let full_str = String::from_utf8_lossy(&bytes);
        let (_, body_inner) = split_xhtml_head_and_body(&full_str);
        let plain = xhtml_body_to_plain(&body_inner, None);
        if is_cover_wrapper_doc(first_node, &toc_map, &plain, &body_inner) {
            leading_cover_wrapper_id = Some(first_node.id.clone());
        }
    }

    // Build normalized spine_ids with jacket right after cover wrapper (or at index 0)
    let mut spine_ids: Vec<String> = Vec::with_capacity(info.spine_idrefs.len() + 1);
    if let Some(ref cov_id) = leading_cover_wrapper_id {
        spine_ids.push(cov_id.clone());
    }
    spine_ids.push(jacket_id.clone());
    for idref in &info.spine_idrefs {
        if idref == &jacket_id || leading_cover_wrapper_id.as_deref() == Some(idref.as_str()) {
            continue;
        }
        if manifest_by_id.contains_key(idref.as_str()) {
            spine_ids.push(idref.clone());
        }
    }

    // Build synchronized TOC triples for nav.xhtml and toc.ncx
    let mut toc_triples: Vec<(String, String, u8)> = Vec::new();
    toc_triples.push(("书籍简介".to_string(), jacket_full_path.clone(), 0));

    for (idx, idref) in spine_ids.iter().enumerate() {
        if idref == &jacket_id || leading_cover_wrapper_id.as_deref() == Some(idref.as_str()) {
            continue;
        }
        let Some(node) = manifest_by_id.get(idref.as_str()).copied() else {
            continue;
        };
        if is_nav_manifest_node(node) {
            continue;
        }
        if let Some((t, lvl)) = toc_map.get(&node.full_path) {
            toc_triples.push((t.clone(), node.full_path.clone(), *lvl));
        } else if let Some(bytes) = read_zip_bytes(&mut archive, &node.full_path) {
            let s = String::from_utf8_lossy(&bytes);
            let title =
                extract_heading_from_xhtml(&s).unwrap_or_else(|| format!("第 {} 章", idx + 1));
            toc_triples.push((title, node.full_path.clone(), 0));
        }
    }

    // Collect all text/xhtml/svg/css content to detect unreferenced non-cover images
    let mut all_doc_sources = String::new();
    all_doc_sources.push_str(&jacket_xhtml);
    for m in &info.manifest {
        if m.full_path == jacket_full_path {
            continue;
        }
        let mt = m.media_type.to_ascii_lowercase();
        if (mt.contains("html") || mt.contains("xml") || mt.contains("css") || mt.contains("svg"))
            && let Some(bytes) = read_zip_bytes(&mut archive, &m.full_path)
        {
            all_doc_sources.push_str(&String::from_utf8_lossy(&bytes));
            all_doc_sources.push('\n');
        }
    }

    // Identify all entries currently in the ZIP archive
    let mut all_zip_files = Vec::new();
    for i in 0..archive.len() {
        if let Ok(zf) = archive.by_index(i)
            && let Ok(name_ref) = zf.name()
        {
            let name = name_ref.into_owned();
            if !name.ends_with('/') {
                all_zip_files.push(name);
            }
        }
    }
    drop(archive);

    // Ensure nav.xhtml and toc.ncx exist and are updated
    let nav_full_path =
        if let Some(existing_nav) = info.manifest.iter().find(|m| is_nav_manifest_node(m)) {
            existing_nav.full_path.clone()
        } else {
            let nf = normalize_zip_rel(&info.opf_dir, "nav.xhtml");
            let nh = relative_href_from_dir(&info.opf_dir, &nf);
            extra_manifest_nodes.push(ManifestNode {
                id: "xmedia-nav".to_string(),
                href: nh,
                full_path: nf.clone(),
                media_type: "application/xhtml+xml".to_string(),
                properties: Some("nav".to_string()),
            });
            nf
        };
    let nav_dir = Path::new(&nav_full_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let nav_xml = render_nav_xhtml(&meta.title, lang, &toc_triples, nav_dir);
    replacements.push((nav_full_path, nav_xml.into_bytes()));

    let (ncx_id, ncx_full_path) = if let Some(existing_ncx) = info.manifest.iter().find(|m| {
        info.toc_ncx_attr.as_deref() == Some(m.id.as_str())
            || m.media_type
                .eq_ignore_ascii_case("application/x-dtbncx+xml")
    }) {
        (existing_ncx.id.clone(), existing_ncx.full_path.clone())
    } else {
        let nf = normalize_zip_rel(&info.opf_dir, "toc.ncx");
        let nh = relative_href_from_dir(&info.opf_dir, &nf);
        let nid = "xmedia-ncx".to_string();
        extra_manifest_nodes.push(ManifestNode {
            id: nid.clone(),
            href: nh,
            full_path: nf.clone(),
            media_type: "application/x-dtbncx+xml".to_string(),
            properties: None,
        });
        (nid, nf)
    };
    let ncx_dir = Path::new(&ncx_full_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let ncx_xml = render_toc_ncx(&info.unique_id, &meta.title, &toc_triples, ncx_dir);
    replacements.push((ncx_full_path, ncx_xml.into_bytes()));

    // Prune unused non-cover images from manifest (accounting for percent-encoded vs UTF-8 hrefs)
    let decoded_doc_sources = if all_doc_sources.contains('%') {
        Some(percent_decode_path(&all_doc_sources))
    } else {
        None
    };
    let mut unused_manifest_paths: HashSet<String> = HashSet::new();
    let mut merged_manifest: Vec<ManifestNode> =
        Vec::with_capacity(info.manifest.len() + extra_manifest_nodes.len());
    for m in info.manifest {
        let is_cover_img = cover_img_id.as_deref() == Some(m.id.as_str())
            || m.id.to_ascii_lowercase().contains("cover")
            || m.href.to_ascii_lowercase().contains("cover");
        if m.media_type.starts_with("image/") && !is_cover_img {
            let basename = Path::new(&m.full_path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let decoded_basename = percent_decode_path(basename);
            let is_referenced = basename.is_empty()
                || all_doc_sources.contains(basename)
                || all_doc_sources.contains(&decoded_basename)
                || decoded_doc_sources
                    .as_deref()
                    .is_some_and(|d| d.contains(&decoded_basename));
            if !is_referenced {
                unused_manifest_paths.insert(m.full_path.clone());
                let decoded_full = percent_decode_path(&m.full_path);
                if decoded_full != m.full_path {
                    unused_manifest_paths.insert(decoded_full);
                }
                continue;
            }
        }
        merged_manifest.push(m);
    }
    merged_manifest.extend(extra_manifest_nodes);

    // Compute orphan files in ZIP archive (unmanifested junk + pruned unused images)
    let mut valid_zip_paths: HashSet<String> = HashSet::new();
    valid_zip_paths.insert("mimetype".to_string());
    valid_zip_paths.insert(info.opf_path.clone());
    for m in &merged_manifest {
        valid_zip_paths.insert(m.full_path.clone());
        if m.full_path.contains('%') {
            valid_zip_paths.insert(percent_decode_path(&m.full_path));
        }
    }

    let mut orphan_paths: Vec<String> = Vec::new();
    for zpath in all_zip_files {
        if zpath.starts_with("META-INF/") || zpath.eq_ignore_ascii_case("mimetype") {
            continue;
        }
        if !valid_zip_paths.contains(&zpath) || unused_manifest_paths.contains(&zpath) {
            orphan_paths.push(zpath);
        }
    }

    let updated_opf = rebuild_opf_manifest_and_spine(
        &info.raw_opf,
        &merged_manifest,
        &spine_ids,
        &ncx_id,
        info.spine_page_progression.as_deref(),
        &meta,
        &info.unique_id,
        &info.package_version,
    );
    replacements.push((info.opf_path, updated_opf.into_bytes()));

    let removed_orphan_files = orphan_paths.len();
    rewrite_epub_entries(epub_path, replacements, &orphan_paths)?;

    Ok(PolishBookReportDto {
        jacket_updated: true,
        removed_orphan_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::media::{BookMetaPatch, MediaMetadataPatch};
    use crate::domain::ports::MediaInspectorPort;
    use crate::infra::inspectors::book::EpubBookInspector;

    #[tokio::test]
    async fn txt_segmentation_convert_to_epub_and_dual_mode_epub_edit_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let txt_path = tmp.path().join("三体.txt");
        let raw_txt = "楔子\n疯狂年代的记忆。\n\n第一卷 地球往事\n\n第一章 科学边界\n叶文洁看着星空。\n第二段文字。\n\n第二章 台球\n丁仪推开了桌球。\n";
        std::fs::write(&txt_path, raw_txt.as_bytes()).unwrap();

        // Write sidecar OPF metadata (author, series, description)
        let inspector = EpubBookInspector;
        inspector
            .write_metadata(
                &txt_path,
                &MediaMetadataPatch::Book(BookMetaPatch {
                    title: Some("三体".into()),
                    authors: Some(vec!["刘慈欣".into()]),
                    series: Some("地球往事三部曲".into()),
                    series_index: Some("1".into()),
                    description: Some("地球文明向宇宙发出的第一声啼鸣。".into()),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();

        // 1. Read TXT structured chapters
        let txt_doc = read_book_content(&txt_path).unwrap();
        assert_eq!(txt_doc.format, "txt");
        assert_eq!(txt_doc.title, "三体");
        assert_eq!(txt_doc.chapters.len(), 4);
        assert_eq!(txt_doc.chapters[0].title, "楔子");
        assert_eq!(txt_doc.chapters[0].level, 0);
        assert_eq!(txt_doc.chapters[1].title, "第一卷 地球往事");
        assert_eq!(txt_doc.chapters[1].level, 0);
        assert_eq!(txt_doc.chapters[2].title, "第一章 科学边界");
        assert_eq!(txt_doc.chapters[2].level, 1);

        // 2. Convert TXT -> EPUB (keeping source TXT first)
        let epub_path = convert_txt_to_epub(&txt_path, None, None, false).unwrap();
        assert!(epub_path.is_file());
        assert!(txt_path.is_file());

        // Verify EPUB metadata & TOC + chapters
        let epub_doc = read_book_content(&epub_path).unwrap();
        assert_eq!(epub_doc.format, "epub");
        assert_eq!(epub_doc.title, "三体");
        assert_eq!(epub_doc.authors, vec!["刘慈欣"]);
        assert_eq!(epub_doc.chapters.len(), 4);
        assert_eq!(epub_doc.chapters[2].title, "第一章 科学边界");
        assert_eq!(epub_doc.chapters[2].level, 1);
        assert!(epub_doc.chapters[2].plain_text.contains("叶文洁看着星空。"));

        // 3. Edit EPUB chapters in BOTH Plain mode and XHTML mode, reorder & add a new chapter
        let edited_inputs = vec![
            SaveBookChapterInput {
                title: "第一卷 地球往事（修订版）".into(),
                level: 0,
                mode: ChapterEditMode::Plain,
                plain_text: "卷首导读。".into(),
                xhtml_body: String::new(),
                source_href: epub_doc.chapters[1].source_href.clone(),
            },
            SaveBookChapterInput {
                title: "第一章 科学边界·改".into(),
                level: 1,
                mode: ChapterEditMode::Xhtml,
                plain_text: String::new(),
                xhtml_body: "  <h2>第一章 科学边界·改</h2>\n  <p class=\"lead\">自定义 <strong>XHTML</strong> 正文段落。</p>".into(),
                source_href: epub_doc.chapters[2].source_href.clone(),
            },
            SaveBookChapterInput {
                title: "第二章 新增番外".into(),
                level: 1,
                mode: ChapterEditMode::Plain,
                plain_text: "这是新插入的一章正文。".into(),
                xhtml_body: String::new(),
                source_href: None,
            },
        ];
        save_book_content(&epub_path, &edited_inputs, None, false).unwrap();

        // 4. Reload EPUB and verify TOC hierarchy + plain & XHTML edits persisted cleanly
        let reloaded = read_book_content(&epub_path).unwrap();
        assert_eq!(reloaded.chapters.len(), 3);
        assert_eq!(reloaded.chapters[0].title, "第一卷 地球往事（修订版）");
        assert_eq!(reloaded.chapters[0].level, 0);
        assert_eq!(reloaded.chapters[0].plain_text, "卷首导读。");

        assert_eq!(reloaded.chapters[1].title, "第一章 科学边界·改");
        assert_eq!(reloaded.chapters[1].level, 1);
        assert!(
            reloaded.chapters[1]
                .xhtml_body
                .contains("<strong>XHTML</strong>")
        );
        assert_eq!(reloaded.chapters[1].plain_text, "自定义 XHTML 正文段落。");

        assert_eq!(reloaded.chapters[2].title, "第二章 新增番外");
        assert_eq!(reloaded.chapters[2].level, 1);
        assert_eq!(reloaded.chapters[2].plain_text, "这是新插入的一章正文。");

        // 5. Insert two images into EPUB; reference only the first in chapter 1, leave the second orphaned
        let png_1x1: &[u8] = &[
            0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H',
            b'D', b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, b'I', b'D', b'A', b'T', 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
        ];
        let img_used = insert_epub_image(
            &epub_path,
            reloaded.chapters[0].source_href.as_deref(),
            png_1x1,
            Some("image/png"),
        )
        .unwrap();
        let img_orphan = insert_epub_image(
            &epub_path,
            reloaded.chapters[0].source_href.as_deref(),
            png_1x1,
            Some("image/png"),
        )
        .unwrap();
        assert!(img_used.chapter_rel_href.contains("Images/ximg_"));

        let with_img_inputs = vec![SaveBookChapterInput {
            title: "第一卷 含插图章".into(),
            level: 0,
            mode: ChapterEditMode::Xhtml,
            plain_text: String::new(),
            xhtml_body: format!(
                "  <h2>第一卷 含插图章</h2>\n  <figure><img src=\"{}\" alt=\"插图\"/></figure>\n  <p>「軟體測試」</p>",
                img_used.chapter_rel_href
            ),
            source_href: reloaded.chapters[0].source_href.clone(),
        }];
        let custom_css = vec!["body { writing-mode: vertical-rl; font-size: 18px; }".to_string()];
        save_book_content(&epub_path, &with_img_inputs, Some(&custom_css), false).unwrap();

        // 6. Batch transform: T2S + vertical_to_horizontal
        let before_tf = read_book_content(&epub_path).unwrap();
        let tf_res = transform_book_content_batch(
            before_tf.chapters,
            before_tf.stylesheets,
            &BookTextTransformOptions {
                zh_direction: Some(crate::infra::inspectors::zh_text::ZhConvertDirection::T2s),
                vertical_to_horizontal: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(tf_res.changed_chapters, 1);
        assert!(tf_res.chapters[0].xhtml_body.contains("“软件测试”"));
        assert!(tf_res.stylesheets[0].contains("writing-mode: horizontal-tb"));

        // 7. Polish EPUB: generates xmedia_jacket.xhtml and cleans up unreferenced img_orphan!
        let polish_res = polish_epub_book(&epub_path).unwrap();
        assert!(polish_res.jacket_updated);
        assert_eq!(polish_res.removed_orphan_files, 1);
        assert!(read_epub_internal_asset(&epub_path, &img_used.asset_path).is_ok());
        assert!(read_epub_internal_asset(&epub_path, &img_orphan.asset_path).is_err());

        // Idempotent second polish removes 0 orphans and updates jacket in-place
        let polish_again = polish_epub_book(&epub_path).unwrap();
        assert_eq!(polish_again.removed_orphan_files, 0);
        let after_polish = read_book_content(&epub_path).unwrap();
        assert_eq!(after_polish.chapters[0].title, "书籍简介");
        assert!(
            after_polish.chapters[0]
                .plain_text
                .contains("地球往事三部曲")
        );

        // 8. Verify locked_fields persistence in OPF
        inspector
            .write_metadata(
                &epub_path,
                &MediaMetadataPatch::Book(BookMetaPatch {
                    locked_fields: Some(vec!["title".into(), "series".into(), "cover".into()]),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        let locked_meta = read_book_meta_any(&epub_path).unwrap();
        assert_eq!(locked_meta.locked_fields, vec!["title", "series", "cover"]);
    }

    #[test]
    fn polish_epub_preserves_percent_encoded_cjk_chapters_and_images() {
        let tmp = tempfile::tempdir().unwrap();
        let epub_path = tmp.path().join("cjk_encoded.epub");
        {
            let f = File::create(&epub_path).unwrap();
            let mut zw = ZipWriter::new(f);
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            let deflated =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            zw.start_file("META-INF/container.xml", deflated).unwrap();
            zw.write_all(
                br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
            )
            .unwrap();
            zw.start_file("OEBPS/content.opf", deflated).unwrap();
            zw.write_all(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">urn:uuid:test-cjk</dc:identifier>
    <dc:title>中文编码测试</dc:title>
    <dc:language>zh-CN</dc:language>
  </metadata>
  <manifest>
    <item id="ch1" href="Text/%E7%AC%AC%E4%B8%80%E7%AB%A0.xhtml" media-type="application/xhtml+xml"/>
    <item id="img1" href="Images/%E6%8F%92%E5%9B%BE%201.png" media-type="image/png"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>"#
                    .as_bytes(),
            )
            .unwrap();
            zw.start_file("OEBPS/Text/第一章.xhtml", deflated).unwrap();
            zw.write_all(
                r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>第一章</title></head><body><h1>第一章</h1><p><img src="../Images/插图 1.png" alt="图"/></p></body></html>"#
                    .as_bytes(),
            )
            .unwrap();
            zw.start_file("OEBPS/Images/插图 1.png", stored).unwrap();
            zw.write_all(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
                .unwrap();
            zw.finish().unwrap();
        }

        let report = polish_epub_book(&epub_path).unwrap();
        assert_eq!(report.removed_orphan_files, 0);
        assert!(read_epub_internal_asset(&epub_path, "OEBPS/Text/第一章.xhtml").is_ok());
        assert!(read_epub_internal_asset(&epub_path, "OEBPS/Images/插图 1.png").is_ok());
    }
}
