mod user_data;

use async_trait::async_trait;
use jiff::Timestamp;
use toasty::Db;

use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::{Library, LibraryId, MediaKind};
use crate::domain::repository::{
    LibraryMutation, LibraryRepository, UserLookup, UserMutation, UserRepository,
};
use crate::domain::user::{User, UserId, UserRole};

use super::models::{LibraryModel, UserModel};

pub use user_data::{
    ToastyOnlineLibraryItemRepository, ToastyUserActivityRepository,
    ToastyUserMediaBindingRepository, ToastyUserPluginRepository,
};

fn db_err(action: &str, err: impl std::fmt::Display) -> DomainError {
    DomainError::Persistence(format!("{action} failed: {err}"))
}

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
            .map_err(|e| db_err("count users", e))?;
        Ok(rows.len() as u64)
    }

    async fn find(&self, key: UserLookup<'_>) -> DomainResult<Option<User>> {
        let mut db = self.db.clone();
        let opt = match key {
            UserLookup::Id(id) => UserModel::filter_by_id(id.0)
                .first()
                .exec(&mut db)
                .await
                .map_err(|e| db_err("find user by id", e))?,
            UserLookup::Username(username) => UserModel::filter_by_username(username.to_string())
                .first()
                .exec(&mut db)
                .await
                .map_err(|e| db_err("find user by username", e))?,
            UserLookup::ApiKey(api_key) => UserModel::filter_by_api_key(api_key.to_string())
                .first()
                .exec(&mut db)
                .await
                .map_err(|e| db_err("find user by api_key", e))?,
        };
        opt.map(Self::model_to_domain).transpose()
    }

    async fn list_all(&self) -> DomainResult<Vec<User>> {
        let mut db = self.db.clone();
        let rows = UserModel::all()
            .exec(&mut db)
            .await
            .map_err(|e| db_err("list users", e))?;
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
            .map_err(|e| db_err("probe user", e))?;

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
                .map_err(|e| db_err("update user", e))?;
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
            .map_err(|e| db_err("insert user", e))?;
        }
        Ok(())
    }

    async fn update(
        &self,
        id: UserId,
        patch: UserMutation<'_>,
        updated_at: Timestamp,
    ) -> DomainResult<bool> {
        let mut db = self.db.clone();
        let Some(mut m) = UserModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| db_err("probe user", e))?
        else {
            return Ok(false);
        };
        match patch {
            UserMutation::Preferences(preferences) => {
                m.update()
                    .preferences(toasty::Json(preferences.clone()))
                    .updated_at(updated_at)
                    .exec(&mut db)
                    .await
                    .map_err(|e| db_err("update user preferences", e))?;
            }
            UserMutation::Credentials {
                password_hash,
                subsonic_secret_enc,
            } => {
                m.update()
                    .password_hash(password_hash.to_string())
                    .subsonic_secret_enc(subsonic_secret_enc.to_string())
                    .updated_at(updated_at)
                    .exec(&mut db)
                    .await
                    .map_err(|e| db_err("update user credentials", e))?;
            }
            UserMutation::ApiKey(api_key) => {
                m.update()
                    .api_key(api_key.to_string())
                    .updated_at(updated_at)
                    .exec(&mut db)
                    .await
                    .map_err(|e| db_err("update user api_key", e))?;
            }
        }
        Ok(true)
    }

    async fn delete(&self, id: UserId) -> DomainResult<()> {
        let mut db = self.db.clone();
        UserModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(|e| db_err("delete user", e))?;
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

    fn map_sorted(rows: Vec<LibraryModel>) -> DomainResult<Vec<Library>> {
        let mut libs: Vec<Library> = rows
            .into_iter()
            .map(Self::model_to_domain)
            .collect::<DomainResult<_>>()?;
        libs.sort_by_key(|a| a.created_at);
        Ok(libs)
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
            .map_err(|e| db_err("find library", e))?;
        opt.map(Self::model_to_domain).transpose()
    }

    async fn find_managed_by_media(&self, media_kind: MediaKind) -> DomainResult<Option<Library>> {
        let mut db = self.db.clone();
        let rows = LibraryModel::filter_by_media_kind(media_kind.as_str().to_string())
            .exec(&mut db)
            .await
            .map_err(|e| db_err("query managed library", e))?;
        rows.into_iter()
            .find(|m| m.is_managed)
            .map(Self::model_to_domain)
            .transpose()
    }

    async fn list(&self, media_kind: Option<MediaKind>) -> DomainResult<Vec<Library>> {
        let mut db = self.db.clone();
        let rows = match media_kind {
            Some(kind) => LibraryModel::filter_by_media_kind(kind.as_str().to_string())
                .exec(&mut db)
                .await
                .map_err(|e| db_err("list media libraries", e))?,
            None => LibraryModel::all()
                .exec(&mut db)
                .await
                .map_err(|e| db_err("list libraries", e))?,
        };
        Self::map_sorted(rows)
    }

    async fn save(&self, library: &Library) -> DomainResult<()> {
        let mut db = self.db.clone();
        let exists = LibraryModel::filter_by_id(library.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| db_err("probe library", e))?;

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
                .map_err(|e| db_err("update library", e))?;
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
            .map_err(|e| db_err("insert library", e))?;
        }
        Ok(())
    }

    async fn update(
        &self,
        id: LibraryId,
        patch: LibraryMutation<'_>,
        updated_at: Timestamp,
    ) -> DomainResult<bool> {
        let mut db = self.db.clone();
        let Some(mut m) = LibraryModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(|e| db_err("probe library", e))?
        else {
            return Ok(false);
        };
        match patch {
            LibraryMutation::Name(name) => {
                m.update()
                    .name(name.to_string())
                    .updated_at(updated_at)
                    .exec(&mut db)
                    .await
                    .map_err(|e| db_err("update library name", e))?;
            }
            LibraryMutation::ScanState {
                status,
                last_scanned_at,
            } => {
                m.update()
                    .scan_status(toasty::Json(status.clone()))
                    .last_scanned_at(last_scanned_at)
                    .updated_at(updated_at)
                    .exec(&mut db)
                    .await
                    .map_err(|e| db_err("update library scan state", e))?;
            }
        }
        Ok(true)
    }

    async fn delete(&self, id: LibraryId) -> DomainResult<()> {
        let mut db = self.db.clone();
        LibraryModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(|e| db_err("delete library", e))?;
        Ok(())
    }
}
