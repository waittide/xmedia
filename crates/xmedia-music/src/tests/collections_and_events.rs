use xmedia_core::application::dto::{CreateLibraryInput, CreateUserInput, SetupAdminInput};
use xmedia_core::application::ports::SystemEvent;
use xmedia_core::domain::error::DomainError;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::UserRole;

use super::{make_wav, setup_test_env};
use crate::application::dto::{BindMusicItemsInput, UpdatePlaylistInput, UploadFilePathItem};
use crate::domain::catalog::{
    Album, AlbumId, Artist, ArtistId, AudioFormat, ReplayGain, Track, TrackId, TrackSource,
};
use crate::domain::repository::{
    AlbumScope, ArtistScope, CatalogFacetQuery, CatalogFacetResult, CatalogMutation,
    MusicCatalogRepository, MusicCollectionRepository, PlaylistQuery, TrackScope,
};
use crate::infra::persistence::repositories::{
    ToastyMusicCatalogRepository, ToastyMusicCollectionRepository,
};

#[tokio::test]
async fn upload_files_publishes_media_changed_event() {
    let env = setup_test_env("upload_event.db").await;
    let session = env
        .auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: None,
            password: "password123".into(),
        })
        .await
        .unwrap();
    let admin = env
        .auth_svc
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&session.token),
            None,
        )
        .await
        .unwrap();
    let _member_dto = env
        .auth_svc
        .create_user(
            &admin,
            CreateUserInput {
                username: "member".into(),
                display_name: None,
                password: "password123".into(),
                role: UserRole::Member,
            },
        )
        .await
        .unwrap();
    let member_session = env
        .auth_svc
        .login("member", "password123", None)
        .await
        .unwrap();
    let member = env
        .auth_svc
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&member_session.token),
            None,
        )
        .await
        .unwrap();

    let mut rx = env.events.subscribe();

    // Non-admin upload must be rejected without emitting any event
    let blocked_wav = env.tmp.path().join("Blocked.wav");
    std::fs::write(&blocked_wav, make_wav(8_000)).unwrap();
    assert!(matches!(
        env.music_services
            .ingest
            .upload_files_from_paths(
                &member,
                vec![UploadFilePathItem {
                    filename: "Blocked.wav".into(),
                    temp_path: blocked_wav,
                }],
            )
            .await,
        Err(DomainError::Forbidden(_))
    ));
    assert!(rx.try_recv().is_err());

    // Admin upload_files_from_paths emits MediaChanged(Music)
    let temp_wav_a = env.tmp.path().join("SongA.wav");
    std::fs::write(&temp_wav_a, make_wav(8_000)).unwrap();
    env.music_services
        .ingest
        .upload_files_from_paths(
            &admin,
            vec![UploadFilePathItem {
                filename: "SongA.wav".into(),
                temp_path: temp_wav_a,
            }],
        )
        .await
        .unwrap();
    assert!(matches!(
        rx.try_recv(),
        Ok(SystemEvent::MediaChanged {
            media_kind: MediaKind::Music
        })
    ));

    // Second upload_files_from_paths also emits MediaChanged(Music)
    let temp_wav = env.tmp.path().join("SongB.wav");
    std::fs::write(&temp_wav, make_wav(8_000)).unwrap();
    env.music_services
        .ingest
        .upload_files_from_paths(
            &admin,
            vec![UploadFilePathItem {
                filename: "SongB.wav".into(),
                temp_path: temp_wav,
            }],
        )
        .await
        .unwrap();
    assert!(matches!(
        rx.try_recv(),
        Ok(SystemEvent::MediaChanged {
            media_kind: MediaKind::Music
        })
    ));
}

