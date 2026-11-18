pub mod application;
pub mod domain;
pub mod http;
pub mod infra;

pub use infra::persistence::{ensure_core_schema, ensure_sqlite_parent_dir, register_core_models};

#[cfg(test)]
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

        // Create member, change password reissues token, and disabling yields Unauthorized
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

        auth.update_user(
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
        assert!(matches!(
            auth.authenticate_jwt(&rotated_session.token).await,
            Err(DomainError::Unauthorized(_))
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
    }
}
