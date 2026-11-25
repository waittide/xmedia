use axum::body::to_bytes;
use axum::http::{StatusCode, header};
use jiff::Timestamp;

use super::OpdsVersion;
use crate::application::opds::{OpdsChapterEntry, OpdsFeed, OpdsFeedPayload, OpdsNavItem};
use crate::domain::id::ChapterId;

#[tokio::test]
async fn render_atom_and_opds2_navigation_feed_escapes_xml_and_preserves_schema() {
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let feed = OpdsFeed {
        id: "urn:xmedia:comic:opds:root".into(),
        title: "Tom & Jerry <Comics> \"Special\" 'Ed'".into(),
        self_path: "/opds/comic/v1.2".into(),
        updated_at: now,
        payload: OpdsFeedPayload::Navigation(vec![OpdsNavItem {
            id: "urn:xmedia:comic:series:1".into(),
            title: "A & B <Vol.1>".into(),
            summary: "3 chapters & extras".into(),
            subsection_path: "/opds/comic/v1.2/series/1".into(),
            cover_url: Some("/opds/comic/covers/abc.png".into()),
            updated_at: now,
        }]),
    };

    let atom_res = OpdsVersion::V12.render("/opds/comic", &feed);
    assert_eq!(atom_res.status(), StatusCode::OK);
    assert_eq!(
        atom_res.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/atom+xml;profile=opds-catalog;kind=navigation; charset=utf-8"
    );
    let xml = String::from_utf8(
        to_bytes(atom_res.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(xml.contains("Tom &amp; Jerry &lt;Comics&gt; &quot;Special&quot; &apos;Ed&apos;"));
    assert!(xml.contains("<title>A &amp; B &lt;Vol.1&gt;</title>"));
    assert!(xml.contains("kind=acquisition"));
    assert!(xml.contains("href=\"/opds/comic/covers/abc.png\""));

    let v2_feed = OpdsFeed {
        self_path: "/opds/comic/v2.0".into(),
        ..feed
    };
    let json_res = OpdsVersion::V20.render("/opds/comic", &v2_feed);
    assert_eq!(json_res.status(), StatusCode::OK);
    assert_eq!(
        json_res.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/opds+json; charset=utf-8"
    );
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(json_res.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(
        body["metadata"]["title"],
        "Tom & Jerry <Comics> \"Special\" 'Ed'"
    );
    assert_eq!(body["navigation"][0]["title"], "A & B <Vol.1>");
    assert!(body.get("publications").is_none());
}

#[tokio::test]
async fn render_atom_and_opds2_acquisition_feed_includes_pse_and_download_links() {
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let chapter_id = ChapterId::new();
    let feed = OpdsFeed {
        id: "urn:xmedia:comic:series:test".into(),
        title: "Witch Hat".into(),
        self_path: "/opds/comic/v1.2/series/test".into(),
        updated_at: now,
        payload: OpdsFeedPayload::Acquisition(vec![OpdsChapterEntry {
            id: chapter_id,
            series_title: "Witch & Hat".into(),
            title: "#1 - <Start>".into(),
            summary: "Intro & pilot".into(),
            authors: vec!["Author <A>".into()],
            cover_url: Some("/opds/comic/covers/hash.jpg".into()),
            page_count: 24,
            last_read_page: Some(7),
            last_read_at: Some(now),
            can_download_archive: true,
            archive_mime: "application/vnd.comicbook+zip",
            updated_at: now,
        }]),
    };

    let atom_res = OpdsVersion::V12.render("/opds/comic\"&<>", &feed);
    assert_eq!(
        atom_res.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/atom+xml;profile=opds-catalog;kind=acquisition; charset=utf-8"
    );
    let xml = String::from_utf8(
        to_bytes(atom_res.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(xml.contains("Witch &amp; Hat - #1 - &lt;Start&gt;"));
    assert!(xml.contains("<author><name>Author &lt;A&gt;</name></author>"));
    assert!(xml.contains("pse:count=\"24\" pse:lastRead=\"6\""));
    assert!(xml.contains("href=\"/opds/comic&quot;&amp;&lt;&gt;/v1.2\""));
    assert!(xml.contains("href=\"/opds/comic&quot;&amp;&lt;&gt;/v1.2/opensearch.xml\""));
    assert!(xml.contains(&format!(
        "/opds/comic&quot;&amp;&lt;&gt;/v1.2/chapters/{chapter_id}/download"
    )));
    assert!(xml.contains(&format!(
        "/opds/comic&quot;&amp;&lt;&gt;/v1.2/chapters/{chapter_id}/pse/{{pageNumber}}"
    )));

    let json_res = OpdsVersion::V20.render("/opds/comic", &feed);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(json_res.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(body.get("navigation").is_none());
    let pub0 = &body["publications"][0];
    assert_eq!(pub0["metadata"]["numberOfPages"], 24);
    assert_eq!(pub0["links"][0]["properties"]["pse:lastRead"], 6);
    assert_eq!(pub0["links"][1]["rel"], "http://opds-spec.org/acquisition");
    assert_eq!(pub0["images"][0]["href"], "/opds/comic/covers/hash.jpg");
}
