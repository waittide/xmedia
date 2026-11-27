use std::collections::BTreeSet;
use std::sync::LazyLock;

use async_trait::async_trait;
use regex::Regex;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::application::ports::ScraperProviderPort;
use crate::domain::config::ProviderCredentials;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
    score_candidate_relevance,
};
use crate::infra::scrapers::html_util::{
    build_browser_http_client, normalize_cover_url, parse_year_and_date, read_response_text,
    strip_html_to_text, strip_inline_html,
};

pub struct Hanime1Provider;

const HANIME1_WEB_BASE: &str = "https://hanime1.me";

static RE_WATCH_ID_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)hanime1\.me/watch\?[^#\s]*\bv=(\d{1,10})").unwrap());
static RE_ANCHOR_WATCH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<a[^>]+href="(?:https?://hanime1\.me)?/watch\?v=(\d{1,10})[^"]*"[^>]*>(.*?)</a>"#,
    )
    .unwrap()
});
static RE_CARD_TITLE_DIV: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<div[^>]+class="[^"]*card-mobile-title[^"]*"[^>]*>(.*?)</div>"#).unwrap()
});
static RE_ATTR_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)\btitle="([^"]{2,300})""#).unwrap());
static RE_IMG_SRC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<img[^>]+(?:data-src|src)="(https?://[^"]+)"[^>]*>"#).unwrap()
});
static RE_IMG_ALT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<img[^>]+alt="([^"]{2,300})""#).unwrap());
static RE_META_PROP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<meta[^>]+(?:property|name)="([^"]+)"[^>]+content="([^"]*)"[^>]*>"#).unwrap()
});
static RE_META_PROP_REV: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<meta[^>]+content="([^"]*)"[^>]+(?:property|name)="([^"]+)"[^>]*>"#).unwrap()
});
static RE_SHARE_TITLE_H3: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<h3[^>]+id="shareBtn-title"[^>]*>(.*?)</h3>"#).unwrap());
static RE_H1_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<h1[^>]*>(.*?)</h1>"#).unwrap());
static RE_HTML_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<title[^>]*>(.*?)</title>"#).unwrap());
static RE_ARTIST_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<a[^>]+id="video-artist-name"[^>]*>(.*?)</a>"#).unwrap());
static RE_BRAND_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<a[^>]+href="[^"]*[?&]brand=[^"]*"[^>]*>(.*?)</a>"#).unwrap()
});
static RE_GENRE_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<a[^>]+href="[^"]*[?&]genre=[^"]*"[^>]*>(.*?)</a>"#).unwrap()
});
static RE_TAG_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<a[^>]+href="[^"]*[?&]tags(?:\[\]|%5B%5D)=[^"]*"[^>]*>(.*?)</a>"#).unwrap()
});
static RE_CAPTION_DIV: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<div[^>]+class="[^"]*video-caption-text[^"]*"[^>]*>(.*?)</div>"#).unwrap()
});
static RE_VIDEO_POSTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<video[^>]+poster="([^"]+)""#).unwrap());
static RE_ISO_DATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b((?:19|20)\d{2}[-/]\d{1,2}[-/]\d{1,2})\b").unwrap());
static RE_EPISODE_NUM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:第\s*0*(\d{1,3})\s*[集話话]|(?:\bEP?|\s#|\[)\s*0*(\d{1,3})(?:\]|\b))")
        .unwrap()
});

pub(crate) fn extract_hanime1_video_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().all(|c| c.is_ascii_digit()) && (2..=10).contains(&trimmed.len()) {
        return Some(trimmed.to_string());
    }
    if let Some(id) = RE_WATCH_ID_URL
        .captures(trimmed)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
    {
        return Some(id);
    }
    None
}

