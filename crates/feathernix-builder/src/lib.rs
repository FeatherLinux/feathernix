use std::path::PathBuf;

use chrono::Utc;
use diesel::prelude::*;
use thiserror::Error;

use feathernix_archive::{ArchiveError, extract};
use feathernix_db::Db;
use feathernix_db::models::{InstalledPackage, NewInstalledPackage, StorePath};
use feathernix_db::schema::installed_packages::dsl as ip;
use feathernix_repo::{Fetcher, RepoError, ResolvedPackage, gpg, join_url};
use feathernix_store::{ORIGIN_REPOSITORY, StoreError, install_via_temp, register_store_path};

#[derive(Debug, Error)]
pub enum BuilderError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Archive(#[from] ArchiveError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Db(#[from] feathernix_db::DbError),
    #[error(transparent)]
    Diesel(#[from] diesel::result::Error),
}

pub type BuilderResult<T> = Result<T, BuilderError>;

#[derive(Debug, Clone)]
pub struct RealizedPackage {
    pub installed: InstalledPackage,
    pub store_path: StorePath,
    pub freshly_installed: bool,
    pub signature_verified: bool,
}

pub trait Realizer {
    fn realize(&self, pkg: &ResolvedPackage) -> BuilderResult<RealizedPackage>;
}

pub struct RepositoryRealizer<'a> {
    db: &'a Db,
    store_root: PathBuf,
    payloads_cache: PathBuf,
    keyrings_dir: PathBuf,
    fetcher: Fetcher,
}

impl<'a> RepositoryRealizer<'a> {
    pub fn new(
        db: &'a Db,
        store_root: PathBuf,
        payloads_cache: PathBuf,
        keyrings_dir: PathBuf,
    ) -> BuilderResult<Self> {
        let fetcher = Fetcher::new()?;
        std::fs::create_dir_all(&store_root)?;
        std::fs::create_dir_all(&payloads_cache)?;
        Ok(Self {
            db,
            store_root,
            payloads_cache,
            keyrings_dir,
            fetcher,
        })
    }

    fn verify_payload_signature(
        &self,
        pkg: &ResolvedPackage,
        payload_bytes: &[u8],
    ) -> BuilderResult<bool> {
        let keyring = gpg::load_or_fetch(
            &self.fetcher,
            &pkg.repo_name,
            pkg.repo_gpg_key_url.as_deref(),
            &self.keyrings_dir,
        )?;
        let sig_rel = format!("{}.sig", pkg.payload_path);
        let sig_url = join_url(&pkg.repo_base_url, &sig_rel);
        let sig = self.fetcher.try_fetch_bytes(&pkg.repo_base_url, &sig_rel)?;
        match sig {
            Some(sig_bytes) => {
                keyring.verify_detached(&sig_url, payload_bytes, &sig_bytes)?;
                Ok(true)
            }
            None => {
                if pkg.repo_signature_required {
                    Err(RepoError::SignatureMissing { url: sig_url }.into())
                } else {
                    tracing::warn!(
                        "payload signature {} missing; signature_required=false, continuing",
                        sig_url
                    );
                    Ok(false)
                }
            }
        }
    }
}

