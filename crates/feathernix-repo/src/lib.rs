pub mod fetcher;
pub mod gpg;
pub mod index;
pub mod resolver;
pub mod sync;

use thiserror::Error;

pub use fetcher::{Fetcher, join_url, sha256_hex};
pub use gpg::{Keyring, keyring_cache_path, load_or_fetch as load_or_fetch_keyring};
pub use index::{IndexMeta, PackageEntry, RepositoryIndex};
pub use resolver::{ResolvedPackage, Resolver};
pub use sync::{RefreshReport, RepoSync, SignatureStatus};

pub const INDEX_REL_PATH: &str = "repodata/index.toml";
pub const INDEX_SIG_REL_PATH: &str = "repodata/index.toml.sig";

#[derive(Debug, Error)]
pub enum RepoError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Db(#[from] feathernix_db::DbError),
    #[error("index parse error: {0}")]
    ParseIndex(String),
    #[error("serialization error: {0}")]
    Serialize(String),
    #[error("hash mismatch for {url}: expected {expected}, got {actual}")]
    HashMismatch {
        url: String,
        expected: String,
        actual: String,
    },
    #[error("package {name} not found for arch {arch}")]
    PackageNotFound { name: String, arch: String },
    #[error(
        "repository URL {base_url} is not cached; run `fn repo add <name> {base_url}` and `fn repo refresh` first"
    )]
    RepositoryNotCached { base_url: String },
    #[error("signature missing for {url}")]
    SignatureMissing { url: String },
    #[error("signature invalid for {url}: {reason}")]
    SignatureInvalid { url: String, reason: String },
    #[error("failed to load GPG key for repo {repo}: {reason}")]
    KeyloadFailed { repo: String, reason: String },
    #[error("repo {repo} has gpg_check = true but no gpg_key_url configured")]
    GpgCheckButNoKeyUrl { repo: String },
}

pub type RepoResult<T> = Result<T, RepoError>;

#[cfg(test)]
mod tests {
    use super::*;

    use feathernix_archive::{Manifest, pack};
    use feathernix_config::{LoadedRepo, RepoBlock};
    use feathernix_db::Db;

    fn make_fixture_repo(workdir: &std::path::Path) -> std::path::PathBuf {
        let repo_root = workdir.join("repo");
        let packages_dir = repo_root.join("packages");
        let repodata_dir = repo_root.join("repodata");
        std::fs::create_dir_all(&packages_dir).unwrap();
        std::fs::create_dir_all(&repodata_dir).unwrap();

        // Build a tiny .fnpkg.7z
        let src = workdir.join("src-hello");
        std::fs::create_dir_all(src.join("payload/bin")).unwrap();
        let manifest = Manifest::from_str(
            r#"
[package]
name = "hello"
version = "1.0.0"
release = "1"
arch = "x86_64-linux"
"#,
        )
        .unwrap();
        std::fs::write(
            src.join("manifest.toml"),
            manifest.to_pretty_string().unwrap(),
        )
        .unwrap();
        std::fs::write(src.join("payload/bin/hello"), b"echo hello\n").unwrap();
        let pkg_path = packages_dir.join("hello-1.0.0-1-x86_64-linux.fnpkg.7z");
        pack(&src, &pkg_path).unwrap();

        let payload_bytes = std::fs::read(&pkg_path).unwrap();
        let payload_sha = fetcher::sha256_hex(&payload_bytes);
        let payload_size = payload_bytes.len() as i64;

        let index = RepositoryIndex {
            index: IndexMeta {
                format_version: 1,
                generated_at: "2026-05-21T10:00:00Z".to_string(),
                repository: "devrepo".to_string(),
            },
            packages: vec![PackageEntry {
                name: "hello".to_string(),
                version: "1.0.0".to_string(),
                release: "1".to_string(),
                epoch: 0,
                arch: "x86_64-linux".to_string(),
                payload_path: "packages/hello-1.0.0-1-x86_64-linux.fnpkg.7z".to_string(),
                payload_sha256: payload_sha,
                payload_size,
                summary: Some("Greets the world".to_string()),
                dependencies: vec![],
                provides: vec!["hello = 1.0.0".to_string()],
            }],
        };
        std::fs::write(
            repodata_dir.join("index.toml"),
            index.to_pretty_string().unwrap(),
        )
        .unwrap();

        repo_root
    }

    #[test]
    fn refresh_indexes_packages_via_file_url() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = make_fixture_repo(tmp.path());
        let base_url = format!("file://{}", repo_root.display());

