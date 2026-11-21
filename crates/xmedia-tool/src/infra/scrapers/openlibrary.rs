use async_trait::async_trait;
use serde::Deserialize;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};
use crate::infra::scrapers::http_client::build_http_client;

pub struct OpenLibraryProvider;

#[derive(Debug, Deserialize, Default)]
pub(crate) struct OlSearchDoc {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub author_name: Vec<String>,
    #[serde(default)]
    pub first_publish_year: Option<i32>,
    #[serde(default)]
    pub publisher: Vec<String>,
    #[serde(default)]
    pub isbn: Vec<String>,
    #[serde(default)]
    pub language: Vec<String>,
    #[serde(default)]
    pub subject: Vec<String>,
    #[serde(default)]
    pub cover_i: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct OlSearchResponse {
    #[serde(default)]
    pub docs: Vec<OlSearchDoc>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OlDescriptionField {
    Plain(String),
    Typed { value: String },
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct OlWorkDetail {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    description: Option<OlDescriptionField>,
    #[serde(default)]
    pub subjects: Vec<String>,
    #[serde(default)]
    pub covers: Vec<i64>,
}

impl OlWorkDetail {
    pub(crate) fn description_text(&self) -> Option<String> {
        match &self.description {
            Some(OlDescriptionField::Plain(s)) => {
                (!s.trim().is_empty()).then(|| s.trim().to_string())
            }
            Some(OlDescriptionField::Typed { value }) => {
                (!value.trim().is_empty()).then(|| value.trim().to_string())
            }
            None => None,
        }
    }
}

pub(crate) fn map_ol_doc_to_candidate(doc: OlSearchDoc) -> ScrapeCandidate {
    let work_id = doc.key.trim_start_matches("/works/").to_string();
    let cover_url = doc
        .cover_i
        .filter(|&id| id > 0)
        .map(|id| format!("https://covers.openlibrary.org/b/id/{id}-L.jpg"));
    let mut sub = Vec::new();
    if !doc.author_name.is_empty() {
        sub.push(doc.author_name.join(", "));
    }
    if let Some(pub_name) = doc.publisher.first() {
        sub.push(pub_name.clone());
    }

    ScrapeCandidate {
        provider_id: "openlibrary".to_string(),
        provider_name: "OpenLibrary".to_string(),
        external_id: work_id,
        target: ScrapeTargetKind::Book,
        title: doc.title,
        subtitle: (!sub.is_empty()).then(|| sub.join(" · ")),
        year: doc.first_publish_year,
        summary: None,
        cover_url,
        synced_lyrics: None,
        plain_lyrics: None,
    }
}

#[async_trait]
impl ScraperProviderPort for OpenLibraryProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "openlibrary".to_string(),
            name: "OpenLibrary".to_string(),
            description: "互联网档案馆开放图书目录（支持书名、作者、ISBN 与高清封面检索，免密钥）"
                .to_string(),
            website: "https://openlibrary.org".to_string(),
            supported_targets: vec![ScrapeTargetKind::Book],
            requires_api_key: false,
        }
    }

    async fn search(
        &self,
        query: &ScrapeQuery,
        creds: &ProviderCredentials,
    ) -> DomainResult<Vec<ScrapeCandidate>> {
        let client = build_http_client(creds)?;
        let fields =
            "key,title,author_name,first_publish_year,publisher,isbn,language,subject,cover_i";

        let req = if let Some(isbn) = query
            .isbn
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            client.get("https://openlibrary.org/search.json").query(&[
                ("isbn", isbn),
                ("fields", fields),
                ("limit", "12"),
            ])
        } else {
            let mut q = query.keyword.trim().to_string();
            if let Some(author) = query
                .artist_or_author
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                q = format!("{q} {author}");
            }
            client.get("https://openlibrary.org/search.json").query(&[
                ("q", q.as_str()),
                ("fields", fields),
                ("limit", "12"),
            ])
        };

        let resp = req
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("OpenLibrary search failed: {e}")))?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let parsed: OlSearchResponse = resp.json().await.unwrap_or_default();
        Ok(parsed
            .docs
            .into_iter()
            .map(map_ol_doc_to_candidate)
            .collect())
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        _target: ScrapeTargetKind,
        _locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        let client = build_http_client(creds)?;
        let work_id = external_id.trim().trim_start_matches("/works/");
        let work_key = format!("/works/{work_id}");

        let work_url = format!("https://openlibrary.org/works/{work_id}.json");
        let search_fut = client.get("https://openlibrary.org/search.json").query(&[
            ("q", format!("key:{work_key}").as_str()),
            (
                "fields",
                "key,title,author_name,first_publish_year,publisher,isbn,language,subject,cover_i",
            ),
            ("limit", "1"),
        ]);

        let (work_res, search_res) = tokio::join!(client.get(&work_url).send(), search_fut.send());

        let work_detail: OlWorkDetail = match work_res {
            Ok(r) if r.status().is_success() => r.json().await.unwrap_or_default(),
            _ => OlWorkDetail::default(),
        };
        let doc_opt: Option<OlSearchDoc> = match search_res {
            Ok(r) if r.status().is_success() => r
                .json::<OlSearchResponse>()
                .await
                .ok()
                .and_then(|mut s| s.docs.drain(..).next()),
            _ => None,
        };

        if work_detail.title.is_none() && doc_opt.is_none() {
            return Err(DomainError::NotFound(format!(
                "OpenLibrary work '{work_id}' not found"
            )));
        }

        let summary = work_detail.description_text();
        let cover_id = work_detail
            .covers
            .into_iter()
            .find(|&c| c > 0)
            .or_else(|| doc_opt.as_ref().and_then(|d| d.cover_i));
        let cover_url =
            cover_id.map(|id| format!("https://covers.openlibrary.org/b/id/{id}-L.jpg"));

        let mut tags: Vec<String> = work_detail.subjects.into_iter().take(10).collect();
        let (title, creators, year, publisher, isbn, language) = if let Some(d) = doc_opt {
            if tags.is_empty() {
                tags = d.subject.into_iter().take(10).collect();
            }
            let best_isbn = d
                .isbn
                .iter()
                .find(|s| s.len() == 13)
                .or_else(|| d.isbn.first())
                .cloned();
            (
                work_detail.title.or(Some(d.title)),
                d.author_name,
                d.first_publish_year,
                d.publisher.into_iter().next(),
                best_isbn,
                d.language.into_iter().next(),
            )
        } else {
            (work_detail.title, Vec::new(), None, None, None, None)
        };

        Ok(ScrapedMetadata {
            provider_id: "openlibrary".to_string(),
            external_id: work_id.to_string(),
            target: Some(ScrapeTargetKind::Book),
            title,
            creators,
            publisher_or_studio: publisher,
            year,
            published_date: year.map(|y| y.to_string()),
            language,
            isbn,
            summary,
            tags,
            cover_url,
            ..Default::default()
        })
    }
}
