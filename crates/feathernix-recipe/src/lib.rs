pub mod types;

use std::path::{Path, PathBuf};

use thiserror::Error;

pub use types::{
    BuildBlock, DependenciesBlock, PackageBlock, Recipe, RecipeMeta, SourceKind, SourceSpec,
};

pub const RECIPE_FILENAME: &str = "feathernix.recipe.toml";

#[derive(Debug, Error)]
pub enum RecipeError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("recipe parse error in {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("recipe serialize error: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("recipe not found in {0}")]
    NotFound(PathBuf),
    #[error("recipe already exists in {0}")]
    AlreadyExists(PathBuf),
    #[error("recipe validation failed: {0}")]
    Invalid(String),
}

pub type RecipeResult<T> = Result<T, RecipeError>;

pub fn recipe_path(root: &Path) -> PathBuf {
    root.join(RECIPE_FILENAME)
}

pub fn load(root: &Path) -> RecipeResult<Recipe> {
    let p = recipe_path(root);
    if !p.exists() {
        return Err(RecipeError::NotFound(p));
    }
    let raw = std::fs::read_to_string(&p)?;
    let r: Recipe = toml::from_str(&raw).map_err(|e| RecipeError::Parse {
        path: p.display().to_string(),
        source: e,
    })?;
    r.validate()?;
    Ok(r)
}

pub fn init(root: &Path, name: &str, version: &str, arch: &str) -> RecipeResult<PathBuf> {
    let p = recipe_path(root);
    if p.exists() {
        return Err(RecipeError::AlreadyExists(p));
    }
    let body = default_template(name, version, arch);
    std::fs::create_dir_all(root)?;
    std::fs::write(&p, body)?;
    Ok(p)
}

fn default_template(name: &str, version: &str, arch: &str) -> String {
    format!(
        r#"[recipe]
format_version = 1

[package]
name    = "{name}"
version = "{version}"
release = "1"
epoch   = 0
arch    = "{arch}"
summary = "TODO: one-line description"
license = "TODO"
url     = "https://example.org/{name}"

[source]
kind   = "git"
url    = "https://example.org/{name}.git"
rev    = "v{version}"
# sha256 = "fill in after first successful clone"

[build]
prepare = []
build   = ["echo TODO: build {name}"]
install = ["install -Dm755 hello $OUT/bin/{name}"]

[build.env]
# RUSTFLAGS = "-C target-cpu=x86-64-v2"

[dependencies]
requires       = []
provides       = ["{name} = {version}"]
build_requires = []
"#
    )
}

