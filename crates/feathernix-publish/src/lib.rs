use std::path::{Path, PathBuf};

use chrono::Utc;
use sha2::{Digest, Sha256};
use thiserror::Error;

use feathernix_archive::{Manifest, read_manifest};
use feathernix_repo::{IndexMeta, PackageEntry, RepositoryIndex};
use feathernix_sign::{SecretKey, SignError};

pub const PACKAGES_SUBDIR: &str = "packages";
pub const REPODATA_SUBDIR: &str = "repodata";
pub const INDEX_FILENAME: &str = "index.toml";
pub const INDEX_SIG_FILENAME: &str = "index.toml.sig";

#[derive(Debug, Error)]
pub enum PublishError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Archive(#[from] feathernix_archive::ArchiveError),
    #[error(transparent)]
    Sign(#[from] SignError),
    #[error("toml serialize error: {0}")]
    Toml(#[from] toml::ser::Error),
    #[error("repo dir {0} is missing required {PACKAGES_SUBDIR}/ subdirectory")]
    NoPackagesDir(PathBuf),
}

pub type PublishResult<T> = Result<T, PublishError>;

#[derive(Debug, Clone)]
pub struct PublishReport {
    pub repo_dir: PathBuf,
    pub packages_indexed: usize,
    pub index_path: PathBuf,
    pub index_sig_path: Option<PathBuf>,
}

/// Build a [`RepositoryIndex`] by scanning `<repo_dir>/packages/*.fnpkg.7z`,
/// reading each manifest, and computing the payload hash + size.
pub fn build_index(repo_dir: &Path, repo_name: &str) -> PublishResult<RepositoryIndex> {
    let packages_dir = repo_dir.join(PACKAGES_SUBDIR);
    if !packages_dir.exists() {
        return Err(PublishError::NoPackagesDir(packages_dir));
    }
    let mut entries: Vec<PackageEntry> = Vec::new();
    for entry in std::fs::read_dir(&packages_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy();
        if !name.ends_with(".fnpkg.7z") {
            continue;
        }
        let manifest: Manifest = read_manifest(&path)?;
        let payload_bytes = std::fs::read(&path)?;
        let payload_sha256 = hex::encode(Sha256::digest(&payload_bytes));
        let payload_size = payload_bytes.len() as i64;
        entries.push(PackageEntry {
            name: manifest.package.name.clone(),
            version: manifest.package.version.clone(),
            release: manifest.package.release.clone(),
            epoch: manifest.package.epoch,
            arch: manifest.package.arch.clone(),
            payload_path: format!("{PACKAGES_SUBDIR}/{}", name),
            payload_sha256,
            payload_size,
            summary: manifest.package.summary.clone(),
            dependencies: manifest
                .dependencies
                .requires
                .iter()
                .map(|d| format_dep(&d.name, d.version.as_deref(), d.version_constraint.as_deref()))
                .collect(),
            provides: manifest
                .dependencies
                .provides
                .iter()
                .map(|d| format_dep(&d.name, d.version.as_deref(), d.version_constraint.as_deref()))
                .collect(),
        });
    }
    // Stable ordering so the file is deterministic across re-publishes.
    entries.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.arch.cmp(&b.arch))
            .then(a.version.cmp(&b.version))
            .then(a.release.cmp(&b.release))
    });
    Ok(RepositoryIndex {
        index: IndexMeta {
            format_version: 1,
            generated_at: Utc::now().to_rfc3339(),
            repository: repo_name.to_string(),
        },
        packages: entries,
    })
}

/// Generate (and optionally sign) `repodata/index.toml`. Also signs every payload
/// in `packages/` if `signer` is provided.
pub fn publish(
    repo_dir: &Path,
    repo_name: &str,
    signer: Option<&SecretKey>,
) -> PublishResult<PublishReport> {
    let index = build_index(repo_dir, repo_name)?;
    let repodata = repo_dir.join(REPODATA_SUBDIR);
    std::fs::create_dir_all(&repodata)?;
    let index_path = repodata.join(INDEX_FILENAME);
    let body = toml::to_string_pretty(&index)?;
    write_atomic(&index_path, body.as_bytes())?;

    let mut index_sig_path = None;
    if let Some(sk) = signer {
        let sig = sk.sign_detached_armor(body.as_bytes())?;
        let sig_path = repodata.join(INDEX_SIG_FILENAME);
        write_atomic(&sig_path, &sig)?;
        index_sig_path = Some(sig_path);

        // Sign each payload too (best-effort: only write if not already present
        // or if the .sig is stale).
        let packages_dir = repo_dir.join(PACKAGES_SUBDIR);
        for entry in std::fs::read_dir(&packages_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if !name.ends_with(".fnpkg.7z") {
                continue;
            }
            sk.sign_file(&path)?;
        }
    }

    Ok(PublishReport {
        repo_dir: repo_dir.to_path_buf(),
        packages_indexed: index.packages.len(),
        index_path,
        index_sig_path,
    })
}

