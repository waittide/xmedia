use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;
use xmedia_core::application::online_items::{OnlineSourceDirectory, OnlineSourceInfo};
use xmedia_core::domain::error::DomainResult;

use crate::domain::repository::ComicSourceRepository;

/// Exposes comic rule sources to the shared online library item service.
pub struct ComicOnlineSourceDirectory {
    sources: Arc<dyn ComicSourceRepository>,
}

impl ComicOnlineSourceDirectory {
    pub fn new(sources: Arc<dyn ComicSourceRepository>) -> Self {
        Self { sources }
    }
}

#[async_trait]
impl OnlineSourceDirectory for ComicOnlineSourceDirectory {
    async fn list_sources(&self) -> DomainResult<HashMap<Uuid, OnlineSourceInfo>> {
        Ok(self
            .sources
            .list_all()
            .await?
            .into_iter()
            .map(|s| {
                (
                    s.id,
                    OnlineSourceInfo {
                        name: s.name,
                        enabled: s.enabled,
                    },
                )
            })
            .collect())
    }

    async fn find_source(&self, id: Uuid) -> DomainResult<Option<OnlineSourceInfo>> {
        Ok(self
            .sources
            .find_by_id(id)
            .await?
            .map(|s| OnlineSourceInfo {
                name: s.name,
                enabled: s.enabled,
            }))
    }
}
