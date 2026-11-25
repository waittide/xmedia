use std::collections::HashMap;

use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::user::User;

use crate::application::ports::{HlsProxyOutcome, ProxyStreamOutcome, VideoProxyUrlBuilder};

use super::VideoOnlineService;

impl VideoOnlineService {
    pub async fn fetch_poster_proxy(
        &self,
        actor: &User,
        raw_url: &str,
        source_id: Option<Uuid>,
    ) -> DomainResult<(Vec<u8>, &'static str)> {
        let headers = match source_id {
            Some(sid) => {
                self.resolve_source_media_headers(actor, sid, raw_url)
                    .await?
            }
            None => HashMap::new(),
        };
        self.transport.fetch_poster_bytes(raw_url, &headers).await
    }

    pub async fn open_subtitle_proxy(
        &self,
        actor: &User,
        raw_url: &str,
        source_id: Uuid,
    ) -> DomainResult<String> {
        let headers = self
            .resolve_source_media_headers_with_cookies(actor, source_id, raw_url.trim())
            .await?;
        self.transport.fetch_subtitle_vtt(raw_url, &headers).await
    }

    pub async fn open_stream_proxy(
        &self,
        actor: &User,
        raw_url: &str,
        source_id: Uuid,
        range_header: Option<&str>,
    ) -> DomainResult<ProxyStreamOutcome> {
        let source_headers = self
            .resolve_source_media_headers(actor, source_id, raw_url)
            .await?;
        self.transport
            .open_stream_proxy(raw_url, source_headers, range_header)
            .await
    }

    pub async fn open_stream_proxy_for_tracked(
        &self,
        raw_url: &str,
        source_id: Uuid,
        range_header: Option<&str>,
    ) -> DomainResult<ProxyStreamOutcome> {
        let source_headers = self
            .resolve_internal_media_headers(Some(source_id), raw_url)
            .await;
        self.transport
            .open_stream_proxy(raw_url, source_headers, range_header)
            .await
    }

    pub async fn open_hls_proxy(
        &self,
        actor: &User,
        raw_url: &str,
        source_id: Uuid,
        range_header: Option<&str>,
        token: Option<&str>,
        url_builder: &dyn VideoProxyUrlBuilder,
    ) -> DomainResult<HlsProxyOutcome> {
        let source_headers = self
            .resolve_source_media_headers(actor, source_id, raw_url)
            .await?;
        self.transport
            .open_hls_proxy(raw_url, source_headers, range_header, &|resolved| {
                url_builder.hls_proxy_url(resolved, source_id, token)
            })
            .await
    }

    pub async fn open_hls_proxy_for_tracked<F>(
        &self,
        actor: &User,
        raw_url: &str,
        source_id: Uuid,
        range_header: Option<&str>,
        rewrite_uri: F,
    ) -> DomainResult<HlsProxyOutcome>
    where
        F: Fn(&str) -> String + Send + Sync,
    {
        let source_headers = self
            .resolve_source_media_headers_with_cookies(actor, source_id, raw_url)
            .await?;
        self.transport
            .open_hls_proxy(raw_url, source_headers, range_header, &rewrite_uri)
            .await
    }
}
