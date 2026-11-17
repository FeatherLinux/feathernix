use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Flake {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub inputs: BTreeMap<String, FlakeInput>,
    #[serde(default)]
    pub outputs: FlakeOutputs,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FlakeInput {
    pub kind: InputKind,
    pub url: String,
    #[serde(default, rename = "ref")]
    pub ref_: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InputKind {
    Repo,
    Flake,
    Path,
}

impl fmt::Display for InputKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            InputKind::Repo => "repo",
            InputKind::Flake => "flake",
            InputKind::Path => "path",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct FlakeOutputs {
    #[serde(default)]
    pub packages: BTreeMap<String, OutputSet>,
    #[serde(rename = "devShells", default)]
    pub dev_shells: BTreeMap<String, DevShell>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct OutputSet {
    #[serde(default)]
    pub packages: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DevShell {
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(rename = "shellHook", default)]
    pub shell_hook: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct FlakeLock {
    pub format_version: u32,
    pub locked_at: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, LockedInput>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LockedInput {
    pub kind: InputKind,
    pub url: String,
    #[serde(default)]
    pub locked_revision: Option<String>,
    #[serde(default)]
    pub locked_sha256: Option<String>,
    pub fetched_at: String,
}
