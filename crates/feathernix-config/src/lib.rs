pub mod config;
pub mod paths;
pub mod repo;

pub use config::{GeneralBlock, InstallBlock, SystemConfig};
pub use paths::{FEATHERNIX_ROOT_ENV, Paths};
pub use repo::{LoadedRepo, RefreshBlock, RepoBlock, RepoFile, SigningBlock, load_all, load_dir};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("i/o error reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("parse error in {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("repo filename {path} ({expected}) does not match [repository].name = {actual:?}")]
    NameMismatch {
        path: String,
        expected: String,
        actual: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const EXAMPLE_REPO: &str = r#"
[repository]
name        = "feathernix-stable"
description = "FeatherNix Stable for x86_64-linux"
base_url    = "https://repo.feathernix.org/stable/x86_64-linux"
enabled     = true
priority    = 50

[repository.signing]
gpg_check          = true
gpg_key_url        = "https://repo.feathernix.org/keys/release.asc"
signature_required = true

[repository.refresh]
metadata_expire_seconds = 21600
"#;

    #[test]
    fn parses_example_repo() {
        let file = RepoFile::from_str(EXAMPLE_REPO).expect("parse");
        let r = &file.repository;
        assert_eq!(r.name, "feathernix-stable");
        assert_eq!(r.base_url, "https://repo.feathernix.org/stable/x86_64-linux");
        assert!(r.enabled);
        assert_eq!(r.priority, 50);
        let signing = r.signing.as_ref().expect("signing block");
        assert!(signing.gpg_check);
        assert!(signing.signature_required);
        let refresh = r.refresh.as_ref().expect("refresh block");
        assert_eq!(refresh.metadata_expire_seconds, 21600);
    }

    #[test]
    fn round_trips_example_repo() {
        let parsed = RepoFile::from_str(EXAMPLE_REPO).expect("parse");
        let written = toml::to_string_pretty(&parsed).expect("ser");
        let reparsed = RepoFile::from_str(&written).expect("reparse");
        assert_eq!(reparsed.repository.name, "feathernix-stable");
    }

    #[test]
    fn name_must_match_filename() {
        let tmp = tempfile::tempdir().expect("tmp");
        let path = tmp.path().join("not-the-name.toml");
        fs::write(&path, EXAMPLE_REPO).unwrap();
        let err = RepoFile::load(&path).unwrap_err();
        assert!(matches!(err, ConfigError::NameMismatch { .. }));
    }

    #[test]
    fn load_all_merges_system_and_user() {
        let tmp = tempfile::tempdir().expect("tmp");
        let system_dir = tmp.path().join("system");
        let user_dir = tmp.path().join("user");
        fs::create_dir_all(&system_dir).unwrap();
        fs::create_dir_all(&user_dir).unwrap();
        fs::write(system_dir.join("feathernix-stable.toml"), EXAMPLE_REPO).unwrap();
        fs::write(
            user_dir.join("devrepo.toml"),
            r#"
[repository]
name = "devrepo"
base_url = "http://localhost:8080/devrepo"
priority = 10
"#,
        )
        .unwrap();

        let all = load_all(&system_dir, &user_dir).expect("load_all");
        let names: Vec<_> = all.iter().map(|r| r.repo.name.as_str()).collect();
        assert_eq!(names, vec!["devrepo", "feathernix-stable"]);
    }

    #[test]
    fn user_overrides_system_by_name() {
        let tmp = tempfile::tempdir().expect("tmp");
        let system_dir = tmp.path().join("system");
        let user_dir = tmp.path().join("user");
        fs::create_dir_all(&system_dir).unwrap();
        fs::create_dir_all(&user_dir).unwrap();
        fs::write(system_dir.join("feathernix-stable.toml"), EXAMPLE_REPO).unwrap();
        fs::write(
            user_dir.join("feathernix-stable.toml"),
            r#"
[repository]
name = "feathernix-stable"
base_url = "http://localhost:8080/override"
enabled = false
"#,
        )
        .unwrap();

        let all = load_all(&system_dir, &user_dir).expect("load_all");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].repo.base_url, "http://localhost:8080/override");
        assert!(!all[0].repo.enabled);
    }
}
