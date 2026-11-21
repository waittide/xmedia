use std::collections::HashSet;
use std::sync::LazyLock;

use jiff::Timestamp;
use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::{BytesStart, Event};
use regex::Regex;

use super::epub_zip::attr_value;
use crate::domain::media::BookFileMeta;
use crate::infra::inspectors::append_xml_ref;

pub(super) type OpfManifestEntry = (String, String, String, String);

pub(super) struct ParsedOpfData {
    pub(super) meta: BookFileMeta,
    pub(super) package_version: String,
    pub(super) unique_identifier_id: String,
    pub(super) cover_meta_id: Option<String>,
    pub(super) manifest_items: Vec<OpfManifestEntry>,
}

pub(super) fn parse_opf_bytes(
    opf_bytes: &[u8],
    format: &str,
    fallback_title: &str,
) -> ParsedOpfData {
    let opf_str = String::from_utf8_lossy(opf_bytes);
    let mut reader = Reader::from_str(&opf_str);
    reader.config_mut().trim_text(false);

    let mut meta = BookFileMeta {
        format: format.to_string(),
        title: fallback_title.to_string(),
        writable: true,
        ..Default::default()
    };

    let mut package_version = "2.0".to_string();
    let mut unique_identifier_id = "bookid".to_string();
    let mut in_metadata = false;
    let mut cur_tag: Option<String> = None;
    let mut cur_meta_id: Option<String> = None;
    let mut cur_meta_prop: Option<String> = None;
    let mut cur_meta_refines: Option<String> = None;
    let mut text_acc = String::new();
    let mut cover_meta_id: Option<String> = None;
    let mut calibre_series: Option<String> = None;
    let mut calibre_subseries: Option<String> = None;
    let mut calibre_series_index: Option<String> = None;
    let mut collections: Vec<(Option<String>, String)> = Vec::new();
    let mut collection_types: Vec<(String, String)> = Vec::new();
    let mut group_positions: Vec<(String, String)> = Vec::new();
    let mut manifest_items: Vec<(String, String, String, String)> = Vec::new();

    let mut apply_meta_name_attr = |e: &BytesStart<'_>| {
        if let Some(name_val) = attr_value(e, "name") {
            let content = attr_value(e, "content")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            match name_val.trim().to_ascii_lowercase().as_str() {
                "cover" => cover_meta_id = content,
                "calibre:series" => calibre_series = content,
                "calibre:subseries" => calibre_subseries = content,
                "calibre:series_index" => calibre_series_index = content,
                "xmedia:locked_fields" => {
                    if let Some(c) = content {
                        for item in c.split(',') {
                            let f = item.trim().to_ascii_lowercase();
                            if !f.is_empty() && !meta.locked_fields.contains(&f) {
                                meta.locked_fields.push(f);
                            }
                        }
                    }
                }
                "xmedia:normalized" => {
                    if let Some(c) = content {
                        let v = c.to_ascii_lowercase();
                        meta.is_normalized =
                            matches!(v.as_str(), "1" | "true" | "yes") || v.starts_with("epub3");
                    }
                }
                _ => {}
            }
        }
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                if local == "package" {
                    if let Some(v) = attr_value(e, "version") {
                        package_version = v;
                    }
                    if let Some(uid) = attr_value(e, "unique-identifier")
                        && !uid.trim().is_empty()
                    {
                        unique_identifier_id = uid.trim().to_string();
                    }
                } else if local == "metadata" {
                    in_metadata = true;
                } else if in_metadata {
                    if local == "meta" {
                        apply_meta_name_attr(e);
                        cur_meta_id = attr_value(e, "id");
                        cur_meta_prop = attr_value(e, "property");
                        cur_meta_refines = attr_value(e, "refines");
                    }
                    cur_tag = Some(local);
                    text_acc.clear();
                } else if local == "item" {
                    manifest_items.push((
                        attr_value(e, "id").unwrap_or_default(),
                        attr_value(e, "href").unwrap_or_default(),
                        attr_value(e, "media-type").unwrap_or_default(),
                        attr_value(e, "properties").unwrap_or_default(),
                    ));
                }
            }
            Ok(Event::Empty(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                if in_metadata && local == "meta" {
                    apply_meta_name_attr(e);
                } else if local == "item" {
                    manifest_items.push((
                        attr_value(e, "id").unwrap_or_default(),
                        attr_value(e, "href").unwrap_or_default(),
                        attr_value(e, "media-type").unwrap_or_default(),
                        attr_value(e, "properties").unwrap_or_default(),
                    ));
                }
            }
            Ok(Event::Text(t)) if in_metadata => {
                if cur_tag.is_some() {
                    text_acc.push_str(&t.xml10_content());
                }
            }
            Ok(Event::GeneralRef(r)) if in_metadata => {
                if cur_tag.is_some() {
                    append_xml_ref(&mut text_acc, &r);
                }
            }
            Ok(Event::CData(c)) if in_metadata => {
                if cur_tag.is_some() {
                    text_acc.push_str(c.as_ref());
                }
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                if local == "metadata" {
                    in_metadata = false;
                } else if in_metadata && let Some(tag) = cur_tag.take() {
                    let trimmed = text_acc.trim();
                    if !trimmed.is_empty() {
                        match tag.as_str() {
                            "title" => meta.title = trimmed.to_string(),
                            "creator" if !meta.authors.iter().any(|a| a == trimmed) => {
                                meta.authors.push(trimmed.to_string());
                            }
                            "description" => meta.description = Some(trimmed.to_string()),
                            "publisher" => meta.publisher = Some(trimmed.to_string()),
                            "date" => meta.published_at = Some(trimmed.to_string()),
                            "language" => meta.language = Some(trimmed.to_string()),
                            "identifier" => {
                                let lower = trimmed.to_ascii_lowercase();
                                let stripped = if lower.starts_with("urn:isbn:") {
                                    &trimmed[9..]
                                } else if lower.starts_with("isbn:") {
                                    &trimmed[5..]
                                } else {
                                    trimmed
                                }
                                .trim();
                                let digits: String = stripped
                                    .chars()
                                    .filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x')
                                    .collect();
                                if meta.isbn.is_none() && (digits.len() == 10 || digits.len() == 13)
                                {
                                    meta.isbn = Some(stripped.to_string());
                                }
                            }
                            "subject"
                                if !meta.tags.iter().any(|s| s.eq_ignore_ascii_case(trimmed)) =>
                            {
                                meta.tags.push(trimmed.to_string());
                            }
                            "meta" => {
                                if let Some(prop) = cur_meta_prop.as_deref() {
                                    let prop_lower = prop.trim().to_ascii_lowercase();
                                    if prop_lower == "belongs-to-collection" {
                                        collections.push((
                                            cur_meta_id.as_ref().map(|s| s.trim().to_string()),
                                            trimmed.to_string(),
                                        ));
                                    } else if prop_lower == "collection-type" {
                                        if let Some(ref_id) = cur_meta_refines
                                            .as_deref()
                                            .and_then(|r| r.trim().strip_prefix('#'))
                                        {
                                            collection_types.push((
                                                ref_id.to_string(),
                                                trimmed.to_ascii_lowercase(),
                                            ));
                                        }
                                    } else if prop_lower == "group-position"
                                        && let Some(ref_id) = cur_meta_refines
                                            .as_deref()
                                            .and_then(|r| r.trim().strip_prefix('#'))
                                    {
                                        group_positions
                                            .push((ref_id.to_string(), trimmed.to_string()));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                cur_tag = None;
                cur_meta_id = None;
                cur_meta_prop = None;
                cur_meta_refines = None;
                text_acc.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    // Resolve EPUB 3 belongs-to-collection (set + series + group-position)
    if !collections.is_empty() {
        let type_for = |cid: Option<&str>| -> Option<&str> {
            let id = cid?;
            collection_types
                .iter()
                .find(|(k, _)| k == id)
                .map(|(_, v)| v.as_str())
        };
        let pos_for = |cid: Option<&str>| -> Option<String> {
            let id = cid?;
            group_positions
                .iter()
                .find(|(k, _)| k == id)
                .map(|(_, v)| v.clone())
        };

        if collections.len() >= 2 {
            let set_entry = collections
                .iter()
                .find(|(id, _)| type_for(id.as_deref()) == Some("set"));
            let ser_entry = collections
                .iter()
                .find(|(id, _)| type_for(id.as_deref()) == Some("series"));
            if let (Some((_, s_name)), Some((sub_id, sub_name))) = (set_entry, ser_entry) {
                meta.series = Some(s_name.clone());
                meta.subseries = Some(sub_name.clone());
                meta.series_index = pos_for(sub_id.as_deref());
            } else {
                meta.series = Some(collections[0].1.clone());
                meta.subseries = Some(collections[1].1.clone());
                meta.series_index = pos_for(collections[1].0.as_deref())
                    .or_else(|| pos_for(collections[0].0.as_deref()));
            }
        } else {
            let (cid, cname) = &collections[0];
            meta.series = Some(cname.clone());
            meta.series_index =
                pos_for(cid.as_deref()).or_else(|| group_positions.first().map(|(_, v)| v.clone()));
        }
    }

    if meta.series.is_none() {
        meta.series = calibre_series;
    }
    if meta.subseries.is_none() {
        meta.subseries = calibre_subseries;
    }
    if meta.series_index.is_none() {
        meta.series_index = calibre_series_index;
    }

    ParsedOpfData {
        meta,
        package_version,
        unique_identifier_id,
        cover_meta_id,
        manifest_items,
    }
}

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
        .filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x')
        .collect();
    !lower.starts_with("urn:uuid:") && (digits.len() == 10 || digits.len() == 13)
}

static RE_OPF_METADATA_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?is)<(?P<prefix>\w+:)?metadata\b(?P<attrs>[^>]*)>(?P<inner>.*?)</(?:\w+:)?metadata>",
    )
    .expect("valid opf metadata regex")
});

pub(crate) fn replace_opf_metadata_block(
    original_opf: &str,
    meta: &BookFileMeta,
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
                        "belongs-to-collection" | "collection-type" | "group-position"
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
                                escape(isbn_val.trim())
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
            _ => {
                preserved.push((None, trimmed_raw.to_string()));
            }
        }
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                if depth == 0 {
                    let rel = inner[prev_pos..].find('<').unwrap_or(0);
                    elem_start = prev_pos + rel;
                    elem_local = e.local_name().as_ref().to_ascii_lowercase();
                    elem_id = attr_value(e, "id");
                    elem_name = attr_value(e, "name");
                    elem_prop = attr_value(e, "property");
                    elem_refines = attr_value(e, "refines");
                    elem_scheme = attr_value(e, "scheme");
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
                        attr_value(e, "id"),
                        attr_value(e, "name"),
                        attr_value(e, "property"),
                        attr_value(e, "refines"),
                        attr_value(e, "scheme"),
                        "",
                        &inner[start..end],
                    );
                }
            }
            Ok(Event::Text(t)) if depth > 0 => {
                text_acc.push_str(&t.xml10_content());
            }
            Ok(Event::GeneralRef(r)) if depth > 0 => {
                append_xml_ref(&mut text_acc, &r);
            }
            Ok(Event::CData(c)) if depth > 0 => {
                text_acc.push_str(c.as_ref());
            }
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
            escape(&book_id)
        ));
    }

    if !emitted_isbn
        && let Some(ref isbn_val) = meta.isbn
        && !isbn_val.trim().is_empty()
    {
        block.push_str(&format!(
            "    <dc:identifier>urn:isbn:{}</dc:identifier>\n",
            escape(isbn_val.trim())
        ));
    }

    block.push_str(&format!(
        "    <dc:title>{}</dc:title>\n",
        escape(meta.title.trim())
    ));
    for author in &meta.authors {
        let clean = author.trim();
        if !clean.is_empty() {
            block.push_str(&format!("    <dc:creator>{}</dc:creator>\n", escape(clean)));
        }
    }
    let lang = meta
        .language
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("zh");
    block.push_str(&format!(
        "    <dc:language>{}</dc:language>\n",
        escape(lang)
    ));
    if let Some(ref desc) = meta.description
        && !desc.trim().is_empty()
    {
        block.push_str(&format!(
            "    <dc:description>{}</dc:description>\n",
            escape(desc.trim())
        ));
    }
    if let Some(ref pub_name) = meta.publisher
        && !pub_name.trim().is_empty()
    {
        block.push_str(&format!(
            "    <dc:publisher>{}</dc:publisher>\n",
            escape(pub_name.trim())
        ));
    }
    if let Some(ref date) = meta.published_at
        && !date.trim().is_empty()
    {
        block.push_str(&format!("    <dc:date>{}</dc:date>\n", escape(date.trim())));
    }
    for subject in &meta.tags {
        let clean = subject.trim();
        if !clean.is_empty() {
            block.push_str(&format!("    <dc:subject>{}</dc:subject>\n", escape(clean)));
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
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if package_version_hint.trim().starts_with('3') {
        match (clean_series, clean_subseries) {
            (Some(ser), Some(sub)) => {
                block.push_str(&format!(
                    "    <meta property=\"belongs-to-collection\" id=\"xmedia-col-set\">{}</meta>\n",
                    escape(ser)
                ));
                block.push_str(
                    "    <meta refines=\"#xmedia-col-set\" property=\"collection-type\">set</meta>\n",
                );
                block.push_str(&format!(
                    "    <meta property=\"belongs-to-collection\" id=\"xmedia-col-series\">{}</meta>\n",
                    escape(sub)
                ));
                block.push_str(
                    "    <meta refines=\"#xmedia-col-series\" property=\"collection-type\">series</meta>\n",
                );
                if let Some(idx) = clean_index {
                    block.push_str(&format!(
                        "    <meta refines=\"#xmedia-col-series\" property=\"group-position\">{}</meta>\n",
                        escape(idx)
                    ));
                }
            }
            (Some(ser), None) | (None, Some(ser)) => {
                block.push_str(&format!(
                    "    <meta property=\"belongs-to-collection\" id=\"xmedia-col-series\">{}</meta>\n",
                    escape(ser)
                ));
                block.push_str(
                    "    <meta refines=\"#xmedia-col-series\" property=\"collection-type\">series</meta>\n",
                );
                if let Some(idx) = clean_index {
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
                escape(ser)
            ));
        }
        if let Some(sub) = clean_subseries {
            block.push_str(&format!(
                "    <meta name=\"calibre:subseries\" content=\"{}\"/>\n",
                escape(sub)
            ));
        }
        if let Some(idx) = clean_index {
            block.push_str(&format!(
                "    <meta name=\"calibre:series_index\" content=\"{}\"/>\n",
                escape(idx)
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

pub(super) fn render_sidecar_opf(meta: &BookFileMeta) -> String {
    let base = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
  </metadata>
</package>
"#;
    replace_opf_metadata_block(base, meta, None, "bookid", "2.0")
}

pub(super) fn swap_href_extension(href: &str, new_ext: &str) -> String {
    match href.rsplit_once('.') {
        Some((stem, _)) if !stem.ends_with('/') => format!("{stem}.{new_ext}"),
        _ => format!("{href}.{new_ext}"),
    }
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

pub(super) fn update_manifest_cover_item(
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
