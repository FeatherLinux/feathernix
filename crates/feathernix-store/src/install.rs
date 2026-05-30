use std::path::{Path, PathBuf};

use crate::StoreError;
use crate::path::compute_store_path;

#[derive(Debug, Clone)]
pub struct InstalledStorePath {
    pub hash: String,
    pub full_path: PathBuf,
    pub size_bytes: u64,
    pub freshly_installed: bool,
}

pub fn install_via_temp<F>(
    store_root: &Path,
    name: &str,
    version: &str,
    payload_sha256: &str,
    write_payload: F,
) -> Result<InstalledStorePath, StoreError>
where
    F: FnOnce(&Path) -> Result<(), StoreError>,
{
    std::fs::create_dir_all(store_root)?;
    let (hash, dest) = compute_store_path(store_root, name, version, payload_sha256);

    if dest.exists() {
        let size = dir_size(&dest)?;
        return Ok(InstalledStorePath {
            hash,
            full_path: dest,
            size_bytes: size,
            freshly_installed: false,
        });
    }

    let tmp = store_root.join(format!(".tmp.{hash}-{name}-{version}"));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }
    std::fs::create_dir_all(&tmp)?;

    let result = write_payload(&tmp);
    if let Err(err) = result {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(err);
    }

    if let Err(e) = std::fs::rename(&tmp, &dest) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(StoreError::Io(e));
    }

    let size = dir_size(&dest)?;
    Ok(InstalledStorePath {
        hash,
        full_path: dest,
        size_bytes: size,
        freshly_installed: true,
    })
}

pub fn dir_size(path: &Path) -> Result<u64, StoreError> {
    let mut total = 0u64;
    for entry in walkdir::WalkDir::new(path).follow_links(false) {
        let entry = entry.map_err(|e| StoreError::WalkDir(e.to_string()))?;
        if entry.file_type().is_file() {
            total += entry.metadata()
                .map_err(|e| StoreError::WalkDir(e.to_string()))?
                .len();
        }
    }
    Ok(total)
}
