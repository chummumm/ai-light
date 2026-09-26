use anyhow::{Context, Result};
use serde::Serialize;
use std::{fs::{self, OpenOptions}, io::Write, path::Path};

/// Same-directory atomic replacement. Never truncate the active configuration.
pub fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    atomic_bytes(path, &bytes)
}
pub fn atomic_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().context("path has no parent")?;
    fs::create_dir_all(dir)?;
    let temp = dir.join(format!(".ailight-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)] {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path).with_context(|| format!("atomic replacement of {}", path.display()))?;
        #[cfg(unix)] { std::fs::File::open(dir)?.sync_all()?; }
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(&temp); }
    result
}
