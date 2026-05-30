use std::path::PathBuf;

use chrono::Utc;
use diesel::prelude::*;

use feathernix_config::{LoadedRepo, RepoBlock, SigningBlock};
use feathernix_db::Db;
use feathernix_db::models::{NewPackageAvailable, NewRepository, Repository};
use feathernix_db::schema::{packages_available::dsl as pa, repositories::dsl as r};

use crate::fetcher::{Fetcher, join_url};
use crate::gpg::{self, Keyring};
use crate::index::RepositoryIndex;
use crate::{INDEX_REL_PATH, INDEX_SIG_REL_PATH, RepoError};

#[derive(Debug, Clone)]
pub enum SignatureStatus {
    NotChecked,
    Verified { fingerprint: String },
    MissingButOptional,
}

#[derive(Debug, Clone)]
pub struct RefreshReport {
    pub repository_id: i32,
    pub repository_name: String,
    pub packages_indexed: usize,
    pub signature_status: SignatureStatus,
}

pub struct RepoSync<'a> {
    pub db: &'a Db,
    pub fetcher: Fetcher,
    pub keyrings_dir: PathBuf,
}

impl<'a> RepoSync<'a> {
    pub fn new(db: &'a Db, keyrings_dir: PathBuf) -> Result<Self, RepoError> {
        Ok(Self {
            db,
            fetcher: Fetcher::new()?,
            keyrings_dir,
        })
    }

    pub fn upsert_repo(&self, loaded: &LoadedRepo) -> Result<Repository, RepoError> {
        let mut conn = self.db.conn()?;
        let block = &loaded.repo;
        let signing = block.signing.clone().unwrap_or(SigningBlock {
            gpg_check: false,
            gpg_key_url: None,
            signature_required: false,
        });

        let existing: Option<Repository> = r::repositories
            .filter(r::name.eq(&block.name))
            .first(&mut conn)
            .optional()?;

        let now = Utc::now().naive_utc();
        if existing.is_some() {
            diesel::update(r::repositories.filter(r::name.eq(&block.name)))
                .set((
                    r::base_url.eq(&block.base_url),
                    r::enabled.eq(block.enabled),
                    r::priority.eq(block.priority),
                    r::gpg_check.eq(signing.gpg_check),
                    r::gpg_key_url.eq(signing.gpg_key_url.as_deref()),
                    r::signature_required.eq(signing.signature_required),
                ))
                .execute(&mut conn)?;
        } else {
            let new = NewRepository {
                name: &block.name,
                base_url: &block.base_url,
                enabled: block.enabled,
                priority: block.priority,
                gpg_check: signing.gpg_check,
                gpg_key_url: signing.gpg_key_url.as_deref(),
                signature_required: signing.signature_required,
                last_refresh_at: None,
                etag: None,
                created_at: now,
            };
            diesel::insert_into(r::repositories)
                .values(&new)
                .execute(&mut conn)?;
        }

        Ok(r::repositories
            .filter(r::name.eq(&block.name))
            .first(&mut conn)?)
    }

    pub fn refresh(&self, repo: &Repository) -> Result<RefreshReport, RepoError> {
        let bytes = self.fetcher.fetch_bytes(&repo.base_url, INDEX_REL_PATH)?;

        let signature_status = if repo.gpg_check {
            self.verify_index_signature(repo, &bytes)?
        } else {
            SignatureStatus::NotChecked
        };

        let text =
            std::str::from_utf8(&bytes).map_err(|e| RepoError::ParseIndex(e.to_string()))?;
        let index =
            RepositoryIndex::from_str(text).map_err(|e| RepoError::ParseIndex(e.to_string()))?;

        let mut conn = self.db.conn()?;
        let now = Utc::now().naive_utc();

        conn.transaction::<_, RepoError, _>(|conn| {
            diesel::delete(pa::packages_available.filter(pa::repo_id.eq(repo.id))).execute(conn)?;
            for pkg in &index.packages {
                let deps_json = serde_json::to_string(&pkg.dependencies)
                    .map_err(|e| RepoError::Serialize(e.to_string()))?;
                let provides_json = serde_json::to_string(&pkg.provides)
                    .map_err(|e| RepoError::Serialize(e.to_string()))?;
                let row = NewPackageAvailable {
                    repo_id: repo.id,
                    name: &pkg.name,
                    version: &pkg.version,
                    release: &pkg.release,
                    epoch: pkg.epoch,
                    arch: &pkg.arch,
                    payload_path: &pkg.payload_path,
                    payload_sha256: &pkg.payload_sha256,
                    payload_size: pkg.payload_size,
                    summary: pkg.summary.as_deref(),
                    dependencies_json: &deps_json,
                    provides_json: &provides_json,
                    created_at: now,
                };
                diesel::insert_into(pa::packages_available)
                    .values(&row)
                    .execute(conn)?;
            }
            diesel::update(r::repositories.filter(r::id.eq(repo.id)))
                .set(r::last_refresh_at.eq(Some(now)))
                .execute(conn)?;
            Ok(())
        })?;

        Ok(RefreshReport {
            repository_id: repo.id,
            repository_name: repo.name.clone(),
            packages_indexed: index.packages.len(),
            signature_status,
        })
    }

    fn verify_index_signature(
        &self,
        repo: &Repository,
        index_bytes: &[u8],
    ) -> Result<SignatureStatus, RepoError> {
        let keyring: Keyring = gpg::load_or_fetch(
            &self.fetcher,
            &repo.name,
            repo.gpg_key_url.as_deref(),
            &self.keyrings_dir,
        )?;
        let sig_url = join_url(&repo.base_url, INDEX_SIG_REL_PATH);
        let sig = self
            .fetcher
            .try_fetch_bytes(&repo.base_url, INDEX_SIG_REL_PATH)?;
        match sig {
            Some(sig_bytes) => {
                keyring.verify_detached(&sig_url, index_bytes, &sig_bytes)?;
                Ok(SignatureStatus::Verified {
                    fingerprint: keyring.fingerprint,
                })
            }
            None => {
                if repo.signature_required {
                    Err(RepoError::SignatureMissing { url: sig_url })
                } else {
                    tracing::warn!(
                        "repo {}: signature {} missing; signature_required=false, continuing",
                        repo.name,
                        sig_url
                    );
                    Ok(SignatureStatus::MissingButOptional)
                }
            }
        }
    }

    pub fn refresh_all(&self, loaded: &[LoadedRepo]) -> Result<Vec<RefreshReport>, RepoError> {
        let mut reports = Vec::with_capacity(loaded.len());
        for r in loaded {
            if !r.repo.enabled {
                continue;
            }
            let repo = self.upsert_repo(r)?;
            reports.push(self.refresh(&repo)?);
        }
        Ok(reports)
    }
}

// Allow `?` from `RepoError` inside a `diesel::Connection::transaction` closure.
impl From<diesel::result::Error> for RepoError {
    fn from(e: diesel::result::Error) -> Self {
        RepoError::Db(feathernix_db::DbError::Query(e))
    }
}

#[allow(dead_code)]
fn _silence_unused(_: &RepoBlock) {}
