use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;
use xmedia_core::application::online_items::{OnlineSourceDirectory, OnlineSourceInfo};
use xmedia_core::domain::error::DomainResult;

use crate::domain::repository::MusicSourceRepository;

/// Exposes music rule sources to the shared online library item service.
pub struct MusicOnlineSourceDirectory {
    sources: Arc<dyn MusicSourceRepository>,
}

impl MusicOnlineSourceDirectory {
    pub fn new(sources: Arc<dyn MusicSourceRepository>) -> Self {
        Self { sources }
    }
}

#[async_trait]
impl OnlineSourceDirectory for MusicOnlineSourceDirectory {
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
