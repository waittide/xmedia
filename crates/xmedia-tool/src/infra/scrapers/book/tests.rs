use crate::application::ports::ScraperProviderPort;
use crate::domain::config::ProviderCredentials;
use crate::domain::scraper::{ScrapeQuery, ScrapeTargetKind};

use super::douban::{DoubanBookProvider, parse_douban_subject_html};
use super::neodb::{NeoDbBookItem, NeoDbProvider, map_neodb_detail};
use super::openlibrary::{OlSearchDoc, map_ol_doc_to_candidate};
use super::xinhua::{XinhuaBookProvider, parse_xinhua_detail_html};

#[test]
fn book_provider_mappers_normalize_openlibrary_douban_neodb_and_xinhua() {
    let ol: OlSearchDoc = serde_json::from_str(
        r#"{
            "key": "/works/OL17365W",
            "title": "The Three-Body Problem",
            "author_name": ["Cixin Liu"],
            "first_publish_year": 2006,
            "publisher": ["Tor Books"],
            "cover_i": 8231990
        }"#,
    )
    .unwrap();
    let ol_cand = map_ol_doc_to_candidate(ol);
    assert_eq!(ol_cand.external_id, "OL17365W");
    assert_eq!(
        ol_cand.cover_url.as_deref(),
        Some("https://covers.openlibrary.org/b/id/8231990-L.jpg")
    );

    let db_html = r#"
        <html>
        <head>
          <script type="application/ld+json">
          {"@type":"Book","name":"三体","author":[{"name":"刘慈欣"}],"isbn":"9787536692930"}
          </script>
        </head>
        <body>
          <a class="nbg" href="https://img3.doubanio.com/view/subject/s/public/s2768378.jpg"></a>
          <div id="info">
            <span class="pl">出版社:</span> 重庆出版社<br/>
            <span class="pl">出版年:</span> 2008-1<br/>
            <span class="pl">丛书:</span> 科幻世界·中国科幻基石丛书<br/>
          </div>
          <strong property="v:average">8.9</strong>
          <div id="link-report"><div class="intro"><p>文化大革命如火如荼进行的同时。</p></div></div>
          <script>var criteria = '7:科幻|7:刘慈欣';</script>
        </body>
        </html>
    "#;
    let db_meta = parse_douban_subject_html("2567698", db_html, ScrapeTargetKind::Book).unwrap();
    assert_eq!(db_meta.title.as_deref(), Some("三体"));
    assert_eq!(db_meta.isbn.as_deref(), Some("9787536692930"));
    assert_eq!(db_meta.year, Some(2008));
    assert_eq!(db_meta.published_date.as_deref(), Some("2008-01-01"));
    assert_eq!(
        db_meta.cover_url.as_deref(),
        Some("https://img3.doubanio.com/view/subject/l/public/s2768378.jpg")
    );
    assert!(db_meta.tags.contains(&"科幻".to_string()));

    let neo_meta = map_neodb_detail(
        NeoDbBookItem {
            uuid: "7OGqnJkxiQZImAxD9aAgMr".into(),
            title: "三体".into(),
            subtitle: Some(serde_json::json!("地球往事")),
            author: serde_json::json!(["刘慈欣"]),
            pub_house: Some(serde_json::json!("重庆出版社")),
            pub_year: Some(2008),
            pub_month: Some(1),
            isbn: Some(serde_json::json!("9787536692930")),
            rating: Some(8.6),
            ..Default::default()
        },
        ScrapeTargetKind::Book,
    );
    assert_eq!(neo_meta.title.as_deref(), Some("三体：地球往事"));
    assert_eq!(neo_meta.published_date.as_deref(), Some("2008-01-01"));

    let xh_html = r#"
        <div class="item-title" data-item="{&quot;name&quot;:&quot;三体&quot;,&quot;mainImage&quot;:&quot;//img1.xinhuashudian.com/book.jpg&quot;,&quot;extra&quot;:{&quot;author&quot;:&quot;刘慈欣&quot;}}"></div>
        <li class="attribute-li"><span class="attr-label">商品编码（ISBN）</span><span class="attr-val">9787536692930</span></li>
        <li class="attribute-li"><span class="attr-label">出版社</span><span class="attr-val">重庆出版社</span></li>
        <li class="attribute-li"><span class="attr-label">中图分类号</span><span class="attr-val">I247.5</span></li>
    "#;
    let xh_meta = parse_xinhua_detail_html("1001", xh_html).unwrap();
    assert_eq!(xh_meta.isbn.as_deref(), Some("9787536692930"));
    assert!(xh_meta.genres.contains(&"中国当代小说".to_string()));
}

#[tokio::test]
#[ignore = "live network smoke check across Douban, NeoDB, and Xinhua"]
async fn live_smoke_check_published_book_providers() {
    let creds = ProviderCredentials::default();

    let neo = NeoDbProvider;
    let neo_cands = neo
        .search(
            &ScrapeQuery {
                keyword: "三体".into(),
                isbn: Some("9787536692930".into()),
                target: Some(ScrapeTargetKind::Book),
                ..Default::default()
            },
            &creds,
        )
        .await
        .unwrap();
    assert!(!neo_cands.is_empty());

    let db = DoubanBookProvider;
    let db_cands = db
        .search(
            &ScrapeQuery {
                keyword: "三体".into(),
                target: Some(ScrapeTargetKind::Book),
                ..Default::default()
            },
            &creds,
        )
        .await
        .unwrap();
    assert!(!db_cands.is_empty());

    let xh = XinhuaBookProvider;
    let xh_cands = xh
        .search(
            &ScrapeQuery {
                keyword: "三体".into(),
                isbn: Some("9787536692930".into()),
                target: Some(ScrapeTargetKind::Book),
                ..Default::default()
            },
            &creds,
        )
        .await
        .unwrap();
    assert!(!xh_cands.is_empty());
}
