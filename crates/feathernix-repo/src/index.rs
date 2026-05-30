use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryIndex {
    pub index: IndexMeta,
    #[serde(default)]
    pub packages: Vec<PackageEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IndexMeta {
    pub format_version: u32,
    pub generated_at: String,
    pub repository: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageEntry {
    pub name: String,
    pub version: String,
    #[serde(default = "default_release")]
    pub release: String,
    #[serde(default)]
    pub epoch: i32,
    pub arch: String,
    pub payload_path: String,
    pub payload_sha256: String,
    pub payload_size: i64,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub provides: Vec<String>,
}

fn default_release() -> String {
    "1".to_string()
}

impl RepositoryIndex {
    pub fn from_str(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    pub fn to_pretty_string(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}
