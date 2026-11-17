pub mod types;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use feathernix_repo::{Fetcher, INDEX_REL_PATH, RepoError, RepositoryIndex, sha256_hex};
use thiserror::Error;

pub use types::{
    DevShell, Flake, FlakeInput, FlakeLock, FlakeOutputs, InputKind, LockedInput, OutputSet,
};

pub const FLAKE_FILENAME: &str = "flake.toml";
pub const FLAKE_LOCK_FILENAME: &str = "flake.lock.toml";

#[derive(Debug, Error)]
pub enum FlakeError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("flake parse error in {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("flake serialize error: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("flake.toml not found in {0}")]
    NotFound(PathBuf),
    #[error("flake.toml already exists in {0}")]
    AlreadyExists(PathBuf),
    #[error("unsupported {operation} input {name:?} of kind {kind}")]
    UnsupportedInputKind {
        name: String,
        kind: InputKind,
        operation: &'static str,
    },
    #[error("failed to lock input {name:?} from {url}: {source}")]
    LockInput {
        name: String,
        url: String,
        #[source]
        source: RepoError,
    },
}

pub type FlakeResult<T> = Result<T, FlakeError>;

pub fn flake_path(root: &Path) -> PathBuf {
    root.join(FLAKE_FILENAME)
}

pub fn lock_path(root: &Path) -> PathBuf {
    root.join(FLAKE_LOCK_FILENAME)
}

pub fn load(root: &Path) -> FlakeResult<Flake> {
    let p = flake_path(root);
    if !p.exists() {
        return Err(FlakeError::NotFound(p));
    }
    let raw = std::fs::read_to_string(&p)?;
    let f: Flake = toml::from_str(&raw).map_err(|e| FlakeError::Parse {
        path: p.display().to_string(),
        source: e,
    })?;
    Ok(f)
}

pub fn load_lock(root: &Path) -> FlakeResult<Option<FlakeLock>> {
    let p = lock_path(root);
    if !p.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&p)?;
    let lk: FlakeLock = toml::from_str(&raw).map_err(|e| FlakeError::Parse {
        path: p.display().to_string(),
        source: e,
    })?;
    Ok(Some(lk))
}

