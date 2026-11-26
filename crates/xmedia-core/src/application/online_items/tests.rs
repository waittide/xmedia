use super::*;
use crate::application::auth::AuthService;
use crate::application::bindings::MediaBindingService;
use crate::application::dto::{CreateUserInput, SetupAdminInput};
use crate::domain::repository::{UserLookup, UserRepository};
use crate::domain::user::UserRole;
use crate::infra::persistence::models::{LibraryModel, UserModel};
use crate::infra::persistence::repositories::{
    ToastyOnlineLibraryItemRepository, ToastyUserMediaBindingRepository, ToastyUserRepository,
};
use crate::infra::security::{JwtTokenService, SecurityManager};
use std::sync::RwLock;
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

    svc.update(
        user,
        a.id,
        OnlineItemMutation::Progress(Some(OnlineItemProgress {
            unit_index: Some(1),
            total_units: Some(4),
            position: 5.0,
            duration: Some(10.0),
            ..Default::default()
        })),
    )
    .await
    .unwrap();
    let b_progress = svc
        .update(
            user,
            b.id,
            OnlineItemMutation::Progress(Some(OnlineItemProgress {
                position: 1.0,
                ..Default::default()
            })),
        )
        .await
        .unwrap();
    assert_eq!(
        b_progress.availability,
        OnlineItemAvailability::SourceDisabled
    );
    svc.update(
        user,
        c.id,
        OnlineItemMutation::Progress(Some(OnlineItemProgress {
            completed: true,
            ..Default::default()
        })),
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

    svc.update(
        user,
        c.id,
        OnlineItemMutation::Metadata(PatchOnlineItemInput {
            favorite: Some(true),
            ..Default::default()
        }),
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

    let cleared = svc
        .update(user, a.id, OnlineItemMutation::Progress(None))
        .await
        .unwrap();
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
async fn auth_service_delete_user_purges_online_items_and_bindings_via_ports() {
    let fx = fixture().await;
    let users_repo = Arc::new(ToastyUserRepository::new(fx.db.clone()));
    let online_repo = Arc::new(ToastyOnlineLibraryItemRepository::new(fx.db.clone()));
    let binding_repo = Arc::new(ToastyUserMediaBindingRepository::new(fx.db.clone()));
    let bindings = Arc::new(MediaBindingService::new(binding_repo, None));

    let auth = AuthService::new(
        users_repo.clone(),
        Arc::new(SecurityManager::new("purge-test-secret")),
        Arc::new(JwtTokenService::new("purge-test-secret", 3600)),
        Arc::new(RwLock::new(HashMap::new())),
    )
    .with_cleanup(None, Some(online_repo.clone()), Some(bindings.clone()));

    let admin_sess = auth
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: None,
            password: "password123".into(),
        })
        .await
        .unwrap();
    let admin = users_repo
        .find(UserLookup::Id(admin_sess.user.id))
        .await
        .unwrap()
        .unwrap();

    let alice = auth
        .create_user(
            &admin,
            CreateUserInput {
                username: "alice".into(),
                display_name: Some("Alice".into()),
                password: "password123".into(),
                role: UserRole::Member,
            },
        )
        .await
        .unwrap();

    let svc = service(&fx, MediaKind::Music);
    let item = svc
        .upsert(alice.id, upsert_input(fx.enabled, "track-1"))
        .await
        .unwrap();
    let bound_item = Uuid::now_v7();
    assert!(
        bindings
            .bind(alice.id, MediaKind::Music, bound_item)
            .await
            .unwrap()
    );

    auth.delete_user(&admin, alice.id).await.unwrap();

    assert!(online_repo.find_by_id(item.id).await.unwrap().is_none());
    assert!(
        !bindings
            .is_bound(alice.id, MediaKind::Music, bound_item)
            .await
            .unwrap()
    );
}