impl Recipe {
    pub fn validate(&self) -> RecipeResult<()> {
        if self.recipe.format_version != 1 {
            return Err(RecipeError::Invalid(format!(
                "unsupported recipe format_version {} (only 1 is known)",
                self.recipe.format_version
            )));
        }
        if self.package.name.trim().is_empty() {
            return Err(RecipeError::Invalid("[package].name is empty".into()));
        }
        if self.package.version.trim().is_empty() {
            return Err(RecipeError::Invalid("[package].version is empty".into()));
        }
        if self.package.arch.trim().is_empty() {
            return Err(RecipeError::Invalid("[package].arch is empty".into()));
        }
        match self.source.kind {
            SourceKind::Git => {
                if self.source.url.is_none() {
                    return Err(RecipeError::Invalid(
                        "[source].url required when kind = \"git\"".into(),
                    ));
                }
                let rev = self.source.rev.as_deref().unwrap_or("");
                if rev.is_empty() {
                    return Err(RecipeError::Invalid(
                        "[source].rev required when kind = \"git\" (no floating branches)".into(),
                    ));
                }
            }
            SourceKind::Tarball => {
                if self.source.url.is_none() {
                    return Err(RecipeError::Invalid(
                        "[source].url required when kind = \"tarball\"".into(),
                    ));
                }
                if self.source.sha256.is_none() {
                    return Err(RecipeError::Invalid(
                        "[source].sha256 required when kind = \"tarball\"".into(),
                    ));
                }
            }
            SourceKind::Path => {
                if self.source.path.is_none() {
                    return Err(RecipeError::Invalid(
                        "[source].path required when kind = \"path\"".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn from_str(s: &str) -> RecipeResult<Self> {
        let r: Recipe = toml::from_str(s).map_err(|e| RecipeError::Parse {
            path: "<string>".to_string(),
            source: e,
        })?;
        r.validate()?;
        Ok(r)
    }

    pub fn to_pretty_string(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    pub fn artifact_filename(&self) -> String {
        format!(
            "{}-{}-{}-{}.fnpkg.7z",
            self.package.name, self.package.version, self.package.release, self.package.arch
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[recipe]
format_version = 1

[package]
name    = "ripgrep"
version = "14.1.0"
release = "1"
arch    = "x86_64-linux"
summary = "Fast recursive grep"
license = "MIT"
url     = "https://github.com/BurntSushi/ripgrep"

[source]
kind   = "git"
url    = "https://github.com/BurntSushi/ripgrep.git"
rev    = "14.1.0"

[build]
prepare = []
build   = ["cargo build --release --locked"]
install = ["install -Dm755 target/release/rg $OUT/bin/rg"]

[build.env]
RUSTFLAGS = "-C target-cpu=x86-64-v2"

[dependencies]
requires       = ["glibc >= 2.17"]
provides       = ["rg = 14.1.0"]
build_requires = ["rustc >= 1.85", "cargo"]
"#;

    #[test]
    fn parses_sample() {
        let r = Recipe::from_str(SAMPLE).expect("parse");
        assert_eq!(r.package.name, "ripgrep");
        assert_eq!(r.source.kind, SourceKind::Git);
        assert_eq!(r.source.rev.as_deref(), Some("14.1.0"));
        assert_eq!(r.build.build.len(), 1);
        assert_eq!(
            r.build.env.get("RUSTFLAGS").map(String::as_str),
            Some("-C target-cpu=x86-64-v2")
        );
        assert_eq!(r.dependencies.requires, vec!["glibc >= 2.17"]);
    }

    #[test]
    fn init_writes_template_that_parses() {
        let tmp = tempfile::tempdir().unwrap();
        let p = init(tmp.path(), "hello", "0.1.0", "x86_64-linux").unwrap();
        assert!(p.exists());
        let r = load(tmp.path()).unwrap();
        assert_eq!(r.package.name, "hello");
        assert_eq!(r.package.version, "0.1.0");
        assert_eq!(r.package.arch, "x86_64-linux");
    }

    #[test]
    fn init_rejects_existing() {
        let tmp = tempfile::tempdir().unwrap();
        init(tmp.path(), "a", "1", "x").unwrap();
        let err = init(tmp.path(), "a", "1", "x").unwrap_err();
        assert!(matches!(err, RecipeError::AlreadyExists(_)));
    }

    #[test]
    fn git_without_rev_is_invalid() {
        let bad = r#"
[recipe]
format_version = 1
[package]
name = "a"
version = "1"
arch = "x86_64-linux"
[source]
kind = "git"
url  = "https://x/a.git"
[build]
prepare = []
build   = []
install = []
[dependencies]
"#;
        let err = Recipe::from_str(bad).unwrap_err();
        assert!(matches!(err, RecipeError::Invalid(_)));
    }

    #[test]
    fn tarball_without_sha_is_invalid() {
        let bad = r#"
[recipe]
format_version = 1
[package]
name = "a"
version = "1"
arch = "x86_64-linux"
[source]
kind = "tarball"
url  = "https://x/a.tar.gz"
[build]
prepare = []
build   = []
install = []
[dependencies]
"#;
        let err = Recipe::from_str(bad).unwrap_err();
        assert!(matches!(err, RecipeError::Invalid(_)));
    }

    #[test]
    fn artifact_filename_matches_pattern() {
        let r = Recipe::from_str(SAMPLE).unwrap();
        assert_eq!(r.artifact_filename(), "ripgrep-14.1.0-1-x86_64-linux.fnpkg.7z");
    }
}
