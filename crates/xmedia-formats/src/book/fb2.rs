use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use super::{BookFormatExt, BookTextChapter, BookTocEntry, ParsedBookMeta, ParsedBookPackage};
use crate::common::{
    BinaryAsset, append_xml_ref, decode_text_smart, sniff_raster_image_mime, xml_attr_value_ci,
};
use crate::error::{FormatError, FormatResult};

struct Fb2ParsedDoc {
    meta: ParsedBookMeta,
    cover: Option<BinaryAsset>,
    chapters: Vec<BookTextChapter>,
    toc: Vec<BookTocEntry>,
}

fn find_href_attr(e: &BytesStart<'_>) -> Option<String> {
    xml_attr_value_ci(e, "href")
}

fn decode_fb2_xml(bytes: &[u8]) -> FormatResult<String> {
    let (decoded, _) = decode_text_smart(bytes);
    if !decoded.contains("<FictionBook") && !decoded.to_ascii_lowercase().contains("<fictionbook") {
        return Err(FormatError::Validation(
            "invalid FB2 file: missing <FictionBook> root element".into(),
        ));
    }
    Ok(decoded)
}

fn parse_fb2_internal(bytes: &[u8], fallback_title: &str) -> FormatResult<Fb2ParsedDoc> {
    let xml_str = decode_fb2_xml(bytes)?;
    let mut reader = Reader::from_str(&xml_str);
    reader.config_mut().trim_text(false);

    let mut meta = ParsedBookMeta::with_fallback(BookFormatExt::Fb2, fallback_title);
    let mut stack: Vec<String> = Vec::new();
    let mut text_acc = String::new();

    let mut in_title_info = false;
    let mut in_publish_info = false;
    let mut in_author = false;
    let mut author_parts: [String; 4] = Default::default();
    let mut annotation_lines: Vec<String> = Vec::new();
    let mut in_annotation = false;
    let mut cover_href_id: Option<String> = None;

    let mut current_binary_id: Option<String> = None;
    let mut binary_acc = String::new();
    let mut cover_bytes: Option<Vec<u8>> = None;

    let mut in_primary_body = false;
    let mut body_count = 0usize;
    let mut section_depth = 0usize;
    let mut in_section_title = false;
    let mut sec_titles: Vec<String> = Vec::new();
    let mut sec_paras: Vec<Vec<String>> = Vec::new();
    let mut chapters: Vec<BookTextChapter> = Vec::new();

    let flush_section_if_non_empty =
        |title: String, paras: Vec<String>, level: u8, out: &mut Vec<BookTextChapter>| {
            let clean_title = title.trim().to_string();
            let content = paras
                .into_iter()
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            if clean_title.is_empty() && content.is_empty() {
                return;
            }
            let idx = u32::try_from(out.len()).unwrap_or(0);
            let effective_title = if clean_title.is_empty() {
                format!("Section {}", idx + 1)
            } else {
                clean_title
            };
            out.push(BookTextChapter {
                index: idx,
                volume_title: None,
                title: effective_title,
                content,
                level,
            });
        };

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                match local.as_str() {
                    "title-info" => in_title_info = true,
                    "publish-info" => in_publish_info = true,
                    "author" if in_title_info => {
                        in_author = true;
                        author_parts = Default::default();
                    }
                    "annotation" if in_title_info => {
                        in_annotation = true;
                        annotation_lines.clear();
                    }
                    "sequence" if in_title_info && meta.series.is_none() => {
                        if let Some(name) = xml_attr_value_ci(e, "name")
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                        {
                            meta.series = Some(name);
                            meta.series_index = xml_attr_value_ci(e, "number")
                                .and_then(|n| n.trim().parse::<f64>().ok())
                                .filter(|v| v.is_finite() && *v >= 0.0);
                        }
                    }
                    "image"
                        if in_title_info
                            && cover_href_id.is_none()
                            && stack.iter().any(|s| s == "coverpage") =>
                    {
                        if let Some(href) = find_href_attr(e) {
                            let id = href.trim().trim_start_matches('#').to_string();
                            if !id.is_empty() {
                                cover_href_id = Some(id);
                            }
                        }
                    }
                    "body" => {
                        body_count += 1;
                        let is_notes = xml_attr_value_ci(e, "name")
                            .is_some_and(|n| n.eq_ignore_ascii_case("notes"));
                        in_primary_body = body_count == 1 && !is_notes;
                    }
                    "section" if in_primary_body => {
                        if section_depth > 0
                            && (!sec_titles.last().is_none_or(|t| t.trim().is_empty())
                                || !sec_paras.last().is_none_or(Vec::is_empty))
                        {
                            let parent_title =
                                std::mem::take(sec_titles.last_mut().expect("section title"));
                            let parent_paras =
                                std::mem::take(sec_paras.last_mut().expect("section paras"));
                            let lvl = u8::try_from(section_depth.saturating_sub(1)).unwrap_or(0);
                            flush_section_if_non_empty(
                                parent_title,
                                parent_paras,
                                lvl,
                                &mut chapters,
                            );
                        }
                        section_depth += 1;
                        sec_titles.push(String::new());
                        sec_paras.push(Vec::new());
                    }
                    "title" if in_primary_body && section_depth > 0 => {
                        in_section_title = true;
                    }
                    "binary" => {
                        let bin_id = xml_attr_value_ci(e, "id").unwrap_or_default();
                        if cover_bytes.is_none()
                            && (cover_href_id.as_deref() == Some(bin_id.as_str())
                                || (cover_href_id.is_none()
                                    && bin_id.to_ascii_lowercase().contains("cover")))
                        {
                            current_binary_id = Some(bin_id);
                            binary_acc.clear();
                        }
                    }
                    _ => {}
                }
                stack.push(local);
                text_acc.clear();
            }
            Ok(Event::Empty(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                if in_title_info && local == "sequence" && meta.series.is_none() {
                    if let Some(name) = xml_attr_value_ci(e, "name")
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                    {
                        meta.series = Some(name);
                        meta.series_index = xml_attr_value_ci(e, "number")
                            .and_then(|n| n.trim().parse::<f64>().ok())
                            .filter(|v| v.is_finite() && *v >= 0.0);
                    }
                } else if in_title_info
                    && local == "image"
                    && cover_href_id.is_none()
                    && stack.iter().any(|s| s == "coverpage")
                    && let Some(href) = find_href_attr(e)
                {
                    let id = href.trim().trim_start_matches('#').to_string();
                    if !id.is_empty() {
                        cover_href_id = Some(id);
                    }
                }
            }
            Ok(Event::Text(t)) => {
                let content = t.xml10_content();
                if current_binary_id.is_some() {
                    binary_acc.push_str(&content);
                } else {
                    text_acc.push_str(&content);
                }
            }
            Ok(Event::GeneralRef(r)) if current_binary_id.is_none() => {
                append_xml_ref(&mut text_acc, &r);
            }
            Ok(Event::CData(c)) if current_binary_id.is_none() => {
                text_acc.push_str(c.as_ref());
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                let trimmed = text_acc.trim();

                if in_title_info {
                    if in_author {
                        match local.as_str() {
                            "first-name" => author_parts[0] = trimmed.to_string(),
                            "middle-name" => author_parts[1] = trimmed.to_string(),
                            "last-name" => author_parts[2] = trimmed.to_string(),
                            "nickname" => author_parts[3] = trimmed.to_string(),
                            "author" => {
                                in_author = false;
                                let joined = author_parts[..3]
                                    .iter()
                                    .map(|s| s.trim())
                                    .filter(|s| !s.is_empty())
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                let full = if !joined.is_empty() {
                                    joined
                                } else {
                                    author_parts[3].trim().to_string()
                                };
                                if !full.is_empty() && !meta.authors.contains(&full) {
                                    meta.authors.push(full);
                                }
                            }
                            _ => {}
                        }
                    } else if in_annotation {
                        if matches!(local.as_str(), "p" | "subtitle" | "v") && !trimmed.is_empty() {
                            annotation_lines.push(trimmed.to_string());
                        } else if local == "annotation" {
                            in_annotation = false;
                            if !annotation_lines.is_empty() {
                                meta.description = Some(annotation_lines.join("\n"));
                            } else if !trimmed.is_empty() {
                                meta.description = Some(trimmed.to_string());
                            }
                        }
                    } else if !trimmed.is_empty() {
                        match local.as_str() {
                            "book-title" => meta.title = trimmed.to_string(),
                            "genre"
                                if !meta.tags.iter().any(|g| g.eq_ignore_ascii_case(trimmed)) =>
                            {
                                meta.tags.push(trimmed.to_string());
                            }
                            "date" if meta.published_at.is_none() => {
                                meta.published_at = Some(trimmed.to_string());
                            }
                            "lang" if meta.language.is_none() => {
                                meta.language = Some(trimmed.to_string());
                            }
                            _ => {}
                        }
                    }
                    if local == "title-info" {
                        in_title_info = false;
                    }
                } else if in_publish_info {
                    if !trimmed.is_empty() {
                        match local.as_str() {
                            "publisher" if meta.publisher.is_none() => {
                                meta.publisher = Some(trimmed.to_string());
                            }
                            "isbn" if meta.isbn.is_none() => {
                                meta.isbn = Some(trimmed.to_string());
                            }
                            "year" if meta.published_at.is_none() => {
                                meta.published_at = Some(trimmed.to_string());
                            }
                            _ => {}
                        }
                    }
                    if local == "publish-info" {
                        in_publish_info = false;
                    }
                } else if in_primary_body {
                    match local.as_str() {
                        "title" if in_section_title => {
                            in_section_title = false;
                            if !trimmed.is_empty()
                                && let Some(t) = sec_titles.last_mut()
                                && t.is_empty()
                            {
                                *t = trimmed.to_string();
                            }
                        }
                        "p" | "subtitle" | "v" if section_depth > 0 && !trimmed.is_empty() => {
                            if in_section_title {
                                if let Some(t) = sec_titles.last_mut() {
                                    if !t.is_empty() {
                                        t.push_str(" · ");
                                    }
                                    t.push_str(trimmed);
                                }
                            } else if let Some(paras) = sec_paras.last_mut() {
                                paras.push(trimmed.to_string());
                            }
                        }
                        "section" if section_depth > 0 => {
                            let title = sec_titles.pop().unwrap_or_default();
                            let paras = sec_paras.pop().unwrap_or_default();
                            section_depth -= 1;
                            let lvl = u8::try_from(section_depth).unwrap_or(0).min(3);
                            flush_section_if_non_empty(title, paras, lvl, &mut chapters);
                        }
                        "body" => in_primary_body = false,
                        _ => {}
                    }
                } else if local == "binary" && current_binary_id.take().is_some() {
                    let compact: String =
                        binary_acc.chars().filter(|c| !c.is_whitespace()).collect();
                    if let Ok(decoded_bin) = BASE64_STANDARD.decode(compact.as_bytes()) {
                        cover_bytes = Some(decoded_bin);
                    }
                    binary_acc.clear();
                }

                stack.pop();
                text_acc.clear();
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    if chapters.is_empty() {
        chapters.push(BookTextChapter {
            index: 0,
            volume_title: None,
            title: meta.title.clone(),
            content: String::new(),
            level: 0,
        });
    }

    let toc: Vec<BookTocEntry> = chapters
        .iter()
        .map(|ch| BookTocEntry {
            index: ch.index,
            title: ch.title.clone(),
            href: format!("chapter:{}", ch.index),
            level: ch.level,
        })
        .collect();

    let cover = cover_bytes.and_then(|bytes| {
        let mime = sniff_raster_image_mime(&bytes)?;
        Some(BinaryAsset::new(bytes, mime))
    });
    meta.has_cover = cover.is_some();
    meta.chapter_count = u32::try_from(chapters.len()).ok();

    Ok(Fb2ParsedDoc {
        meta,
        cover,
        chapters,
        toc,
    })
}

pub fn parse_fb2_bytes(bytes: &[u8], fallback_title: &str) -> FormatResult<ParsedBookPackage> {
    let parsed = parse_fb2_internal(bytes, fallback_title)?;
    Ok(ParsedBookPackage {
        metadata: parsed.meta,
        cover: parsed.cover,
        toc: parsed.toc,
    })
}

pub fn list_fb2_toc_bytes(bytes: &[u8]) -> FormatResult<Vec<BookTocEntry>> {
    Ok(parse_fb2_internal(bytes, "Untitled")?.toc)
}

pub fn read_fb2_chapter_bytes(
    bytes: &[u8],
    chapter_index: u32,
) -> FormatResult<(BookTextChapter, u32)> {
    let parsed = parse_fb2_internal(bytes, "Untitled")?;
    let total = u32::try_from(parsed.chapters.len()).unwrap_or(0);
    let ch = parsed
        .chapters
        .into_iter()
        .nth(chapter_index as usize)
        .ok_or_else(|| FormatError::NotFound(format!("FB2 chapter {chapter_index} not found")))?;
    Ok((ch, total))
}
