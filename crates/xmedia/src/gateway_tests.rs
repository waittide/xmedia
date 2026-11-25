use super::*;
use std::future::poll_fn;
use tokio::sync::broadcast;

async fn next_event(stream: &mut SseEventStream) -> Option<Result<Event, Infallible>> {
    poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await
}

#[tokio::test]
async fn sse_stream_forwards_events_and_cleans_up_receiver_on_drop() {
    let (tx, rx) = broadcast::channel(16);
    assert_eq!(tx.receiver_count(), 1);

    let initial = vec![SystemEvent::LibraryScanProgress {
        library_id: LibraryId::new(),
        library_name: "Books".into(),
        media_kind: MediaKind::Book,
        scanned: 3,
        total: Some(10),
    }];
    let mut stream = spawn_sse_event_stream(rx, initial, vec![MediaKind::Book], true, None);

    let first = tokio::time::timeout(Duration::from_secs(2), next_event(&mut stream))
        .await
        .unwrap();
    assert!(first.is_some());

    tx.send(SystemEvent::MediaChanged {
        media_kind: MediaKind::Video,
    })
    .unwrap();
    let live = tokio::time::timeout(Duration::from_secs(2), next_event(&mut stream))
        .await
        .unwrap();
    assert!(live.is_some());

    // Dropping the SSE stream must terminate the forwarding task and drop the broadcast receiver
    drop(stream);
    tokio::time::timeout(Duration::from_secs(2), async {
        while tx.receiver_count() > 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("broadcast receiver should be dropped promptly after SSE client disconnect");
}

#[tokio::test]
async fn sse_stream_redacts_scan_failure_host_paths_for_non_admin_only() {
    use axum::body::to_bytes;

    let (tx, rx_member) = broadcast::channel(16);
    let rx_admin = tx.subscribe();

    let mut member_stream =
        spawn_sse_event_stream(rx_member, Vec::new(), vec![MediaKind::Video], false, None);
    let mut admin_stream =
        spawn_sse_event_stream(rx_admin, Vec::new(), vec![MediaKind::Video], true, None);

    tx.send(SystemEvent::LibraryScanFailed {
        library_id: LibraryId::new(),
        library_name: "Movies".into(),
        media_kind: MediaKind::Video,
        message: "mount directory '/srv/secret-nas/movies' is offline or missing".into(),
    })
    .unwrap();

    let member_ev = tokio::time::timeout(Duration::from_secs(2), next_event(&mut member_stream))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let admin_ev = tokio::time::timeout(Duration::from_secs(2), next_event(&mut admin_stream))
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    let member_sse = Sse::new(futures_stream_once(member_ev)).into_response();
    let member_body = String::from_utf8(
        to_bytes(member_sse.into_body(), 4096)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(
        !member_body.contains("/srv/secret-nas"),
        "non-admin SSE frame must never leak host path: {member_body}"
    );
    assert!(member_body.contains("library scan failed"));

    let admin_sse = Sse::new(futures_stream_once(admin_ev)).into_response();
    let admin_body = String::from_utf8(
        to_bytes(admin_sse.into_body(), 4096)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(
        admin_body.contains("/srv/secret-nas/movies"),
        "admin SSE frame should retain full diagnostic message: {admin_body}"
    );
}

fn futures_stream_once(
    ev: Event,
) -> impl Stream<Item = Result<Event, Infallible>> + Send + 'static {
    let (tx, rx) = mpsc::channel(1);
    let _ = tx.try_send(Ok(ev));
    SseEventStream { rx, _permit: None }
}

#[cfg(unix)]
#[tokio::test]
async fn ffmpeg_status_cache_reuses_result_within_ttl_and_refreshes_after() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::TempDir::new().unwrap();
    let counter_path = tmp.path().join("invocations.log");
    let fake_bin = tmp.path().join("fake-ffmpeg.sh");
    let script = format!(
        "#!/bin/sh\necho x >> \"{}\"\necho 'ffmpeg version fake-1.0'\n",
        counter_path.display()
    );
    std::fs::write(&fake_bin, script).unwrap();
    let mut perms = std::fs::metadata(&fake_bin).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fake_bin, perms).unwrap();

    let toolkit = FfmpegToolkit::new(
        fake_bin.to_string_lossy().to_string(),
        fake_bin.to_string_lossy().to_string(),
    );
    let cache = FfmpegStatusCache::new();
    let ttl = Duration::from_millis(80);

    let s1 = cache.get_or_refresh_with_ttl(&toolkit, ttl).await;
    assert!(s1.ffmpeg_available);
    assert!(s1.ffprobe_available);
    assert_eq!(
        s1.ffmpeg_version.as_deref(),
        Some("ffmpeg version fake-1.0")
    );

    // Second & third calls within TTL must hit cache without spawning subprocesses
    let s2 = cache.get_or_refresh_with_ttl(&toolkit, ttl).await;
    let s3 = cache.get_or_refresh_with_ttl(&toolkit, ttl).await;
    assert_eq!(s2.ffmpeg_version, s1.ffmpeg_version);
    assert_eq!(s3.ffmpeg_version, s1.ffmpeg_version);

    let count_after_cached = std::fs::read_to_string(&counter_path)
        .unwrap()
        .lines()
        .count();
    assert_eq!(
        count_after_cached, 2,
        "expected exactly 1 ffmpeg + 1 ffprobe invocation within TTL"
    );

    tokio::time::sleep(Duration::from_millis(100)).await;
    let _s4 = cache.get_or_refresh_with_ttl(&toolkit, ttl).await;
    let count_after_expiry = std::fs::read_to_string(&counter_path)
        .unwrap()
        .lines()
        .count();
    assert_eq!(
        count_after_expiry, 4,
        "expected cache refresh after TTL expiry"
    );
}
