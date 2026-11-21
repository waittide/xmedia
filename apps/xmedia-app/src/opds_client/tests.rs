use url::Url;

use super::helpers::{
    expand_search_template, parse_filename_from_content_disposition, percent_decode_utf8,
    resolve_target_url,
};
use super::http::is_safe_raster_image_content_type;
use super::parser::parse_opds_feed;

#[test]
fn parses_opds12_atom_navigation_and_acquisition_feeds() {
    let base = Url::parse("http://example.com/opds/book/v1.2").unwrap();
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:opds="http://opds-spec.org/2010/catalog" xmlns:pse="http://vaemendis.net/ns/opds-pse/ns">
  <title>Test Books</title>
  <link rel="search" href="/opds/book/v1.2/opensearch.xml" type="application/opensearchdescription+xml"/>
  <link rel="previous" href="/opds/book/v1.2/all?page=1"/>
  <link rel="next" href="/opds/book/v1.2/all?page=3"/>
  <entry>
    <id>nav-1</id>
    <title>All Books</title>
    <link rel="subsection" href="/opds/book/v1.2/all" type="application/atom+xml;profile=opds-catalog;kind=acquisition"/>
  </entry>
  <entry>
    <id>urn:uuid:book-1</id>
    <title>Foundation</title>
    <source>
      <id>urn:uuid:source-should-not-overwrite</id>
      <title>Source Title Should Not Overwrite</title>
    </source>
    <updated>2026-01-01T00:00:00Z</updated>
    <author><name>Isaac Asimov</name></author>
    <summary>A classic sci-fi novel.</summary>
    <link rel="http://opds-spec.org/image/thumbnail" href="/opds/book/books/1/cover" type="image/jpeg"/>
    <link rel="http://opds-spec.org/acquisition/buy" href="/buy/1" type="text/html"/>
    <link rel="http://opds-spec.org/acquisition/borrow" href="/borrow/1" type="text/html"/>
    <link rel="http://opds-spec.org/acquisition" href="/opds/book/books/1/download" type="application/epub+zip"/>
  </entry>
  <entry>
    <id>urn:uuid:comic-1</id>
    <title>Folder Manga</title>
    <link rel="http://vaemendis.net/opds-pse/stream" href="/opds/comic/chapters/1/page?page={pageNumber}" type="image/jpeg" pse:count="24"/>
  </entry>
</feed>"#;

    let (feed, is_opensearch) =
        parse_opds_feed("src-1", &base, "application/atom+xml", xml).unwrap();
    assert_eq!(feed.opds_version, "1.2");
    assert!(is_opensearch);
    assert_eq!(feed.title, "Test Books");
    assert_eq!(
        feed.prev_url.as_deref(),
        Some("http://example.com/opds/book/v1.2/all?page=1")
    );
    assert_eq!(
        feed.next_url.as_deref(),
        Some("http://example.com/opds/book/v1.2/all?page=3")
    );
    assert_eq!(feed.nav_links.len(), 1);
    assert_eq!(feed.nav_links[0].title, "All Books");
    assert_eq!(feed.entries.len(), 2);

    let book = &feed.entries[0];
    assert_eq!(book.id, "urn:uuid:book-1");
    assert_eq!(book.title, "Foundation");
    assert_eq!(book.authors, vec!["Isaac Asimov"]);
    assert_eq!(
        book.downloads.len(),
        1,
        "buy/borrow links must be filtered out"
    );
    assert_eq!(book.downloads[0].format, "epub");
    assert!(
        book.cover_url
            .as_deref()
            .unwrap()
            .starts_with("/_app/opds/src-1/proxy-image?url=")
    );

    let comic = &feed.entries[1];
    assert_eq!(comic.title, "Folder Manga");
    assert_eq!(
        comic.pse_stream_url.as_deref(),
        Some("http://example.com/opds/comic/chapters/1/page?page={pageNumber}")
    );
    assert_eq!(comic.page_count, Some(24));
}

