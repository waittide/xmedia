use async_trait::async_trait;
use jiff::Timestamp;
use toasty::Db;
use uuid::Uuid;

use crate::domain::activity::{UserActivityDaily, UserActivityRepository};
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::{Library, LibraryId, MediaKind, ScanStatus};
use crate::domain::online_item::{OnlineItemSourceKind, OnlineLibraryItem};
use crate::domain::repository::{LibraryRepository, OnlineLibraryItemRepository, UserRepository};
use crate::domain::user::{User, UserId, UserPreferences, UserRole};

use super::models::{LibraryModel, OnlineLibraryItemModel, UserActivityDailyModel, UserModel};

#[derive(Clone)]
pub struct ToastyUserRepository {
    db: Db,
}

impl ToastyUserRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn model_to_domain(m: UserModel) -> DomainResult<User> {
        Ok(User {
            id: UserId(m.id),
            username: m.username,
            display_name: m.display_name,
            password_hash: m.password_hash,
            subsonic_secret_enc: m.subsonic_secret_enc,
            api_key: m.api_key,
            role: UserRole::parse(&m.role)?,
            is_active: m.is_active,
            preferences: m.preferences.0.normalized(),
            created_at: m.created_at,
            updated_at: m.updated_at,
        })
    }
}

#[async_trait]
impl UserRepository for ToastyUserRepository {
    async fn count(&self) -> DomainResult<u64> {
        let mut db = self.db.clone();
        let rows = UserModel::all()
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("count users failed: {e}")))?;
        Ok(rows.len() as u64)
    }

    async fn has_any_user(&self) -> DomainResult<bool> {
        let mut db = self.db.clone();
        let rows = UserModel::all()
            .limit(1)
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("probe users failed: {e}")))?;
        Ok(!rows.is_empty())
    }

    async fn find_by_id(&self, id: UserId) -> DomainResult<Option<User>> {
        let mut db = self.db.clone();
        let opt = UserModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("find user by id failed: {e}")))?;
        opt.map(Self::model_to_domain).transpose()
    }

    async fn find_by_username(&self, username: &str) -> DomainResult<Option<User>> {
        let mut db = self.db.clone();
        let opt = UserModel::filter_by_username(username.to_string())
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("find user by username failed: {e}")))?;
        opt.map(Self::model_to_domain).transpose()
    }

    async fn find_by_api_key(&self, api_key: &str) -> DomainResult<Option<User>> {
        let mut db = self.db.clone();
        let opt = UserModel::filter_by_api_key(api_key.to_string())
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("find user by api_key failed: {e}")))?;
        opt.map(Self::model_to_domain).transpose()
    }

    async fn list_all(&self) -> DomainResult<Vec<User>> {
        let mut db = self.db.clone();
        let rows = UserModel::all()
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("list users failed: {e}")))?;
        let mut users: Vec<User> = rows
            .into_iter()
            .map(Self::model_to_domain)
            .collect::<DomainResult<_>>()?;
        users.sort_by_key(|a| a.created_at);
        Ok(users)
    }

    async fn save(&self, user: &User) -> DomainResult<()> {
        let mut db = self.db.clone();
        let exists = UserModel::filter_by_id(user.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("probe user failed: {e}")))?;

        if let Some(mut m) = exists {
            m.update()
                .username(user.username.clone())
                .display_name(user.display_name.clone())
                .password_hash(user.password_hash.clone())
                .subsonic_secret_enc(user.subsonic_secret_enc.clone())
                .api_key(user.api_key.clone())
                .role(user.role.as_str().to_string())
                .is_active(user.is_active)
                .preferences(toasty::Json(user.preferences.clone()))
                .updated_at(user.updated_at)
                .exec(&mut db)
                .await
                .map_err(|e| DomainError::Persistence(format!("update user failed: {e}")))?;
        } else {
            toasty::create!(UserModel {
                id: user.id.0,
                username: user.username.clone(),
                display_name: user.display_name.clone(),
                password_hash: user.password_hash.clone(),
                subsonic_secret_enc: user.subsonic_secret_enc.clone(),
                api_key: user.api_key.clone(),
                role: user.role.as_str().to_string(),
                is_active: user.is_active,
                preferences: toasty::Json(user.preferences.clone()),
                created_at: user.created_at,
                updated_at: user.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("insert user failed: {e}")))?;
        }
        Ok(())
    }

    async fn update_preferences(
        &self,
        id: UserId,
        preferences: &UserPreferences,
        updated_at: Timestamp,
    ) -> DomainResult<bool> {
        let mut db = self.db.clone();
        let Some(mut m) = UserModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("probe user failed: {e}")))?
        else {
            return Ok(false);
        };
        m.update()
            .preferences(toasty::Json(preferences.clone()))
            .updated_at(updated_at)
            .exec(&mut db)
            .await
            .map_err(|e| {
                DomainError::Persistence(format!("update user preferences failed: {e}"))
            })?;
        Ok(true)
    }

    async fn update_credentials(
        &self,
        id: UserId,
        password_hash: &str,
        subsonic_secret_enc: &str,
        updated_at: Timestamp,
    ) -> DomainResult<bool> {
        let mut db = self.db.clone();
        let Some(mut m) = UserModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("probe user failed: {e}")))?
        else {
            return Ok(false);
        };
        m.update()
            .password_hash(password_hash.to_string())
            .subsonic_secret_enc(subsonic_secret_enc.to_string())
            .updated_at(updated_at)
            .exec(&mut db)
            .await
            .map_err(|e| {
                DomainError::Persistence(format!("update user credentials failed: {e}"))
            })?;
        Ok(true)
    }

    async fn update_api_key(
        &self,
        id: UserId,
        api_key: &str,
        updated_at: Timestamp,
    ) -> DomainResult<bool> {
        let mut db = self.db.clone();
        let Some(mut m) = UserModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("probe user failed: {e}")))?
        else {
            return Ok(false);
        };
        m.update()
            .api_key(api_key.to_string())
            .updated_at(updated_at)
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("update user api_key failed: {e}")))?;
        Ok(true)
    }

    async fn delete(&self, id: UserId) -> DomainResult<()> {
        let mut db = self.db.clone();
        OnlineLibraryItemModel::filter_by_user_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(|e| online_item_err("purge user", e))?;
        UserModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("delete user failed: {e}")))?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct ToastyLibraryRepository {
    db: Db,
}

