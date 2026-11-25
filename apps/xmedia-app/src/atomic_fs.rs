use std::fs;
use std::io::Write;
use std::path::Path;

use uuid::Uuid;

/// Atomically writes `payload` to `path` via a unique private (`0o600` on Unix)
/// sibling `.tmp` file, syncing to disk before `fs::rename` and removing the `.tmp`
/// file on any intermediate failure.
pub(crate) fn write_private_file_atomic(path: &Path, payload: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("xmedia-atomic");
    let tmp_path = path.with_file_name(format!(".{file_name}.{}.tmp", Uuid::new_v4()));

    {
        let mut file = create_private_file(&tmp_path)?;
        if let Err(err) = file.write_all(payload).and_then(|()| file.sync_all()) {
            let _ = fs::remove_file(&tmp_path);
            return Err(err);
        }
    }

    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }

    Ok(())
}

fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
}
