#[doc(hidden)]
pub mod __private {
    pub use uuid::{Error as UuidError, Uuid};
}

/// Defines one or more strongly-typed UUID v7 newtype identifiers with transparent serde
/// serialization, `Default`, `Display`, `FromStr`, and `From<Uuid>` conversions.
#[macro_export]
macro_rules! define_uuid_id {
    ($($(#[$meta:meta])* $name:ident),+ $(,)?) => {
        $(
            $(#[$meta])*
            #[derive(
                Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
                ::serde::Serialize, ::serde::Deserialize
            )]
            #[serde(transparent)]
            pub struct $name(pub $crate::__private::Uuid);

            impl $name {
                pub fn new() -> Self {
                    Self($crate::__private::Uuid::now_v7())
                }
            }

            impl Default for $name {
                fn default() -> Self {
                    Self::new()
                }
            }

            impl ::std::fmt::Display for $name {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    self.0.fmt(f)
                }
            }

            impl ::std::str::FromStr for $name {
                type Err = $crate::__private::UuidError;

                fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                    $crate::__private::Uuid::parse_str(s).map(Self)
                }
            }

            impl ::std::convert::From<$crate::__private::Uuid> for $name {
                fn from(value: $crate::__private::Uuid) -> Self {
                    Self(value)
                }
            }

            impl ::std::convert::From<$name> for $crate::__private::Uuid {
                fn from(value: $name) -> Self {
                    value.0
                }
            }
        )+
    };
}

pub mod application;
pub mod domain;
pub mod http;
pub mod infra;

#[cfg(feature = "server")]
pub use infra::persistence::{
    ensure_core_schema, ensure_sqlite_parent_dir, is_core_schema_initialized, register_core_models,
};

