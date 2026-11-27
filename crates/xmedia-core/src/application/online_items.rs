//! Application service for per-user favorites and reading/playback progress of online
//! (rule-source or remote-server) items, shared by the book, comic, music and video domains.

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use async_trait::async_trait;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::MediaKind;
pub use crate::domain::online_item::OnlineItemView;
use crate::domain::online_item::{
    OnlineItemFields, OnlineItemFilter, OnlineItemProgress, OnlineItemSourceKind, OnlineLibraryItem,
};
use crate::domain::repository::{OnlineItemDeleteScope, OnlineLibraryItemRepository};
use crate::domain::user::UserId;

pub const DEFAULT_LIST_LIMIT: u32 = 50;
pub const MAX_LIST_LIMIT: u32 = 200;
pub const MAX_LOOKUP_ITEMS: usize = 200;
pub const MAX_ITEMS_PER_USER: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnlineSourceInfo {
    pub name: String,
    pub enabled: bool,
}

#[async_trait]
pub trait OnlineSourceDirectory: Send + Sync {
    async fn list_sources(&self, user_id: UserId) -> DomainResult<HashMap<Uuid, OnlineSourceInfo>>;

    async fn find_source(
        &self,
        user_id: UserId,
        id: Uuid,
    ) -> DomainResult<Option<OnlineSourceInfo>> {
        Ok(self.list_sources(user_id).await?.remove(&id))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnlineItemAvailability {
    Ok,
    SourceDisabled,
    SourceMissing,
    Unverified,
}

#[derive(Debug, Clone, Serialize)]
pub struct OnlineItemDto {
    pub id: Uuid,
    pub media_kind: MediaKind,
    pub source_kind: OnlineItemSourceKind,
    pub source_id: String,
    pub source_name: Option<String>,
    pub availability: OnlineItemAvailability,
    pub item_key: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub extra: serde_json::Value,
    pub favorite: bool,
    pub favorited_at: Option<Timestamp>,
    pub progress: Option<OnlineItemProgress>,
    pub progress_ratio: Option<f64>,
    pub progress_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize)]
pub struct OnlineItemListDto {
    pub items: Vec<OnlineItemDto>,
    pub total: usize,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct OnlineItemQuery {
    pub view: OnlineItemView,
    pub source_kind: Option<OnlineItemSourceKind>,
    pub source_id: Option<String>,
    pub q: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OnlineItemRefInput {
    #[serde(default)]
    pub source_kind: OnlineItemSourceKind,
    pub source_id: String,
    pub item_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LookupOnlineItemsInput {
    pub items: Vec<OnlineItemRefInput>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpsertOnlineItemInput {
    #[serde(default)]
    pub source_kind: OnlineItemSourceKind,
    pub source_id: String,
    pub item_key: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub intro: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub extra: Option<serde_json::Value>,
    #[serde(default)]
    pub favorite: Option<bool>,
    #[serde(default)]
    pub progress: Option<OnlineItemProgress>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PatchOnlineItemInput {
    pub favorite: Option<bool>,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub extra: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub enum OnlineItemMutation {
    Metadata(PatchOnlineItemInput),
    Progress(Option<OnlineItemProgress>),
}

pub struct OnlineLibraryService {
    media_kind: MediaKind,
    repo: Arc<dyn OnlineLibraryItemRepository>,
    sources: Arc<dyn OnlineSourceDirectory>,
    upsert_locks: std::sync::Mutex<HashMap<Uuid, Weak<Mutex<()>>>>,
}

impl OnlineLibraryService {
    pub fn new(
        media_kind: MediaKind,
        repo: Arc<dyn OnlineLibraryItemRepository>,
        sources: Arc<dyn OnlineSourceDirectory>,
    ) -> Self {
        Self {
            media_kind,
            repo,
            sources,
            upsert_locks: std::sync::Mutex::new(HashMap::new()),
        }
    }

    fn user_upsert_lock(&self, user_id: UserId) -> Arc<Mutex<()>> {
        let mut guard = self.upsert_locks.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = guard.get(&user_id.0).and_then(Weak::upgrade) {
            return existing;
        }
        if guard.len() > 256 {
            guard.retain(|_, w| w.strong_count() > 0);
        }
        let lock = Arc::new(Mutex::new(()));
        guard.insert(user_id.0, Arc::downgrade(&lock));
        lock
    }

    pub fn media_kind(&self) -> MediaKind {
        self.media_kind
    }

    fn availability(
        item: &OnlineLibraryItem,
        sources: &HashMap<Uuid, OnlineSourceInfo>,
    ) -> (Option<String>, OnlineItemAvailability) {
        if item.source_kind == OnlineItemSourceKind::Remote {
            return (None, OnlineItemAvailability::Unverified);
        }
        let info = Uuid::parse_str(&item.source_id)
            .ok()
            .and_then(|id| sources.get(&id));
        match info {
            Some(s) if s.enabled => (Some(s.name.clone()), OnlineItemAvailability::Ok),
            Some(s) => (Some(s.name.clone()), OnlineItemAvailability::SourceDisabled),
            None => (None, OnlineItemAvailability::SourceMissing),
        }
    }

    fn to_dto(item: OnlineLibraryItem, sources: &HashMap<Uuid, OnlineSourceInfo>) -> OnlineItemDto {
        let (source_name, availability) = Self::availability(&item, sources);
        let progress_ratio = item.progress_ratio();
        OnlineItemDto {
            id: item.id,
            media_kind: item.media_kind,
            source_kind: item.source_kind,
            source_id: item.source_id,
            source_name,
            availability,
            item_key: item.item_key,
            title: item.title,
            subtitle: item.subtitle,
            cover_url: item.cover_url,
            intro: item.intro,
            kind: item.kind,
            extra: item.extra,
            favorite: item.favorite,
            favorited_at: item.favorited_at,
            progress: item.progress,
            progress_ratio,
            progress_at: item.progress_at,
            created_at: item.created_at,
            updated_at: item.updated_at,
        }
    }

    async fn single_dto(
        &self,
        user_id: UserId,
        item: OnlineLibraryItem,
    ) -> DomainResult<OnlineItemDto> {
        let sources = self.sources.list_sources(user_id).await?;
        Ok(Self::to_dto(item, &sources))
    }

    async fn load_owned(&self, user_id: UserId, id: Uuid) -> DomainResult<OnlineLibraryItem> {
        match self.repo.find_by_id(id).await? {
            Some(item) if item.user_id == user_id && item.media_kind == self.media_kind => Ok(item),
            _ => Err(DomainError::NotFound("online item not found".into())),
        }
    }

    pub async fn list(
        &self,
        user_id: UserId,
        query: OnlineItemQuery,
    ) -> DomainResult<OnlineItemListDto> {
        let items = OnlineLibraryItem::filter_and_sort(
            self.repo.list_by_user(user_id, self.media_kind).await?,
            &OnlineItemFilter {
                view: query.view,
                source_kind: query.source_kind,
                source_id: query.source_id,
                query: query.q,
            },
        );

        let total = items.len();
        let limit = query
            .limit
            .unwrap_or(DEFAULT_LIST_LIMIT)
            .clamp(1, MAX_LIST_LIMIT) as usize;
        let offset = query.offset.unwrap_or(0) as usize;
        let sources = self.sources.list_sources(user_id).await?;
        let items = items
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|item| Self::to_dto(item, &sources))
            .collect();
        Ok(OnlineItemListDto { items, total })
    }

    pub async fn get(&self, user_id: UserId, id: Uuid) -> DomainResult<OnlineItemDto> {
        let item = self.load_owned(user_id, id).await?;
        self.single_dto(user_id, item).await
    }

    pub async fn lookup(
        &self,
        user_id: UserId,
        input: LookupOnlineItemsInput,
    ) -> DomainResult<OnlineItemListDto> {
        if input.items.len() > MAX_LOOKUP_ITEMS {
            return Err(DomainError::Validation(format!(
                "lookup accepts at most {MAX_LOOKUP_ITEMS} items"
            )));
        }
        let raw_refs: Vec<(OnlineItemSourceKind, &str, &str)> = input
            .items
            .iter()
            .map(|r| (r.source_kind, r.source_id.as_str(), r.item_key.as_str()))
            .collect();
        if raw_refs.is_empty() {
            return Ok(OnlineItemListDto {
                items: Vec::new(),
                total: 0,
            });
        }
        let all = self.repo.list_by_user(user_id, self.media_kind).await?;
        let sources = self.sources.list_sources(user_id).await?;
        let items: Vec<OnlineItemDto> = OnlineLibraryItem::lookup_matching(all, &raw_refs)
            .into_iter()
            .map(|item| Self::to_dto(item, &sources))
            .collect();
        let total = items.len();
        Ok(OnlineItemListDto { items, total })
    }

    pub async fn upsert(
        &self,
        user_id: UserId,
        input: UpsertOnlineItemInput,
    ) -> DomainResult<OnlineItemDto> {
        let fields = OnlineItemFields {
            title: Some(input.title),
            subtitle: input.subtitle,
            cover_url: input.cover_url,
            intro: input.intro,
            kind: input.kind,
            extra: input.extra,
            favorite: input.favorite,
            progress: input.progress,
        };

        let user_lock = self.user_upsert_lock(user_id);
        let guard = user_lock.lock().await;
        let existing = self.repo.list_by_user(user_id, self.media_kind).await?;
        let (item, verify_source_uuid) = OnlineLibraryItem::upsert_in(
            existing,
            MAX_ITEMS_PER_USER,
            user_id,
            self.media_kind,
            input.source_kind,
            &input.source_id,
            &input.item_key,
            fields,
            Timestamp::now(),
        )?;
        if let Some(src_id) = verify_source_uuid
            && self.sources.find_source(user_id, src_id).await?.is_none()
        {
            return Err(DomainError::NotFound("online source not found".into()));
        }
        self.repo.save(&item).await?;
        drop(guard);
        self.single_dto(user_id, item).await
    }

    pub async fn update(
        &self,
        user_id: UserId,
        id: Uuid,
        mutation: OnlineItemMutation,
    ) -> DomainResult<OnlineItemDto> {
        let mut item = self.load_owned(user_id, id).await?;
        apply_online_mutation(&mut item, mutation, Timestamp::now())?;
        self.repo.save(&item).await?;
        self.single_dto(user_id, item).await
    }

    pub async fn delete(&self, user_id: UserId, id: Uuid) -> DomainResult<()> {
        let item = self.load_owned(user_id, id).await?;
        self.repo
            .delete_scope(OnlineItemDeleteScope::Id(item.id))
            .await
    }
}

fn apply_online_mutation(
    item: &mut OnlineLibraryItem,
    mutation: OnlineItemMutation,
    now: Timestamp,
) -> DomainResult<()> {
    match mutation {
        OnlineItemMutation::Metadata(input) => item.apply_fields(
            OnlineItemFields {
                title: input.title,
                subtitle: input.subtitle,
                cover_url: input.cover_url,
                intro: input.intro,
                kind: input.kind,
                extra: input.extra,
                favorite: input.favorite,
                progress: None,
            },
            now,
        ),
        OnlineItemMutation::Progress(progress) => item.apply_progress(progress, now),
    }
}
