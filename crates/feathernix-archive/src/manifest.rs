use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manifest {
    pub package: PackageBlock,
    #[serde(default)]
    pub dependencies: DependenciesBlock,
    #[serde(default)]
    pub outputs: OutputsBlock,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageBlock {
    pub name: String,
    pub version: String,
    #[serde(default = "default_release")]
    pub release: String,
    #[serde(default)]
    pub epoch: i32,
    pub arch: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

fn default_release() -> String {
    "1".to_string()
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DependenciesBlock {
    #[serde(default)]
    pub requires: Vec<DependencyRef>,
    #[serde(default)]
    pub provides: Vec<DependencyRef>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DependencyRef {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub version_constraint: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct OutputsBlock {
    #[serde(default = "default_output")]
    pub default: String,
}

fn default_output() -> String {
    "out".to_string()
}

impl Manifest {
    pub fn from_str(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    pub fn to_pretty_string(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}
