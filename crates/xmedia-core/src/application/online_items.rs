//! Application service for per-user favorites and reading/playback progress of online
//! (rule-source or remote-server) items, shared by the comic, music and video domains.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use async_trait::async_trait;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::MediaKind;
use crate::domain::online_item::{
    MAX_INTRO_CHARS, MAX_ITEM_KEY_CHARS, MAX_SHORT_TEXT_CHARS, MAX_SOURCE_ID_CHARS,
    OnlineItemProgress, OnlineItemSourceKind, OnlineLibraryItem, normalize_opt_text,
    validate_cover_url, validate_extra, validate_key, validate_title,
};
use crate::domain::repository::OnlineLibraryItemRepository;
use crate::domain::user::UserId;

pub const DEFAULT_LIST_LIMIT: u32 = 50;
pub const MAX_LIST_LIMIT: u32 = 200;
pub const MAX_LOOKUP_ITEMS: usize = 200;
pub const MAX_ITEMS_PER_USER: usize = 10_000;

/// Display metadata of a server-managed online source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnlineSourceInfo {
    pub name: String,
    pub enabled: bool,
}

/// Read-only view onto a domain's rule-source registry.
#[async_trait]
pub trait OnlineSourceDirectory: Send + Sync {
    async fn list_sources(&self) -> DomainResult<HashMap<Uuid, OnlineSourceInfo>>;

