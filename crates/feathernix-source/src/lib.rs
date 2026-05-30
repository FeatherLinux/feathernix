pub mod git;
pub mod tarball;

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use thiserror::Error;
use walkdir::WalkDir;

use feathernix_recipe::{Recipe, SourceKind};
use feathernix_repo::{Fetcher, RepoError};

#[derive(Debug, Error)]
pub enum SourceError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error("git command failed: {0}")]
    Git(String),
    #[error("source url required for kind={kind:?}")]
    UrlRequired { kind: SourceKind },
    #[error("source rev required for kind=git")]
    RevRequired,
    #[error("source path required for kind=path")]
    PathRequired,
    #[error("source path does not exist: {0}")]
    PathMissing(PathBuf),
    #[error(
        "source tree sha256 mismatch: expected {expected}, got {actual} (recipe pin out of date or upstream tampered)"
    )]
    TreeShaMismatch { expected: String, actual: String },
    #[error("tarball archive failed to unpack: {0}")]
    Unpack(String),
}

pub type SourceResult<T> = Result<T, SourceError>;

#[derive(Debug, Clone)]
pub struct FetchedSource {
    pub root: PathBuf,
    pub tree_sha256: String,
}

pub struct SourceFetcher {
    cache_root: PathBuf,
    http: Fetcher,
}

impl SourceFetcher {
    pub fn new(cache_root: PathBuf) -> SourceResult<Self> {
        std::fs::create_dir_all(&cache_root)?;
        let http = Fetcher::new()?;
        Ok(Self { cache_root, http })
    }

    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    pub fn fetch(&self, recipe: &Recipe) -> SourceResult<FetchedSource> {
        match recipe.source.kind {
            SourceKind::Git => self.fetch_git(recipe),
            SourceKind::Tarball => self.fetch_tarball(recipe),
            SourceKind::Path => self.fetch_path(recipe),
        }
    }

    fn fetch_git(&self, recipe: &Recipe) -> SourceResult<FetchedSource> {
        let url = recipe
            .source
            .url
            .as_deref()
            .ok_or(SourceError::UrlRequired { kind: SourceKind::Git })?;
        let rev = recipe.source.rev.as_deref().ok_or(SourceError::RevRequired)?;
        let id = cache_id(&format!("git:{url}@{rev}"));
        let dest = self.cache_root.join(&id);
        if !dest.join(".fnsource-ok").exists() {
            if dest.exists() {
                std::fs::remove_dir_all(&dest)?;
            }
            git::clone_into(url, rev, &dest)?;
            std::fs::write(dest.join(".fnsource-ok"), b"")?;
        }
        let tree_sha = compute_tree_sha256(&dest)?;
        if let Some(expected) = recipe.source.sha256.as_deref() {
            if expected != tree_sha {
                return Err(SourceError::TreeShaMismatch {
                    expected: expected.to_string(),
                    actual: tree_sha,
                });
            }
        }
        Ok(FetchedSource { root: dest, tree_sha256: tree_sha })
    }

    fn fetch_tarball(&self, recipe: &Recipe) -> SourceResult<FetchedSource> {
        let url = recipe
            .source
            .url
            .as_deref()
            .ok_or(SourceError::UrlRequired { kind: SourceKind::Tarball })?;
        let expected_sha = recipe
            .source
            .sha256
            .as_deref()
            .ok_or_else(|| SourceError::Unpack("tarball requires sha256 pin".into()))?;
        let id = cache_id(&format!("tar:{url}#{expected_sha}"));
        let dest = self.cache_root.join(&id);
        if !dest.join(".fnsource-ok").exists() {
            if dest.exists() {
                std::fs::remove_dir_all(&dest)?;
            }
            std::fs::create_dir_all(&dest)?;
            let archive = self.cache_root.join(format!("{id}.tar"));
            self.http.fetch_to_file(url, "", &archive, Some(expected_sha))?;
            tarball::unpack(&archive, &dest)?;
            let _ = std::fs::remove_file(&archive);
            std::fs::write(dest.join(".fnsource-ok"), b"")?;
        }
        let tree_sha = compute_tree_sha256(&dest)?;
        Ok(FetchedSource { root: dest, tree_sha256: tree_sha })
    }