/// Move `archive` (and `archive.sig` if present) into `<repo_dir>/packages/`.
/// Idempotent: overwrites existing files of the same name.
pub fn ingest_archive(repo_dir: &Path, archive: &Path) -> PublishResult<PathBuf> {
    let packages_dir = repo_dir.join(PACKAGES_SUBDIR);
    std::fs::create_dir_all(&packages_dir)?;
    let dest = packages_dir.join(archive.file_name().unwrap());
    copy_or_rename(archive, &dest)?;
    let sig = archive.with_extension(
        archive
            .extension()
            .map(|e| {
                let mut s = e.to_string_lossy().into_owned();
                s.push_str(".sig");
                s
            })
            .unwrap_or_else(|| "sig".to_string()),
    );
    if sig.exists() {
        let dest_sig = packages_dir.join(sig.file_name().unwrap());
        copy_or_rename(&sig, &dest_sig)?;
    }
    Ok(dest)
}

fn copy_or_rename(src: &Path, dest: &Path) -> std::io::Result<()> {
    if src == dest {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Try rename first; fall back to copy if cross-device.
    if std::fs::rename(src, dest).is_ok() {
        return Ok(());
    }
    std::fs::copy(src, dest)?;
    Ok(())
}

fn write_atomic(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = dest.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

fn format_dep(name: &str, version: Option<&str>, constraint: Option<&str>) -> String {
    if let Some(v) = version {
        format!("{name} = {v}")
    } else if let Some(c) = constraint {
        format!("{name} {c}")
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use feathernix_archive::{Manifest, manifest::PackageBlock, pack, MANIFEST_FILENAME};
    use feathernix_sign::generate_ephemeral;

    fn make_pkg(workdir: &Path, name: &str, version: &str) -> PathBuf {
        let src = workdir.join(format!("src-{name}-{version}"));
        std::fs::create_dir_all(src.join("payload/bin")).unwrap();
        let manifest = Manifest {
            package: PackageBlock {
                name: name.to_string(),
                version: version.to_string(),
                release: "1".to_string(),
                epoch: 0,
                arch: "x86_64-linux".to_string(),
                summary: Some(format!("greet {name}")),
                license: Some("MIT".to_string()),
                url: None,
            },
            dependencies: Default::default(),
            outputs: Default::default(),
        };
        std::fs::write(
            src.join(MANIFEST_FILENAME),
            manifest.to_pretty_string().unwrap(),
        )
        .unwrap();
        std::fs::write(src.join("payload/bin").join(name), b"#!/bin/sh\n").unwrap();
        let archive =
            workdir.join(format!("{name}-{version}-1-x86_64-linux.fnpkg.7z"));
        pack(&src, &archive).unwrap();
        archive
    }

    #[test]
    fn build_index_finds_all_packages() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join(PACKAGES_SUBDIR)).unwrap();

        let p1 = make_pkg(tmp.path(), "hello", "1.0.0");
        let p2 = make_pkg(tmp.path(), "world", "2.0.0");
        for p in [p1, p2] {
            ingest_archive(&repo, &p).unwrap();
        }

        let idx = build_index(&repo, "devrepo").unwrap();
        assert_eq!(idx.packages.len(), 2);
        assert_eq!(idx.index.repository, "devrepo");
        // Sorted by name
        assert_eq!(idx.packages[0].name, "hello");
        assert_eq!(idx.packages[1].name, "world");
    }

    #[test]
    fn publish_writes_index_and_optionally_signs() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join(PACKAGES_SUBDIR)).unwrap();
        let p = make_pkg(tmp.path(), "hello", "1.0.0");
        ingest_archive(&repo, &p).unwrap();

        let report = publish(&repo, "devrepo", None).unwrap();
        assert_eq!(report.packages_indexed, 1);
        assert!(report.index_path.exists());
        assert!(report.index_sig_path.is_none());
        assert!(!repo.join(REPODATA_SUBDIR).join(INDEX_SIG_FILENAME).exists());

        let (sk, _) = generate_ephemeral("Publisher <p@x>").unwrap();
        let report2 = publish(&repo, "devrepo", Some(&sk)).unwrap();
        assert!(report2.index_sig_path.unwrap().exists());
        // Each payload also gets a .sig
        let pkg_sig = repo
            .join(PACKAGES_SUBDIR)
            .join("hello-1.0.0-1-x86_64-linux.fnpkg.7z.sig");
        assert!(pkg_sig.exists());
    }

    #[test]
    fn publish_is_idempotent_on_second_call() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join(PACKAGES_SUBDIR)).unwrap();
        let p = make_pkg(tmp.path(), "a", "1.0.0");
        ingest_archive(&repo, &p).unwrap();

        let r1 = publish(&repo, "devrepo", None).unwrap();
        let r2 = publish(&repo, "devrepo", None).unwrap();
        assert_eq!(r1.packages_indexed, r2.packages_indexed);
        assert!(r2.index_path.exists());
    }
}