#[tokio::test]
async fn delete_library_cascades_all_collection_references() {
    let env = setup_test_env("delete_cascade.db").await;
    let session = env
        .auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: None,
            password: "password123".into(),
        })
        .await
        .unwrap();
    let admin = env
        .auth_svc
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&session.token),
            None,
        )
        .await
        .unwrap();

    let managed_lib = env
        .library_svc
        .ensure_managed_library(MediaKind::Music)
        .await
        .unwrap();
    let mount_dir = env.tmp.path().join("mount_cascade");
    std::fs::create_dir_all(&mount_dir).unwrap();
    let mounted_lib = env
        .library_svc
        .create_local_mount(
            &admin,
            CreateLibraryInput {
                name: "ToDelete".into(),
                media_kind: MediaKind::Music,
                root_path: mount_dir.to_string_lossy().into_owned(),
            },
        )
        .await
        .unwrap();

    let catalog_repo = ToastyMusicCatalogRepository::new(env.db.clone());
    let collections_repo = ToastyMusicCollectionRepository::new(env.db.clone());
    let now = jiff::Timestamp::now();

    let shared_artist = Artist {
        id: ArtistId::new(),
        name: "Shared Artist".into(),
        is_album_artist: true,
        is_track_artist: true,
        album_count: 1,
        track_count: 2,
        cover_key: None,
        created_at: now,
        updated_at: now,
    };
    let exclusive_artist = Artist {
        id: ArtistId::new(),
        name: "Exclusive Artist".into(),
        is_album_artist: true,
        is_track_artist: true,
        album_count: 1,
        track_count: 1,
        cover_key: None,
        created_at: now,
        updated_at: now,
    };
    catalog_repo
        .save(CatalogMutation::Artist(&shared_artist))
        .await
        .unwrap();
    catalog_repo
        .save(CatalogMutation::Artist(&exclusive_artist))
        .await
        .unwrap();

    let keep_album = Album {
        id: AlbumId::new(),
        library_id: managed_lib.id,
        album_key: Album::lookup_key(managed_lib.id, "Keep Album", &shared_artist.name, false).1,
        title: "Keep Album".into(),
        album_artist_id: shared_artist.id,
        album_artist_name: shared_artist.name.clone(),
        year: Some(2024),
        genre: Some("Jazz".into()),
        disc_count: 1,
        track_count: 1,
        duration_secs: 180,
        is_compilation: false,
        cover_key: None,
        created_at: now,
        updated_at: now,
    };
    let del_album = Album {
        id: AlbumId::new(),
        library_id: mounted_lib.id,
        album_key: Album::lookup_key(
            mounted_lib.id,
            "Delete Album",
            &exclusive_artist.name,
            false,
        )
        .1,
        title: "Delete Album".into(),
        album_artist_id: exclusive_artist.id,
        album_artist_name: exclusive_artist.name.clone(),
        year: Some(2023),
        genre: Some("Rock".into()),
        disc_count: 1,
        track_count: 1,
        duration_secs: 200,
        is_compilation: false,
        cover_key: None,
        created_at: now,
        updated_at: now,
    };
    catalog_repo
        .save(CatalogMutation::Album(&keep_album))
        .await
        .unwrap();
    catalog_repo
        .save(CatalogMutation::Album(&del_album))
        .await
        .unwrap();

    let keep_track = Track {
        id: TrackId::new(),
        library_id: managed_lib.id,
        album_id: keep_album.id,
        artist_id: shared_artist.id,
        album_artist_id: shared_artist.id,
        title: "Keep Track".into(),
        artist_name: shared_artist.name.clone(),
        album_artist_name: shared_artist.name.clone(),
        album_title: keep_album.title.clone(),
        track_number: Some(1),
        disc_number: Some(1),
        year: Some(2024),
        genre: Some("Jazz".into()),
        duration_secs: 180,
        bit_rate_kbps: Some(320),
        sample_rate_hz: Some(44_100),
        bit_depth: None,
        channels: Some(2),
        format: AudioFormat::Mp3,
        file_size: 1024,
        file_hash: "hash-keep".into(),
        replay_gain: ReplayGain::default(),
        has_embedded_lyrics: false,
        has_sidecar_lyrics: false,
        is_compilation: false,
        cover_key: None,
        source: TrackSource::Managed {
            storage_key: "keep.mp3".into(),
            sidecar_lrc_key: None,
        },
        is_missing: false,
        created_at: now,
        updated_at: now,
    };
    let del_track = Track {
        id: TrackId::new(),
        library_id: mounted_lib.id,
        album_id: del_album.id,
        artist_id: shared_artist.id,
        album_artist_id: exclusive_artist.id,
        title: "Delete Track".into(),
        artist_name: shared_artist.name.clone(),
        album_artist_name: exclusive_artist.name.clone(),
        album_title: del_album.title.clone(),
        track_number: Some(1),
        disc_number: Some(1),
        year: Some(2023),
        genre: Some("Rock".into()),
        duration_secs: 200,
        bit_rate_kbps: Some(320),
        sample_rate_hz: Some(44_100),
        bit_depth: None,
        channels: Some(2),
        format: AudioFormat::Flac,
        file_size: 2048,
        file_hash: "hash-del".into(),
        replay_gain: ReplayGain::default(),
        has_embedded_lyrics: false,
        has_sidecar_lyrics: false,
        is_compilation: false,
        cover_key: None,
        source: TrackSource::LocalPath {
            relative_path: "del.flac".into(),
            file_mtime: 1,
            sidecar_lrc_path: None,
        },
        is_missing: false,
        created_at: now,
        updated_at: now,
    };
    catalog_repo
        .save(CatalogMutation::Track(&keep_track))
        .await
        .unwrap();
    catalog_repo
        .save(CatalogMutation::Track(&del_track))
        .await
        .unwrap();
    catalog_repo
        .save(CatalogMutation::RecomputeAggregates)
        .await
        .unwrap();
    env.music_services
        .ingest
        .bind_tracks(
            &admin,
            BindMusicItemsInput {
                item_ids: vec![keep_track.id.0, del_track.id.0],
                ..Default::default()
            },
        )
        .await
        .unwrap();

    // Populate stars, ratings, playlist, bookmarks, play queue, scrobbles/stats, and tags
    env.music_services
        .collection
        .set_starred(
            admin.id,
            true,
            &[del_track.id.to_string(), keep_track.id.to_string()],
            &[del_album.id, keep_album.id],
            &[exclusive_artist.id, shared_artist.id],
        )
        .await
        .unwrap();
    for target in [
        del_track.id.to_string(),
        keep_track.id.to_string(),
        del_album.id.to_string(),
        keep_album.id.to_string(),
        exclusive_artist.id.to_string(),
        shared_artist.id.to_string(),
    ] {
        env.music_services
            .collection
            .set_rating(admin.id, &target, 5)
            .await
            .unwrap();
    }
    let playlist = env
        .music_services
        .collection
        .create_playlist(
            &admin,
            "Mixed Playlist",
            None,
            true,
            vec![del_track.id, keep_track.id],
        )
        .await
        .unwrap();
    env.music_services
        .collection
        .upsert_bookmark(admin.id, del_track.id, 12_000, Some("del bm".into()))
        .await
        .unwrap();
    env.music_services
        .collection
        .upsert_bookmark(admin.id, keep_track.id, 24_000, Some("keep bm".into()))
        .await
        .unwrap();
    env.music_services
        .collection
        .save_play_queue(
            admin.id,
            vec![del_track.id, keep_track.id],
            Some(del_track.id),
            42_000,
            "unit-test",
        )
        .await
        .unwrap();
    env.music_services
        .collection
        .scrobble(
            &admin,
            vec![(del_track.id, None), (keep_track.id, None)],
            true,
            "unit-test",
        )
        .await
        .unwrap();
    env.music_services
        .catalog
        .set_track_tags(&admin, del_track.id, vec!["ephemeral".into()])
        .await
        .unwrap();

    let mut rx = env.events.subscribe();
    env.library_svc
        .delete_library(&admin, mounted_lib.id)
        .await
        .unwrap();

    // MediaChanged event emitted
    assert!(matches!(
        rx.try_recv(),
        Ok(SystemEvent::MediaChanged {
            media_kind: MediaKind::Music
        })
    ));

    // Catalog entities deleted (shared artist survives)
    assert!(
        catalog_repo
            .query_tracks(TrackScope::Id(del_track.id))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        catalog_repo
            .query_albums(AlbumScope::Id(del_album.id))
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        catalog_repo
            .query_artists(ArtistScope::Id(exclusive_artist.id))
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        !catalog_repo
            .query_artists(ArtistScope::Id(shared_artist.id))
            .await
            .unwrap()
            .items
            .is_empty()
    );

    // Stars cleaned for deleted track/album/exclusive artist; surviving stars intact
    let engagement = collections_repo
        .load_engagement(admin.id, None)
        .await
        .unwrap();
    let starred_ids: Vec<_> = engagement.starred.iter().map(|s| s.target_id).collect();
    assert!(!starred_ids.contains(&del_track.id.0));
    assert!(!starred_ids.contains(&del_album.id.0));
    assert!(!starred_ids.contains(&exclusive_artist.id.0));
    assert!(starred_ids.contains(&keep_track.id.0));
    assert!(starred_ids.contains(&keep_album.id.0));
    assert!(starred_ids.contains(&shared_artist.id.0));

    // Ratings cleaned for deleted track/album/exclusive artist; surviving ratings intact
    let rated_ids: Vec<_> = engagement.ratings.iter().map(|r| r.target_id).collect();
    assert!(!rated_ids.contains(&del_track.id.0));
    assert!(!rated_ids.contains(&del_album.id.0));
    assert!(!rated_ids.contains(&exclusive_artist.id.0));
    assert!(rated_ids.contains(&keep_track.id.0));
    assert!(rated_ids.contains(&keep_album.id.0));
    assert!(rated_ids.contains(&shared_artist.id.0));

    // Playlist tracks stripped of deleted track, surviving track retained
    let pl_after = collections_repo
        .query_playlists(PlaylistQuery::Id(playlist.playlist.id))
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(pl_after.track_ids, vec![keep_track.id]);

    // Bookmarks cleaned
    let bms = collections_repo
        .query_bookmarks(admin.id, None)
        .await
        .unwrap();
    assert_eq!(bms.len(), 1);
    assert_eq!(bms[0].track_id, keep_track.id);

    // Play queue cleaned and current track advanced to surviving track with reset position
    let queue = collections_repo
        .get_play_queue(admin.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(queue.track_ids, vec![keep_track.id]);
    assert_eq!(queue.current_track_id, Some(keep_track.id));
    assert_eq!(queue.position_ms, 0);

    // Play stats and scrobbles cleaned
    assert_eq!(engagement.play_stats.len(), 1);
    assert_eq!(engagement.play_stats[0].track_id, keep_track.id);
    let scrobbles = collections_repo
        .list_recent_scrobbles(admin.id, 20)
        .await
        .unwrap();
    assert_eq!(scrobbles.len(), 1);
    assert_eq!(scrobbles[0].track_id, keep_track.id);

    // Tags cleaned
    assert!(matches!(
        catalog_repo
            .query_facets(CatalogFacetQuery::Tags(None))
            .await
            .unwrap(),
        CatalogFacetResult::Tags(tags) if tags.is_empty()
    ));
}

