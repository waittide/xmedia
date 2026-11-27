use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tempfile::TempDir;
use tower::ServiceExt;
use xmedia_core::domain::error::DomainError;
use xmedia_core::domain::library::MediaKind;
use xmedia_tool::application::ops::{
    BatchComicSeriesSyncInput, BatchMusicAlbumSyncInput, BatchRemapEpisodesInput,
};
use xmedia_tool::application::ports::ArtworkIo;
use xmedia_tool::application::scraping::ApplyScrapeInput;
use xmedia_tool::application::workbench::{
    BatchSyncSpec, PreviewRenameOptions, WorkspaceArtworkOutput,
};
use xmedia_tool::domain::media::{
    BookMetaPatch, ComicMetaPatch, MediaMetadataPatch, MediaMetadataPayload, MusicMetaPatch,
    RenameConflictPolicy, RenamePlanItem, VideoMetaPatch,
};
use xmedia_tool::{XMediaToolApp, XMediaToolConfig};
use zip::CompressionMethod;

use crate::common::{TINY_PNG, write_minimal_cbz, write_minimal_epub, write_minimal_wav};

#[tokio::test]
async fn password_guard_and_root_sandbox_traversal_rejection() {
    let tmp = TempDir::new().unwrap();
    let media_root = tmp.path().join("media");
    std::fs::create_dir_all(&media_root).unwrap();
    std::fs::write(tmp.path().join("outside_secret.txt"), b"top-secret").unwrap();

    let app = XMediaToolApp::init(XMediaToolConfig {
        data_dir: tmp.path().join("data"),
        initial_roots: vec![media_root],
        password: Some("tool-pass-2026".into()),
        ffmpeg_bin: "__missing_ffmpeg__".into(),
        external_web_dir: None,
    })
    .await
    .unwrap();

    let status_res = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/tool/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status_res.status(), StatusCode::OK);

    let unauth_res = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/tool/settings")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauth_res.status(), StatusCode::UNAUTHORIZED);

    let unlock_res = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/tool/auth/unlock")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"password":"tool-pass-2026"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unlock_res.status(), StatusCode::OK);
    let unlock_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(unlock_res.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    let token = unlock_json["token"].as_str().unwrap();

    let settings_res = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/tool/settings")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(settings_res.status(), StatusCode::OK);
    let settings_json: serde_json::Value = serde_json::from_slice(
        &to_bytes(settings_res.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let root_id = settings_json["roots"][0]["id"].as_str().unwrap();

    let trav_res = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/tool/fs/browse?root_id={root_id}&path=../outside_secret.txt"
                ))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(trav_res.status(), StatusCode::FORBIDDEN);

    let rebind_res = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/tool/status")
                .header(header::HOST, "evil.rebind.test:8091")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rebind_res.status(), StatusCode::FORBIDDEN);

    let cross_site_res = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/tool/auth/lock")
                .header(header::HOST, "127.0.0.1:8091")
                .header("sec-fetch-site", "cross-site")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cross_site_res.status(), StatusCode::FORBIDDEN);

    let cross_origin_res = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/tool/auth/lock")
                .header(header::HOST, "127.0.0.1:8091")
                .header(header::ORIGIN, "http://127.0.0.1:3000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cross_origin_res.status(), StatusCode::FORBIDDEN);

    let lock_res = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/tool/auth/lock")
                .header(header::HOST, "127.0.0.1:8091")
                .header(header::ORIGIN, "http://127.0.0.1:8091")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(lock_res.status(), StatusCode::OK);

    let after_lock_res = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/tool/settings")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(after_lock_res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn all_four_domains_atomic_metadata_artwork_and_rename_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let media_root = tmp.path().join("library");
    std::fs::create_dir_all(&media_root).unwrap();

    let wav_path = media_root.join("raw_song.wav");
    let cbz_path = media_root.join("raw_chapter.cbz");
    let epub_path = media_root.join("raw_book.epub");
    let mkv_path = media_root.join("raw_ep.mkv");
    let srt_path = media_root.join("raw_ep.zh.srt");

    write_minimal_wav(&wav_path);
    write_minimal_cbz(&cbz_path);
    write_minimal_epub(&epub_path);
    std::fs::write(&mkv_path, b"dummy-matroska-bytes").unwrap();
    std::fs::write(&srt_path, b"1\n00:00:01,000 --> 00:00:02,000\nHello\n").unwrap();

    let app = XMediaToolApp::init(XMediaToolConfig {
        data_dir: tmp.path().join("data"),
        initial_roots: vec![media_root.clone()],
        password: None,
        ffmpeg_bin: "__missing_ffmpeg__".into(),
        external_web_dir: None,
    })
    .await
    .unwrap();

    let root_id = app.settings.get_public_settings().await.unwrap().roots[0].id;

    let music_detail = app
        .metadata
        .write(
            root_id,
            "raw_song.wav",
            MediaMetadataPatch::Music(MusicMetaPatch {
                title: Some("晴天".into()),
                artist: Some("周杰伦".into()),
                album: Some("叶惠美".into()),
                track_number: Some(3),
                year: Some(2003),
                embedded_lyrics: Some("[00:01.00]故事的小黄花".into()),
                sidecar_lrc: Some("[00:01.00]故事的小黄花".into()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    if let MediaMetadataPayload::Music(m) = music_detail.metadata {
        assert_eq!(m.title, "晴天");
        assert_eq!(m.artist, "周杰伦");
        assert_eq!(m.track_number, Some(3));
        assert_eq!(m.embedded_lyrics.as_deref(), Some("[00:01.00]故事的小黄花"));
        assert_eq!(m.sidecar_lrc.as_deref(), Some("[00:01.00]故事的小黄花"));
    } else {
        panic!("expected music payload");
    }
    app.workspace
        .artwork(
            root_id,
            "raw_song.wav",
            Some(MediaKind::Music),
            ArtworkIo::Write {
                slot: "cover",
                bytes: TINY_PNG,
                mime_type: "image/png",
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        app.workspace
            .artwork(
                root_id,
                "raw_song.wav",
                Some(MediaKind::Music),
                ArtworkIo::Read { slot: None },
            )
            .await
            .unwrap(),
        WorkspaceArtworkOutput::Read(Some(_))
    ));

    let music_plans = app
        .rename
        .preview(
            root_id,
            &["raw_song.wav".to_string()],
            PreviewRenameOptions {
                template_override: Some("{track:2} - {artist} - {title}"),
                preferred_kind: Some(MediaKind::Music),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(music_plans[0].to_rel_path, "03 - 周杰伦 - 晴天.wav");
    assert_eq!(music_plans[0].companion_moves.len(), 1);
    app.rename
        .execute(root_id, &music_plans, None)
        .await
        .unwrap();
    assert!(media_root.join("03 - 周杰伦 - 晴天.wav").exists());
    assert!(media_root.join("03 - 周杰伦 - 晴天.lrc").exists());

    let comic_detail = app
        .metadata
        .write(
            root_id,
            "raw_chapter.cbz",
            MediaMetadataPatch::Comic(ComicMetaPatch {
                series: Some("葬送的芙莉莲".into()),
                title: Some("冒险的终点".into()),
                number: Some("1".into()),
                writers: Some(vec!["山田钟人".into()]),
                pencillers: Some(vec!["阿部司".into()]),
                year: Some(2020),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    assert!(comic_detail.entry.completeness.has_nfo_or_tag_block);
    app.workspace
        .artwork(
            root_id,
            "raw_chapter.cbz",
            Some(MediaKind::Comic),
            ArtworkIo::Write {
                slot: "cover",
                bytes: TINY_PNG,
                mime_type: "image/png",
            },
        )
        .await
        .unwrap();
    let comic_reloaded = app
        .workspace
        .inspect(root_id, "raw_chapter.cbz", Some(MediaKind::Comic))
        .await
        .unwrap();
    if let MediaMetadataPayload::Comic(c) = comic_reloaded.metadata {
        assert_eq!(c.series.as_deref(), Some("葬送的芙莉莲"));
        assert!(c.has_comic_info_xml);
        assert!(c.has_cover);
    } else {
        panic!("expected comic payload");
    }

    app.metadata
        .write(
            root_id,
            "raw_book.epub",
            MediaMetadataPatch::Book(BookMetaPatch {
                title: Some("三体".into()),
                authors: Some(vec!["刘慈欣".into()]),
                description: Some("地球往事三部曲之一".into()),
                isbn: Some("9787536692930".into()),
                tags: Some(vec!["科幻".into(), "硬科幻".into()]),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    app.workspace
        .artwork(
            root_id,
            "raw_book.epub",
            Some(MediaKind::Book),
            ArtworkIo::Write {
                slot: "cover",
                bytes: TINY_PNG,
                mime_type: "image/png",
            },
        )
        .await
        .unwrap();
    let book_reloaded = app
        .workspace
        .inspect(root_id, "raw_book.epub", Some(MediaKind::Book))
        .await
        .unwrap();
    if let MediaMetadataPayload::Book(b) = book_reloaded.metadata {
        assert_eq!(b.title, "三体");
        assert_eq!(b.authors, vec!["刘慈欣"]);
        assert_eq!(b.isbn.as_deref(), Some("9787536692930"));
        assert!(b.has_cover);
    } else {
        panic!("expected book payload");
    }
    {
        let f = std::fs::File::open(media_root.join("raw_book.epub")).unwrap();
        let mut archive = zip::ZipArchive::new(f).unwrap();
        let first = archive.by_index(0).unwrap();
        assert_eq!(first.name().unwrap(), "mimetype");
        assert_eq!(first.compression(), CompressionMethod::Stored);
    }

    app.metadata
        .write(
            root_id,
            "raw_ep.mkv",
            MediaMetadataPatch::Video(VideoMetaPatch {
                is_episode: Some(true),
                show_title: Some("Frieren".into()),
                title: Some("The Journey's End".into()),
                season_number: Some(1),
                episode_number: Some(1),
                year: Some(2023),
                tmdb_id: Some("209867".into()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    app.workspace
        .artwork(
            root_id,
            "raw_ep.mkv",
            Some(MediaKind::Video),
            ArtworkIo::Write {
                slot: "poster",
                bytes: TINY_PNG,
                mime_type: "image/png",
            },
        )
        .await
        .unwrap();

    let video_plans = app
        .rename
        .preview(
            root_id,
            &["raw_ep.mkv".to_string()],
            PreviewRenameOptions {
                preferred_kind: Some(MediaKind::Video),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        video_plans[0].to_rel_path,
        "Frieren (2023)/Season 01/Frieren - S01E01 - The Journey's End.mkv"
    );
    app.rename
        .execute(root_id, &video_plans, None)
        .await
        .unwrap();
    let expected_ep_dir = media_root.join("Frieren (2023)").join("Season 01");
    assert!(
        expected_ep_dir
            .join("Frieren - S01E01 - The Journey's End.mkv")
            .exists()
    );
    assert!(
        expected_ep_dir
            .join("Frieren - S01E01 - The Journey's End.nfo")
            .exists()
    );
    assert!(
        expected_ep_dir
            .join("Frieren - S01E01 - The Journey's End.zh.srt")
            .exists()
    );
}

#[tokio::test]
async fn folder_raw_files_and_delete_companions_vs_single_file() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("data");
    let root = dir.path().join("books");
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();

    std::fs::write(sub.join("novel.txt"), "第一章 测试正文").unwrap();
    std::fs::write(sub.join("novel.opf"), "<package/>").unwrap();
    std::fs::write(sub.join("novel.jpg"), b"fakejpg").unwrap();
    std::fs::write(sub.join("extra_note.md"), "keep me").unwrap();

    let app = XMediaToolApp::init(XMediaToolConfig {
        data_dir,
        initial_roots: vec![root.clone()],
        password: None,
        ffmpeg_bin: "__missing_ffmpeg__".into(),
        external_web_dir: None,
    })
    .await
    .unwrap();
    let root_id = app.settings.get_public_settings().await.unwrap().roots[0].id;

    let snap = app
        .workspace
        .browse(root_id, "sub", Some(MediaKind::Book), false, None)
        .await
        .unwrap();
    let raw_names: Vec<&str> = snap
        .raw_files
        .iter()
        .map(|f| f.file_name.as_str())
        .collect();
    assert_eq!(
        raw_names,
        vec!["extra_note.md", "novel.jpg", "novel.opf", "novel.txt"]
    );
    assert!(
        snap.raw_files
            .iter()
            .find(|f| f.file_name == "novel.txt")
            .unwrap()
            .is_media
    );
    assert!(
        !snap
            .raw_files
            .iter()
            .find(|f| f.file_name == "novel.opf")
            .unwrap()
            .is_media
    );

    let rep_single = app
        .workspace
        .delete(root_id, &["sub/extra_note.md".into()], false)
        .await
        .unwrap();
    assert_eq!(rep_single.deleted_count, 1);
    assert!(rep_single.deleted_companions.is_empty());
    assert!(!sub.join("extra_note.md").exists());
    assert!(sub.join("novel.txt").exists());
    assert!(sub.join("novel.opf").exists());

    let rep_media = app
        .workspace
        .delete(root_id, &["sub/novel.txt".into()], true)
        .await
        .unwrap();
    assert_eq!(rep_media.deleted_count, 1);
    assert_eq!(rep_media.deleted_companions.len(), 2);
    assert!(!sub.join("novel.txt").exists());
    assert!(!sub.join("novel.opf").exists());
    assert!(!sub.join("novel.jpg").exists());
    assert!(!sub.exists());

    let folder_a = root.join("Author A");
    let folder_b = root.join("Author B");
    std::fs::create_dir_all(folder_a.join("nested")).unwrap();
    std::fs::create_dir_all(&folder_b).unwrap();
    std::fs::write(folder_a.join("book1.txt"), "b1").unwrap();
    std::fs::write(folder_a.join("nested/book2.txt"), "b2").unwrap();
    std::fs::write(folder_b.join("keep.txt"), "keep").unwrap();

    let rep_main_dir = app
        .workspace
        .delete(root_id, &["Author A".into()], true)
        .await
        .unwrap();
    assert_eq!(rep_main_dir.deleted_count, 0);
    assert_eq!(rep_main_dir.failed.len(), 1);
    assert!(folder_a.exists());

    let rep_root_dir = app
        .workspace
        .delete(root_id, &[".".into()], false)
        .await
        .unwrap();
    assert_eq!(rep_root_dir.deleted_count, 0);
    assert_eq!(rep_root_dir.failed.len(), 1);
    assert!(root.exists());

    let rep_folder = app
        .workspace
        .delete(root_id, &["Author A".into()], false)
        .await
        .unwrap();
    assert_eq!(rep_folder.deleted_count, 1);
    assert!(rep_folder.failed.is_empty());
    assert!(!folder_a.exists());
    assert!(folder_b.join("keep.txt").exists());
}

#[tokio::test]
async fn rename_conflict_policies_skip_auto_suffix_and_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("data");
    let root = dir.path().join("books");
    std::fs::create_dir_all(root.join("夏目漱石")).unwrap();

    std::fs::write(root.join("夏目漱石/草枕.txt"), "existing version").unwrap();
    std::fs::write(root.join("草枕_竖排.txt"), "vertical version").unwrap();

    let app = XMediaToolApp::init(XMediaToolConfig {
        data_dir,
        initial_roots: vec![root.clone()],
        password: None,
        ffmpeg_bin: "__missing_ffmpeg__".into(),
        external_web_dir: None,
    })
    .await
    .unwrap();
    let root_id = app.settings.get_public_settings().await.unwrap().roots[0].id;

    app.metadata
        .write(
            root_id,
            "草枕_竖排.txt",
            MediaMetadataPatch::Book(BookMetaPatch {
                title: Some("草枕".into()),
                authors: Some(vec!["夏目漱石".into()]),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    let tpl = "{author}/{title}";

    let skip_plans = app
        .rename
        .preview(
            root_id,
            &["草枕_竖排.txt".into()],
            PreviewRenameOptions {
                template_override: Some(tpl),
                episode_template_override: None,
                preferred_kind: Some(MediaKind::Book),
                conflict_policy: RenameConflictPolicy::Skip,
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(skip_plans.len(), 1);
    assert!(skip_plans[0].conflict.is_some());
    assert!(!skip_plans[0].will_overwrite);

    let suffix_plans = app
        .rename
        .preview(
            root_id,
            &["草枕_竖排.txt".into()],
            PreviewRenameOptions {
                template_override: Some(tpl),
                episode_template_override: None,
                preferred_kind: Some(MediaKind::Book),
                conflict_policy: RenameConflictPolicy::AutoSuffix,
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(suffix_plans[0].conflict, None);
    assert_eq!(suffix_plans[0].to_rel_path, "夏目漱石/草枕 (2).txt");
    let moved = app
        .rename
        .execute(root_id, &suffix_plans, None)
        .await
        .unwrap();
    assert_eq!(moved, 1);
    assert!(root.join("夏目漱石/草枕 (2).txt").is_file());
    assert!(root.join("夏目漱石/草枕.txt").is_file());

    let overwrite_plans = app
        .rename
        .preview(
            root_id,
            &["夏目漱石/草枕 (2).txt".into()],
            PreviewRenameOptions {
                template_override: Some(tpl),
                episode_template_override: None,
                preferred_kind: Some(MediaKind::Book),
                conflict_policy: RenameConflictPolicy::Overwrite,
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(overwrite_plans[0].conflict, None);
    assert!(overwrite_plans[0].will_overwrite);
    assert_eq!(overwrite_plans[0].to_rel_path, "夏目漱石/草枕.txt");
    let moved_ov = app
        .rename
        .execute(root_id, &overwrite_plans, None)
        .await
        .unwrap();
    assert_eq!(moved_ov, 1);
    assert!(!root.join("夏目漱石/草枕 (2).txt").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("夏目漱石/草枕.txt")).unwrap(),
        "vertical version"
    );
}

#[tokio::test]
async fn batch_sync_comic_music_and_video_record_single_undoable_batch_and_handle_multidir_and_readonly()
 {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("media");
    let s1 = root.join("Show/Season 01");
    let s2 = root.join("Show/Season 02");
    std::fs::create_dir_all(&s1).unwrap();
    std::fs::create_dir_all(&s2).unwrap();

    for name in ["c1.cbz", "c2.cbz", "c3.cbz"] {
        write_minimal_cbz(&root.join(name));
    }
    for name in ["m1.wav", "m2.wav", "m3.wav"] {
        write_minimal_wav(&root.join(name));
    }
    std::fs::write(s1.join("ep_a.mkv"), b"v1").unwrap();
    std::fs::write(s2.join("ep_b.mkv"), b"v2").unwrap();

    let app = XMediaToolApp::init(XMediaToolConfig {
        data_dir: tmp.path().join("data"),
        initial_roots: vec![root.clone()],
        password: None,
        ffmpeg_bin: "__missing_ffmpeg__".into(),
        external_web_dir: None,
    })
    .await
    .unwrap();
    let root_id = app.settings.get_public_settings().await.unwrap().roots[0].id;

    let comic_rep = app
        .metadata
        .batch_sync(
            root_id,
            vec!["c1.cbz".into(), "c2.cbz".into(), "c3.cbz".into()],
            BatchSyncSpec::ComicSeries(BatchComicSeriesSyncInput {
                series: Some("Unified Series".into()),
                numbering_mode: "chapter".into(),
                start_index: 1,
                pad_width: 2,
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    assert_eq!(comic_rep.updated_count, 3);
    assert!(comic_rep.failed_paths.is_empty());

    let hist1 = app.metadata.list_history(Some(root_id)).await;
    assert_eq!(hist1.len(), 1);
    assert_eq!(hist1[0].records.len(), 3);
    let comic_batch_id = hist1[0].batch_id;

    let undo_comic = app.metadata.undo_batch(comic_batch_id).await.unwrap();
    assert_eq!(undo_comic.restored_count, 3);
    let c1_after = app
        .workspace
        .inspect(root_id, "c1.cbz", Some(MediaKind::Comic))
        .await
        .unwrap();
    if let MediaMetadataPayload::Comic(c) = c1_after.metadata {
        assert_ne!(c.series.as_deref(), Some("Unified Series"));
    } else {
        panic!("expected comic payload");
    }

    let music_rep = app
        .metadata
        .batch_sync(
            root_id,
            vec!["m1.wav".into(), "m2.wav".into(), "m3.wav".into()],
            BatchSyncSpec::MusicAlbum(BatchMusicAlbumSyncInput {
                artist: Some("Artist X".into()),
                album: Some("Album Y".into()),
                auto_number_tracks: true,
                start_track: 1,
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    assert_eq!(music_rep.updated_count, 3);
    let hist2 = app.metadata.list_history(Some(root_id)).await;
    assert_eq!(hist2.len(), 1);
    assert_eq!(hist2[0].records.len(), 3);
    let music_batch_id = hist2[0].batch_id;

    let undo_music = app.metadata.undo_batch(music_batch_id).await.unwrap();
    assert_eq!(undo_music.restored_count, 3);
    let m2_after = app
        .workspace
        .inspect(root_id, "m2.wav", Some(MediaKind::Music))
        .await
        .unwrap();
    if let MediaMetadataPayload::Music(m) = m2_after.metadata {
        assert_ne!(m.album, "Album Y");
    } else {
        panic!("expected music payload");
    }

    let video_rep = app
        .metadata
        .batch_sync(
            root_id,
            vec![
                "Show/Season 01/ep_a.mkv".into(),
                "Show/Season 02/ep_b.mkv".into(),
            ],
            BatchSyncSpec::VideoEpisodes(BatchRemapEpisodesInput {
                mode: "renumber".into(),
                show_title: Some("Demo Show".into()),
                season_number: Some(2),
                start_episode: 10,
                episode_offset: 0,
                write_season_nfo: true,
            }),
        )
        .await
        .unwrap();
    assert_eq!(video_rep.updated_count, 2);
    assert!(s1.join("season.nfo").is_file());
    assert!(s2.join("season.nfo").is_file());

    let hist3 = app.metadata.list_history(Some(root_id)).await;
    assert_eq!(hist3.len(), 1);
    assert_eq!(hist3[0].records.len(), 2);
    let undo_video = app.metadata.undo_batch(hist3[0].batch_id).await.unwrap();
    assert_eq!(undo_video.restored_count, 2);

    app.settings
        .set_root_read_only(root_id, true)
        .await
        .unwrap();
    let ro_err = app
        .metadata
        .batch_sync(
            root_id,
            vec!["Show/Season 01/ep_a.mkv".into()],
            BatchSyncSpec::VideoEpisodes(BatchRemapEpisodesInput {
                mode: "renumber".into(),
                show_title: Some("Blocked".into()),
                season_number: Some(1),
                start_episode: 1,
                episode_offset: 0,
                write_season_nfo: false,
            }),
        )
        .await
        .unwrap_err();
    assert!(matches!(ro_err, DomainError::Forbidden(_)));
}

#[tokio::test]
async fn execute_rename_partial_failure_records_history_and_undo_retains_when_zero_restored() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("books");
    std::fs::create_dir_all(&root).unwrap();

    std::fs::write(root.join("a.txt"), "content-a").unwrap();
    std::fs::write(root.join("b.txt"), "content-b").unwrap();
    std::fs::write(root.join("existing.txt"), "blocker").unwrap();

    let app = XMediaToolApp::init(XMediaToolConfig {
        data_dir: tmp.path().join("data"),
        initial_roots: vec![root.clone()],
        password: None,
        ffmpeg_bin: "__missing_ffmpeg__".into(),
        external_web_dir: None,
    })
    .await
    .unwrap();
    let root_id = app.settings.get_public_settings().await.unwrap().roots[0].id;

    let plans = vec![
        RenamePlanItem {
            from_rel_path: "a.txt".into(),
            to_rel_path: "a_moved.txt".into(),
            companion_moves: Vec::new(),
            conflict: None,
            unchanged: false,
            will_overwrite: false,
        },
        RenamePlanItem {
            from_rel_path: "b.txt".into(),
            to_rel_path: "existing.txt".into(),
            companion_moves: Vec::new(),
            conflict: None,
            unchanged: false,
            will_overwrite: false,
        },
    ];

    let err = app.rename.execute(root_id, &plans, None).await.unwrap_err();
    assert!(matches!(err, DomainError::Conflict(_)));
    assert!(root.join("a_moved.txt").is_file());
    assert!(!root.join("a.txt").exists());
    assert!(root.join("b.txt").is_file());

    let history = app.rename.list_history(Some(root_id)).await;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].moved_count, 1);
    assert_eq!(history[0].moves.len(), 1);
    let batch_id = history[0].batch_id;

    std::fs::write(root.join("a.txt"), "temporary-blocker").unwrap();
    let failed_undo = app.rename.undo_batch(batch_id).await.unwrap();
    assert_eq!(failed_undo.restored_count, 0);
    assert_eq!(failed_undo.skipped_conflicts, vec!["a.txt".to_string()]);
    assert_eq!(app.rename.list_history(Some(root_id)).await.len(), 1);

    std::fs::remove_file(root.join("a.txt")).unwrap();
    let ok_undo = app.rename.undo_batch(batch_id).await.unwrap();
    assert_eq!(ok_undo.restored_count, 1);
    assert!(ok_undo.skipped_conflicts.is_empty());
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "content-a"
    );
    assert!(!root.join("a_moved.txt").exists());
    assert!(app.rename.list_history(Some(root_id)).await.is_empty());
}

#[tokio::test]
async fn upload_artwork_http_prefers_magic_bytes_over_spoofed_content_type() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("books");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("sample.txt"), "Chapter 1\nBody\n").unwrap();

    let app = XMediaToolApp::init(XMediaToolConfig {
        data_dir: tmp.path().join("data"),
        initial_roots: vec![root.clone()],
        password: None,
        ffmpeg_bin: "__missing_ffmpeg__".into(),
        external_web_dir: None,
    })
    .await
    .unwrap();
    let root_id = app.settings.get_public_settings().await.unwrap().roots[0].id;

    let boundary = "XMEDIA_TEST_BOUNDARY";
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"root_id\"\r\n\r\n{root_id}\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"path\"\r\n\r\nsample.txt\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"spoof.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(TINY_PNG);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let res = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/tool/files/artwork")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(root.join("sample.png").is_file());
    assert!(!root.join("sample.jpg").exists());
}

#[tokio::test]
async fn apply_video_jellyfin_archive_enforces_read_only_root() {
    let tmp = TempDir::new().unwrap();
    let video_dir = tmp.path().join("videos");
    std::fs::create_dir_all(&video_dir).unwrap();
    std::fs::write(video_dir.join("Movie (2024).mkv"), b"dummy").unwrap();

    let app = XMediaToolApp::init(XMediaToolConfig {
        data_dir: tmp.path().join("data"),
        initial_roots: Vec::new(),
        password: None,
        ffmpeg_bin: "__missing_ffmpeg__".into(),
        external_web_dir: None,
    })
    .await
    .unwrap();

    let root = app
        .settings
        .add_root(
            "ReadOnly".into(),
            video_dir.to_string_lossy().into_owned(),
            Some(MediaKind::Video),
            true,
        )
        .await
        .unwrap();

    let err = app
        .scrape_apply
        .apply(ApplyScrapeInput {
            root_id: root.id,
            rel_path: "Movie (2024).mkv".into(),
            media_kind: MediaKind::Video,
            scraped: xmedia_tool::domain::scraper::ScrapedMetadata {
                title: Some("Movie".into()),
                year: Some(2024),
                ..Default::default()
            },
            apply_cover: false,
            write_sidecar_lrc: false,
            organize_by_template: Some(true),
            only_fill_missing: false,
            write_album_cover: false,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, DomainError::Forbidden(_)));
}