pub fn save_lock(root: &Path, lock: &FlakeLock) -> FlakeResult<()> {
    let p = lock_path(root);
    let raw = toml::to_string_pretty(lock)?;
    let tmp = p.with_extension("toml.tmp");
    std::fs::write(&tmp, raw)?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

pub fn build_lock(root: &Path) -> FlakeResult<FlakeLock> {
    let flake = load(root)?;
    let fetcher = Fetcher::new().map_err(|source| FlakeError::LockInput {
        name: "<fetcher>".to_string(),
        url: String::new(),
        source,
    })?;
    let locked_at = utc_now_rfc3339();
    let mut inputs = BTreeMap::new();

    for (name, input) in &flake.inputs {
        let locked = match input.kind {
            InputKind::Repo => lock_repo_input(&fetcher, name, input)?,
            InputKind::Flake | InputKind::Path => {
                return Err(FlakeError::UnsupportedInputKind {
                    name: name.clone(),
                    kind: input.kind,
                    operation: "lock",
                });
            }
        };
        inputs.insert(name.clone(), locked);
    }

    Ok(FlakeLock {
        format_version: 1,
        locked_at,
        inputs,
    })
}

fn lock_repo_input(fetcher: &Fetcher, name: &str, input: &FlakeInput) -> FlakeResult<LockedInput> {
    let bytes = fetcher
        .fetch_bytes(&input.url, INDEX_REL_PATH)
        .map_err(|source| FlakeError::LockInput {
            name: name.to_string(),
            url: input.url.clone(),
            source,
        })?;
    let index_text = std::str::from_utf8(&bytes).map_err(|source| FlakeError::LockInput {
        name: name.to_string(),
        url: input.url.clone(),
        source: RepoError::ParseIndex(source.to_string()),
    })?;
    let index = RepositoryIndex::from_str(index_text).map_err(|source| FlakeError::LockInput {
        name: name.to_string(),
        url: input.url.clone(),
        source: RepoError::ParseIndex(source.to_string()),
    })?;

    Ok(LockedInput {
        kind: InputKind::Repo,
        url: input.url.clone(),
        locked_revision: Some(index.index.generated_at),
        locked_sha256: Some(sha256_hex(&bytes)),
        fetched_at: utc_now_rfc3339(),
    })
}

fn utc_now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub fn init(root: &Path, description: Option<&str>) -> FlakeResult<PathBuf> {
    let p = flake_path(root);
    if p.exists() {
        return Err(FlakeError::AlreadyExists(p));
    }
    let body = default_template(description);
    std::fs::create_dir_all(root)?;
    std::fs::write(&p, body)?;
    Ok(p)
}

fn default_template(description: Option<&str>) -> String {
    let desc = description.unwrap_or("A FeatherNix flake");
    format!(
        r#"description = "{desc}"

[inputs.feathernix-stable]
kind = "repo"
url  = "https://repo.feathernix.org/stable/x86_64-linux"

[outputs.packages.default]
packages = ["feathernix-stable#hello"]

[outputs.devShells.default]
packages  = ["feathernix-stable#hello"]
shellHook = "echo 'entered FeatherNix dev shell'"
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_writes_default_flake_and_load_reads_it() {
        let tmp = tempfile::tempdir().unwrap();
        let p = init(tmp.path(), Some("test flake")).unwrap();
        assert!(p.exists());
        let f = load(tmp.path()).unwrap();
        assert_eq!(f.description.as_deref(), Some("test flake"));
        assert!(f.inputs.contains_key("feathernix-stable"));
        assert_eq!(f.inputs["feathernix-stable"].kind, InputKind::Repo);
        assert!(f.outputs.packages.contains_key("default"));
        assert!(f.outputs.dev_shells.contains_key("default"));
    }

    #[test]
    fn init_rejects_existing_flake() {
        let tmp = tempfile::tempdir().unwrap();
        init(tmp.path(), None).unwrap();
        let err = init(tmp.path(), None).unwrap_err();
        assert!(matches!(err, FlakeError::AlreadyExists(_)));
    }

    #[test]
    fn lock_roundtrips() {
        let tmp = tempfile::tempdir().unwrap();
        let mut inputs = std::collections::BTreeMap::new();
        inputs.insert(
            "stable".to_string(),
            LockedInput {
                kind: InputKind::Repo,
                url: "http://x".to_string(),
                locked_revision: Some("2026-05-21T10:00:00Z".to_string()),
                locked_sha256: Some("abc".to_string()),
                fetched_at: "2026-05-21T10:00:00Z".to_string(),
            },
        );
        let lock = FlakeLock {
            format_version: 1,
            locked_at: "2026-05-21T10:00:00Z".to_string(),
            inputs,
        };
        save_lock(tmp.path(), &lock).unwrap();
        let loaded = load_lock(tmp.path()).unwrap().unwrap();
        assert_eq!(loaded.format_version, 1);
        assert!(loaded.inputs.contains_key("stable"));
        assert_eq!(
            loaded.inputs["stable"].locked_revision.as_deref(),
            Some("2026-05-21T10:00:00Z")
        );
        assert_eq!(
            loaded.inputs["stable"].locked_sha256.as_deref(),
            Some("abc")
        );
    }

    #[test]
    fn build_lock_locks_repo_input_from_index() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("repodata")).unwrap();
        std::fs::write(
            repo.join("repodata/index.toml"),
            r#"
[index]
format_version = 1
generated_at = "2026-05-21T10:00:00Z"
repository = "devrepo"
"#,
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(FLAKE_FILENAME),
            format!(
                r#"
[inputs.devrepo]
kind = "repo"
url = "file://{}"
"#,
                repo.display()
            ),
        )
        .unwrap();

        let lock = build_lock(tmp.path()).unwrap();
        let input = lock.inputs.get("devrepo").unwrap();
        assert_eq!(
            input.locked_revision.as_deref(),
            Some("2026-05-21T10:00:00Z")
        );
        assert!(input.locked_sha256.as_deref().unwrap().len() == 64);
        assert!(input.fetched_at.ends_with('Z'));
    }

    #[test]
    fn build_lock_rejects_flake_inputs() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(FLAKE_FILENAME),
            r#"
[inputs.other]
kind = "flake"
url = "https://example.invalid/flake"
"#,
        )
        .unwrap();
        let err = build_lock(tmp.path()).unwrap_err();
        assert!(
            matches!(err, FlakeError::UnsupportedInputKind { name, kind: InputKind::Flake, .. } if name == "other")
        );
    }

    #[test]
    fn build_lock_rejects_path_inputs() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(FLAKE_FILENAME),
            r#"
[inputs.local]
kind = "path"
url = "../local"
"#,
        )
        .unwrap();
        let err = build_lock(tmp.path()).unwrap_err();
        assert!(
            matches!(err, FlakeError::UnsupportedInputKind { name, kind: InputKind::Path, .. } if name == "local")
        );
    }
}
