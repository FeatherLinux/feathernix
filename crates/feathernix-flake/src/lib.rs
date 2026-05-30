pub mod types;

use std::path::{Path, PathBuf};

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
    std::fs::write(&p, raw)?;
    Ok(())
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
                revision: None,
                sha256: None,
                index_sha256: Some("abc".to_string()),
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
    }
}
