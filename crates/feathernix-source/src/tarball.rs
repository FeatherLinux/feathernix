use std::path::Path;
use std::process::Command;

use crate::SourceError;

/// Unpack `archive` into `dest`, autodetecting `.tar.gz`/`.tgz`/`.tar.xz`/`.tar.zst`/`.tar.bz2`/`.tar` via tar's `-a`.
/// Shells out to `tar` rather than embedding decoders — keeps the dep graph slim
/// and matches the bwrap shell-out approach.
pub fn unpack(archive: &Path, dest: &Path) -> Result<(), SourceError> {
    std::fs::create_dir_all(dest)?;
    let out = Command::new("tar")
        .arg("-xaf")
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .arg("--strip-components=1")
        .output()
        .map_err(|e| SourceError::Unpack(format!("spawn tar: {e}")))?;
    if !out.status.success() {
        // Try without --strip-components for archives that don't have a single root dir.
        let out2 = Command::new("tar")
            .arg("-xaf")
            .arg(archive)
            .arg("-C")
            .arg(dest)
            .output()
            .map_err(|e| SourceError::Unpack(format!("spawn tar: {e}")))?;
        if !out2.status.success() {
            return Err(SourceError::Unpack(
                String::from_utf8_lossy(&out2.stderr).trim().to_string(),
            ));
        }
    }
    Ok(())
}
