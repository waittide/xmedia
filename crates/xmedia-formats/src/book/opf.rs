use std::collections::HashMap;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

pub use super::opf_write::{
    render_sidecar_opf, replace_opf_metadata_block, swap_href_extension, update_manifest_cover_item,
};
use super::{BookFormatExt, ParsedBookMeta};
use crate::common::{append_xml_ref, xml_attr_value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpfManifestItem {
    pub id: String,
    pub href: String,
    pub media_type: String,
    pub properties: String,
}

#[derive(Debug, Clone)]
pub struct ParsedOpfDocument {
    pub meta: ParsedBookMeta,
    pub package_version: String,
    pub unique_identifier_id: String,
    pub cover_meta_id: Option<String>,
    pub spine_toc_id: Option<String>,
    pub spine_progression_direction: Option<String>,
    pub rendition_flow: Option<String>,
    pub spine_ids: Vec<String>,
    pub manifest: Vec<OpfManifestItem>,
}

impl ParsedOpfDocument {
    pub fn manifest_by_id(&self) -> HashMap<&str, &OpfManifestItem> {
        self.manifest
            .iter()
            .filter(|i| !i.id.is_empty())
            .map(|i| (i.id.as_str(), i))
            .collect()
    }
}

fn parse_finite_non_neg_f64(raw: &str) -> Option<f64> {
    raw.trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
}

fn strip_html_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.trim().to_string()
}

#[derive(Default)]
struct CalibreMetaAttrs {
    cover_meta_id: Option<String>,
    series: Option<String>,
    subseries: Option<String>,
    series_index: Option<f64>,
    rating: Option<f64>,
}

