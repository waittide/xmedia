use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde_json::Value;
use url::Url;
use uuid::Uuid;

use super::helpers::{
    detect_format_from_mime_or_href, proxy_image_path, proxy_pse_stream_path, resolve_link_url,
};
use super::{
    MAX_XML_DEPTH, OpdsDownloadLinkDto, OpdsEntryDto, OpdsError, OpdsFeedDto, OpdsNavLinkDto,
};

pub(super) fn parse_opds_feed(
    source_id: &str,
    base_url: &Url,
    content_type: &str,
    body: &str,
) -> Result<(OpdsFeedDto, bool), OpdsError> {
    let trimmed = body.trim_start();
    let lower_ct = content_type.to_ascii_lowercase();
    if lower_ct.contains("json") || trimmed.starts_with('{') {
        let feed = parse_opds2_json(source_id, base_url, trimmed)?;
        Ok((feed, false))
    } else if lower_ct.contains("xml") || trimmed.starts_with('<') {
        parse_opds1_atom(source_id, base_url, trimmed)
    } else {
        Err(OpdsError::Upstream(
            "Response is neither valid OPDS 1.2 Atom XML nor OPDS 2.0 JSON".to_string(),
        ))
    }
}

fn xml_attr(e: &BytesStart<'_>, local_key: &str) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.local_name().as_ref() == local_key
            && let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
        {
            return Some(val.into_owned());
        }
    }
    None
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