impl ToastyLibraryRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn model_to_domain(m: LibraryModel) -> DomainResult<Library> {
        Ok(Library {
            id: LibraryId(m.id),
            name: m.name,
            media_kind: MediaKind::parse(&m.media_kind)?,
            kind: m.kind.0,
            scan_status: m.scan_status.0,
            last_scanned_at: m.last_scanned_at,
            created_at: m.created_at,
            updated_at: m.updated_at,
        })
    }
}

#[async_trait]
impl LibraryRepository for ToastyLibraryRepository {
    async fn find_by_id(&self, id: LibraryId) -> DomainResult<Option<Library>> {
        let mut db = self.db.clone();
        let opt = LibraryModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("find library failed: {e}")))?;
        opt.map(Self::model_to_domain).transpose()
    }

    async fn find_managed_by_media(&self, media_kind: MediaKind) -> DomainResult<Option<Library>> {
        let mut db = self.db.clone();
        let rows = LibraryModel::filter_by_media_kind(media_kind.as_str().to_string())
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("query managed library failed: {e}")))?;
        for m in rows {
            if m.is_managed {
                return Ok(Some(Self::model_to_domain(m)?));
            }
        }
        Ok(None)
    }

    async fn list_all(&self) -> DomainResult<Vec<Library>> {
        let mut db = self.db.clone();
        let rows = LibraryModel::all()
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("list libraries failed: {e}")))?;
        let mut libs: Vec<Library> = rows
            .into_iter()
            .map(Self::model_to_domain)
            .collect::<DomainResult<_>>()?;
        libs.sort_by_key(|a| a.created_at);
        Ok(libs)
    }

    async fn list_by_media(&self, media_kind: MediaKind) -> DomainResult<Vec<Library>> {
        let mut db = self.db.clone();
        let rows = LibraryModel::filter_by_media_kind(media_kind.as_str().to_string())
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("list media libraries failed: {e}")))?;
        let mut libs: Vec<Library> = rows
            .into_iter()
            .map(Self::model_to_domain)
            .collect::<DomainResult<_>>()?;
        libs.sort_by_key(|a| a.created_at);
        Ok(libs)
    }

    async fn save(&self, library: &Library) -> DomainResult<()> {
        let mut db = self.db.clone();
        let exists = LibraryModel::filter_by_id(library.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("probe library failed: {e}")))?;

        if let Some(mut m) = exists {
            m.update()
                .name(library.name.clone())
                .media_kind(library.media_kind.as_str().to_string())
                .is_managed(library.kind.is_managed())
                .kind(toasty::Json(library.kind.clone()))
                .scan_status(toasty::Json(library.scan_status.clone()))
                .last_scanned_at(library.last_scanned_at)
                .updated_at(library.updated_at)
                .exec(&mut db)
                .await
                .map_err(|e| DomainError::Persistence(format!("update library failed: {e}")))?;
        } else {
            toasty::create!(LibraryModel {
                id: library.id.0,
                name: library.name.clone(),
                media_kind: library.media_kind.as_str().to_string(),
                is_managed: library.kind.is_managed(),
                kind: toasty::Json(library.kind.clone()),
                scan_status: toasty::Json(library.scan_status.clone()),
                last_scanned_at: library.last_scanned_at,
                created_at: library.created_at,
                updated_at: library.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("insert library failed: {e}")))?;
        }
        Ok(())
    }

    async fn update_name(
        &self,
        id: LibraryId,
        name: &str,
        updated_at: Timestamp,
    ) -> DomainResult<bool> {
        let mut db = self.db.clone();
        let Some(mut m) = LibraryModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("probe library failed: {e}")))?
        else {
            return Ok(false);
        };
        m.update()
            .name(name.to_string())
            .updated_at(updated_at)
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("update library name failed: {e}")))?;
        Ok(true)
    }

    async fn update_scan_state(
        &self,
        id: LibraryId,
        scan_status: &ScanStatus,
        last_scanned_at: Option<Timestamp>,
        updated_at: Timestamp,
    ) -> DomainResult<bool> {
        let mut db = self.db.clone();
        let Some(mut m) = LibraryModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("probe library failed: {e}")))?
        else {
            return Ok(false);
        };
        m.update()
            .scan_status(toasty::Json(scan_status.clone()))
            .last_scanned_at(last_scanned_at)
            .updated_at(updated_at)
            .exec(&mut db)
            .await
            .map_err(|e| {
                DomainError::Persistence(format!("update library scan state failed: {e}"))
            })?;
        Ok(true)
    }

    async fn delete(&self, id: LibraryId) -> DomainResult<()> {
        let mut db = self.db.clone();
        LibraryModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(|e| DomainError::Persistence(format!("delete library failed: {e}")))?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct ToastyOnlineLibraryItemRepository {
    db: Db,
}

impl ToastyOnlineLibraryItemRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn model_to_domain(m: OnlineLibraryItemModel) -> DomainResult<OnlineLibraryItem> {
        Ok(OnlineLibraryItem {
            id: m.id,
            user_id: UserId(m.user_id),
            media_kind: MediaKind::parse(&m.media_kind)?,
            source_kind: OnlineItemSourceKind::parse(&m.source_kind)?,
            source_id: m.source_id,
            item_key: m.item_key,
            title: m.title,
            subtitle: m.subtitle,
            cover_url: m.cover_url,
            intro: m.intro,
            kind: m.kind,
            extra: m.extra.0,
            favorite: m.favorite,
            favorited_at: m.favorited_at,
            progress: m.progress.0,
            progress_at: m.progress_at,
            created_at: m.created_at,
            updated_at: m.updated_at,
        })
    }
}