fn apply_meta_name_attr(e: &BytesStart<'_>, meta: &mut ParsedBookMeta, cal: &mut CalibreMetaAttrs) {
    let Some(name_val) = xml_attr_value(e, "name") else {
        return;
    };
    let content = xml_attr_value(e, "content")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    match name_val.trim().to_ascii_lowercase().as_str() {
        "cover" => cal.cover_meta_id = content,
        "calibre:series" => cal.series = content,
        "calibre:subseries" => cal.subseries = content,
        "calibre:series_index" => {
            cal.series_index = content.as_deref().and_then(parse_finite_non_neg_f64);
        }
        "calibre:rating" => {
            cal.rating = content.as_deref().and_then(parse_finite_non_neg_f64);
        }
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

fn push_manifest_item(e: &BytesStart<'_>, out: &mut Vec<OpfManifestItem>) {
    out.push(OpfManifestItem {
        id: xml_attr_value(e, "id").unwrap_or_default(),
        href: xml_attr_value(e, "href").unwrap_or_default(),
        media_type: xml_attr_value(e, "media-type").unwrap_or_default(),
        properties: xml_attr_value(e, "properties").unwrap_or_default(),
    });
}

/// Parses an OPF XML document (EPUB 2/3 `content.opf` or standalone Calibre `metadata.opf`).
pub fn parse_opf_xml(
    opf_bytes: &[u8],
    format: BookFormatExt,
    fallback_title: &str,
) -> ParsedOpfDocument {
    let opf_str = String::from_utf8_lossy(opf_bytes);
    let mut reader = Reader::from_str(&opf_str);
    reader.config_mut().trim_text(false);

    let mut meta = ParsedBookMeta::with_fallback(format, fallback_title);
    let mut package_version = "2.0".to_string();
    let mut unique_identifier_id = "bookid".to_string();
    let mut spine_toc_id: Option<String> = None;
    let mut spine_progression_direction: Option<String> = None;
    let mut rendition_flow: Option<String> = None;
    let mut spine_ids: Vec<String> = Vec::new();
    let mut manifest: Vec<OpfManifestItem> = Vec::new();

    let mut in_metadata = false;
    let mut cur_tag: Option<String> = None;
    let mut cur_meta_id: Option<String> = None;
    let mut cur_meta_prop: Option<String> = None;
    let mut cur_meta_refines: Option<String> = None;
    let mut text_acc = String::new();

    let mut cal = CalibreMetaAttrs::default();
    let mut collections: Vec<(Option<String>, String)> = Vec::new();
    let mut collection_types: Vec<(String, String)> = Vec::new();
    let mut group_positions: Vec<(String, f64)> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                match local.as_str() {
                    "package" => {
                        if let Some(v) = xml_attr_value(e, "version") {
                            package_version = v;
                        }
                        if let Some(uid) = xml_attr_value(e, "unique-identifier")
                            && !uid.trim().is_empty()
                        {
                            unique_identifier_id = uid.trim().to_string();
                        }
                    }
                    "metadata" => in_metadata = true,
                    "item" if !in_metadata => push_manifest_item(e, &mut manifest),
                    "spine" if !in_metadata => {
                        spine_toc_id = xml_attr_value(e, "toc");
                        spine_progression_direction =
                            xml_attr_value(e, "page-progression-direction");
                    }
                    "itemref" if !in_metadata => {
                        if let Some(idref) = xml_attr_value(e, "idref") {
                            spine_ids.push(idref);
                        }
                    }
                    _ if in_metadata => {
                        if local == "meta" {
                            apply_meta_name_attr(e, &mut meta, &mut cal);
                            cur_meta_id = xml_attr_value(e, "id");
                            cur_meta_prop = xml_attr_value(e, "property");
                            cur_meta_refines = xml_attr_value(e, "refines");
                        }
                        cur_tag = Some(local);
                        text_acc.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                if in_metadata && local == "meta" {
                    apply_meta_name_attr(e, &mut meta, &mut cal);
                } else if !in_metadata {
                    match local.as_str() {
                        "item" => push_manifest_item(e, &mut manifest),
                        "spine" => {
                            spine_toc_id = xml_attr_value(e, "toc");
                            spine_progression_direction =
                                xml_attr_value(e, "page-progression-direction");
                        }
                        "itemref" => {
                            if let Some(idref) = xml_attr_value(e, "idref") {
                                spine_ids.push(idref);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::Text(t)) if in_metadata && cur_tag.is_some() => {
                text_acc.push_str(&t.xml10_content());
            }
            Ok(Event::GeneralRef(r)) if in_metadata && cur_tag.is_some() => {
                append_xml_ref(&mut text_acc, &r);
            }
            Ok(Event::CData(c)) if in_metadata && cur_tag.is_some() => {
                text_acc.push_str(c.as_ref());
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
                            "description" => {
                                let clean = if trimmed.contains('<') && trimmed.contains('>') {
                                    let stripped = strip_html_tags(trimmed);
                                    if stripped.is_empty() {
                                        trimmed.to_string()
                                    } else {
                                        stripped
                                    }
                                } else {
                                    trimmed.to_string()
                                };
                                meta.description = Some(clean);
                            }
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
                                    .filter(|c| c.is_ascii_digit() || matches!(*c, 'X' | 'x'))
                                    .collect();
                                if meta.isbn.is_none()
                                    && !lower.starts_with("urn:uuid:")
                                    && (digits.len() == 10 || digits.len() == 13)
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
                                    match prop_lower.as_str() {
                                        "belongs-to-collection" => {
                                            collections.push((
                                                cur_meta_id.as_ref().map(|s| s.trim().to_string()),
                                                trimmed.to_string(),
                                            ));
                                        }
                                        "collection-type" => {
                                            if let Some(ref_id) = cur_meta_refines
                                                .as_deref()
                                                .and_then(|r| r.trim().strip_prefix('#'))
                                            {
                                                collection_types.push((
                                                    ref_id.to_string(),
                                                    trimmed.to_ascii_lowercase(),
                                                ));
                                            }
                                        }
                                        "group-position" => {
                                            if let Some(ref_id) = cur_meta_refines
                                                .as_deref()
                                                .and_then(|r| r.trim().strip_prefix('#'))
                                                && let Some(pos) = parse_finite_non_neg_f64(trimmed)
                                            {
                                                group_positions.push((ref_id.to_string(), pos));
                                            }
                                        }
                                        "calibre:rating" | "schema:ratingvalue" => {
                                            if cal.rating.is_none() {
                                                cal.rating = parse_finite_non_neg_f64(trimmed);
                                            }
                                        }
                                        "rendition:flow" => {
                                            rendition_flow = Some(trimmed.to_ascii_lowercase());
                                        }
                                        _ => {}
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

    if !collections.is_empty() {
        let type_for = |cid: Option<&str>| -> Option<&str> {
            let id = cid?;
            collection_types
                .iter()
                .find(|(k, _)| k == id)
                .map(|(_, v)| v.as_str())
        };
        let pos_for = |cid: Option<&str>| -> Option<f64> {
            let id = cid?;
            group_positions
                .iter()
                .find(|(k, _)| k == id)
                .map(|(_, v)| *v)
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
                pos_for(cid.as_deref()).or_else(|| group_positions.first().map(|(_, v)| *v));
        }
    }

    if meta.series.is_none() {
        meta.series = cal.series;
    }
    if meta.subseries.is_none() {
        meta.subseries = cal.subseries;
    }
    if meta.series_index.is_none() {
        meta.series_index = cal.series_index;
    }
    meta.rating = cal.rating;

    ParsedOpfDocument {
        meta,
        package_version,
        unique_identifier_id,
        cover_meta_id: cal.cover_meta_id,
        spine_toc_id,
        spine_progression_direction,
        rendition_flow,
        spine_ids,
        manifest,
    }
}

/// Overlays non-empty fields from a sidecar `metadata.opf` onto `target`.
pub fn overlay_opf_sidecar_bytes(opf_bytes: &[u8], target: &mut ParsedBookMeta) {
    let sidecar = parse_opf_xml(opf_bytes, target.format, &target.title).meta;
    if !sidecar.title.trim().is_empty() {
        target.title = sidecar.title;
    }
    if !sidecar.authors.is_empty() {
        target.authors = sidecar.authors;
    }
    if sidecar.description.is_some() {
        target.description = sidecar.description;
    }
    if sidecar.isbn.is_some() {
        target.isbn = sidecar.isbn;
    }
    if sidecar.language.is_some() {
        target.language = sidecar.language;
    }
    if sidecar.publisher.is_some() {
        target.publisher = sidecar.publisher;
    }
    if sidecar.published_at.is_some() {
        target.published_at = sidecar.published_at;
    }
    if !sidecar.tags.is_empty() {
        target.tags = sidecar.tags;
    }
    if sidecar.series.is_some() {
        target.series = sidecar.series;
    }
    if sidecar.subseries.is_some() {
        target.subseries = sidecar.subseries;
    }
    if sidecar.series_index.is_some() {
        target.series_index = sidecar.series_index;
    }
    if sidecar.rating.is_some() {
        target.rating = sidecar.rating;
    }
    if !sidecar.locked_fields.is_empty() {
        target.locked_fields = sidecar.locked_fields;
    }
}
