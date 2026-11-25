use std::collections::HashSet;
use std::sync::LazyLock;

use jiff::Timestamp;
use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::Event;
use regex::Regex;

use super::ParsedBookMeta;
use crate::common::{append_xml_ref, format_clean_float, xml_attr_value, xml_escape_clean};

fn looks_like_isbn(text: &str, scheme_attr: Option<&str>) -> bool {
    if scheme_attr.is_some_and(|s| s.eq_ignore_ascii_case("isbn")) {
        return true;
    }
    let lower = text.trim().to_ascii_lowercase();
    if lower.starts_with("urn:isbn:") || lower.starts_with("isbn:") {
        return true;
    }
    let digits: String = text
        .chars()
        .filter(|c| c.is_ascii_digit() || matches!(*c, 'X' | 'x'))
        .collect();
    !lower.starts_with("urn:uuid:") && (digits.len() == 10 || digits.len() == 13)
}

static RE_OPF_METADATA_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?is)<(?P<prefix>\w+:)?metadata\b(?P<attrs>[^>]*)>(?P<inner>.*?)</(?:\w+:)?metadata>",
    )
    .expect("valid opf metadata regex")
});

/// Non-destructively updates `<metadata>...</metadata>` in an OPF XML string while preserving
/// unknown `<meta>`, `<dc:identifier>`, and custom extension elements.
pub fn replace_opf_metadata_block(
    original_opf: &str,
    meta: &ParsedBookMeta,
    cover_id_hint: Option<&str>,
    unique_id_hint: &str,
    package_version_hint: &str,
) -> String {
    let Some(caps) = RE_OPF_METADATA_BLOCK.captures(original_opf) else {
        return original_opf.to_string();
    };
    let prefix = caps.name("prefix").map(|m| m.as_str()).unwrap_or("");
    let open_attrs = caps.name("attrs").map(|m| m.as_str()).unwrap_or("");
    let inner = caps.name("inner").map(|m| m.as_str()).unwrap_or("");

    let mut preserved: Vec<(Option<String>, String)> = Vec::new();
    let mut removed_ids: HashSet<String> = HashSet::new();
    let mut has_primary_unique_id = false;
    let mut emitted_isbn = false;
    let mut had_dcterms_modified = false;

    let mut reader = Reader::from_str(inner);
    reader.config_mut().trim_text(false);
    let mut prev_pos = 0usize;
    let mut depth = 0usize;
    let mut elem_start = 0usize;
    let mut elem_local = String::new();
    let mut elem_id: Option<String> = None;
    let mut elem_name: Option<String> = None;
    let mut elem_prop: Option<String> = None;
    let mut elem_refines: Option<String> = None;
    let mut elem_scheme: Option<String> = None;
    let mut text_acc = String::new();

    let mut handle_top_element = |local: &str,
                                  id: Option<String>,
                                  name: Option<String>,
                                  prop: Option<String>,
                                  refines: Option<String>,
                                  scheme: Option<String>,
                                  inner_text: &str,
                                  raw_slice: &str| {
        let trimmed_raw = raw_slice.trim();
        if trimmed_raw.is_empty() {
            return;
        }
        match local {
            "title" | "creator" | "description" | "publisher" | "date" | "language" | "subject" => {
                if let Some(elem_id_val) = id {
                    removed_ids.insert(elem_id_val);
                }
            }
            "meta" => {
                if prop
                    .as_deref()
                    .is_some_and(|p| p.eq_ignore_ascii_case("dcterms:modified"))
                {
                    had_dcterms_modified = true;
                    return;
                }
                if prop.as_deref().is_some_and(|p| {
                    matches!(
                        p.trim().to_ascii_lowercase().as_str(),
                        "belongs-to-collection"
                            | "collection-type"
                            | "group-position"
                            | "calibre:rating"
                    )
                }) {
                    if let Some(elem_id_val) = id {
                        removed_ids.insert(elem_id_val);
                    }
                    return;
                }
                if name.as_deref().is_some_and(|n| {
                    matches!(
                        n.trim().to_ascii_lowercase().as_str(),
                        "calibre:series"
                            | "calibre:subseries"
                            | "calibre:series_index"
                            | "calibre:subseries_index"
                            | "calibre:rating"
                            | "xmedia:locked_fields"
                            | "xmedia:normalized"
                    )
                }) {
                    if let Some(elem_id_val) = id {
                        removed_ids.insert(elem_id_val);
                    }
                    return;
                }
                if name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case("cover"))
                    && cover_id_hint.is_some()
                {
                    return;
                }
                let refines_id = refines
                    .as_deref()
                    .and_then(|r| r.trim().strip_prefix('#'))
                    .map(str::to_owned);
                preserved.push((refines_id, trimmed_raw.to_string()));
            }
            "identifier" => {
                let is_primary = id.as_deref() == Some(unique_id_hint);
                let is_isbn = looks_like_isbn(inner_text, scheme.as_deref());
                if is_primary {
                    has_primary_unique_id = true;
                    if !is_isbn {
                        preserved.push((None, trimmed_raw.to_string()));
                    } else if let Some(ref isbn_val) = meta.isbn
                        && !isbn_val.trim().is_empty()
                    {
                        emitted_isbn = true;
                        preserved.push((
                            None,
                            format!(
                                "<dc:identifier id=\"{}\">urn:isbn:{}</dc:identifier>",
                                escape(unique_id_hint),
                                xml_escape_clean(isbn_val.trim())
                            ),
                        ));
                    } else {
                        preserved.push((
                            None,
                            format!(
                                "<dc:identifier id=\"{}\">urn:uuid:{}</dc:identifier>",
                                escape(unique_id_hint),
                                uuid::Uuid::now_v7()
                            ),
                        ));
                    }
                } else if !is_isbn {
                    preserved.push((None, trimmed_raw.to_string()));
                }
            }
            _ => preserved.push((None, trimmed_raw.to_string())),
        }
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                if depth == 0 {
                    let rel = inner[prev_pos..].find('<').unwrap_or(0);
                    elem_start = prev_pos + rel;
                    elem_local = e.local_name().as_ref().to_ascii_lowercase();
                    elem_id = xml_attr_value(e, "id");
                    elem_name = xml_attr_value(e, "name");
                    elem_prop = xml_attr_value(e, "property");
                    elem_refines = xml_attr_value(e, "refines");
                    elem_scheme = xml_attr_value(e, "scheme");
                    text_acc.clear();
                }
                depth += 1;
            }
            Ok(Event::Empty(ref e)) => {
                if depth == 0 {
                    let rel = inner[prev_pos..].find('<').unwrap_or(0);
                    let start = prev_pos + rel;
                    let end = reader.buffer_position() as usize;
                    let local = e.local_name().as_ref().to_ascii_lowercase();
                    handle_top_element(
                        &local,
                        xml_attr_value(e, "id"),
                        xml_attr_value(e, "name"),
                        xml_attr_value(e, "property"),
                        xml_attr_value(e, "refines"),
                        xml_attr_value(e, "scheme"),
                        "",
                        &inner[start..end],
                    );
                }
            }
            Ok(Event::Text(t)) if depth > 0 => text_acc.push_str(&t.xml10_content()),
            Ok(Event::GeneralRef(r)) if depth > 0 => append_xml_ref(&mut text_acc, &r),
            Ok(Event::CData(c)) if depth > 0 => text_acc.push_str(c.as_ref()),
            Ok(Event::End(_)) => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        let end = reader.buffer_position() as usize;
                        handle_top_element(
                            &elem_local,
                            elem_id.take(),
                            elem_name.take(),
                            elem_prop.take(),
                            elem_refines.take(),
                            elem_scheme.take(),
                            &text_acc,
                            &inner[elem_start..end],
                        );
                        text_acc.clear();
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        prev_pos = reader.buffer_position() as usize;
    }

    let mut attrs_out = open_attrs.to_string();
    if !original_opf.contains("xmlns:dc=") && !attrs_out.contains("xmlns:dc=") {
        attrs_out.push_str(" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"");
    }

    let mut block = String::with_capacity(1024);
    block.push_str(&format!("<{prefix}metadata{attrs_out}>\n"));

    if !has_primary_unique_id {
        let book_id = if let Some(ref isbn_val) = meta.isbn
            && !isbn_val.trim().is_empty()
        {
            emitted_isbn = true;
            format!("urn:isbn:{}", isbn_val.trim())
        } else {
            format!("urn:uuid:{}", uuid::Uuid::now_v7())
        };
        block.push_str(&format!(
            "    <dc:identifier id=\"{}\">{}</dc:identifier>\n",
            escape(unique_id_hint),
            xml_escape_clean(&book_id)
        ));
    }

    if !emitted_isbn
        && let Some(ref isbn_val) = meta.isbn
        && !isbn_val.trim().is_empty()
    {
        block.push_str(&format!(
            "    <dc:identifier>urn:isbn:{}</dc:identifier>\n",
            xml_escape_clean(isbn_val.trim())
        ));
    }

    block.push_str(&format!(
        "    <dc:title>{}</dc:title>\n",
        xml_escape_clean(meta.title.trim())
    ));
    for author in &meta.authors {
        let clean = author.trim();
        if !clean.is_empty() {
            block.push_str(&format!(
                "    <dc:creator>{}</dc:creator>\n",
                xml_escape_clean(clean)
            ));
        }
    }
    let lang = meta
        .language
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("zh");
    block.push_str(&format!(
        "    <dc:language>{}</dc:language>\n",
        xml_escape_clean(lang)
    ));
    if let Some(ref desc) = meta.description
        && !desc.trim().is_empty()
    {
        block.push_str(&format!(
            "    <dc:description>{}</dc:description>\n",
            xml_escape_clean(desc.trim())
        ));
    }
    if let Some(ref pub_name) = meta.publisher
        && !pub_name.trim().is_empty()
    {
        block.push_str(&format!(
            "    <dc:publisher>{}</dc:publisher>\n",
            xml_escape_clean(pub_name.trim())
        ));
    }
    if let Some(ref date) = meta.published_at
        && !date.trim().is_empty()
    {
        block.push_str(&format!(
            "    <dc:date>{}</dc:date>\n",
            xml_escape_clean(date.trim())
        ));
    }
    for subject in &meta.tags {
        let clean = subject.trim();
        if !clean.is_empty() {
            block.push_str(&format!(
                "    <dc:subject>{}</dc:subject>\n",
                xml_escape_clean(clean)
            ));
        }
    }

    let clean_series = meta
        .series
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let clean_subseries = meta
        .subseries
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let clean_index = meta
        .series_index
        .map(format_clean_float)
        .filter(|s| !s.is_empty());

    if package_version_hint.trim().starts_with('3') {
        match (clean_series, clean_subseries) {
            (Some(ser), Some(sub)) => {
                block.push_str(&format!(
                    "    <meta property=\"belongs-to-collection\" id=\"xmedia-col-set\">{}</meta>\n",
                    xml_escape_clean(ser)
                ));
                block.push_str(
                    "    <meta refines=\"#xmedia-col-set\" property=\"collection-type\">set</meta>\n",
                );
                block.push_str(&format!(
                    "    <meta property=\"belongs-to-collection\" id=\"xmedia-col-series\">{}</meta>\n",
                    xml_escape_clean(sub)
                ));
                block.push_str(
                    "    <meta refines=\"#xmedia-col-series\" property=\"collection-type\">series</meta>\n",
                );
                if let Some(ref idx) = clean_index {
                    block.push_str(&format!(
                        "    <meta refines=\"#xmedia-col-series\" property=\"group-position\">{}</meta>\n",
                        escape(idx)
                    ));
                }
            }
            (Some(ser), None) | (None, Some(ser)) => {
                block.push_str(&format!(
                    "    <meta property=\"belongs-to-collection\" id=\"xmedia-col-series\">{}</meta>\n",
                    xml_escape_clean(ser)
                ));
                block.push_str(
                    "    <meta refines=\"#xmedia-col-series\" property=\"collection-type\">series</meta>\n",
                );
                if let Some(ref idx) = clean_index {
                    block.push_str(&format!(
                        "    <meta refines=\"#xmedia-col-series\" property=\"group-position\">{}</meta>\n",
                        escape(idx)
                    ));
                }
            }
            (None, None) => {}
        }
    } else {
        if let Some(ser) = clean_series {
            block.push_str(&format!(
                "    <meta name=\"calibre:series\" content=\"{}\"/>\n",
                xml_escape_clean(ser)
            ));
        }
        if let Some(sub) = clean_subseries {
            block.push_str(&format!(
                "    <meta name=\"calibre:subseries\" content=\"{}\"/>\n",
                xml_escape_clean(sub)
            ));
        }
        if let Some(ref idx) = clean_index {
            block.push_str(&format!(
                "    <meta name=\"calibre:series_index\" content=\"{}\"/>\n",
                escape(idx)
            ));
        }
    }

    if let Some(rating) = meta.rating.filter(|r| r.is_finite() && *r >= 0.0) {
        let r_str = format_clean_float(rating);
        if !r_str.is_empty() {
            block.push_str(&format!(
                "    <meta name=\"calibre:rating\" content=\"{}\"/>\n",
                escape(&r_str)
            ));
        }
    }

    let clean_locked: Vec<&str> = meta
        .locked_fields
        .iter()
        .map(String::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if !clean_locked.is_empty() {
        block.push_str(&format!(
            "    <meta name=\"xmedia:locked_fields\" content=\"{}\"/>\n",
            escape(clean_locked.join(","))
        ));
    }

    if meta.is_normalized {
        block.push_str("    <meta name=\"xmedia:normalized\" content=\"epub3\"/>\n");
    }

    if let Some(cid) = cover_id_hint {
        block.push_str(&format!(
            "    <meta name=\"cover\" content=\"{}\"/>\n",
            escape(cid)
        ));
    }

    for (refines_target, raw_elem) in preserved {
        if let Some(target_id) = refines_target
            && removed_ids.contains(&target_id)
        {
            continue;
        }
        block.push_str("    ");
        block.push_str(&raw_elem);
        block.push('\n');
    }

    if package_version_hint.trim().starts_with('3') || had_dcterms_modified {
        let modified = Timestamp::now().strftime("%Y-%m-%dT%H:%M:%SZ").to_string();
        block.push_str(&format!(
            "    <meta property=\"dcterms:modified\">{modified}</meta>\n"
        ));
    }

    block.push_str(&format!("  </{prefix}metadata>"));
    RE_OPF_METADATA_BLOCK
        .replace(original_opf, block.as_str())
        .into_owned()
}

/// Renders a standalone sidecar `metadata.opf` document for `meta`.
pub fn render_sidecar_opf(meta: &ParsedBookMeta) -> String {
    let base = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
  </metadata>
</package>
"#;
    replace_opf_metadata_block(base, meta, None, "bookid", "2.0")
}

static RE_MANIFEST_ITEM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<item\b[^>]*?/?>").expect("valid item regex"));
static RE_MANIFEST_HREF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\bhref\s*=\s*(?:"[^"]*"|'[^']*')"#).expect("valid href regex")
});
static RE_MANIFEST_MIME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\bmedia-type\s*=\s*(?:"[^"]*"|'[^']*')"#).expect("valid mime regex")
});
static RE_MANIFEST_ID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\bid\s*=\s*(?:"([^"]*)"|'([^']*)')"#).expect("valid id regex")
});
static RE_MANIFEST_HREF_VAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\bhref\s*=\s*(?:"([^"]*)"|'([^']*)')"#).expect("valid href val regex")
});