pub(super) fn parse_opensearch_template(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut fallback: Option<String> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e) | Event::Empty(ref e))
                if e.local_name().as_ref().eq_ignore_ascii_case("url") =>
            {
                if let Some(template) = xml_attr(e, "template") {
                    let mime = xml_attr(e, "type").unwrap_or_default().to_ascii_lowercase();
                    if mime.contains("atom") || mime.contains("opds") {
                        return Some(template);
                    }
                    if fallback.is_none() {
                        fallback = Some(template);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    fallback
}

fn is_disallowed_acquisition_rel(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    lower.contains("acquisition/buy")
        || lower.contains("acquisition/borrow")
        || lower.contains("acquisition/subscribe")
}

fn is_direct_acquisition_rel(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    (lower.starts_with("http://opds-spec.org/acquisition") || lower.contains("acquisition"))
        && !is_disallowed_acquisition_rel(&lower)
}

#[derive(Default)]
struct AtomEntryBuilder {
    id: String,
    title: String,
    authors: Vec<String>,
    summary: Option<String>,
    content: Option<String>,
    updated: Option<String>,
    cover_raw: Option<String>,
    thumbnail_raw: Option<String>,
    pse_stream_url: Option<String>,
    page_count: Option<u64>,
    downloads: Vec<OpdsDownloadLinkDto>,
    nav_href: Option<String>,
    nav_count: Option<u64>,
}

fn stack_matches(stack: &[String], expected: &[&str]) -> bool {
    stack.len() == expected.len() && stack.iter().zip(expected.iter()).all(|(a, b)| a == b)
}

fn parse_opds1_atom(
    source_id: &str,
    base_url: &Url,
    xml: &str,
) -> Result<(OpdsFeedDto, bool), OpdsError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut saw_feed = false;
    let mut feed_title = String::new();
    let mut search_url: Option<String> = None;
    let mut search_is_opensearch = false;
    let mut prev_url: Option<String> = None;
    let mut next_url: Option<String> = None;
    let mut nav_links: Vec<OpdsNavLinkDto> = Vec::new();
    let mut entries: Vec<OpdsEntryDto> = Vec::new();

    let mut tag_stack: Vec<String> = Vec::new();
    let mut capture_depth: Option<usize> = None;
    let mut text_buf = String::new();
    let mut cur_entry = AtomEntryBuilder::default();

    let handle_link = |e: &BytesStart<'_>,
                       in_entry: bool,
                       cur_entry: &mut AtomEntryBuilder,
                       search_url: &mut Option<String>,
                       search_is_opensearch: &mut bool,
                       prev_url: &mut Option<String>,
                       next_url: &mut Option<String>| {
        let Some(raw_href) = xml_attr(e, "href").filter(|h| !h.trim().is_empty()) else {
            return;
        };
        let abs_href = resolve_link_url(base_url, &raw_href);
        let rel = xml_attr(e, "rel").unwrap_or_default().to_ascii_lowercase();
        let media_type = xml_attr(e, "type").unwrap_or_default();
        let lower_type = media_type.to_ascii_lowercase();
        let title = xml_attr(e, "title").filter(|t| !t.trim().is_empty());
        let pse_count = xml_attr(e, "count").and_then(|c| c.trim().parse::<u64>().ok());

        if !in_entry {
            if rel == "next" && next_url.is_none() {
                *next_url = Some(abs_href);
            } else if (rel == "previous" || rel == "prev") && prev_url.is_none() {
                *prev_url = Some(abs_href);
            } else if rel == "search" && search_url.is_none() {
                *search_is_opensearch = lower_type.contains("opensearchdescription");
                *search_url = Some(abs_href);
            }
            return;
        }

        if is_disallowed_acquisition_rel(&rel) {
            return;
        }

        if is_direct_acquisition_rel(&rel) {
            let format = detect_format_from_mime_or_href(&media_type, &abs_href)
                .unwrap_or_else(|| "file".to_string());
            cur_entry.downloads.push(OpdsDownloadLinkDto {
                href: abs_href,
                title,
                format,
                media_type: if media_type.is_empty() {
                    "application/octet-stream".to_string()
                } else {
                    media_type
                },
            });
            return;
        }

        if rel == "http://vaemendis.net/opds-pse/stream" {
            if !abs_href.is_empty() {
                cur_entry.pse_stream_url = Some(proxy_pse_stream_path(source_id, &abs_href));
            }
            if let Some(count) = pse_count {
                cur_entry.page_count = Some(count);
            }
            return;
        }

        if rel == "http://opds-spec.org/image/thumbnail" || rel == "thumbnail" {
            if cur_entry.thumbnail_raw.is_none() {
                cur_entry.thumbnail_raw = Some(abs_href);
            }
            return;
        }

        if rel == "http://opds-spec.org/image"
            || rel == "cover"
            || rel == "http://opds-spec.org/cover"
        {
            if cur_entry.cover_raw.is_none() {
                cur_entry.cover_raw = Some(abs_href);
            }
            return;
        }

        if rel == "subsection"
            || rel == "http://opds-spec.org/sort/new"
            || rel == "http://opds-spec.org/sort/popular"
            || rel == "http://opds-spec.org/featured"
            || lower_type.contains("profile=opds-catalog")
            || lower_type.contains("application/atom+xml")
        {
            if cur_entry.nav_href.is_none() {
                cur_entry.nav_href = Some(abs_href);
            }
            if let Some(count) = pse_count {
                cur_entry.nav_count = Some(count);
            }
        }
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                if tag_stack.len() >= MAX_XML_DEPTH {
                    return Err(OpdsError::Upstream(
                        "OPDS XML nesting depth exceeds limit".to_string(),
                    ));
                }
                let tag = e.local_name().as_ref().to_ascii_lowercase();
                tag_stack.push(tag);

                if stack_matches(&tag_stack, &["feed"]) {
                    saw_feed = true;
                } else if stack_matches(&tag_stack, &["feed", "entry"]) {
                    cur_entry = AtomEntryBuilder::default();
                } else if stack_matches(&tag_stack, &["feed", "link"]) {
                    handle_link(
                        e,
                        false,
                        &mut cur_entry,
                        &mut search_url,
                        &mut search_is_opensearch,
                        &mut prev_url,
                        &mut next_url,
                    );
                } else if stack_matches(&tag_stack, &["feed", "entry", "link"]) {
                    handle_link(
                        e,
                        true,
                        &mut cur_entry,
                        &mut search_url,
                        &mut search_is_opensearch,
                        &mut prev_url,
                        &mut next_url,
                    );
                } else if capture_depth.is_none()
                    && (stack_matches(&tag_stack, &["feed", "title"])
                        || stack_matches(&tag_stack, &["feed", "entry", "id"])
                        || stack_matches(&tag_stack, &["feed", "entry", "title"])
                        || stack_matches(&tag_stack, &["feed", "entry", "summary"])
                        || stack_matches(&tag_stack, &["feed", "entry", "content"])
                        || stack_matches(&tag_stack, &["feed", "entry", "updated"])
                        || stack_matches(&tag_stack, &["feed", "entry", "author", "name"]))
                {
                    capture_depth = Some(tag_stack.len());
                    text_buf.clear();
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = e.local_name().as_ref().to_ascii_lowercase();
                if tag_stack.is_empty() && tag == "feed" {
                    saw_feed = true;
                } else if tag == "link" {
                    if stack_matches(&tag_stack, &["feed"]) {
                        handle_link(
                            e,
                            false,
                            &mut cur_entry,
                            &mut search_url,
                            &mut search_is_opensearch,
                            &mut prev_url,
                            &mut next_url,
                        );
                    } else if stack_matches(&tag_stack, &["feed", "entry"]) {
                        handle_link(
                            e,
                            true,
                            &mut cur_entry,
                            &mut search_url,
                            &mut search_is_opensearch,
                            &mut prev_url,
                            &mut next_url,
                        );
                    }
                }
            }
            Ok(Event::Text(ref t)) => {
                if capture_depth.is_some() {
                    text_buf.push_str(&t.xml_content(quick_xml::XmlVersion::Implicit1_0));
                }
            }
            Ok(Event::CData(ref c)) => {
                if capture_depth.is_some() {
                    text_buf.push_str(c.as_ref());
                }
            }
            Ok(Event::GeneralRef(ref r)) => {
                if capture_depth.is_some() {
                    append_xml_ref(&mut text_buf, r);
                }
            }
            Ok(Event::End(ref _e)) => {
                if capture_depth == Some(tag_stack.len()) {
                    let value = text_buf.trim().to_string();
                    if !value.is_empty() {
                        if stack_matches(&tag_stack, &["feed", "title"]) && feed_title.is_empty() {
                            feed_title = value;
                        } else if stack_matches(&tag_stack, &["feed", "entry", "author", "name"]) {
                            cur_entry.authors.push(value);
                        } else if stack_matches(&tag_stack, &["feed", "entry", "id"]) {
                            cur_entry.id = value;
                        } else if stack_matches(&tag_stack, &["feed", "entry", "title"]) {
                            cur_entry.title = value;
                        } else if stack_matches(&tag_stack, &["feed", "entry", "summary"]) {
                            cur_entry.summary = Some(value);
                        } else if stack_matches(&tag_stack, &["feed", "entry", "content"]) {
                            cur_entry.content = Some(value);
                        } else if stack_matches(&tag_stack, &["feed", "entry", "updated"]) {
                            cur_entry.updated = Some(value);
                        }
                    }
                    capture_depth = None;
                    text_buf.clear();
                } else if stack_matches(&tag_stack, &["feed", "entry"]) {
                    let finished = std::mem::take(&mut cur_entry);
                    if finished.downloads.is_empty()
                        && finished.pse_stream_url.is_none()
                        && let Some(href) = finished.nav_href
                    {
                        nav_links.push(OpdsNavLinkDto {
                            title: if finished.title.is_empty() {
                                href.clone()
                            } else {
                                finished.title
                            },
                            href,
                            count: finished.nav_count,
                        });
                    } else if !finished.title.is_empty()
                        || !finished.downloads.is_empty()
                        || finished.pse_stream_url.is_some()
                    {
                        let cover_url = finished
                            .thumbnail_raw
                            .or(finished.cover_raw)
                            .map(|raw| proxy_image_path(source_id, &raw));
                        entries.push(OpdsEntryDto {
                            id: if finished.id.is_empty() {
                                Uuid::new_v4().to_string()
                            } else {
                                finished.id
                            },
                            title: finished.title,
                            authors: finished.authors,
                            summary: finished.summary.or(finished.content),
                            cover_url,
                            updated: finished.updated,
                            pse_stream_url: finished.pse_stream_url,
                            page_count: finished.page_count,
                            downloads: finished.downloads,
                        });
                    }
                }
                tag_stack.pop();
            }
            Ok(Event::Eof) => break,
            Err(err) => {
                return Err(OpdsError::Upstream(format!(
                    "Failed to parse OPDS 1.2 Atom XML: {err}"
                )));
            }
            _ => {}
        }
    }

    if !saw_feed {
        return Err(OpdsError::Upstream(
            "XML document does not contain an Atom <feed> root element".to_string(),
        ));
    }

    Ok((
        OpdsFeedDto {
            title: if feed_title.is_empty() {
                "OPDS Catalog".to_string()
            } else {
                feed_title
            },
            url: base_url.to_string(),
            opds_version: "1.2".to_string(),
            search_url,
            prev_url,
            next_url,
            nav_links,
            entries,
        },
        search_is_opensearch,
    ))
}

