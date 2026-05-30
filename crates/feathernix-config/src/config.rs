use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ConfigError;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SystemConfig {
    #[serde(default)]
    pub general: GeneralBlock,
    #[serde(default)]
    pub install: InstallBlock,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            general: GeneralBlock::default(),
            install: InstallBlock::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralBlock {
    #[serde(default = "default_arch")]
    pub default_arch: String,
    #[serde(default = "default_profile")]
    pub default_profile: String,
}

impl Default for GeneralBlock {
    fn default() -> Self {
        Self {
            default_arch: default_arch(),
            default_profile: default_profile(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstallBlock {
    #[serde(default = "default_signature_policy")]
    pub require_signatures: bool,
    #[serde(default = "default_metadata_expire")]
    pub metadata_expire_seconds: u64,
}

impl Default for InstallBlock {
    fn default() -> Self {
        Self {
            require_signatures: default_signature_policy(),
            metadata_expire_seconds: default_metadata_expire(),
        }
    }
}

fn default_arch() -> String {
    if cfg!(target_arch = "x86_64") {
        "x86_64-linux".to_string()
    } else if cfg!(target_arch = "aarch64") {
        "aarch64-linux".to_string()
    } else {
        "unknown".to_string()
    }
}

fn default_profile() -> String {
    "system".to_string()
}

fn default_signature_policy() -> bool {
    false
}

fn default_metadata_expire() -> u64 {
    21600
}

impl SystemConfig {
    pub fn from_str(s: &str) -> Result<Self, ConfigError> {
        toml::from_str(s).map_err(|e| ConfigError::Parse {
            path: "<inline>".into(),
            source: e,
        })
    }

    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
            path: path.display().to_string(),
            source: e,
        })?;
        toml::from_str(&raw).map_err(|e| ConfigError::Parse {
            path: path.display().to_string(),
            source: e,
        })
    }
}
