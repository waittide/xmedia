use std::collections::HashMap;

use quick_xml::Reader;
use quick_xml::events::Event;

use super::ComicReadingDirection;
use super::comicinfo::ComicInfoMeta;
use crate::common::{decode_text_smart, format_clean_float, xml_escape_clean};

#[derive(Default)]
struct PreservedComicInfoNodes {
    pages_xml: Option<String>,
    non_webtoon_format_xml: Option<String>,
    preserved_by_tag: HashMap<String, Vec<String>>,
    extra_elements: Vec<String>,
}

fn is_always_managed_comic_tag(tag_lower: &str) -> bool {
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

fn is_optional_extended_comic_tag(tag_lower: &str) -> bool {
    matches!(
        tag_lower,
        "inker"
            | "colorist"
            | "letterer"
            | "coverartist"
            | "characters"
            | "agerating"
            | "web"
            | "month"
            | "day"
            | "count"
    )
}

fn extract_preserved_comic_info_nodes(xml_bytes: &[u8]) -> PreservedComicInfoNodes {
    let (xml_str, _) = decode_text_smart(xml_bytes);
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
                        let formatted = format!("  {}\n", raw_slice.trim());
                        if name == "pages" {
                            out.pages_xml = Some(formatted);
                        } else if is_optional_extended_comic_tag(&name) {
                            out.preserved_by_tag
                                .entry(name)
                                .or_default()
                                .push(formatted);
                        } else if !is_always_managed_comic_tag(&name) {
                            out.extra_elements.push(formatted);
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
                        } else if is_optional_extended_comic_tag(&name) {
                            out.preserved_by_tag
                                .entry(name)
                                .or_default()
                                .push(formatted);
                        } else if !is_always_managed_comic_tag(&name) {
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

/// Renders `ComicInfo.xml` non-destructively, preserving `<Pages>` and all unknown/extended XML
/// elements from `existing_xml` when not explicitly populated on `meta`.
pub fn render_comic_info_xml_preserving(
    meta: &ComicInfoMeta,
    existing_xml: Option<&[u8]>,
    override_pages_xml: Option<&str>,
) -> String {
    let mut preserved = existing_xml
        .map(extract_preserved_comic_info_nodes)
        .unwrap_or_default();
    let mut xml = String::with_capacity(1024);
    xml.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    xml.push_str("<ComicInfo xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n");

    let push_opt = |buf: &mut String, tag: &str, val: Option<&str>| {
        if let Some(v) = val.map(str::trim).filter(|s| !s.is_empty()) {
            buf.push_str(&format!("  <{tag}>{}</{tag}>\n", xml_escape_clean(v)));
        }
    };
    let push_csv = |buf: &mut String, tag: &str, items: &[String]| {
        if !items.is_empty() {
            buf.push_str(&format!(
                "  <{tag}>{}</{tag}>\n",
                xml_escape_clean(&items.join(", "))
            ));
        }
    };

    push_opt(&mut xml, "Series", meta.series.as_deref());
    push_opt(&mut xml, "SeriesGroup", meta.series_group.as_deref());
    push_opt(&mut xml, "Title", meta.title.as_deref());

    let num_str = meta
        .number_raw
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            meta.number
                .map(format_clean_float)
                .filter(|s| !s.is_empty())
        });
    push_opt(&mut xml, "Number", num_str.as_deref());

    if let Some(c) = meta.count {
        xml.push_str(&format!("  <Count>{c}</Count>\n"));
        preserved.preserved_by_tag.remove("count");
    }
    if let Some(v) = meta.volume {
        let vol_str = format_clean_float(v);
        push_opt(&mut xml, "Volume", Some(&vol_str));
    }
    push_opt(&mut xml, "Summary", meta.summary.as_deref());
    push_csv(&mut xml, "Writer", &meta.writers);
    push_csv(&mut xml, "Penciller", &meta.pencillers);

    if !meta.inkers.is_empty() {
        push_csv(&mut xml, "Inker", &meta.inkers);
        preserved.preserved_by_tag.remove("inker");
    }
    if !meta.colorists.is_empty() {
        push_csv(&mut xml, "Colorist", &meta.colorists);
        preserved.preserved_by_tag.remove("colorist");
    }
    if !meta.letterers.is_empty() {
        push_csv(&mut xml, "Letterer", &meta.letterers);
        preserved.preserved_by_tag.remove("letterer");
    }
    if !meta.cover_artists.is_empty() {
        push_csv(&mut xml, "CoverArtist", &meta.cover_artists);
        preserved.preserved_by_tag.remove("coverartist");
    }

    push_opt(&mut xml, "Publisher", meta.publisher.as_deref());
    push_csv(&mut xml, "Genre", &meta.genres);
    push_csv(&mut xml, "Tags", &meta.tags);

    if !meta.characters.is_empty() {
        push_csv(&mut xml, "Characters", &meta.characters);
        preserved.preserved_by_tag.remove("characters");
    }
    if let Some(ref ar) = meta.age_rating
        && !ar.trim().is_empty()
    {
        push_opt(&mut xml, "AgeRating", Some(ar));
        preserved.preserved_by_tag.remove("agerating");
    }
    if let Some(ref w) = meta.web
        && !w.trim().is_empty()
    {
        push_opt(&mut xml, "Web", Some(w));
        preserved.preserved_by_tag.remove("web");
    }

    if let Some(y) = meta.year {
        xml.push_str(&format!("  <Year>{y}</Year>\n"));
    }
    if let Some(m) = meta.month {
        xml.push_str(&format!("  <Month>{m}</Month>\n"));
        preserved.preserved_by_tag.remove("month");
    }
    if let Some(d) = meta.day {
        xml.push_str(&format!("  <Day>{d}</Day>\n"));
        preserved.preserved_by_tag.remove("day");
    }

    push_opt(&mut xml, "LanguageISO", meta.language_iso.as_deref());
    if let Some(pc) = meta.page_count {
        xml.push_str(&format!("  <PageCount>{pc}</PageCount>\n"));
    }

    let dir = meta.reading_direction.unwrap_or(ComicReadingDirection::Rtl);
    let manga_tag = match dir {
        ComicReadingDirection::Ltr | ComicReadingDirection::Webtoon => "No",
        ComicReadingDirection::Rtl => "YesAndRightToLeft",
    };
    xml.push_str(&format!("  <Manga>{manga_tag}</Manga>\n"));
    if dir == ComicReadingDirection::Webtoon {
        xml.push_str("  <Format>Webtoon</Format>\n");
    } else if let Some(ref fmt_xml) = preserved.non_webtoon_format_xml {
        xml.push_str(fmt_xml);
    }

    for tag in [
        "count",
        "month",
        "day",
        "inker",
        "colorist",
        "letterer",
        "coverartist",
        "characters",
        "agerating",
        "web",
    ] {
        if let Some(blocks) = preserved.preserved_by_tag.get(tag) {
            for b in blocks {
                xml.push_str(b);
            }
        }
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
