use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Recipe {
    pub recipe: RecipeMeta,
    pub package: PackageBlock,
    pub source: SourceSpec,
    pub build: BuildBlock,
    #[serde(default)]
    pub dependencies: DependenciesBlock,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RecipeMeta {
    pub format_version: u32,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SourceSpec {
    pub kind: SourceKind,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub rev: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Git,
    Tarball,
    Path,
}

impl SourceSpec {
    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            SourceKind::Git => "git",
            SourceKind::Tarball => "tarball",
            SourceKind::Path => "path",
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BuildBlock {
    #[serde(default)]
    pub prepare: Vec<String>,
    #[serde(default)]
    pub build: Vec<String>,
    #[serde(default)]
    pub install: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DependenciesBlock {
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub provides: Vec<String>,
    #[serde(default)]
    pub build_requires: Vec<String>,
}
