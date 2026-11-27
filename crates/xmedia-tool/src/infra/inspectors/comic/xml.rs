use xmedia_formats::book::BookFormatExt;
use xmedia_formats::book::opf::parse_opf_xml;
use xmedia_formats::comic::ComicReadingDirection;
pub(crate) use xmedia_formats::comic::comicinfo::extract_existing_comic_info_bytes;
use xmedia_formats::comic::comicinfo::{
    ComicInfoMeta, parse_comic_info_xml,
    render_comic_info_xml_preserving as fmt_render_comic_info_xml_preserving,
};

use crate::domain::media::ComicFileMeta;

pub(super) fn apply_epub_opf_fallback_into_comic(opf_bytes: &[u8], meta: &mut ComicFileMeta) {
    let doc = parse_opf_xml(opf_bytes, BookFormatExt::Epub, "");
    let opf_title = (!doc.meta.title.trim().is_empty() && doc.meta.title != "Untitled")
        .then_some(doc.meta.title);

    if let Some(series) = doc.meta.series.filter(|s| !s.trim().is_empty()) {
        meta.series = Some(series);
    }
    for creator in doc.meta.authors {
        if !meta.writers.contains(&creator) {
            meta.writers.push(creator);
        }
    }
    if meta.publisher.is_none() {
        meta.publisher = doc.meta.publisher;
    }
    if meta.summary.is_none() {
        meta.summary = doc.meta.description;
    }
    if meta.language_iso.is_none() {
        meta.language_iso = doc.meta.language;
    }
    for subject in doc.meta.tags {
        if !meta.genres.contains(&subject) {
            meta.genres.push(subject);
        }
    }
    if meta.year.is_none() {
        meta.year = doc
            .meta
            .published_at
            .as_deref()
            .and_then(|d| d.get(..4))
            .and_then(|y| y.parse::<i32>().ok());
    }
    if let Some(t) = opf_title {
        if meta.series.is_none() {
            meta.series = Some(t.clone());
        }
        meta.title = Some(t);
    }

    if doc
        .rendition_flow
        .as_deref()
        .is_some_and(|f| f.contains("scrolled"))
    {
        meta.reading_direction = "webtoon".to_string();
    } else if let Some(dir) = doc.spine_progression_direction {
        if dir.eq_ignore_ascii_case("ltr") {
            meta.reading_direction = "ltr".to_string();
        } else if dir.eq_ignore_ascii_case("rtl") {
            meta.reading_direction = "rtl".to_string();
        }
    } else if meta.language_iso.as_deref().is_some_and(|l| {
        let low = l.trim().to_ascii_lowercase();
        matches!(low.get(..2), Some("en" | "fr" | "es" | "de"))
    }) {
        meta.reading_direction = "ltr".to_string();
    }
}

pub(super) fn parse_comic_info_into(xml_bytes: &[u8], meta: &mut ComicFileMeta) {
    let Some(parsed) = parse_comic_info_xml(xml_bytes) else {
        return;
    };
    if parsed.series.is_some() {
        meta.series = parsed.series;
    }
    if parsed.series_group.is_some() {
        meta.series_group = parsed.series_group;
    }
    if parsed.title.is_some() {
        meta.title = parsed.title;
    }
    if let Some(num) = parsed
        .number_raw
        .or_else(|| parsed.number.map(|n| n.to_string()))
    {
        meta.number = Some(num);
    }
    if parsed.volume.is_some() {
        meta.volume = parsed.volume;
    }
    if parsed.summary.is_some() {
        meta.summary = parsed.summary;
    }
    if !parsed.writers.is_empty() {
        meta.writers = parsed.writers;
    }
    if !parsed.pencillers.is_empty() {
        for p in parsed.pencillers {
            if !meta.pencillers.contains(&p) {
                meta.pencillers.push(p);
            }
        }
    }
    if parsed.publisher.is_some() {
        meta.publisher = parsed.publisher;
    }
    if !parsed.genres.is_empty() {
        meta.genres = parsed.genres;
    }
    if !parsed.tags.is_empty() {
        meta.tags = parsed.tags;
    }
    if parsed.year.is_some() {
        meta.year = parsed.year;
    }
    if parsed.language_iso.is_some() {
        meta.language_iso = parsed.language_iso;
    }
    if let Some(dir) = parsed.reading_direction {
        meta.reading_direction = dir.to_string();
    }
}

fn file_meta_to_comic_info(meta: &ComicFileMeta) -> ComicInfoMeta {
    let number_raw = meta
        .number
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let number = number_raw
        .as_deref()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0);
    let reading_direction = ComicReadingDirection::parse(&meta.reading_direction);
    ComicInfoMeta {
        series: meta.series.clone(),
        series_group: meta.series_group.clone(),
        title: meta.title.clone(),
        number,
        number_raw,
        volume: meta.volume,
        summary: meta.summary.clone(),
        writers: meta.writers.clone(),
        pencillers: meta.pencillers.clone(),
        publisher: meta.publisher.clone(),
        genres: meta.genres.clone(),
        tags: meta.tags.clone(),
        year: meta.year,
        language_iso: meta.language_iso.clone(),
        page_count: (meta.page_count > 0).then_some(meta.page_count),
        reading_direction,
        ..Default::default()
    }
}

pub(crate) fn render_comic_info_xml_preserving(
    meta: &ComicFileMeta,
    existing_xml: Option<&[u8]>,
    override_pages_xml: Option<&str>,
) -> String {
    let info = file_meta_to_comic_info(meta);
    fmt_render_comic_info_xml_preserving(&info, existing_xml, override_pages_xml)
}

pub(crate) fn render_comic_info_xml_with_pages(
    meta: &ComicFileMeta,
    pages_xml_block: Option<&str>,
) -> String {
    render_comic_info_xml_preserving(meta, None, pages_xml_block)
}
