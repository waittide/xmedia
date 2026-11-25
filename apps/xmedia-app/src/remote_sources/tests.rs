use std::fs;

use tempfile::TempDir;
use url::Url;

use super::*;

#[test]
fn remote_source_store_redacts_password_and_preserves_on_empty_update() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("remote_sources.json");
    let mut store = RemoteSourceStore::load(file.clone()).unwrap();

    let (created, list) = store
        .create(CreateRemoteSourceInput {
            name: "Home OPDS".into(),
            protocol: RemoteProtocol::Opds,
            url: "http://127.0.0.1:8080/opds/book/v1.2/".into(),
            username: Some("alice".into()),
            password: Some("super-secret".into()),
            allow_insecure_tls: false,
        })
        .unwrap();

    assert_eq!(list.len(), 1);
    assert!(created.has_password);
    assert_eq!(created.url, "http://127.0.0.1:8080/opds/book/v1.2");

    // Serialized summary must never contain the raw password.
    let summary_json = serde_json::to_string(&created).unwrap();
    assert!(!summary_json.contains("super-secret"));
    assert!(summary_json.contains("\"has_password\":true"));

    // Updating with None or empty password retains the existing secret.
    let (updated, _) = store
        .update(
            &created.id,
            UpdateRemoteSourceInput {
                name: Some("Renamed OPDS".into()),
                protocol: None,
                url: None,
                username: None,
                password: Some(String::new()),
                clear_password: false,
                allow_insecure_tls: Some(true),
            },
        )
        .unwrap();
    assert_eq!(updated.name, "Renamed OPDS");
    assert!(updated.has_password);
    assert!(updated.allow_insecure_tls);
    assert_eq!(store.get(&created.id).unwrap().password, "super-secret");

    // Reload from disk and verify 0600 permissions on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    let disk_json = fs::read_to_string(&file).unwrap();
    assert!(
        !disk_json.contains("super-secret"),
        "persisted remote_sources.json must never store plaintext password"
    );
    assert!(
        disk_json.contains("enc:v1:"),
        "persisted remote_sources.json must store AES-GCM encrypted password"
    );

    let reloaded = RemoteSourceStore::load(file.clone()).unwrap();
    assert_eq!(reloaded.count(), 1);
    assert_eq!(reloaded.get(&created.id).unwrap().password, "super-secret");

    // Explicit clear_password removes the saved secret.
    let (cleared, _) = store
        .update(
            &created.id,
            UpdateRemoteSourceInput {
                name: None,
                protocol: None,
                url: None,
                username: None,
                password: None,
                clear_password: true,
                allow_insecure_tls: None,
            },
        )
        .unwrap();
    assert!(!cleared.has_password);
    assert!(store.get(&created.id).unwrap().password.is_empty());

    // Throttled mark_ok returns a persist task first time, then throttles immediate repeat.
    let pending = store.mark_ok_throttled(&created.id);
    assert!(pending.is_some());
    pending.unwrap().persist().unwrap();
    assert!(store.mark_ok_throttled(&created.id).is_none());

    // Delete works.
    let after_delete = store.delete(&created.id).unwrap();
    assert!(after_delete.is_empty());
}

#[test]
fn legacy_plaintext_remote_sources_are_migrated_to_aes_gcm_on_load() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("remote_sources.json");
    let legacy_json = r#"{
  "sources": [
    {
      "id": "src-legacy-1",
      "name": "Legacy Navidrome",
      "protocol": "subsonic",
      "url": "http://127.0.0.1:4533",
      "username": "alice",
      "password": "legacy-plaintext-password",
      "allow_insecure_tls": false
    }
  ]
}"#;
    fs::write(&file, legacy_json).unwrap();

    let store = RemoteSourceStore::load(file.clone()).unwrap();
    assert_eq!(store.count(), 1);
    assert_eq!(
        store.get("src-legacy-1").unwrap().password,
        "legacy-plaintext-password"
    );

    let migrated_disk = fs::read_to_string(&file).unwrap();
    assert!(
        !migrated_disk.contains("legacy-plaintext-password"),
        "plaintext secret must be migrated out of remote_sources.json on load"
    );
    assert!(migrated_disk.contains("enc:v1:"));
}

#[test]
fn corrupt_remote_sources_json_never_writes_plaintext_secrets_to_backup() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("remote_sources.json");
    fs::write(
        &file,
        b"{\"sources\":[{\"password\":\"LEAKED_PLAIN_SECRET\" broken json",
    )
    .unwrap();

    let store = RemoteSourceStore::load(file.clone()).unwrap();
    assert_eq!(store.count(), 0);
    assert!(!file.exists(), "corrupt file should be removed");

    for entry in fs::read_dir(tmp.path()).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            !name.contains(".corrupt."),
            "must not write .corrupt.*.bak files: found {name}"
        );
        let content = fs::read_to_string(entry.path()).unwrap_or_default();
        assert!(!content.contains("LEAKED_PLAIN_SECRET"));
    }
}

#[test]
fn normalize_remote_url_and_same_origin_checks() {
    assert!(normalize_remote_url("").is_err());
    assert!(normalize_remote_url("file:///etc/passwd").is_err());
    assert!(normalize_remote_url("ftp://example.com/opds").is_err());
    assert!(normalize_remote_url("http://user:pass@example.com/opds").is_err());
    assert!(normalize_remote_url("https://example.com/opds#frag").is_err());
    assert_eq!(
        normalize_remote_url("https://example.com/opds/").unwrap(),
        "https://example.com/opds"
    );

    let a = Url::parse("http://example.com:80/a").unwrap();
    let b = Url::parse("http://example.com/b").unwrap();
    let c = Url::parse("https://example.com/b").unwrap();
    let d = Url::parse("http://example.com:8080/b").unwrap();
    assert!(is_same_origin(&a, &b));
    assert!(!is_same_origin(&a, &c));
    assert!(!is_same_origin(&a, &d));
}
