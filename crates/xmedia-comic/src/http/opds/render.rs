use axum::Json;
use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::json;
use xmedia_core::domain::xml::xml_escape;

use crate::application::opds::{OpdsChapterEntry, OpdsFeed, OpdsFeedPayload, OpdsNavItem};

pub(super) fn render_atom_feed(prefix: &str, feed: &OpdsFeed) -> Response {
    let is_acq = matches!(feed.payload, OpdsFeedPayload::Acquisition(_));
    let kind = if is_acq { "acquisition" } else { "navigation" };
    let feed_mime = format!("application/atom+xml;profile=opds-catalog;kind={kind}");
    let updated = feed.updated_at.to_string();

    let mut xml = String::with_capacity(4096);
    xml.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    xml.push('\n');
    xml.push_str(
        r#"<feed xmlns="http://www.w3.org/2005/Atom" xmlns:opds="http://opds-spec.org/2010/catalog" xmlns:pse="http://vaemendis.net/opds-pse/ns">"#,
    );
    xml.push_str(&format!("\n  <id>{}</id>", xml_escape(&feed.id)));
    xml.push_str(&format!("\n  <title>{}</title>", xml_escape(&feed.title)));
    xml.push_str(&format!("\n  <updated>{updated}</updated>"));
    xml.push_str("\n  <author><name>xmedia</name></author>");
    let escaped_prefix = xml_escape(prefix);
    xml.push_str(&format!(
        "\n  <link rel=\"self\" href=\"{}\" type=\"{feed_mime}\"/>",
        xml_escape(&feed.self_path)
    ));
    xml.push_str(&format!(
        "\n  <link rel=\"start\" href=\"{escaped_prefix}/v1.2\" type=\"application/atom+xml;profile=opds-catalog;kind=navigation\"/>"
    ));
    xml.push_str(&format!(
        "\n  <link rel=\"search\" href=\"{escaped_prefix}/v1.2/opensearch.xml\" type=\"application/opensearchdescription+xml\"/>"
    ));

    match &feed.payload {
        OpdsFeedPayload::Navigation(items) => {
            for item in items {
                append_nav_entry_xml(&mut xml, item);
            }
        }
        OpdsFeedPayload::Acquisition(entries) => {
            for entry in entries {
                append_acq_entry_xml(&mut xml, prefix, entry);
            }
        }
    }

    xml.push_str("\n</feed>\n");

    let content_type = format!("{feed_mime}; charset=utf-8");
    let mut res = Response::new(Body::from(xml));
    *res.status_mut() = StatusCode::OK;
    if let Ok(hv) = HeaderValue::from_str(&content_type) {
        res.headers_mut().insert(header::CONTENT_TYPE, hv);
    }
    res
}

fn infer_cover_mime(cover_url: &str) -> &'static str {
    let clean = cover_url
        .split('?')
        .next()
        .unwrap_or(cover_url)
        .to_ascii_lowercase();
    if clean.ends_with(".png") {
        "image/png"
    } else if clean.ends_with(".webp") {
        "image/webp"
    } else if clean.ends_with(".gif") {
        "image/gif"
    } else if clean.ends_with(".avif") {
        "image/avif"
    } else {
        "image/jpeg"
    }
}

fn append_nav_entry_xml(xml: &mut String, item: &OpdsNavItem) {
    let updated = item.updated_at.to_string();
    let sub_kind = if item.subsection_path.contains("/series/")
        || item.subsection_path.ends_with("/continue")
    {
        "acquisition"
    } else {
        "navigation"
    };
    xml.push_str("\n  <entry>");
    xml.push_str(&format!("\n    <id>{}</id>", xml_escape(&item.id)));
    xml.push_str(&format!("\n    <title>{}</title>", xml_escape(&item.title)));
    xml.push_str(&format!("\n    <updated>{updated}</updated>"));
    xml.push_str(&format!(
        "\n    <content type=\"text\">{}</content>",
        xml_escape(&item.summary)
    ));
    xml.push_str(&format!(
        "\n    <link rel=\"subsection\" href=\"{}\" type=\"application/atom+xml;profile=opds-catalog;kind={sub_kind}\"/>",
        xml_escape(&item.subsection_path)
    ));
    if let Some(cover) = &item.cover_url {
        let cover_mime = infer_cover_mime(cover);
        xml.push_str(&format!(
            "\n    <link rel=\"http://opds-spec.org/image/thumbnail\" href=\"{}\" type=\"{cover_mime}\"/>",
            xml_escape(cover)
        ));
    }
    xml.push_str("\n  </entry>");
}