    async fn find_source(&self, id: Uuid) -> DomainResult<Option<OnlineSourceInfo>> {
        Ok(self.list_sources().await?.remove(&id))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnlineItemAvailability {
    Ok,
    SourceDisabled,
    SourceMissing,
    /// Remote (client-side) sources cannot be verified by the server.
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnlineItemView {
    #[default]
    All,
    Continue,
    Favorites,
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

/// Validated natural key of an online item.
struct ItemKey {
    source_kind: OnlineItemSourceKind,
    source_id: String,
    source_uuid: Option<Uuid>,
    item_key: String,
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

    fn parse_key(
        source_kind: OnlineItemSourceKind,
        source_id: &str,
        item_key: &str,
    ) -> DomainResult<ItemKey> {
        let source_id = validate_key(source_id, "source_id", MAX_SOURCE_ID_CHARS)?;
        let item_key = validate_key(item_key, "item_key", MAX_ITEM_KEY_CHARS)?;
        let (source_id, source_uuid) = match source_kind {
            OnlineItemSourceKind::Online => {
                let id = Uuid::parse_str(&source_id).map_err(|_| {
                    DomainError::Validation("source_id must be a UUID for online sources".into())
                })?;
                (id.to_string(), Some(id))
            }
            OnlineItemSourceKind::Remote => (source_id, None),
        };
        Ok(ItemKey {
            source_kind,
            source_id,
            source_uuid,
            item_key,
        })
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
        let progress_ratio = item.progress.as_ref().and_then(OnlineItemProgress::ratio);
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

    async fn single_dto(&self, item: OnlineLibraryItem) -> DomainResult<OnlineItemDto> {
        let sources = self.sources.list_sources().await?;
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
        let mut items = self.repo.list_by_user(user_id, self.media_kind).await?;
        let needle = query
            .q
            .as_deref()
            .map(|q| q.trim().to_lowercase())
            .filter(|q| !q.is_empty());
        let source_id = query
            .source_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                Uuid::parse_str(s)
                    .map(|u| u.to_string())
                    .unwrap_or_else(|_| s.to_string())
            });
        items.retain(|item| {
            let view_ok = match query.view {
                OnlineItemView::All => true,
                OnlineItemView::Continue => item.is_in_progress(),
                OnlineItemView::Favorites => item.favorite,
            };
            view_ok
                && query.source_kind.is_none_or(|k| item.source_kind == k)
                && source_id.as_deref().is_none_or(|s| item.source_id == s)
                && needle.as_deref().is_none_or(|n| {
                    item.title.to_lowercase().contains(n)
                        || item
                            .subtitle
                            .as_deref()
                            .is_some_and(|s| s.to_lowercase().contains(n))
                })
        });
        match query.view {
            OnlineItemView::All => items.sort_by_key(|i| std::cmp::Reverse(i.updated_at)),
            OnlineItemView::Continue => {
                items.sort_by_key(|i| std::cmp::Reverse(i.progress_at.unwrap_or(i.updated_at)))
            }
            OnlineItemView::Favorites => {
                items.sort_by_key(|i| std::cmp::Reverse(i.favorited_at.unwrap_or(i.updated_at)))
            }
        }

        let total = items.len();
        let limit = query
            .limit
            .unwrap_or(DEFAULT_LIST_LIMIT)
            .clamp(1, MAX_LIST_LIMIT) as usize;
        let offset = query.offset.unwrap_or(0) as usize;
        let sources = self.sources.list_sources().await?;
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
        self.single_dto(item).await
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
        let keys: Vec<ItemKey> = input
            .items
            .iter()
            .filter_map(|r| Self::parse_key(r.source_kind, &r.source_id, &r.item_key).ok())
            .collect();
        if keys.is_empty() {
            return Ok(OnlineItemListDto {
                items: Vec::new(),
                total: 0,
            });
        }
        let all = self.repo.list_by_user(user_id, self.media_kind).await?;
        let sources = self.sources.list_sources().await?;
        let items: Vec<OnlineItemDto> = all
            .into_iter()
            .filter(|item| {
                keys.iter()
                    .any(|k| item.matches_key(k.source_kind, &k.source_id, &k.item_key))
            })
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
        let key = Self::parse_key(input.source_kind, &input.source_id, &input.item_key)?;
        let title = validate_title(&input.title)?;
        let subtitle = input
            .subtitle
            .map(|s| normalize_opt_text(Some(s), MAX_SHORT_TEXT_CHARS));
        let cover_url = input
            .cover_url
            .map(|c| validate_cover_url(Some(c)))
            .transpose()?;
        let intro = input
            .intro
            .map(|s| normalize_opt_text(Some(s), MAX_INTRO_CHARS));
        let kind = input
            .kind
            .map(|s| normalize_opt_text(Some(s), MAX_SHORT_TEXT_CHARS));
        let extra = input.extra.map(|e| validate_extra(Some(e))).transpose()?;
        let progress = input
            .progress
            .map(OnlineItemProgress::normalized)
            .transpose()?;

        let user_lock = self.user_upsert_lock(user_id);
        let guard = user_lock.lock().await;
        let existing = self.repo.list_by_user(user_id, self.media_kind).await?;
        let count = existing.len();
        let found = existing
            .into_iter()
            .find(|i| i.matches_key(key.source_kind, &key.source_id, &key.item_key));
        let now = Timestamp::now();

        let item = match found {
            Some(mut item) => {
                item.title = title;
                if let Some(subtitle) = subtitle {
                    item.subtitle = subtitle;
                }
                if let Some(cover_url) = cover_url {
                    item.cover_url = cover_url;
                }
                if let Some(intro) = intro {
                    item.intro = intro;
                }
                if let Some(kind) = kind {
                    item.kind = kind;
                }
                if let Some(extra) = extra {
                    item.extra = extra;
                }
                if let Some(favorite) = input.favorite {
                    item.set_favorite(favorite, now);
                }
                if progress.is_some() {
                    item.set_progress(progress, now);
                }
                item.updated_at = now;
                item
            }
            None => {
                if count >= MAX_ITEMS_PER_USER {
                    return Err(DomainError::Conflict(format!(
                        "online item limit reached ({MAX_ITEMS_PER_USER})"
                    )));
                }
                if let Some(source_uuid) = key.source_uuid
                    && self.sources.find_source(source_uuid).await?.is_none()
                {
                    return Err(DomainError::NotFound("online source not found".into()));
                }
                let mut item = OnlineLibraryItem {
                    id: Uuid::now_v7(),
                    user_id,
                    media_kind: self.media_kind,
                    source_kind: key.source_kind,
                    source_id: key.source_id,
                    item_key: key.item_key,
                    title,
                    subtitle: subtitle.flatten(),
                    cover_url: cover_url.flatten(),
                    intro: intro.flatten(),
                    kind: kind.flatten(),
                    extra: extra.unwrap_or_else(|| serde_json::json!({})),
                    favorite: false,
                    favorited_at: None,
                    progress: None,
                    progress_at: None,
                    created_at: now,
                    updated_at: now,
                };
                if let Some(favorite) = input.favorite {
                    item.set_favorite(favorite, now);
                }
                if progress.is_some() {
                    item.set_progress(progress, now);
                }
                item
            }
        };
        self.repo.save(&item).await?;
        drop(guard);
        self.single_dto(item).await
    }

    pub async fn patch(
        &self,
        user_id: UserId,
        id: Uuid,
        input: PatchOnlineItemInput,
    ) -> DomainResult<OnlineItemDto> {
        let mut item = self.load_owned(user_id, id).await?;
        let now = Timestamp::now();
        if let Some(title) = input.title {
            item.title = validate_title(&title)?;
        }
        if let Some(subtitle) = input.subtitle {
            item.subtitle = normalize_opt_text(Some(subtitle), MAX_SHORT_TEXT_CHARS);
        }
        if let Some(cover_url) = input.cover_url {
            item.cover_url = validate_cover_url(Some(cover_url))?;
        }
        if let Some(intro) = input.intro {
            item.intro = normalize_opt_text(Some(intro), MAX_INTRO_CHARS);
        }
        if let Some(kind) = input.kind {
            item.kind = normalize_opt_text(Some(kind), MAX_SHORT_TEXT_CHARS);
        }
        if let Some(extra) = input.extra {
            item.extra = validate_extra(Some(extra))?;
        }
        if let Some(favorite) = input.favorite {
            item.set_favorite(favorite, now);
        }
        item.updated_at = now;
        self.repo.save(&item).await?;
        self.single_dto(item).await
    }

    pub async fn update_progress(
        &self,
        user_id: UserId,
        id: Uuid,
        progress: OnlineItemProgress,
    ) -> DomainResult<OnlineItemDto> {
        let progress = progress.normalized()?;
        let mut item = self.load_owned(user_id, id).await?;
        let now = Timestamp::now();
        item.set_progress(Some(progress), now);
        item.updated_at = now;
        self.repo.save(&item).await?;
        self.single_dto(item).await
    }

    pub async fn clear_progress(&self, user_id: UserId, id: Uuid) -> DomainResult<OnlineItemDto> {
        let mut item = self.load_owned(user_id, id).await?;
        let now = Timestamp::now();
        item.set_progress(None, now);
        item.updated_at = now;
        self.repo.save(&item).await?;
        self.single_dto(item).await
    }

    pub async fn delete(&self, user_id: UserId, id: Uuid) -> DomainResult<()> {
        let item = self.load_owned(user_id, id).await?;
        self.repo.delete(item.id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::repository::UserRepository;
    use crate::domain::user::{User, UserRole};
    use crate::infra::persistence::models::{LibraryModel, UserModel};
    use crate::infra::persistence::repositories::{
        ToastyOnlineLibraryItemRepository, ToastyUserRepository,
    };
    use tempfile::TempDir;
    use toasty::Db;
    use toasty::schema::{Model, ModelSet};

    struct FakeSources(HashMap<Uuid, OnlineSourceInfo>);

    #[async_trait]
    impl OnlineSourceDirectory for FakeSources {
        async fn list_sources(&self) -> DomainResult<HashMap<Uuid, OnlineSourceInfo>> {
            Ok(self.0.clone())
        }
    }

    struct Fixture {
        _dir: TempDir,
        db: Db,
        enabled: Uuid,
        disabled: Uuid,
    }

    async fn fixture() -> Fixture {
        let dir = TempDir::new().unwrap();
        let db_url = format!("sqlite:{}", dir.path().join("test.db").display());
        // Simulate a pre-existing database that only has the original core tables.
        let mut legacy = ModelSet::new();
        <UserModel as Model>::register(&mut legacy);
        <LibraryModel as Model>::register(&mut legacy);
        let bootstrap = Db::builder().models(legacy).connect(&db_url).await.unwrap();
        bootstrap.push_schema().await.unwrap();

        let mut models = ModelSet::new();
        crate::infra::persistence::register_core_models(&mut models);
        let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
        crate::infra::persistence::ensure_core_schema(&mut db, &db_url)
            .await
            .unwrap();
        Fixture {
            _dir: dir,
            db,
            enabled: Uuid::now_v7(),
            disabled: Uuid::now_v7(),
        }
    }

    fn service(fx: &Fixture, media_kind: MediaKind) -> OnlineLibraryService {
        let mut sources = HashMap::new();
        sources.insert(
            fx.enabled,
            OnlineSourceInfo {
                name: "Enabled".into(),
                enabled: true,
            },
        );
        sources.insert(
            fx.disabled,
            OnlineSourceInfo {
                name: "Disabled".into(),
                enabled: false,
            },
        );
        OnlineLibraryService::new(
            media_kind,
            Arc::new(ToastyOnlineLibraryItemRepository::new(fx.db.clone())),
            Arc::new(FakeSources(sources)),
        )
    }

    fn upsert_input(source_id: Uuid, key: &str) -> UpsertOnlineItemInput {
        UpsertOnlineItemInput {
            source_kind: OnlineItemSourceKind::Online,
            source_id: source_id.to_string().to_uppercase(),
            item_key: key.into(),
            title: format!("Title {key}"),
            subtitle: Some("Author".into()),
            cover_url: Some("https://img.example/c.jpg".into()),
            intro: None,
            kind: None,
            extra: Some(serde_json::json!({"toc_url": "https://example.com/toc"})),
            favorite: None,
            progress: None,
        }
    }

    #[tokio::test]
    async fn upsert_is_idempotent_and_partial_updates_keep_metadata() {
        let fx = fixture().await;
        let svc = service(&fx, MediaKind::Comic);
        let user = UserId::new();

        let first = svc
            .upsert(user, upsert_input(fx.enabled, "https://example.com/c/1"))
            .await
            .unwrap();
        assert_eq!(
            first.source_id,
            fx.enabled.to_string(),
            "uuid is canonicalized"
        );
        assert_eq!(first.source_name.as_deref(), Some("Enabled"));
        assert_eq!(first.availability, OnlineItemAvailability::Ok);
        assert!(!first.favorite);

        let mut again = upsert_input(fx.enabled, "https://example.com/c/1");
        again.subtitle = None;
        again.cover_url = None;
        again.extra = None;
        again.favorite = Some(true);
        let second = svc.upsert(user, again).await.unwrap();
        assert_eq!(second.id, first.id);
        assert!(second.favorite);
        assert!(second.favorited_at.is_some());
        assert_eq!(second.subtitle.as_deref(), Some("Author"));
        assert_eq!(
            second.cover_url.as_deref(),
            Some("https://img.example/c.jpg")
        );
        assert_eq!(second.extra["toc_url"], "https://example.com/toc");

        let all = svc.list(user, OnlineItemQuery::default()).await.unwrap();
        assert_eq!(all.total, 1);
    }

    #[tokio::test]
    async fn upsert_validates_source_and_payload() {
        let fx = fixture().await;
        let svc = service(&fx, MediaKind::Video);
        let user = UserId::new();

        let missing = svc
            .upsert(user, upsert_input(Uuid::now_v7(), "vod-1"))
            .await
            .unwrap_err();
        assert!(matches!(missing, DomainError::NotFound(_)));

        let mut not_uuid = upsert_input(fx.enabled, "vod-1");
        not_uuid.source_id = "not-a-uuid".into();
        assert!(matches!(
            svc.upsert(user, not_uuid).await.unwrap_err(),
            DomainError::Validation(_)
        ));

        let mut bad_cover = upsert_input(fx.enabled, "vod-1");
        bad_cover.cover_url = Some("javascript:alert(1)".into());
        assert!(matches!(
            svc.upsert(user, bad_cover).await.unwrap_err(),
            DomainError::Validation(_)
        ));

        let mut bad_progress = upsert_input(fx.enabled, "vod-1");
        bad_progress.progress = Some(OnlineItemProgress {
            position: f64::NAN,
            ..Default::default()
        });
        assert!(matches!(
            svc.upsert(user, bad_progress).await.unwrap_err(),
            DomainError::Validation(_)
        ));

        // Remote items are accepted without server-side source verification.
        let remote = svc
            .upsert(
                user,
                UpsertOnlineItemInput {
                    source_kind: OnlineItemSourceKind::Remote,
                    source_id: "remote-opds-1".into(),
                    cover_url: Some("/_app/opds/remote-opds-1/proxy-image?url=x".into()),
                    ..upsert_input(fx.enabled, "urn:book:1")
                },
            )
            .await
            .unwrap();
        assert_eq!(remote.availability, OnlineItemAvailability::Unverified);
        assert_eq!(remote.source_name, None);
    }

    #[tokio::test]
    async fn continue_and_favorite_views_filter_and_sort() {
        let fx = fixture().await;
        let svc = service(&fx, MediaKind::Comic);
        let user = UserId::new();

        let a = svc
            .upsert(user, upsert_input(fx.enabled, "a"))
            .await
            .unwrap();
        let b = svc
            .upsert(user, upsert_input(fx.disabled, "b"))
            .await
            .unwrap();
        let c = svc
            .upsert(user, upsert_input(fx.enabled, "c"))
            .await
            .unwrap();

        svc.update_progress(
            user,
            a.id,
            OnlineItemProgress {
                unit_index: Some(1),
                total_units: Some(4),
                position: 5.0,
                duration: Some(10.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let b_progress = svc
            .update_progress(
                user,
                b.id,
                OnlineItemProgress {
                    position: 1.0,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            b_progress.availability,
            OnlineItemAvailability::SourceDisabled
        );
        svc.update_progress(
            user,
            c.id,
            OnlineItemProgress {
                completed: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let cont = svc
            .list(
                user,
                OnlineItemQuery {
                    view: OnlineItemView::Continue,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let ids: Vec<Uuid> = cont.items.iter().map(|i| i.id).collect();
        assert_eq!(
            ids,
            vec![b.id, a.id],
            "completed items are excluded, newest first"
        );
        assert_eq!(cont.items[1].progress_ratio, Some(0.375));

        svc.patch(
            user,
            c.id,
            PatchOnlineItemInput {
                favorite: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let favs = svc
            .list(
                user,
                OnlineItemQuery {
                    view: OnlineItemView::Favorites,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(favs.total, 1);
        assert_eq!(favs.items[0].id, c.id);

        let by_source = svc
            .list(
                user,
                OnlineItemQuery {
                    source_id: Some(fx.disabled.to_string().to_uppercase()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(by_source.total, 1);
        assert_eq!(by_source.items[0].id, b.id);

        let searched = svc
            .list(
                user,
                OnlineItemQuery {
                    q: Some("title C".into()),
                    limit: Some(1),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(searched.total, 1);

        let cleared = svc.clear_progress(user, a.id).await.unwrap();
        assert!(cleared.progress.is_none());
        assert!(cleared.progress_at.is_none());
    }

    #[tokio::test]
    async fn items_are_isolated_by_user_and_media_kind() {
        let fx = fixture().await;
        let comic = service(&fx, MediaKind::Comic);
        let music = service(&fx, MediaKind::Music);
        let alice = UserId::new();
        let bob = UserId::new();

        let item = comic
            .upsert(alice, upsert_input(fx.enabled, "shared-key"))
            .await
            .unwrap();
        assert!(matches!(
            comic.get(bob, item.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(matches!(
            music.get(alice, item.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(matches!(
            comic.delete(bob, item.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert_eq!(
            music
                .list(alice, OnlineItemQuery::default())
                .await
                .unwrap()
                .total,
            0
        );

        let lookup = comic
            .lookup(
                alice,
                LookupOnlineItemsInput {
                    items: vec![
                        OnlineItemRefInput {
                            source_kind: OnlineItemSourceKind::Online,
                            source_id: fx.enabled.to_string(),
                            item_key: "shared-key".into(),
                        },
                        OnlineItemRefInput {
                            source_kind: OnlineItemSourceKind::Online,
                            source_id: fx.enabled.to_string(),
                            item_key: "unknown".into(),
                        },
                        OnlineItemRefInput {
                            source_kind: OnlineItemSourceKind::Online,
                            source_id: "garbage".into(),
                            item_key: "x".into(),
                        },
                    ],
                },
            )
            .await
            .unwrap();
        assert_eq!(lookup.total, 1);
        assert!(
            comic
                .lookup(bob, LookupOnlineItemsInput { items: vec![] })
                .await
                .unwrap()
                .items
                .is_empty()
        );

        comic.delete(alice, item.id).await.unwrap();
        assert!(matches!(
            comic.get(alice, item.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn deleting_user_purges_online_items() {
        let fx = fixture().await;
        let users = ToastyUserRepository::new(fx.db.clone());
        let user = User::new("alice", "Alice", "hash", "enc", "api-key", UserRole::Member).unwrap();
        users.save(&user).await.unwrap();

        let svc = service(&fx, MediaKind::Music);
        let item = svc
            .upsert(user.id, upsert_input(fx.enabled, "track-1"))
            .await
            .unwrap();
        users.delete(user.id).await.unwrap();

        let repo = ToastyOnlineLibraryItemRepository::new(fx.db.clone());
        assert!(repo.find_by_id(item.id).await.unwrap().is_none());
    }
}
