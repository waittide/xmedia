pub mod application;
pub mod domain;
pub mod http;
pub mod infra;

pub use infra::persistence::{ensure_core_schema, ensure_sqlite_parent_dir, register_core_models};

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};

    use md5::{Digest, Md5};
    use tempfile::TempDir;
    use toasty::Db;
    use toasty::schema::ModelSet;

    use crate::application::auth::AuthService;
    use crate::application::dto::{SetupAdminInput, SubsonicCredentials};
    use crate::application::library::LibraryService;
    use crate::domain::library::MediaKind;
    use crate::infra::events::BroadcastEventHub;
    use crate::infra::persistence::repositories::{ToastyLibraryRepository, ToastyUserRepository};
    use crate::infra::security::{JwtTokenService, SecurityManager};

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
}