fn clean_hanime1_title(raw: &str) -> String {
    let mut t = strip_inline_html(raw);
    for suffix in [
        " - H動漫/裏番/線上看 - Hanime1.me",
        " - H动漫/里番/在线看 - Hanime1.me",
        " - Hanime1.me",
        " | Hanime1.me",
    ] {
        if let Some(stripped) = t.strip_suffix(suffix) {
            t = stripped.trim().to_string();
        }
    }
    t
}

fn is_valid_thumbnail(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    !lower.contains("avatar")
        && !lower.contains("favicon")
        && !lower.contains("logo")
        && !lower.contains("banner")
        && !lower.ends_with(".svg")
}

fn extract_meta_tag(html: &str, key: &str) -> Option<String> {
    for cap in RE_META_PROP.captures_iter(html) {
        if cap[1].eq_ignore_ascii_case(key) {
            let val = strip_inline_html(&cap[2]);
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    for cap in RE_META_PROP_REV.captures_iter(html) {
        if cap[2].eq_ignore_ascii_case(key) {
            let val = strip_inline_html(&cap[1]);
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

pub(crate) fn parse_hanime1_search_html(
    html: &str,
    target: ScrapeTargetKind,
) -> Vec<ScrapeCandidate> {
    let mut seen_ids = BTreeSet::new();
    let mut out = Vec::new();

    for cap in RE_ANCHOR_WATCH.captures_iter(html) {
        let vid = cap[1].trim().to_string();
        if vid.is_empty() || !seen_ids.insert(vid.clone()) {
            continue;
        }
        let whole = cap.get(0).map(|m| m.as_str()).unwrap_or_default();
        let inner = &cap[2];

        let title = RE_CARD_TITLE_DIV
            .captures(inner)
            .map(|c| clean_hanime1_title(&c[1]))
            .filter(|s| !s.is_empty())
            .or_else(|| {
                RE_ATTR_TITLE
                    .captures(whole)
                    .map(|c| clean_hanime1_title(&c[1]))
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                RE_IMG_ALT
                    .captures(inner)
                    .map(|c| clean_hanime1_title(&c[1]))
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                let plain = clean_hanime1_title(inner);
                (!plain.is_empty()).then_some(plain)
            });

        let Some(title) = title else {
            continue;
        };

        let cover_url = RE_IMG_SRC
            .captures_iter(inner)
            .filter_map(|c| normalize_cover_url(&c[1]))
            .find(|u| is_valid_thumbnail(u));

        out.push(ScrapeCandidate {
            provider_id: "hanime1".into(),
            provider_name: "Hanime1".into(),
            external_id: vid,
            target,
            title,
            subtitle: None,
            year: None,
            summary: None,
            cover_url,
            synced_lyrics: None,
            plain_lyrics: None,
        });
    }

    out
}

pub(crate) fn parse_hanime1_watch_html(
    video_id: &str,
    html: &str,
    target: ScrapeTargetKind,
) -> Option<ScrapedMetadata> {
    let title = RE_SHARE_TITLE_H3
        .captures(html)
        .map(|c| clean_hanime1_title(&c[1]))
        .filter(|s| !s.is_empty())
        .or_else(|| {
            extract_meta_tag(html, "og:title")
                .map(|s| clean_hanime1_title(&s))
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            RE_H1_TITLE
                .captures(html)
                .map(|c| clean_hanime1_title(&c[1]))
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            RE_HTML_TITLE
                .captures(html)
                .map(|c| clean_hanime1_title(&c[1]))
                .filter(|s| !s.is_empty())
        })?;

    let cover_url = extract_meta_tag(html, "og:image")
        .and_then(|u| normalize_cover_url(&u))
        .or_else(|| {
            RE_VIDEO_POSTER
                .captures(html)
                .and_then(|c| normalize_cover_url(&c[1]))
        });

    let studio = RE_ARTIST_LINK
        .captures(html)
        .map(|c| strip_inline_html(&c[1]))
        .filter(|s| !s.is_empty())
        .or_else(|| {
            RE_BRAND_LINK
                .captures(html)
                .map(|c| strip_inline_html(&c[1]))
                .filter(|s| !s.is_empty())
        });

    let caption_summary = RE_CAPTION_DIV
        .captures(html)
        .map(|c| strip_html_to_text(&c[1]))
        .filter(|s| !s.is_empty());
    let og_desc =
        extract_meta_tag(html, "og:description").or_else(|| extract_meta_tag(html, "description"));

    let summary = match (caption_summary, og_desc) {
        (Some(cap), Some(og)) => {
            if cap.len() >= og.len() {
                Some(cap)
            } else {
                Some(og)
            }
        }
        (Some(cap), None) => Some(cap),
        (None, Some(og)) => Some(og),
        (None, None) => None,
    };

    let raw_date = extract_meta_tag(html, "video:release_date")
        .or_else(|| extract_meta_tag(html, "article:published_time"))
        .or_else(|| RE_ISO_DATE.captures(html).map(|c| c[1].to_string()));
    let (year, published_date) = raw_date
        .as_deref()
        .map(parse_year_and_date)
        .unwrap_or((None, None));

    let mut genres = Vec::new();
    let mut seen_genres = BTreeSet::new();
    for cap in RE_GENRE_LINK.captures_iter(html) {
        let g = strip_inline_html(&cap[1]);
        if !g.is_empty() && seen_genres.insert(g.to_ascii_lowercase()) {
            genres.push(g);
        }
    }

    let mut tags = Vec::new();
    let mut seen_tags = BTreeSet::new();
    for cap in RE_TAG_LINK.captures_iter(html) {
        let raw_t = strip_inline_html(&cap[1]);
        let cleaned = raw_t.trim_start_matches('#').trim().to_string();
        if !cleaned.is_empty() && seen_tags.insert(cleaned.to_ascii_lowercase()) {
            tags.push(cleaned);
        }
    }

    let episode_number = if target == ScrapeTargetKind::TvShow {
        RE_EPISODE_NUM.captures(&title).and_then(|c| {
            c.get(1)
                .or_else(|| c.get(2))
                .and_then(|m| m.as_str().parse::<u32>().ok())
        })
    } else {
        None
    };

    let mut external_ids = std::collections::BTreeMap::new();
    external_ids.insert("hanime1".to_string(), video_id.to_string());

    Some(ScrapedMetadata {
        provider_id: "hanime1".into(),
        external_id: video_id.to_string(),
        target: Some(target),
        title: Some(title),
        publisher_or_studio: studio,
        year,
        published_date,
        track_or_episode_number: episode_number,
        summary,
        genres,
        tags,
        cover_url,
        external_ids,
        ..Default::default()
    })
}

fn check_hanime1_response(status: reqwest::StatusCode, body: &str) -> DomainResult<()> {
    let lower = body.to_ascii_lowercase();
    let looks_cf = status.as_u16() == 403
        || status.as_u16() == 503
        || lower.contains("just a moment")
        || lower.contains("cf-browser-verification")
        || lower.contains("challenge-platform");
    if !status.is_success() {
        if looks_cf {
            return Err(DomainError::Storage(format!(
                "Hanime1 触发了 Cloudflare 访问限制 (HTTP {})，请检查设置中的 HTTP 代理出口后重试",
                status.as_u16()
            )));
        }
        return Err(DomainError::Storage(format!(
            "Hanime1 返回 HTTP {}",
            status.as_u16()
        )));
    }
    if lower.contains("just a moment") && lower.contains("cloudflare") {
        return Err(DomainError::Storage(
            "Hanime1 返回了 Cloudflare 人机校验页，请更换可用代理出口后重试".into(),
        ));
    }
    Ok(())
}

async fn fetch_watch_page(
    video_id: &str,
    target: ScrapeTargetKind,
    creds: &ProviderCredentials,
) -> DomainResult<ScrapedMetadata> {
    let client = build_browser_http_client(creds, false)?;
    let url = format!("{HANIME1_WEB_BASE}/watch?v={video_id}");
    let resp = client
        .get(&url)
        .header(reqwest::header::REFERER, format!("{HANIME1_WEB_BASE}/"))
        .header(
            reqwest::header::ACCEPT,
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .send()
        .await
        .map_err(|e| DomainError::Storage(format!("请求 Hanime1 视频 #{video_id} 失败: {e}")))?;

    let status = resp.status();
    let html = read_response_text(resp, encoding_rs::UTF_8).await?;
    check_hanime1_response(status, &html)?;

    parse_hanime1_watch_html(video_id, &html, target).ok_or_else(|| {
        DomainError::NotFound(format!("未能从 Hanime1 页面解析视频 #{video_id} 元数据"))
    })
}

#[async_trait]
impl ScraperProviderPort for Hanime1Provider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "hanime1".into(),
            name: "Hanime1".into(),
            description:
                "成人向动漫/里番元数据、社团与封面（支持关键词或视频 ID，需开启 R18 刮削源）".into(),
            website: HANIME1_WEB_BASE.into(),
            supported_targets: vec![ScrapeTargetKind::Movie, ScrapeTargetKind::TvShow],
            requires_api_key: false,
        }
    }

    fn is_configured(&self, creds: &ProviderCredentials) -> bool {
        creds.adult_scrapers_enabled()
    }

    async fn search(
        &self,
        query: &ScrapeQuery,
        creds: &ProviderCredentials,
    ) -> DomainResult<Vec<ScrapeCandidate>> {
        if !creds.adult_scrapers_enabled() {
            return Ok(Vec::new());
        }
        let target = query.target.unwrap_or(ScrapeTargetKind::Movie);
        if !matches!(target, ScrapeTargetKind::Movie | ScrapeTargetKind::TvShow) {
            return Ok(Vec::new());
        }
        let kw = query.keyword.trim();
        if kw.is_empty() {
            return Ok(Vec::new());
        }

        if let Some(vid) = extract_hanime1_video_id(kw)
            && let Ok(meta) = fetch_watch_page(&vid, target, creds).await
        {
            return Ok(vec![ScrapeCandidate {
                provider_id: "hanime1".into(),
                provider_name: "Hanime1".into(),
                external_id: vid,
                target,
                title: meta.title.unwrap_or_else(|| kw.to_string()),
                subtitle: meta.publisher_or_studio,
                year: meta.year,
                summary: meta.summary,
                cover_url: meta.cover_url,
                synced_lyrics: None,
                plain_lyrics: None,
            }]);
        }

        let client = build_browser_http_client(creds, false)?;
        let resp = client
            .get(format!("{HANIME1_WEB_BASE}/search"))
            .query(&[("query", kw)])
            .header(reqwest::header::REFERER, format!("{HANIME1_WEB_BASE}/"))
            .header(
                reqwest::header::ACCEPT,
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("请求 Hanime1 搜索失败: {e}")))?;

        let status = resp.status();
        let html = read_response_text(resp, encoding_rs::UTF_8).await?;
        check_hanime1_response(status, &html)?;

        let mut scored: Vec<(i32, ScrapeCandidate)> = parse_hanime1_search_html(&html, target)
            .into_iter()
            .take(20)
            .map(|cand| {
                let score = score_candidate_relevance(
                    kw,
                    query.year,
                    &cand.title,
                    cand.subtitle.as_deref(),
                    cand.year,
                    0,
                );
                (score, cand)
            })
            .collect();

        scored.sort_by_key(|a| std::cmp::Reverse(a.0));
        Ok(scored.into_iter().take(12).map(|(_, c)| c).collect())
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        target: ScrapeTargetKind,
        _locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        if !creds.adult_scrapers_enabled() {
            return Err(DomainError::Validation(
                "尚未开启 R18 / 成人向刮削源，请在设置页启用后重试".into(),
            ));
        }
        let vid = extract_hanime1_video_id(external_id).ok_or_else(|| {
            DomainError::Validation(format!("无效的 Hanime1 视频 ID: '{external_id}'"))
        })?;
        fetch_watch_page(&vid, target, creds).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_hanime1_video_ids_from_digits_and_urls() {
        assert_eq!(extract_hanime1_video_id("98765").as_deref(), Some("98765"));
        assert_eq!(
            extract_hanime1_video_id("https://hanime1.me/watch?v=98765").as_deref(),
            Some("98765")
        );
        assert_eq!(extract_hanime1_video_id("样本动画"), None);
    }

    #[test]
    fn parses_search_results_and_deduplicates_watch_links() {
        let html = r#"
        <div class="search-doujin-videos">
          <a href="https://hanime1.me/watch?v=10240" title="測試作品 第1集">
            <img src="https://vdownload.hembed.com/image/thumbnail/10240.jpg" alt="測試作品 第1集" />
            <div class="card-mobile-title">測試作品 第1集</div>
          </a>
          <a href="/watch?v=10240">重複連結</a>
          <a href="/watch?v=10241">
            <img data-src="https://vdownload.hembed.com/image/thumbnail/10241.jpg" />
            <div class="card-mobile-title">測試作品 第2集</div>
          </a>
        </div>
        "#;
        let items = parse_hanime1_search_html(html, ScrapeTargetKind::TvShow);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].external_id, "10240");
        assert_eq!(items[0].title, "測試作品 第1集");
        assert_eq!(
            items[0].cover_url.as_deref(),
            Some("https://vdownload.hembed.com/image/thumbnail/10240.jpg")
        );
        assert_eq!(items[1].external_id, "10241");
        assert_eq!(items[1].title, "測試作品 第2集");
    }

    #[test]
    fn parses_watch_page_metadata_cleanly() {
        let html = r#"
        <html>
        <head>
          <title>測試作品 第2集 - H動漫/裏番/線上看 - Hanime1.me</title>
          <meta property="og:title" content="測試作品 第2集 - Hanime1.me" />
          <meta property="og:image" content="https://vdownload.hembed.com/image/thumbnail/10241l.jpg" />
          <meta property="og:description" content="短簡介" />
          <meta property="video:release_date" content="2024-06-15" />
        </head>
        <body>
          <h3 id="shareBtn-title">測試作品 第2集</h3>
          <a id="video-artist-name" href="/search?brand=Pink+Pineapple">Pink Pineapple</a>
          <div class="video-caption-text caption-ellipsis">這是一段完整的動畫作品劇情介紹內容。</div>
          <a href="/search?genre=裏番">裏番</a>
          <a href="/search?tags[]=純愛">#純愛</a>
          <a href="/search?tags%5B%5D=中文字幕">#中文字幕</a>
        </body>
        </html>
        "#;
        let meta = parse_hanime1_watch_html("10241", html, ScrapeTargetKind::TvShow).unwrap();
        assert_eq!(meta.provider_id, "hanime1");
        assert_eq!(meta.external_id, "10241");
        assert_eq!(meta.title.as_deref(), Some("測試作品 第2集"));
        assert_eq!(meta.publisher_or_studio.as_deref(), Some("Pink Pineapple"));
        assert_eq!(meta.year, Some(2024));
        assert_eq!(meta.published_date.as_deref(), Some("2024-06-15"));
        assert_eq!(meta.track_or_episode_number, Some(2));
        assert_eq!(
            meta.summary.as_deref(),
            Some("這是一段完整的動畫作品劇情介紹內容。")
        );
        assert_eq!(meta.genres, vec!["裏番"]);
        assert_eq!(meta.tags, vec!["純愛", "中文字幕"]);
        assert_eq!(
            meta.cover_url.as_deref(),
            Some("https://vdownload.hembed.com/image/thumbnail/10241l.jpg")
        );
    }
}