#[cfg(all(test, feature = "server"))]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};

    use async_trait::async_trait;
    use md5::{Digest, Md5};
    use tempfile::TempDir;
    use toasty::Db;
    use toasty::schema::ModelSet;

    use crate::application::auth::AuthService;
    use crate::application::dto::{
        ChangePasswordInput, CreateLibraryInput, CreateUserInput, SetupAdminInput,
        SubsonicCredentials, UpdateUserInput,
    };
    use crate::application::library::LibraryService;
    use crate::application::ports::MediaDomainLifecycle;
    use crate::domain::error::{DomainError, DomainResult};
    use crate::domain::library::{Library, LibraryId, MediaKind};
    use crate::domain::repository::LibraryRepository;
    use crate::domain::user::{UserId, UserRole};
    use crate::infra::events::BroadcastEventHub;
    use crate::infra::persistence::repositories::{ToastyLibraryRepository, ToastyUserRepository};
    use crate::infra::security::{JwtTokenService, SecurityManager};

    struct DummyLifecycle(MediaKind);

    #[async_trait]
    impl MediaDomainLifecycle for DummyLifecycle {
        fn media_kind(&self) -> MediaKind {
            self.0
        }
        async fn scan_library(&self, _library: Library) -> DomainResult<()> {
            Ok(())
        }
        async fn count_library_items(&self, _library_id: LibraryId) -> DomainResult<u64> {
            Ok(0)
        }
        async fn on_library_deleted(&self, _library_id: LibraryId) -> DomainResult<()> {
            Ok(())
        }
        async fn on_user_deleted(&self, _user_id: UserId) -> DomainResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn core_auth_and_library_persistence_roundtrip() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let db_url = format!("sqlite:{}", db_path.display());

        let mut models = ModelSet::new();
        super::register_core_models(&mut models);
        let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
        super::ensure_core_schema(&mut db, &db_url).await.unwrap();

        let users = Arc::new(ToastyUserRepository::new(db.clone()));
        let libraries = Arc::new(ToastyLibraryRepository::new(db));
        let lifecycles = Arc::new(RwLock::new(HashMap::new()));
        let security = Arc::new(SecurityManager::new("test-secret-key"));
        let tokens = Arc::new(JwtTokenService::new("test-secret-key", 3600));
        let events = Arc::new(BroadcastEventHub::default());

        let auth = AuthService::new(users, security, tokens, lifecycles.clone());
        let lib_svc = LibraryService::new(libraries, events, lifecycles);

        assert!(auth.setup_status().await.unwrap().needs_setup);
        let session = auth
            .setup_initial_admin(SetupAdminInput {
                username: "admin".into(),
                display_name: Some("Admin".into()),
                password: "password123".into(),
            })
            .await
            .unwrap();
        assert!(!auth.setup_status().await.unwrap().needs_setup);

        let jwt_user = auth.authenticate_jwt(&session.token).await.unwrap();
        assert_eq!(jwt_user.username, "admin");

        // Local admin session issuance works without password and survives password changes
        let local_session = auth.issue_local_admin_session(Some("admin")).await.unwrap();
        assert_eq!(local_session.user.username, "admin");
        let verified_local = auth.authenticate_jwt(&local_session.token).await.unwrap();
        assert_eq!(verified_local.id, jwt_user.id);

        let fallback_session = auth
            .issue_local_admin_session(Some("missing-username"))
            .await
            .unwrap();
        assert_eq!(fallback_session.user.id, jwt_user.id);

        let basic_user = auth
            .verify_basic_credentials("admin", "password123")
            .await
            .unwrap();
        assert_eq!(basic_user.id, jwt_user.id);

        let salt = "abc123";
        let mut md5 = Md5::new();
        md5.update(b"password123");
        md5.update(salt.as_bytes());
        let token_hex = hex::encode(md5.finalize());
        let sub_user = auth
            .authenticate_subsonic(SubsonicCredentials::TokenSalt {
                username: "admin".into(),
                token: token_hex,
                salt: salt.into(),
            })
            .await
            .unwrap();
        assert_eq!(sub_user.id, jwt_user.id);

        let book_lib = lib_svc
            .ensure_managed_library(MediaKind::Book)
            .await
            .unwrap();
        let video_lib = lib_svc
            .ensure_managed_library(MediaKind::Video)
            .await
            .unwrap();
        assert_ne!(book_lib.id, video_lib.id);
        assert_eq!(
            lib_svc
                .list_libraries(Some(MediaKind::Book))
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(lib_svc.list_libraries(None).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn auth_setup_race_disabled_user_and_library_mount_guard_and_scan_recovery() {
        let dir = TempDir::new().unwrap();
        let data_dir = dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let db_path = data_dir.join("test.db");
        let db_url = format!("sqlite:{}", db_path.display());

        let mut models = ModelSet::new();
        super::register_core_models(&mut models);
        let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
        super::ensure_core_schema(&mut db, &db_url).await.unwrap();

        let users = Arc::new(ToastyUserRepository::new(db.clone()));
        let libraries_repo = Arc::new(ToastyLibraryRepository::new(db));
        let lifecycles = Arc::new(RwLock::new(HashMap::new()));
        let security = Arc::new(SecurityManager::new("test-secret-key"));
        let tokens = Arc::new(JwtTokenService::new("test-secret-key", 1800));
        let events = Arc::new(BroadcastEventHub::default());

        let auth = AuthService::new(users, security, tokens, lifecycles.clone());
        let lib_svc = LibraryService::with_data_dir(
            libraries_repo.clone(),
            events,
            lifecycles,
            Some(data_dir.clone()),
        );
        lib_svc.register_lifecycle(Arc::new(DummyLifecycle(MediaKind::Book)));

        assert_eq!(auth.token_ttl_seconds(), 1800);

        // Concurrent initial setup should allow only one admin
        let (r1, r2) = tokio::join!(
            auth.setup_initial_admin(SetupAdminInput {
                username: "admin1".into(),
                display_name: None,
                password: "password123".into(),
            }),
            auth.setup_initial_admin(SetupAdminInput {
                username: "admin2".into(),
                display_name: None,
                password: "password123".into(),
            }),
        );
        let success_count = usize::from(r1.is_ok()) + usize::from(r2.is_ok());
        assert_eq!(success_count, 1);
        let admin_session = r1.or(r2).unwrap();
        let admin = auth.authenticate_jwt(&admin_session.token).await.unwrap();

        // Create member, verify list_users redacts api_key while get_me returns own api_key,
        // change password reissues token, and disabling yields Forbidden across ALL auth entry points
        let member = auth
            .create_user(
                &admin,
                CreateUserInput {
                    username: "member1".into(),
                    display_name: None,
                    password: "password123".into(),
                    role: UserRole::Member,
                },
            )
            .await
            .unwrap();
        let listed_users = auth.list_users(&admin).await.unwrap();
        assert_eq!(listed_users.len(), 2);
        assert!(listed_users.iter().all(|u| u.api_key.is_empty()));
        let admin_me = auth.get_me(&admin).await.unwrap();
        assert!(!admin_me.api_key.is_empty());

        let member_session = auth.login("member1", "password123").await.unwrap();
        let member_actor = auth.authenticate_jwt(&member_session.token).await.unwrap();
        let rotated_session = auth
            .change_own_password(
                &member_actor,
                ChangePasswordInput {
                    current_password: "password123".into(),
                    new_password: "newpassword123".into(),
                },
            )
            .await
            .unwrap();
        assert!(auth.authenticate_jwt(&rotated_session.token).await.is_ok());

        let updated_member = auth
            .update_user(
                &admin,
                member.id,
                UpdateUserInput {
                    display_name: None,
                    role: None,
                    is_active: Some(false),
                    password: None,
                },
            )
            .await
            .unwrap();
        assert!(updated_member.api_key.is_empty());

        // Disabled account must consistently return 403 Forbidden across all 5 auth entry points
        assert!(matches!(
            auth.login("member1", "newpassword123").await,
            Err(DomainError::Forbidden(_))
        ));
        assert!(matches!(
            auth.authenticate_jwt(&rotated_session.token).await,
            Err(DomainError::Forbidden(_))
        ));
        assert!(matches!(
            auth.authenticate_api_key(&member.api_key).await,
            Err(DomainError::Forbidden(_))
        ));
        assert!(matches!(
            auth.verify_basic_credentials("member1", "newpassword123")
                .await,
            Err(DomainError::Forbidden(_))
        ));
        let salt = "salt42";
        let mut md5 = Md5::new();
        md5.update(b"newpassword123");
        md5.update(salt.as_bytes());
        let sub_token = hex::encode(md5.finalize());
        assert!(matches!(
            auth.authenticate_subsonic(SubsonicCredentials::TokenSalt {
                username: "member1".into(),
                token: sub_token,
                salt: salt.into(),
            })
            .await,
            Err(DomainError::Forbidden(_))
        ));

        // Mount path validation against data_dir and nested mounts
        let inside_data = data_dir.join("submount");
        std::fs::create_dir_all(&inside_data).unwrap();
        assert!(matches!(
            lib_svc
                .create_local_mount(
                    &admin,
                    CreateLibraryInput {
                        name: "BadDataMount".into(),
                        media_kind: MediaKind::Book,
                        root_path: inside_data.to_string_lossy().into_owned(),
                    }
                )
                .await,
            Err(DomainError::Validation(_))
        ));

        let books_root = dir.path().join("books");
        let books_child = books_root.join("child");
        std::fs::create_dir_all(&books_child).unwrap();
        let mounted = lib_svc
            .create_local_mount(
                &admin,
                CreateLibraryInput {
                    name: "Books".into(),
                    media_kind: MediaKind::Book,
                    root_path: books_root.to_string_lossy().into_owned(),
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            lib_svc
                .create_local_mount(
                    &admin,
                    CreateLibraryInput {
                        name: "NestedBooks".into(),
                        media_kind: MediaKind::Book,
                        root_path: books_child.to_string_lossy().into_owned(),
                    }
                )
                .await,
            Err(DomainError::Validation(_))
        ));

        // Simulate stale Scanning status on startup recovery
        let mut stale = libraries_repo
            .find_by_id(mounted.id)
            .await
            .unwrap()
            .unwrap();
        stale.mark_scanning(42, Some(100));
        libraries_repo.save(&stale).await.unwrap();
        lib_svc.recover_interrupted_scans().await.unwrap();
        let recovered = libraries_repo
            .find_by_id(mounted.id)
            .await
            .unwrap()
            .unwrap();
        assert!(!recovered.scan_status.is_scanning());

        // Test injected allowed_mount_prefixes without mutating process env
        let allowed_root = dir.path().join("allowed_only");
        let allowed_sub = allowed_root.join("sub");
        let denied_dir = dir.path().join("denied_dir");
        std::fs::create_dir_all(&allowed_sub).unwrap();
        std::fs::create_dir_all(&denied_dir).unwrap();
        let restricted_lib_svc = lib_svc.with_allowed_mount_prefixes([allowed_root]);
        assert!(matches!(
            restricted_lib_svc
                .create_local_mount(
                    &admin,
                    CreateLibraryInput {
                        name: "Denied".into(),
                        media_kind: MediaKind::Book,
                        root_path: denied_dir.to_string_lossy().into_owned(),
                    }
                )
                .await,
            Err(DomainError::Forbidden(_))
        ));
        let allowed_lib = restricted_lib_svc
            .create_local_mount(
                &admin,
                CreateLibraryInput {
                    name: "Allowed".into(),
                    media_kind: MediaKind::Book,
                    root_path: allowed_sub.to_string_lossy().into_owned(),
                },
            )
            .await
            .unwrap();

        // Test ScanProgressReporter throttled updates + completion + MediaChanged broadcast
        let events_hub = Arc::new(BroadcastEventHub::default());
        let mut rx = events_hub.subscribe();
        let lib_entity = libraries_repo
            .find_by_id(allowed_lib.id)
            .await
            .unwrap()
            .unwrap();
        let mut reporter = crate::application::library::ScanProgressReporter::new(
            lib_entity,
            libraries_repo.clone(),
            events_hub,
            Some(3),
        )
        .with_step(2);
        reporter.start().await;
        reporter.advance().await;
        reporter.advance().await;
        reporter.advance().await;
        let finished = reporter.complete(2, 1, 0).await.unwrap();
        assert!(matches!(
            finished.scan_status,
            crate::domain::library::ScanStatus::Completed {
                scanned: 3,
                added: 2,
                updated: 1,
                missing: 0,
            }
        ));
        let mut seen = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            seen.push(ev);
        }
        // start(0) + step(2) + total(3) + completed + media_changed = 5 events
        assert_eq!(seen.len(), 5);
    }
}

#[cfg(test)]
mod lite_tests {
    use std::str::FromStr;

    crate::define_uuid_id!(SampleMediaId);

    #[test]
    fn define_uuid_id_macro_supports_roundtrip_and_serde() {
        let id = SampleMediaId::new();
        let text = id.to_string();
        let parsed = SampleMediaId::from_str(&text).unwrap();
        assert_eq!(id, parsed);

        let json = serde_json::to_string(&id).unwrap();
        let decoded: SampleMediaId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, decoded);
    }
}
