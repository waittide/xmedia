use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::Event;
use zip::ZipArchive;

use crate::domain::media::{ComicFileMeta, format_comic_volume};
use crate::infra::inspectors::append_xml_ref;
use crate::infra::inspectors::book::attr_value;

pub(super) fn apply_epub_opf_fallback_into_comic(opf_bytes: &[u8], meta: &mut ComicFileMeta) {
    let xml_str = String::from_utf8_lossy(opf_bytes);
    let mut reader = Reader::from_str(&xml_str);
    reader.config_mut().trim_text(false);

    let mut cur_tag: Option<String> = None;
    let mut cur_meta_prop: Option<String> = None;
    let mut text_acc = String::new();
    let mut spine_dir: Option<String> = None;
    let mut rendition_flow: Option<String> = None;
    let mut opf_title: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                if local == "spine" {
                    spine_dir = attr_value(e, "page-progression-direction");
                } else if local == "meta" {
                    cur_meta_prop = attr_value(e, "property");
                    if attr_value(e, "name")
                        .is_some_and(|n| n.eq_ignore_ascii_case("calibre:series"))
                        && let Some(c) = attr_value(e, "content").filter(|s| !s.trim().is_empty())
                    {
                        meta.series = Some(c.trim().to_string());
                    }
                }
                cur_tag = Some(local);
                text_acc.clear();
            }
            Ok(Event::Empty(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                if local == "spine" {
                    spine_dir = attr_value(e, "page-progression-direction");
                } else if local == "meta"
                    && attr_value(e, "name")
                        .is_some_and(|n| n.eq_ignore_ascii_case("calibre:series"))
                    && let Some(c) = attr_value(e, "content").filter(|s| !s.trim().is_empty())
                {
                    meta.series = Some(c.trim().to_string());
                }
            }
            Ok(Event::Text(t)) if cur_tag.is_some() => {
                text_acc.push_str(&t.xml10_content());
            }
            Ok(Event::GeneralRef(r)) if cur_tag.is_some() => {
                append_xml_ref(&mut text_acc, &r);
            }
            Ok(Event::End(_)) => {
                if let Some(tag) = cur_tag.take() {
                    let trimmed = text_acc.trim();
                    if !trimmed.is_empty() {
                        match tag.as_str() {
                            "title" if opf_title.is_none() => {
                                opf_title = Some(trimmed.to_string());
                            }
                            "creator" if !meta.writers.iter().any(|w| w == trimmed) => {
                                meta.writers.push(trimmed.to_string());
                            }
                            "publisher" if meta.publisher.is_none() => {
                                meta.publisher = Some(trimmed.to_string());
                            }
                            "description" if meta.summary.is_none() => {
                                meta.summary = Some(trimmed.to_string());
                            }
                            "language" if meta.language_iso.is_none() => {
                                meta.language_iso = Some(trimmed.to_string());
                            }
                            "subject" if !meta.genres.iter().any(|g| g == trimmed) => {
                                meta.genres.push(trimmed.to_string());
                            }
                            "date" if meta.year.is_none() => {
                                meta.year = trimmed.get(0..4).and_then(|y| y.parse::<i32>().ok());
                            }
                            "meta" => {
                                if let Some(prop) = cur_meta_prop.as_deref() {
                                    let pl = prop.trim().to_ascii_lowercase();
                                    if pl == "belongs-to-collection" && meta.series.is_none() {
                                        meta.series = Some(trimmed.to_string());
                                    } else if pl == "rendition:flow" {
                                        rendition_flow = Some(trimmed.to_ascii_lowercase());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                cur_meta_prop = None;
                text_acc.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    if let Some(t) = opf_title {
        if meta.series.is_none() {
            meta.series = Some(t.clone());
        }
        meta.title = Some(t);
    }

    if rendition_flow
        .as_deref()
        .is_some_and(|f| f.contains("scrolled"))
    {
        meta.reading_direction = "webtoon".to_string();
    } else if let Some(dir) = spine_dir {
        if dir.eq_ignore_ascii_case("ltr") {
            meta.reading_direction = "ltr".to_string();
        } else if dir.eq_ignore_ascii_case("rtl") {
            meta.reading_direction = "rtl".to_string();
        }
    } else if meta.language_iso.as_deref().is_some_and(|l| {
        let low = l.trim().to_ascii_lowercase();
        matches!(low.get(0..2), Some("en" | "fr" | "es" | "de"))
    }) {
        meta.reading_direction = "ltr".to_string();
    }
}

fn split_csv(raw: &str) -> Vec<String> {
    raw.split([',', ';', '、'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(super) fn parse_comic_info_into(xml_bytes: &[u8], meta: &mut ComicFileMeta) {
    let xml_str = String::from_utf8_lossy(xml_bytes);
    let mut reader = Reader::from_str(&xml_str);
    reader.config_mut().trim_text(false);

    let mut cur_tag: Option<String> = None;
    let mut text_acc = String::new();
    let mut manga_raw: Option<String> = None;
    let mut format_raw: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                cur_tag = Some(e.local_name().as_ref().to_ascii_lowercase());
                text_acc.clear();
            }
            Ok(Event::Text(t)) => {
                if cur_tag.is_some() {
                    text_acc.push_str(&t.xml10_content());
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if cur_tag.is_some() {
                    append_xml_ref(&mut text_acc, &r);
                }
            }
            Ok(Event::CData(c)) => {
                if cur_tag.is_some() {
                    text_acc.push_str(c.as_ref());
                }
            }
            Ok(Event::End(_)) => {
                if let Some(tag) = cur_tag.take() {
                    let trimmed = text_acc.trim();
                    if !trimmed.is_empty() {
                        match tag.as_str() {
                            "series" => meta.series = Some(trimmed.to_string()),
                            "seriesgroup" => meta.series_group = Some(trimmed.to_string()),
                            "title" => meta.title = Some(trimmed.to_string()),
                            "number" => meta.number = Some(trimmed.to_string()),
                            "volume" => {
                                meta.volume = trimmed
                                    .parse::<f64>()
                                    .ok()
                                    .filter(|v| v.is_finite() && *v >= 0.0)
                            }
                            "summary" => meta.summary = Some(trimmed.to_string()),
                            "writer" => meta.writers = split_csv(trimmed),
                            "penciller" | "artist" => {
                                for p in split_csv(trimmed) {
                                    if !meta.pencillers.contains(&p) {
                                        meta.pencillers.push(p);
                                    }
                                }
                            }
                            "publisher" => meta.publisher = Some(trimmed.to_string()),
                            "genre" => meta.genres = split_csv(trimmed),
                            "tags" => meta.tags = split_csv(trimmed),
                            "year" => meta.year = trimmed.parse::<i32>().ok(),
                            "languageiso" => meta.language_iso = Some(trimmed.to_string()),
                            "manga" => manga_raw = Some(trimmed.to_ascii_lowercase()),
                            "format" => format_raw = Some(trimmed.to_ascii_lowercase()),
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

    let is_webtoon_format = format_raw
        .as_deref()
        .is_some_and(|f| f.contains("webtoon") || f.contains("条漫"));
    let is_webtoon_tag = meta.genres.iter().chain(meta.tags.iter()).any(|t| {
        let l = t.to_ascii_lowercase();
        l == "webtoon" || l == "条漫"
    });

    if is_webtoon_format {
        meta.reading_direction = "webtoon".to_string();
    } else if let Some(m) = manga_raw {
        if m == "yesandrighttoleft" || m == "yes" || m == "true" {
            meta.reading_direction = "rtl".to_string();
        } else if m == "no" || m == "false" {
            meta.reading_direction = if is_webtoon_tag {
                "webtoon".to_string()
            } else {
                "ltr".to_string()
            };
        }
    } else if is_webtoon_tag {
        meta.reading_direction = "webtoon".to_string();
    }
}

#[derive(Default)]
struct PreservedComicInfoNodes {
    pages_xml: Option<String>,
    non_webtoon_format_xml: Option<String>,
    extra_elements: Vec<String>,
}

fn is_managed_comic_info_tag(tag_lower: &str) -> bool {
    matches!(
        tag_lower,
        "series"
            | "seriesgroup"
            | "title"
            | "number"
            | "volume"
            | "summary"
            | "writer"
            | "penciller"
            | "artist"
            | "publisher"
            | "genre"
            | "tags"
            | "year"
            | "languageiso"
            | "pagecount"
            | "manga"
            | "format"
            | "pages"
    )
}

fn extract_preserved_comic_info_nodes(xml_bytes: &[u8]) -> PreservedComicInfoNodes {
    let xml_str = String::from_utf8_lossy(xml_bytes);
    let mut reader = Reader::from_str(&xml_str);
    reader.config_mut().trim_text(false);

    let mut out = PreservedComicInfoNodes::default();
    let mut depth = 0usize;
    let mut top_elem_start: Option<(String, usize)> = None;
    let mut top_text = String::new();

    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = e.local_name().as_ref().to_ascii_lowercase();
                if depth == 1 {
                    top_elem_start = Some((name, pos_before));
                    top_text.clear();
                }
                depth += 1;
            }
            Ok(Event::Empty(ref e)) => {
                if depth == 1 {
                    let pos_after = reader.buffer_position() as usize;
                    let name = e.local_name().as_ref().to_ascii_lowercase();
                    if let Some(raw_slice) = xml_str.get(pos_before..pos_after) {
                        if name == "pages" {
                            out.pages_xml = Some(format!("  {}\n", raw_slice.trim()));
                        } else if !is_managed_comic_info_tag(&name) {
                            out.extra_elements.push(format!("  {}\n", raw_slice.trim()));
                        }
                    }
                }
            }
            Ok(Event::Text(ref t)) if depth == 2 && top_elem_start.is_some() => {
                top_text.push_str(&t.xml10_content());
            }
            Ok(Event::End(_)) => {
                depth = depth.saturating_sub(1);
                if depth == 1
                    && let Some((name, start_idx)) = top_elem_start.take()
                {
                    let pos_after = reader.buffer_position() as usize;
                    if let Some(raw_slice) = xml_str.get(start_idx..pos_after) {
                        let formatted = format!("  {}\n", raw_slice.trim());
                        if name == "pages" {
                            out.pages_xml = Some(formatted);
                        } else if name == "format" {
                            let val_low = top_text.trim().to_ascii_lowercase();
                            if !val_low.is_empty()
                                && !val_low.contains("webtoon")
                                && !top_text.contains("条漫")
                            {
                                out.non_webtoon_format_xml = Some(formatted);
                            }
                        } else if !is_managed_comic_info_tag(&name) {
                            out.extra_elements.push(formatted);
                        }
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    out
}

pub(crate) fn extract_existing_comic_info_bytes(path: &Path) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    let mut archive = ZipArchive::new(BufReader::new(file)).ok()?;
    for idx in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(idx) else {
            continue;
        };
        let Ok(name_raw) = entry.name() else {
            continue;
        };
        let lower = name_raw.to_ascii_lowercase();
        if lower == "comicinfo.xml" || lower.ends_with("/comicinfo.xml") {
            let mut buf = Vec::new();
            if entry.read_to_end(&mut buf).is_ok() {
                return Some(buf);
            }
        }
    }
    None
}

pub(crate) fn render_comic_info_xml_with_pages(
    meta: &ComicFileMeta,
    pages_xml: Option<&str>,
) -> String {
    render_comic_info_xml_preserving(meta, None, pages_xml)
}

pub(crate) fn render_comic_info_xml_preserving(
    meta: &ComicFileMeta,
    existing_xml: Option<&[u8]>,
    override_pages_xml: Option<&str>,
) -> String {
    let preserved = existing_xml
        .map(extract_preserved_comic_info_nodes)
        .unwrap_or_default();
    let mut xml = String::with_capacity(1024);
    xml.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    xml.push_str("<ComicInfo xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n");

    if let Some(ref s) = meta.series
        && !s.trim().is_empty()
    {
        xml.push_str(&format!("  <Series>{}</Series>\n", escape(s.trim())));
    }
    if let Some(ref sg) = meta.series_group
        && !sg.trim().is_empty()
    {
        xml.push_str(&format!(
            "  <SeriesGroup>{}</SeriesGroup>\n",
            escape(sg.trim())
        ));
    }
    if let Some(ref t) = meta.title
        && !t.trim().is_empty()
    {
        xml.push_str(&format!("  <Title>{}</Title>\n", escape(t.trim())));
    }
    if let Some(ref n) = meta.number
        && !n.trim().is_empty()
    {
        xml.push_str(&format!("  <Number>{}</Number>\n", escape(n.trim())));
    }
    if let Some(v) = meta.volume {
        let vol_str = format_comic_volume(v);
        if !vol_str.is_empty() {
            xml.push_str(&format!("  <Volume>{}</Volume>\n", escape(&vol_str)));
        }
    }
    if let Some(ref sum) = meta.summary
        && !sum.trim().is_empty()
    {
        xml.push_str(&format!("  <Summary>{}</Summary>\n", escape(sum.trim())));
    }
    if !meta.writers.is_empty() {
        xml.push_str(&format!(
            "  <Writer>{}</Writer>\n",
            escape(meta.writers.join(", "))
        ));
    }
    if !meta.pencillers.is_empty() {
        xml.push_str(&format!(
            "  <Penciller>{}</Penciller>\n",
            escape(meta.pencillers.join(", "))
        ));
    }
    if let Some(ref pub_name) = meta.publisher
        && !pub_name.trim().is_empty()
    {
        xml.push_str(&format!(
            "  <Publisher>{}</Publisher>\n",
            escape(pub_name.trim())
        ));
    }
    if !meta.genres.is_empty() {
        xml.push_str(&format!(
            "  <Genre>{}</Genre>\n",
            escape(meta.genres.join(", "))
        ));
    }
    if !meta.tags.is_empty() {
        xml.push_str(&format!(
            "  <Tags>{}</Tags>\n",
            escape(meta.tags.join(", "))
        ));
    }
    if let Some(y) = meta.year {
        xml.push_str(&format!("  <Year>{y}</Year>\n"));
    }
    if let Some(ref lang) = meta.language_iso
        && !lang.trim().is_empty()
    {
        xml.push_str(&format!(
            "  <LanguageISO>{}</LanguageISO>\n",
            escape(lang.trim())
        ));
    }
    xml.push_str(&format!("  <PageCount>{}</PageCount>\n", meta.page_count));
    let manga_tag = match meta.reading_direction.as_str() {
        "ltr" => "No",
        "webtoon" => "No",
        _ => "YesAndRightToLeft",
    };
    xml.push_str(&format!("  <Manga>{manga_tag}</Manga>\n"));
    if meta.reading_direction == "webtoon" {
        xml.push_str("  <Format>Webtoon</Format>\n");
    } else if let Some(ref fmt_xml) = preserved.non_webtoon_format_xml {
        xml.push_str(fmt_xml);
    }
    for extra in &preserved.extra_elements {
        xml.push_str(extra);
    }
    let effective_pages = override_pages_xml.or(preserved.pages_xml.as_deref());
    if let Some(pages_block) = effective_pages
        && !pages_block.trim().is_empty()
    {
        xml.push_str(pages_block);
        if !pages_block.ends_with('\n') {
            xml.push('\n');
        }
    }
    xml.push_str("</ComicInfo>\n");
    xml
}