#[test]
fn parses_opds2_localized_objects_groups_and_pagination() {
    let base = Url::parse("http://example.com/opds/v2").unwrap();
    let json = r#"{
        "metadata": { "title": { "en": "World Library", "zh": "世界书库" } },
        "links": [
            { "rel": "previous", "href": "/opds/v2?page=1" },
            { "rel": "next", "href": "/opds/v2?page=3" },
            { "rel": "search", "href": "/opds/v2/search{?query}", "templated": true }
        ],
        "groups": [
            {
                "metadata": { "title": { "en": "Featured" }, "numberOfItems": 12 },
                "links": [ { "rel": "self", "href": "/opds/v2/featured" } ],
                "navigation": [ { "title": "Sci-Fi", "href": "/opds/v2/scifi" } ],
                "publications": [
                    {
                        "metadata": {
                            "identifier": "pub-1",
                            "title": { "en": "Dune" },
                            "description": { "en": "Desert planet" },
                            "author": { "name": { "en": "Frank Herbert" } },
                            "numberOfPages": 412
                        },
                        "links": [
                            { "rel": "http://opds-spec.org/acquisition/buy", "href": "/buy/dune", "type": "text/html" },
                            { "rel": "http://opds-spec.org/acquisition", "href": "/dl/dune.epub", "type": "application/epub+zip" }
                        ]
                    }
                ]
            }
        ]
    }"#;

    let (feed, _) = parse_opds_feed("src-2", &base, "application/opds+json", json).unwrap();
    assert_eq!(feed.opds_version, "2.0");
    assert_eq!(feed.title, "世界书库");
    assert_eq!(
        feed.prev_url.as_deref(),
        Some("http://example.com/opds/v2?page=1")
    );
    assert_eq!(
        feed.next_url.as_deref(),
        Some("http://example.com/opds/v2?page=3")
    );
    assert!(
        feed.nav_links
            .iter()
            .any(|n| n.title == "Featured" && n.count == Some(12))
    );
    assert!(
        feed.nav_links
            .iter()
            .any(|n| n.title == "Featured · Sci-Fi")
    );
    assert_eq!(feed.entries.len(), 1);
    assert_eq!(feed.entries[0].title, "Dune");
    assert_eq!(feed.entries[0].authors, vec!["Frank Herbert"]);
    assert_eq!(feed.entries[0].summary.as_deref(), Some("Desert planet"));
    assert_eq!(feed.entries[0].page_count, Some(412));
    assert_eq!(feed.entries[0].downloads.len(), 1);
}

#[test]
fn expands_opensearch_and_opds2_templates() {
    assert_eq!(
        expand_search_template("http://h/search?q={searchTerms}", "sci fi"),
        "http://h/search?q=sci+fi"
    );
    assert_eq!(
        expand_search_template(
            "http://h/search?q={searchTerms?}&page={startPage?}&count={count?}",
            "rust"
        ),
        "http://h/search?q=rust"
    );
    assert_eq!(
        expand_search_template("http://h/v2/search{?query}", "Foundation"),
        "http://h/v2/search?query=Foundation"
    );
    assert_eq!(
        expand_search_template("http://h/v2/search?lang=zh{&query}", "三体"),
        "http://h/v2/search?lang=zh&query=%E4%B8%89%E4%BD%93"
    );
}

#[test]
fn resolves_only_same_origin_http_urls_and_blocks_protocol_relative_bypass() {
    let base = "http://127.0.0.1:8080/opds/book/v1.2";
    assert_eq!(
        resolve_target_url(base, Some("/opds/book/v1.2/all")).unwrap(),
        "http://127.0.0.1:8080/opds/book/v1.2/all"
    );
    assert!(resolve_target_url(base, Some("http://evil.example/steal")).is_err());
    assert!(resolve_target_url(base, Some("https://127.0.0.1:8080/opds")).is_err());
    assert!(resolve_target_url(base, Some("http://127.0.0.1:9999/opds")).is_err());
    assert!(resolve_target_url(base, Some("//evil.example/steal")).is_err());
    assert!(resolve_target_url(base, Some("file:///etc/passwd")).is_err());
    assert!(resolve_target_url(base, Some("javascript:alert(1)")).is_err());
}

#[test]
fn rfc5987_content_disposition_preserves_plus_signs() {
    let hdr = r#"attachment; filename="fallback.epub"; filename*=UTF-8''C%2B%2B+Guide%202026.epub"#;
    assert_eq!(
        parse_filename_from_content_disposition(hdr).as_deref(),
        Some("C+++Guide 2026.epub")
    );
    assert_eq!(percent_decode_utf8("C%2B%2B.epub"), "C++.epub");
}

#[test]
fn raster_image_content_type_whitelist_blocks_svg_and_html() {
    assert!(is_safe_raster_image_content_type("image/jpeg"));
    assert!(is_safe_raster_image_content_type(
        "image/png; charset=binary"
    ));
    assert!(is_safe_raster_image_content_type("image/webp"));
    assert!(is_safe_raster_image_content_type("image/gif"));
    assert!(is_safe_raster_image_content_type("image/avif"));
    assert!(!is_safe_raster_image_content_type("image/svg+xml"));
    assert!(!is_safe_raster_image_content_type("text/html"));
    assert!(!is_safe_raster_image_content_type("application/javascript"));
}