fn link_has_rel(link: &Value, expected: &str) -> bool {
    match link.get("rel") {
        Some(Value::String(s)) => s.eq_ignore_ascii_case(expected),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(Value::as_str)
            .any(|s| s.eq_ignore_ascii_case(expected)),
        _ => false,
    }
}

fn link_rel_matches_any<F>(link: &Value, pred: F) -> bool
where
    F: Fn(&str) -> bool,
{
    match link.get("rel") {
        Some(Value::String(s)) => pred(s),
        Some(Value::Array(arr)) => arr.iter().filter_map(Value::as_str).any(pred),
        _ => false,
    }
}

/// Extracts a human-readable string from either a plain JSON string or a Readium/OPDS 2.0
/// localized string map (e.g. `{"en": "Title", "zh": "标题"}`).
fn extract_localized_string(val: &Value) -> Option<String> {
    match val {
        Value::String(s) => {
            let trimmed = s.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Object(map) => {
            for preferred in ["zh-CN", "zh", "en", "en-US", "und"] {
                if let Some(s) = map.get(preferred).and_then(Value::as_str) {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                if let Some(s) = map.get(key).and_then(Value::as_str) {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_opds2_authors(meta: &Value) -> Vec<String> {
    let mut out = Vec::new();
    match meta.get("author") {
        Some(Value::String(s)) if !s.trim().is_empty() => out.push(s.trim().to_string()),
        Some(Value::Object(map)) => {
            if let Some(name_val) = map.get("name")
                && let Some(name) = extract_localized_string(name_val)
            {
                out.push(name);
            }
        }
        Some(Value::Array(arr)) => {
            for item in arr {
                match item {
                    Value::String(s) if !s.trim().is_empty() => out.push(s.trim().to_string()),
                    Value::Object(map) => {
                        if let Some(name_val) = map.get("name")
                            && let Some(name) = extract_localized_string(name_val)
                        {
                            out.push(name);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    out
}

fn parse_opds2_publication(
    source_id: &str,
    base_url: &Url,
    pub_val: &Value,
) -> Option<OpdsEntryDto> {
    let meta = pub_val.get("metadata")?;
    let title = meta
        .get("title")
        .and_then(extract_localized_string)
        .unwrap_or_default();
    let id = meta
        .get("identifier")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let authors = extract_opds2_authors(meta);
    let summary = meta.get("description").and_then(extract_localized_string);
    let updated = meta
        .get("modified")
        .or_else(|| meta.get("published"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut page_count = meta.get("numberOfPages").and_then(Value::as_u64);
    let mut pse_stream_url: Option<String> = None;

    let cover_url = pub_val
        .get("images")
        .and_then(Value::as_array)
        .and_then(|imgs| imgs.first())
        .and_then(|img| img.get("href"))
        .and_then(Value::as_str)
        .map(|href| resolve_link_url(base_url, href))
        .filter(|u| !u.is_empty())
        .map(|abs| proxy_image_path(source_id, &abs));

    let mut downloads = Vec::new();
    if let Some(links) = pub_val.get("links").and_then(Value::as_array) {
        for link in links {
            let Some(raw_href) = link.get("href").and_then(Value::as_str) else {
                continue;
            };
            if link_has_rel(link, "http://vaemendis.net/opds-pse/stream") {
                let abs_pse = resolve_link_url(base_url, raw_href);
                if !abs_pse.is_empty() {
                    pse_stream_url = Some(proxy_pse_stream_path(source_id, &abs_pse));
                }
                if let Some(count) = link
                    .pointer("/properties/numberOfPages")
                    .or_else(|| link.pointer("/properties/count"))
                    .and_then(Value::as_u64)
                {
                    page_count = Some(count);
                }
                continue;
            }
            if link_rel_matches_any(link, is_disallowed_acquisition_rel) {
                continue;
            }
            let media_type = link
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let is_acq = link_rel_matches_any(link, is_direct_acquisition_rel)
                || detect_format_from_mime_or_href(&media_type, raw_href).is_some();
            if !is_acq {
                continue;
            }
            let abs_href = resolve_link_url(base_url, raw_href);
            let format = detect_format_from_mime_or_href(&media_type, &abs_href)
                .unwrap_or_else(|| "file".to_string());
            downloads.push(OpdsDownloadLinkDto {
                href: abs_href,
                title: link.get("title").and_then(extract_localized_string),
                format,
                media_type: if media_type.is_empty() {
                    "application/octet-stream".to_string()
                } else {
                    media_type
                },
            });
        }
    }

    Some(OpdsEntryDto {
        id,
        title,
        authors,
        summary,
        cover_url,
        updated,
        pse_stream_url,
        page_count,
        downloads,
    })
}

fn parse_opds2_json(source_id: &str, base_url: &Url, raw: &str) -> Result<OpdsFeedDto, OpdsError> {
    let root: Value = serde_json::from_str(raw)
        .map_err(|e| OpdsError::Upstream(format!("Failed to parse OPDS 2.0 JSON: {e}")))?;

    let meta = root
        .get("metadata")
        .ok_or_else(|| OpdsError::Upstream("OPDS 2.0 feed is missing 'metadata' object".into()))?;
    let title = meta
        .get("title")
        .and_then(extract_localized_string)
        .unwrap_or_else(|| "OPDS 2.0 Catalog".to_string());

    let mut search_url: Option<String> = None;
    let mut prev_url: Option<String> = None;
    let mut next_url: Option<String> = None;
    if let Some(links) = root.get("links").and_then(Value::as_array) {
        for link in links {
            let Some(href) = link.get("href").and_then(Value::as_str) else {
                continue;
            };
            if link_has_rel(link, "next") && next_url.is_none() {
                next_url = Some(resolve_link_url(base_url, href));
            } else if (link_has_rel(link, "previous") || link_has_rel(link, "prev"))
                && prev_url.is_none()
            {
                prev_url = Some(resolve_link_url(base_url, href));
            } else if link_has_rel(link, "search") && search_url.is_none() {
                search_url = Some(resolve_link_url(base_url, href));
            }
        }
    }

    let mut nav_links = Vec::new();
    let collect_nav =
        |items: &[Value], group_prefix: Option<&str>, out: &mut Vec<OpdsNavLinkDto>| {
            for item in items {
                let Some(href) = item.get("href").and_then(Value::as_str) else {
                    continue;
                };
                let abs = resolve_link_url(base_url, href);
                let raw_title = item
                    .get("title")
                    .and_then(extract_localized_string)
                    .unwrap_or_else(|| abs.clone());
                let nav_title = match group_prefix {
                    Some(prefix) if !prefix.is_empty() && raw_title != prefix => {
                        format!("{prefix} · {raw_title}")
                    }
                    _ => raw_title,
                };
                let count = item
                    .pointer("/properties/numberOfItems")
                    .and_then(Value::as_u64);
                out.push(OpdsNavLinkDto {
                    title: nav_title,
                    href: abs,
                    count,
                });
            }
        };

    if let Some(nav) = root.get("navigation").and_then(Value::as_array) {
        collect_nav(nav, None, &mut nav_links);
    }

    let mut entries = Vec::new();
    if let Some(pubs) = root.get("publications").and_then(Value::as_array) {
        for p in pubs {
            if let Some(entry) = parse_opds2_publication(source_id, base_url, p) {
                entries.push(entry);
            }
        }
    }

    if let Some(groups) = root.get("groups").and_then(Value::as_array) {
        for group in groups {
            let group_title = group
                .get("metadata")
                .and_then(|m| m.get("title"))
                .and_then(extract_localized_string);
            let group_count = group
                .pointer("/metadata/numberOfItems")
                .and_then(Value::as_u64);

            if let Some(ref gtitle) = group_title
                && let Some(links) = group.get("links").and_then(Value::as_array)
                && let Some(link) = links
                    .iter()
                    .find(|l| link_has_rel(l, "self"))
                    .or_else(|| links.first())
                && let Some(href) = link.get("href").and_then(Value::as_str)
            {
                nav_links.push(OpdsNavLinkDto {
                    title: gtitle.clone(),
                    href: resolve_link_url(base_url, href),
                    count: group_count,
                });
            }

            if let Some(nav) = group.get("navigation").and_then(Value::as_array) {
                collect_nav(nav, group_title.as_deref(), &mut nav_links);
            }
            if let Some(pubs) = group.get("publications").and_then(Value::as_array) {
                for p in pubs {
                    if let Some(entry) = parse_opds2_publication(source_id, base_url, p) {
                        entries.push(entry);
                    }
                }
            }
        }
    }

    Ok(OpdsFeedDto {
        title,
        url: base_url.to_string(),
        opds_version: "2.0".to_string(),
        search_url,
        prev_url,
        next_url,
        nav_links,
        entries,
    })
}
