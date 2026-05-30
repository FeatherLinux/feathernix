use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ConfigError;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepoFile {
    pub repository: RepoBlock,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepoBlock {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub base_url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_priority")]
    pub priority: i32,
    #[serde(default)]
    pub signing: Option<SigningBlock>,
    #[serde(default)]
    pub refresh: Option<RefreshBlock>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SigningBlock {
    #[serde(default)]
    pub gpg_check: bool,
    #[serde(default)]
    pub gpg_key_url: Option<String>,
    #[serde(default)]
    pub signature_required: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshBlock {
    pub metadata_expire_seconds: u64,
}

fn default_true() -> bool {
    true
}

fn default_priority() -> i32 {
    50
}

#[derive(Debug, Clone)]
pub struct LoadedRepo {
    pub source_path: PathBuf,
    pub repo: RepoBlock,
}

impl RepoFile {
    pub fn from_str(s: &str) -> Result<Self, ConfigError> {
        toml::from_str(s).map_err(|e| ConfigError::Parse {
            path: "<inline>".into(),
            source: e,
        })
    }

    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let raw = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
            path: path.display().to_string(),
            source: e,
        })?;
        let parsed: RepoFile = toml::from_str(&raw).map_err(|e| ConfigError::Parse {
            path: path.display().to_string(),
            source: e,
        })?;

        if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            && stem != parsed.repository.name
        {
            return Err(ConfigError::NameMismatch {
                path: path.display().to_string(),
                expected: stem.to_string(),
                actual: parsed.repository.name.clone(),
            });
        }
        Ok(parsed)
    }
}

pub fn load_dir(dir: &Path) -> Result<Vec<LoadedRepo>, ConfigError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| ConfigError::Io {
            path: dir.display().to_string(),
            source: e,
        })?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    entries.sort();

    for path in entries {
        let file = RepoFile::load(&path)?;
        out.push(LoadedRepo {
            source_path: path,
            repo: file.repository,
        });
    }
    Ok(out)
}

pub fn load_all(system_dir: &Path, user_dir: &Path) -> Result<Vec<LoadedRepo>, ConfigError> {
    let mut by_name: BTreeMap<String, LoadedRepo> = BTreeMap::new();
    for r in load_dir(system_dir)? {
        by_name.insert(r.repo.name.clone(), r);
    }
    for r in load_dir(user_dir)? {
        by_name.insert(r.repo.name.clone(), r);
    }
    Ok(by_name.into_values().collect())
}