fn online_item_err(action: &str, e: impl std::fmt::Display) -> DomainError {
    DomainError::Persistence(format!("{action} online library item failed: {e}"))
}

#[async_trait]
impl OnlineLibraryItemRepository for ToastyOnlineLibraryItemRepository {
    async fn list_by_user(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
    ) -> DomainResult<Vec<OnlineLibraryItem>> {
        let mut db = self.db.clone();
        let rows = OnlineLibraryItemModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(|e| online_item_err("list", e))?;
        let wanted = media_kind.as_str();
        rows.into_iter()
            .filter(|m| m.media_kind == wanted)
            .map(Self::model_to_domain)
            .collect()
    }

    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<OnlineLibraryItem>> {
        let mut db = self.db.clone();
        let row = OnlineLibraryItemModel::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| online_item_err("find", e))?;
        row.map(Self::model_to_domain).transpose()
    }

    async fn save(&self, item: &OnlineLibraryItem) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = OnlineLibraryItemModel::filter_by_id(item.id)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| online_item_err("load", e))?;
        if let Some(mut rec) = existing {
            rec.update()
                .title(item.title.clone())
                .subtitle(item.subtitle.clone())
                .cover_url(item.cover_url.clone())
                .intro(item.intro.clone())
                .kind(item.kind.clone())
                .extra(toasty::Json(item.extra.clone()))
                .favorite(item.favorite)
                .favorited_at(item.favorited_at)
                .progress(toasty::Json(item.progress.clone()))
                .progress_at(item.progress_at)
                .updated_at(item.updated_at)
                .exec(&mut db)
                .await
                .map_err(|e| online_item_err("update", e))?;
        } else {
            toasty::create!(OnlineLibraryItemModel {
                id: item.id,
                user_id: item.user_id.0,
                media_kind: item.media_kind.as_str().to_string(),
                source_kind: item.source_kind.as_str().to_string(),
                source_id: item.source_id.clone(),
                item_key: item.item_key.clone(),
                title: item.title.clone(),
                subtitle: item.subtitle.clone(),
                cover_url: item.cover_url.clone(),
                intro: item.intro.clone(),
                kind: item.kind.clone(),
                extra: toasty::Json(item.extra.clone()),
                favorite: item.favorite,
                favorited_at: item.favorited_at,
                progress: toasty::Json(item.progress.clone()),
                progress_at: item.progress_at,
                created_at: item.created_at,
                updated_at: item.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(|e| online_item_err("create", e))?;
        }
        Ok(())
    }

    async fn delete(&self, id: Uuid) -> DomainResult<()> {
        let mut db = self.db.clone();
        OnlineLibraryItemModel::filter_by_id(id)
            .delete()
            .exec(&mut db)
            .await
            .map_err(|e| online_item_err("delete", e))?;
        Ok(())
    }

    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()> {
        let mut db = self.db.clone();
        OnlineLibraryItemModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(|e| online_item_err("purge", e))?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct ToastyUserActivityRepository {
    db: Db,
}

impl ToastyUserActivityRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn model_to_domain(m: UserActivityDailyModel) -> DomainResult<UserActivityDaily> {
        Ok(UserActivityDaily {
            id: m.id,
            user_id: UserId(m.user_id),
            media_kind: MediaKind::parse(&m.media_kind)?,
            activity_date: m.activity_date,
            duration_secs: u64::try_from(m.duration_secs.max(0)).unwrap_or(0),
            active_sessions: u32::try_from(m.active_sessions.max(0)).unwrap_or(0),
            last_item_title: m.last_item_title,
            updated_at: m.updated_at,
        })
    }
}

fn activity_err(action: &str, e: impl std::fmt::Display) -> DomainError {
    DomainError::Persistence(format!("{action} user activity failed: {e}"))
}

#[async_trait]
impl UserActivityRepository for ToastyUserActivityRepository {
    async fn list_by_user_and_domain(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
    ) -> DomainResult<Vec<UserActivityDaily>> {
        let mut db = self.db.clone();
        let rows = UserActivityDailyModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(|e| activity_err("list", e))?;
        let wanted = media_kind.as_str();
        rows.into_iter()
            .filter(|m| m.media_kind == wanted)
            .map(Self::model_to_domain)
            .collect()
    }

    async fn find_by_key(
        &self,
        user_id: UserId,
        media_kind: MediaKind,
        activity_date: &str,
    ) -> DomainResult<Option<UserActivityDaily>> {
        let mut db = self.db.clone();
        let rows = UserActivityDailyModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(|e| activity_err("find", e))?;
        let wanted_kind = media_kind.as_str();
        rows.into_iter()
            .find(|m| m.media_kind == wanted_kind && m.activity_date == activity_date)
            .map(Self::model_to_domain)
            .transpose()
    }

    async fn save(&self, record: &UserActivityDaily) -> DomainResult<()> {
        let mut db = self.db.clone();
        let duration_i64 = i64::try_from(record.duration_secs).unwrap_or(i64::MAX);
        let sessions_i64 = i64::from(record.active_sessions);
        let existing = UserActivityDailyModel::filter_by_id(record.id)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| activity_err("probe", e))?;

        if let Some(mut rec) = existing {
            rec.update()
                .duration_secs(duration_i64)
                .active_sessions(sessions_i64)
                .last_item_title(record.last_item_title.clone())
                .updated_at(record.updated_at)
                .exec(&mut db)
                .await
                .map_err(|e| activity_err("update", e))?;
        } else {
            toasty::create!(UserActivityDailyModel {
                id: record.id,
                user_id: record.user_id.0,
                media_kind: record.media_kind.as_str().to_string(),
                activity_date: record.activity_date.clone(),
                duration_secs: duration_i64,
                active_sessions: sessions_i64,
                last_item_title: record.last_item_title.clone(),
                updated_at: record.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(|e| activity_err("create", e))?;
        }
        Ok(())
    }

    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()> {
        let mut db = self.db.clone();
        UserActivityDailyModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(|e| activity_err("purge", e))?;
        Ok(())
    }
}