pub fn swap_href_extension(href: &str, new_ext: &str) -> String {
    match href.rsplit_once('.') {
        Some((stem, _)) if !stem.ends_with('/') => format!("{stem}.{new_ext}"),
        _ => format!("{href}.{new_ext}"),
    }
}

pub fn update_manifest_cover_item(
    opf_str: &str,
    cover_item_id: Option<&str>,
    old_href: &str,
    new_href: &str,
    new_mime: &str,
) -> String {
    RE_MANIFEST_ITEM
        .replace_all(opf_str, |caps: &regex::Captures<'_>| {
            let tag = &caps[0];
            let matched_by_id = cover_item_id.is_some_and(|cid| {
                RE_MANIFEST_ID
                    .captures(tag)
                    .and_then(|c| c.get(1).or_else(|| c.get(2)))
                    .is_some_and(|m| m.as_str() == cid)
            });
            let matched_by_href = RE_MANIFEST_HREF_VAL
                .captures(tag)
                .and_then(|c| c.get(1).or_else(|| c.get(2)))
                .is_some_and(|m| m.as_str() == old_href);
            if !matched_by_id && !matched_by_href {
                return tag.to_string();
            }
            let updated =
                RE_MANIFEST_HREF.replace(tag, format!("href=\"{}\"", escape(new_href)).as_str());
            if RE_MANIFEST_MIME.is_match(&updated) {
                RE_MANIFEST_MIME
                    .replace(
                        &updated,
                        format!("media-type=\"{}\"", escape(new_mime)).as_str(),
                    )
                    .into_owned()
            } else {
                updated.into_owned()
            }
        })
        .into_owned()
}