#[tokio::test]
async fn playlist_visibility_rules_for_public_and_private_playlists() {
    let env = setup_test_env("playlist_visibility.db").await;
    let admin_session = env
        .auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: None,
            password: "password123".into(),
        })
        .await
        .unwrap();
    let admin = env
        .auth_svc
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&admin_session.token),
            None,
        )
        .await
        .unwrap();
    let _alice_dto = env
        .auth_svc
        .create_user(
            &admin,
            CreateUserInput {
                username: "alice".into(),
                display_name: None,
                password: "password123".into(),
                role: UserRole::Member,
            },
        )
        .await
        .unwrap();
    let _bob_dto = env
        .auth_svc
        .create_user(
            &admin,
            CreateUserInput {
                username: "bob".into(),
                display_name: None,
                password: "password123".into(),
                role: UserRole::Member,
            },
        )
        .await
        .unwrap();
    let alice_session = env
        .auth_svc
        .login("alice", "password123", None)
        .await
        .unwrap();
    let bob_session = env
        .auth_svc
        .login("bob", "password123", None)
        .await
        .unwrap();
    let alice = env
        .auth_svc
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&alice_session.token),
            None,
        )
        .await
        .unwrap();
    let bob = env
        .auth_svc
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&bob_session.token),
            None,
        )
        .await
        .unwrap();

    let alice_private = env
        .music_services
        .collection
        .create_playlist(&alice, "Alice Private", None, false, vec![])
        .await
        .unwrap();
    let alice_public = env
        .music_services
        .collection
        .create_playlist(&alice, "Alice Public", None, true, vec![])
        .await
        .unwrap();
    let bob_private = env
        .music_services
        .collection
        .create_playlist(&bob, "Bob Private", None, false, vec![])
        .await
        .unwrap();

    // Alice sees her own private + public playlists, not Bob's private
    let alice_list: Vec<_> = env
        .music_services
        .collection
        .list_playlists(alice.id)
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(alice_list.len(), 2);
    assert!(alice_list.contains(&alice_private.playlist.id));
    assert!(alice_list.contains(&alice_public.playlist.id));

    // Bob sees his own private playlist + Alice's public playlist, not Alice's private
    let bob_list: Vec<_> = env
        .music_services
        .collection
        .list_playlists(bob.id)
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(bob_list.len(), 2);
    assert!(bob_list.contains(&bob_private.playlist.id));
    assert!(bob_list.contains(&alice_public.playlist.id));
    assert!(!bob_list.contains(&alice_private.playlist.id));

    // Home overview playlist count aligns with visible playlists
    let bob_overview = env
        .music_services
        .catalog
        .get_home_overview(bob.id)
        .await
        .unwrap();
    assert_eq!(bob_overview.stats.playlists_count, 2);

    // Reading: Bob can read Alice's public playlist, cannot read Alice's private playlist
    assert!(
        env.music_services
            .collection
            .get_playlist(&bob, alice_public.playlist.id)
            .await
            .is_ok()
    );
    assert!(matches!(
        env.music_services
            .collection
            .get_playlist(&bob, alice_private.playlist.id)
            .await,
        Err(DomainError::Forbidden(_))
    ));
    // Admin can read private playlist
    assert!(
        env.music_services
            .collection
            .get_playlist(&admin, alice_private.playlist.id)
            .await
            .is_ok()
    );

    // Mutation & deletion: only owner or admin allowed, even for public playlists
    assert!(matches!(
        env.music_services
            .collection
            .update_playlist(
                &bob,
                alice_public.playlist.id,
                UpdatePlaylistInput {
                    name: Some("Hijacked".into()),
                    ..Default::default()
                },
            )
            .await,
        Err(DomainError::Forbidden(_))
    ));
    assert!(matches!(
        env.music_services
            .collection
            .delete_playlist(&bob, alice_public.playlist.id)
            .await,
        Err(DomainError::Forbidden(_))
    ));

    // Owner toggles private -> public, making it visible and readable to Bob
    env.music_services
        .collection
        .update_playlist(
            &alice,
            alice_private.playlist.id,
            UpdatePlaylistInput {
                is_public: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let bob_list_after = env
        .music_services
        .collection
        .list_playlists(bob.id)
        .await
        .unwrap();
    assert_eq!(bob_list_after.len(), 3);
    assert!(
        env.music_services
            .collection
            .get_playlist(&bob, alice_private.playlist.id)
            .await
            .is_ok()
    );

    // Admin can modify and delete another user's playlist
    env.music_services
        .collection
        .update_playlist(
            &admin,
            alice_public.playlist.id,
            UpdatePlaylistInput {
                name: Some("Moderated".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    env.music_services
        .collection
        .delete_playlist(&admin, alice_public.playlist.id)
        .await
        .unwrap();
}