impl<'a> Realizer for RepositoryRealizer<'a> {
    fn realize(&self, pkg: &ResolvedPackage) -> BuilderResult<RealizedPackage> {
        let payload_path = self
            .payloads_cache
            .join(format!("{}.fnpkg.7z", pkg.payload_sha256));

        if !payload_path.exists() {
            self.fetcher.fetch_to_file(
                &pkg.repo_base_url,
                &pkg.payload_path,
                &payload_path,
                Some(&pkg.payload_sha256),
            )?;
        }

        let signature_verified = if pkg.repo_gpg_check {
            let payload_bytes = std::fs::read(&payload_path)?;
            self.verify_payload_signature(pkg, &payload_bytes)?
        } else {
            false
        };

        let installed_store = install_via_temp(
            &self.store_root,
            &pkg.name,
            &pkg.version,
            &pkg.payload_sha256,
            |dest| {
                extract(&payload_path, dest).map_err(|e| match e {
                    ArchiveError::Io(io) => StoreError::Io(io),
                    other => StoreError::Io(std::io::Error::other(other.to_string())),
                })
            },
        )?;

        let store_row = register_store_path(
            self.db,
            &installed_store,
            &pkg.name,
            &pkg.payload_sha256,
            ORIGIN_REPOSITORY,
            Some(&pkg.repo_name),
        )?;

        let now = Utc::now().naive_utc();
        let mut conn = self.db.conn()?;
        let existing: Option<InstalledPackage> = ip::installed_packages
            .filter(ip::name.eq(&pkg.name))
            .filter(ip::arch.eq(&pkg.arch))
            .first(&mut conn)
            .optional()?;

        if let Some(existing) = existing {
            diesel::update(ip::installed_packages.filter(ip::id.eq(existing.id)))
                .set((
                    ip::version.eq(&pkg.version),
                    ip::release.eq(&pkg.release),
                    ip::epoch.eq(pkg.epoch),
                    ip::repo_id.eq(Some(pkg.repo_id)),
                    ip::store_path_id.eq(store_row.id),
                    ip::installed_at.eq(now),
                    ip::summary.eq(pkg.summary.as_deref()),
                ))
                .execute(&mut conn)?;
        } else {
            let row = NewInstalledPackage {
                name: &pkg.name,
                version: &pkg.version,
                release: &pkg.release,
                epoch: pkg.epoch,
                arch: &pkg.arch,
                repo_id: Some(pkg.repo_id),
                store_path_id: store_row.id,
                installed_at: now,
                summary: pkg.summary.as_deref(),
                created_at: now,
            };
            diesel::insert_into(ip::installed_packages)
                .values(&row)
                .execute(&mut conn)?;
        }

        let installed: InstalledPackage = ip::installed_packages
            .filter(ip::name.eq(&pkg.name))
            .filter(ip::arch.eq(&pkg.arch))
            .first(&mut conn)?;

        Ok(RealizedPackage {
            installed,
            store_path: store_row,
            freshly_installed: installed_store.freshly_installed,
            signature_verified,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use feathernix_archive::{Manifest, pack};
    use feathernix_config::{LoadedRepo, RepoBlock};
    use feathernix_repo::{IndexMeta, PackageEntry, RepoSync, RepositoryIndex, Resolver, sha256_hex};

    fn build_fixture_repo(workdir: &std::path::Path) -> std::path::PathBuf {
        let repo_root = workdir.join("repo");
        let packages_dir = repo_root.join("packages");
        let repodata_dir = repo_root.join("repodata");
        std::fs::create_dir_all(&packages_dir).unwrap();
        std::fs::create_dir_all(&repodata_dir).unwrap();

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
        std::fs::write(src.join("manifest.toml"), manifest.to_pretty_string().unwrap()).unwrap();
        std::fs::write(src.join("payload/bin/hello"), b"echo hello\n").unwrap();
        let pkg_path = packages_dir.join("hello-1.0.0-1-x86_64-linux.fnpkg.7z");
        pack(&src, &pkg_path).unwrap();

        let bytes = std::fs::read(&pkg_path).unwrap();
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
                payload_sha256: sha256_hex(&bytes),
                payload_size: bytes.len() as i64,
                summary: Some("hi".to_string()),
                dependencies: vec![],
                provides: vec![],
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
    fn realize_writes_store_path_and_installed_row() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = build_fixture_repo(tmp.path());
        let db = Db::open_in_memory().unwrap();

        let sync = RepoSync::new(&db, tmp.path().join("keyrings")).unwrap();
        let loaded = LoadedRepo {
            source_path: std::path::PathBuf::from("/x.toml"),
            repo: RepoBlock {
                name: "devrepo".to_string(),
                description: None,
                base_url: format!("file://{}", repo_root.display()),
                enabled: true,
                priority: 10,
                signing: None,
                refresh: None,
            },
        };
        sync.refresh_all(&[loaded]).unwrap();

        let resolver = Resolver::new(&db, "x86_64-linux");
        let pkg = resolver.resolve("hello").unwrap();

        let store_root = tmp.path().join("store");
        let cache = tmp.path().join("cache");
        let keyrings = tmp.path().join("keyrings");
        let realizer =
            RepositoryRealizer::new(&db, store_root.clone(), cache.clone(), keyrings).unwrap();
        let realized = realizer.realize(&pkg).unwrap();
        assert!(realized.freshly_installed);
        assert!(!realized.signature_verified, "no gpg_check in fixture");
        assert!(std::path::Path::new(&realized.store_path.full_path).exists());
        assert!(std::path::Path::new(&realized.store_path.full_path)
            .join("payload/bin/hello")
            .exists());
        assert_eq!(realized.installed.name, "hello");
        assert_eq!(realized.installed.version, "1.0.0");

        // Re-realizing is idempotent
        let realized2 = realizer.realize(&pkg).unwrap();
        assert!(!realized2.freshly_installed);
        assert_eq!(realized.installed.id, realized2.installed.id);
    }
}
