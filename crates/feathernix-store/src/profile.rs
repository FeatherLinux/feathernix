use std::path::{Path, PathBuf};

use crate::StoreError;

pub fn generation_dir(profile_dir: &Path, generation: i32) -> PathBuf {
    profile_dir.join("generations").join(generation.to_string())
}

pub fn current_link(profile_dir: &Path) -> PathBuf {
    profile_dir.join("current")
}

pub fn build_generation(
    profile_dir: &Path,
    generation: i32,
    store_paths: &[PathBuf],
) -> Result<PathBuf, StoreError> {
    let gen_dir = generation_dir(profile_dir, generation);
    if gen_dir.exists() {
        std::fs::remove_dir_all(&gen_dir)?;
    }
    std::fs::create_dir_all(&gen_dir)?;

    for sp in store_paths {
        let payload = sp.join("payload");
        if !payload.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&payload).min_depth(1).follow_links(false) {
            let entry = entry.map_err(|e| StoreError::WalkDir(e.to_string()))?;
            let rel = entry.path().strip_prefix(&payload).unwrap();
            let target_in_profile = gen_dir.join(rel);

            if entry.file_type().is_dir() {
                if !target_in_profile.exists() {
                    std::fs::create_dir_all(&target_in_profile)?;
                }
            } else {
                if let Some(parent) = target_in_profile.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                if target_in_profile.symlink_metadata().is_ok() {
                    continue;
                }
                #[cfg(unix)]
                std::os::unix::fs::symlink(entry.path(), &target_in_profile)?;
                #[cfg(not(unix))]
                std::fs::copy(entry.path(), &target_in_profile)?;
            }
        }
    }

    Ok(gen_dir)
}

pub fn point_current(profile_dir: &Path, generation: i32) -> Result<(), StoreError> {
    let target = generation_dir(profile_dir, generation);
    let link = current_link(profile_dir);
    if link.symlink_metadata().is_ok() {
        std::fs::remove_file(&link).ok();
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link)?;
    #[cfg(not(unix))]
    return Err(StoreError::Io(std::io::Error::other("symlinks require unix")));
    Ok(())
}
