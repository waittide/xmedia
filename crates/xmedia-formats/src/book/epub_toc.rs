use std::path::Path;

use quick_xml::Reader;
use quick_xml::escape::unescape;
use quick_xml::events::{BytesStart, Event};

use super::BookTocEntry;
use crate::common::{normalize_zip_path, xml_attr_value};

fn decode_xml_text(raw: &str) -> String {
    unescape(raw)
        .map(|cow| cow.into_owned())
        .unwrap_or_else(|_| raw.to_string())
}

pub fn parse_ncx_toc(ncx_xml: &[u8], ncx_dir: &Path) -> Vec<BookTocEntry> {
    let xml_str = String::from_utf8_lossy(ncx_xml);
    let mut reader = Reader::from_str(&xml_str);
    reader.config_mut().trim_text(true);
    let mut items = Vec::new();
    let mut depth: u8 = 0;
    let mut in_text = false;
    let mut current_title = String::new();
    let mut current_src: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => match e.local_name().as_ref() {
                "navPoint" => {
                    depth = depth.saturating_add(1);
                    current_title.clear();
                    current_src = None;
                }
                "text" if depth > 0 => in_text = true,
                "content" if depth > 0 => {
                    if let Some(src) = xml_attr_value(e, "src") {
                        current_src = Some(normalize_zip_path(ncx_dir, &src));
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(ref e)) if e.local_name().as_ref() == "content" && depth > 0 => {
                if let Some(src) = xml_attr_value(e, "src") {
                    let href = normalize_zip_path(ncx_dir, &src);
                    if !current_title.trim().is_empty() {
                        let idx = u32::try_from(items.len()).unwrap_or(0);
                        items.push(BookTocEntry {
                            index: idx,
                            title: current_title.trim().to_string(),
                            href,
                            level: depth.saturating_sub(1),
                        });
                        current_title.clear();
                    } else {
                        current_src = Some(href);
                    }
                }
            }
            Ok(Event::Text(ref t)) if in_text => {
                current_title.push_str(&decode_xml_text(t));
            }
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
                "text" | "content" => {
                    if e.local_name().as_ref() == "text" {
                        in_text = false;
                    }
                    if let Some(href) = current_src.take()
                        && !current_title.trim().is_empty()
                    {
                        let idx = u32::try_from(items.len()).unwrap_or(0);
                        items.push(BookTocEntry {
                            index: idx,
                            title: current_title.trim().to_string(),
                            href,
                            level: depth.saturating_sub(1),
                        });
                        current_title.clear();
                    }
                }
                "navPoint" => depth = depth.saturating_sub(1),
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    items
}

fn is_toc_nav(e: &BytesStart<'_>) -> bool {
    if let Some(t) = xml_attr_value(e, "type")
        && t.split_whitespace()
            .any(|tok| tok.eq_ignore_ascii_case("toc"))
    {
        return true;
    }
    if let Some(r) = xml_attr_value(e, "role")
        && r.split_whitespace()
            .any(|tok| tok.eq_ignore_ascii_case("doc-toc"))
    {
        return true;
    }
    false
}

pub fn parse_nav_xhtml_toc(nav_xml: &[u8], nav_dir: &Path) -> Vec<BookTocEntry> {
    let xml_str = String::from_utf8_lossy(nav_xml);
    let mut reader = Reader::from_str(&xml_str);
    reader.config_mut().trim_text(true);

    let mut preferred_toc: Vec<BookTocEntry> = Vec::new();
    let mut fallback_toc: Vec<BookTocEntry> = Vec::new();
    let mut current_items: Vec<BookTocEntry> = Vec::new();
    let mut pending_group_indices: Vec<usize> = Vec::new();
    let mut nav_depth: usize = 0;
    let mut current_nav_is_toc = false;
    let mut list_depth: u8 = 0;
    let mut active_href: Option<String> = None;
    let mut active_span_heading = false;
    let mut active_text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => match e.local_name().as_ref() {
                "nav" => {
                    if nav_depth == 0 {
                        current_nav_is_toc = is_toc_nav(e);
                        current_items.clear();
                        pending_group_indices.clear();
                        list_depth = 0;
                    }
                    nav_depth += 1;
                }
                "ol" | "ul" if nav_depth > 0 => list_depth = list_depth.saturating_add(1),
                "a" if nav_depth > 0 && list_depth > 0 => {
                    active_href =
                        xml_attr_value(e, "href").map(|h| normalize_zip_path(nav_dir, &h));
                    active_span_heading = false;
                    active_text.clear();
                }
                "span" if nav_depth > 0 && list_depth > 0 && active_href.is_none() => {
                    active_span_heading = true;
                    active_text.clear();
                }
                _ => {}
            },
            Ok(Event::Text(ref t)) if active_href.is_some() || active_span_heading => {
                active_text.push_str(&decode_xml_text(t));
            }
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
                "a" if nav_depth > 0 => {
                    if let Some(href) = active_href.take() {
                        let title = active_text.trim().to_string();
                        if !title.is_empty() {
                            for pending_idx in pending_group_indices.drain(..) {
                                current_items[pending_idx].href = href.clone();
                            }
                            let idx = u32::try_from(current_items.len()).unwrap_or(0);
                            current_items.push(BookTocEntry {
                                index: idx,
                                title,
                                href,
                                level: list_depth.saturating_sub(1),
                            });
                        }
                    }
                    active_text.clear();
                }
                "span" if nav_depth > 0 && active_span_heading => {
                    active_span_heading = false;
                    let title = active_text.trim().to_string();
                    if !title.is_empty() {
                        let pos = current_items.len();
                        current_items.push(BookTocEntry {
                            index: u32::try_from(pos).unwrap_or(0),
                            title,
                            href: String::new(),
                            level: list_depth.saturating_sub(1),
                        });
                        pending_group_indices.push(pos);
                    }
                    active_text.clear();
                }
                "ol" | "ul" if nav_depth > 0 => list_depth = list_depth.saturating_sub(1),
                "nav" if nav_depth > 0 => {
                    nav_depth -= 1;
                    if nav_depth == 0 {
                        current_items.retain(|i| !i.href.is_empty());
                        for (idx, item) in current_items.iter_mut().enumerate() {
                            item.index = u32::try_from(idx).unwrap_or(0);
                        }
                        if !current_items.is_empty() {
                            if current_nav_is_toc {
                                preferred_toc = std::mem::take(&mut current_items);
                                break;
                            } else if fallback_toc.is_empty() {
                                fallback_toc = std::mem::take(&mut current_items);
                            }
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    if !preferred_toc.is_empty() {
        preferred_toc
    } else {
        fallback_toc
    }
}