fn append_acq_entry_xml(xml: &mut String, prefix: &str, entry: &OpdsChapterEntry) {
    let updated = entry.updated_at.to_string();
    xml.push_str("\n  <entry>");
    xml.push_str(&format!(
        "\n    <id>urn:xmedia:comic:chapter:{}</id>",
        entry.id
    ));
    xml.push_str(&format!(
        "\n    <title>{} - {}</title>",
        xml_escape(&entry.series_title),
        xml_escape(&entry.title)
    ));
    xml.push_str(&format!("\n    <updated>{updated}</updated>"));
    for author in &entry.authors {
        xml.push_str(&format!(
            "\n    <author><name>{}</name></author>",
            xml_escape(author)
        ));
    }
    if !entry.summary.is_empty() {
        xml.push_str(&format!(
            "\n    <summary>{}</summary>",
            xml_escape(&entry.summary)
        ));
    }
    if let Some(cover) = &entry.cover_url {
        let cover_mime = infer_cover_mime(cover);
        xml.push_str(&format!(
            "\n    <link rel=\"http://opds-spec.org/image\" href=\"{}\" type=\"{cover_mime}\"/>",
            xml_escape(cover)
        ));
        xml.push_str(&format!(
            "\n    <link rel=\"http://opds-spec.org/image/thumbnail\" href=\"{}\" type=\"{cover_mime}\"/>",
            xml_escape(cover)
        ));
    }
    let escaped_prefix = xml_escape(prefix);
    if entry.can_download_archive {
        xml.push_str(&format!(
            "\n    <link rel=\"http://opds-spec.org/acquisition\" href=\"{escaped_prefix}/v1.2/chapters/{}/download\" type=\"{}\"/>",
            entry.id,
            xml_escape(entry.archive_mime)
        ));
    }

    let mut pse_attrs = format!("pse:count=\"{}\"", entry.page_count);
    if let Some(last_page) = entry.last_read_page {
        let last_read_zero_based = last_page.saturating_sub(1);
        pse_attrs.push_str(&format!(" pse:lastRead=\"{last_read_zero_based}\""));
    }
    if let Some(last_ts) = entry.last_read_at {
        pse_attrs.push_str(&format!(" pse:lastReadDate=\"{last_ts}\""));
    }
    xml.push_str(&format!(
        "\n    <link rel=\"http://vaemendis.net/opds-pse/stream\" href=\"{escaped_prefix}/v1.2/chapters/{}/pse/{{pageNumber}}\" type=\"image/jpeg\" {pse_attrs}/>",
        entry.id
    ));

    xml.push_str("\n  </entry>");
}

#[derive(Serialize)]
struct Opds2Response {
    metadata: serde_json::Value,
    links: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    navigation: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    publications: Option<Vec<serde_json::Value>>,
}

pub(super) fn render_opds2_json(prefix: &str, feed: &OpdsFeed) -> Response {
    let modified = feed.updated_at.to_string();
    let links = vec![
        json!({
            "rel": "self",
            "href": feed.self_path,
            "type": "application/opds+json"
        }),
        json!({
            "rel": "start",
            "href": format!("{prefix}/v2.0"),
            "type": "application/opds+json"
        }),
        json!({
            "rel": "search",
            "href": format!("{prefix}/v2.0/series{{?q}}"),
            "type": "application/opds+json",
            "templated": true
        }),
    ];

    let (navigation, publications) = match &feed.payload {
        OpdsFeedPayload::Navigation(items) => {
            let nav: Vec<serde_json::Value> = items
                .iter()
                .map(|i| {
                    json!({
                        "title": i.title,
                        "href": i.subsection_path,
                        "type": "application/opds+json",
                        "properties": {
                            "subtitle": i.summary
                        }
                    })
                })
                .collect();
            (Some(nav), None)
        }
        OpdsFeedPayload::Acquisition(entries) => {
            let pubs: Vec<serde_json::Value> = entries
                .iter()
                .map(|e| {
                    let mut pub_links = vec![json!({
                        "rel": "http://vaemendis.net/opds-pse/stream",
                        "href": format!("{prefix}/v1.2/chapters/{}/pse/{{pageNumber}}", e.id),
                        "type": "image/jpeg",
                        "templated": true,
                        "properties": {
                            "numberOfItems": e.page_count,
                            "pse:lastRead": e.last_read_page.map(|p| p.saturating_sub(1)),
                        }
                    })];
                    if e.can_download_archive {
                        pub_links.push(json!({
                            "rel": "http://opds-spec.org/acquisition",
                            "href": format!("{prefix}/v1.2/chapters/{}/download", e.id),
                            "type": e.archive_mime,
                        }));
                    }
                    let images: Vec<serde_json::Value> = e
                        .cover_url
                        .iter()
                        .map(|c| json!({ "href": c, "type": infer_cover_mime(c) }))
                        .collect();
                    json!({
                        "metadata": {
                            "@type": "http://schema.org/ComicIssue",
                            "identifier": format!("urn:xmedia:comic:chapter:{}", e.id),
                            "title": format!("{} - {}", e.series_title, e.title),
                            "description": e.summary,
                            "author": e.authors,
                            "numberOfPages": e.page_count,
                            "modified": e.updated_at.to_string(),
                        },
                        "links": pub_links,
                        "images": images,
                    })
                })
                .collect();
            (None, Some(pubs))
        }
    };

    let body = Opds2Response {
        metadata: json!({
            "title": feed.title,
            "modified": modified,
        }),
        links,
        navigation,
        publications,
    };

    let mut res = Json(body).into_response();
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/opds+json; charset=utf-8"),
    );
    res
}