    fn fetch_path(&self, recipe: &Recipe) -> SourceResult<FetchedSource> {
        let p = recipe.source.path.as_deref().ok_or(SourceError::PathRequired)?;
        let root = PathBuf::from(p);
        if !root.exists() {
            return Err(SourceError::PathMissing(root));
        }
        let tree_sha = compute_tree_sha256(&root)?;
        if let Some(expected) = recipe.source.sha256.as_deref() {
            if expected != tree_sha {
                return Err(SourceError::TreeShaMismatch {
                    expected: expected.to_string(),
                    actual: tree_sha,
                });
            }
        }
        Ok(FetchedSource { root, tree_sha256: tree_sha })
    }
}

fn cache_id(input: &str) -> String {
    hex::encode(Sha256::digest(input.as_bytes()))[..32].to_string()
}

/// Deterministic content hash of every file under `root` (path, mode-ish, contents).
/// Skips `.git` and our `.fnsource-ok` sentinel.
pub fn compute_tree_sha256(root: &Path) -> SourceResult<String> {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in WalkDir::new(root).sort_by_file_name() {
        let entry = entry.map_err(|e| SourceError::Io(std::io::Error::other(e.to_string())))?;
        if entry.file_type().is_dir() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(root)
            .map_err(|e| SourceError::Io(std::io::Error::other(e.to_string())))?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if rel_str.starts_with(".git/") || rel_str == ".git" || rel_str == ".fnsource-ok" {
            continue;
        }
        let bytes = std::fs::read(entry.path())?;
        entries.push((rel_str, bytes));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut h = Sha256::new();
    for (path, bytes) in entries {
        h.update(path.as_bytes());
        h.update(b"\0");
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(&bytes);
    }
    Ok(hex::encode(h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    use feathernix_recipe::Recipe;

    #[test]
    fn tree_sha_is_stable_and_changes_with_content() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a"), b"hello").unwrap();
        std::fs::create_dir_all(tmp.path().join("sub")).unwrap();
        std::fs::write(tmp.path().join("sub/b"), b"world").unwrap();
        let h1 = compute_tree_sha256(tmp.path()).unwrap();
        let h2 = compute_tree_sha256(tmp.path()).unwrap();
        assert_eq!(h1, h2);
        std::fs::write(tmp.path().join("a"), b"hello!").unwrap();
        let h3 = compute_tree_sha256(tmp.path()).unwrap();
        assert_ne!(h1, h3);
    }

    #[test]
    fn fetch_path_returns_root_and_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("hello.sh"), b"echo hi\n").unwrap();
        let cache = tmp.path().join("cache");
        let r = Recipe::from_str(&format!(
            r#"
[recipe]
format_version = 1
[package]
name = "hello"
version = "0.1.0"
arch = "x86_64-linux"
[source]
kind = "path"
path = "{}"
[build]
[dependencies]
"#,
            src.display()
        ))
        .unwrap();
        let f = SourceFetcher::new(cache).unwrap();
        let fetched = f.fetch(&r).unwrap();
        assert_eq!(fetched.root, src);
        assert!(!fetched.tree_sha256.is_empty());
    }

    #[test]
    fn fetch_path_with_wrong_sha_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a"), b"x").unwrap();
        let r = Recipe::from_str(&format!(
            r#"
[recipe]
format_version = 1
[package]
name = "a"
version = "1"
arch = "x86_64-linux"
[source]
kind   = "path"
path   = "{}"
sha256 = "{}"
[build]
[dependencies]
"#,
            src.display(),
            "00".repeat(32)
        ))
        .unwrap();
        let f = SourceFetcher::new(tmp.path().join("cache")).unwrap();
        let err = f.fetch(&r).unwrap_err();
        assert!(matches!(err, SourceError::TreeShaMismatch { .. }));
    }
}