        let db = Db::open_in_memory().unwrap();
        let sync = RepoSync::new(&db, tmp.path().join("keyrings")).unwrap();
        let loaded = LoadedRepo {
            source_path: std::path::PathBuf::from("/etc/feathernix/repos/devrepo.toml"),
            repo: RepoBlock {
                name: "devrepo".to_string(),
                description: None,
                base_url,
                enabled: true,
                priority: 10,
                signing: None,
                refresh: None,
            },
        };
        let reports = sync.refresh_all(&[loaded]).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].packages_indexed, 1);

        let resolver = Resolver::new(&db, "x86_64-linux");
        let resolved = resolver.resolve("hello").unwrap();
        assert_eq!(resolved.name, "hello");
        assert_eq!(resolved.version, "1.0.0");
        assert_eq!(resolved.repo_name, "devrepo");
        assert!(resolved.payload_path.ends_with(".fnpkg.7z"));
    }

    #[test]
    fn resolver_can_scope_to_cached_repo_url() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = make_fixture_repo(tmp.path());
        let base_url = format!("file://{}", repo_root.display());

        let db = Db::open_in_memory().unwrap();
        let sync = RepoSync::new(&db, tmp.path().join("keyrings")).unwrap();
        let loaded = LoadedRepo {
            source_path: std::path::PathBuf::from("/etc/feathernix/repos/devrepo.toml"),
            repo: RepoBlock {
                name: "devrepo".to_string(),
                description: None,
                base_url: base_url.clone(),
                enabled: true,
                priority: 10,
                signing: None,
                refresh: None,
            },
        };
        sync.refresh_all(&[loaded]).unwrap();

        let resolver = Resolver::new(&db, "x86_64-linux");
        let resolved = resolver
            .resolve_from_repo_url("hello", &base_url)
            .expect("resolve from exact repo URL");
        assert_eq!(resolved.repo_name, "devrepo");

        let missing_pkg = resolver
            .resolve_from_repo_url("nope", &base_url)
            .unwrap_err();
        assert!(matches!(missing_pkg, RepoError::PackageNotFound { .. }));

        let missing_repo = resolver
            .resolve_from_repo_url("hello", "file:///not-cached")
            .unwrap_err();
        assert!(matches!(
            missing_repo,
            RepoError::RepositoryNotCached { .. }
        ));
    }

    #[test]
    fn resolver_returns_not_found_for_unknown() {
        let db = Db::open_in_memory().unwrap();
        let resolver = Resolver::new(&db, "x86_64-linux");
        let err = resolver.resolve("nope").unwrap_err();
        assert!(matches!(err, RepoError::PackageNotFound { .. }));
    }

    #[test]
    fn refresh_via_http_with_httpmock() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = make_fixture_repo(tmp.path());

        let server = httpmock::MockServer::start();
        let index_bytes = std::fs::read(repo_root.join("repodata/index.toml")).unwrap();
        let _m = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/repodata/index.toml");
            then.status(200).body(index_bytes);
        });

        let db = Db::open_in_memory().unwrap();
        let sync = RepoSync::new(&db, tmp.path().join("keyrings")).unwrap();
        let loaded = LoadedRepo {
            source_path: std::path::PathBuf::from("/x.toml"),
            repo: RepoBlock {
                name: "httprepo".to_string(),
                description: None,
                base_url: server.base_url(),
                enabled: true,
                priority: 5,
                signing: None,
                refresh: None,
            },
        };
        let reports = sync.refresh_all(&[loaded]).unwrap();
        assert_eq!(reports[0].packages_indexed, 1);
    }

    #[test]
    fn fetcher_verifies_sha256() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = b"hello world";
        let sha = fetcher::sha256_hex(bytes);
        let src = tmp.path().join("src.bin");
        std::fs::write(&src, bytes).unwrap();
        let base = format!("file://{}", tmp.path().display());

        let fetcher = Fetcher::new().unwrap();
        let dest = tmp.path().join("out.bin");
        let n = fetcher
            .fetch_to_file(&base, "src.bin", &dest, Some(&sha))
            .unwrap();
        assert_eq!(n, bytes.len() as u64);

        let dest2 = tmp.path().join("out2.bin");
        let err = fetcher
            .fetch_to_file(&base, "src.bin", &dest2, Some("0".repeat(64).as_str()))
            .unwrap_err();
        assert!(matches!(err, RepoError::HashMismatch { .. }));
    }
}
