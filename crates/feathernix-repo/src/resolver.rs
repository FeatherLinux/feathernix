use diesel::prelude::*;

use feathernix_db::Db;
use feathernix_db::schema::{packages_available::dsl as pa, repositories::dsl as r};

use crate::RepoError;

#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: String,
    pub release: String,
    pub epoch: i32,
    pub arch: String,
    pub repo_id: i32,
    pub repo_name: String,
    pub repo_base_url: String,
    pub repo_priority: i32,
    pub payload_path: String,
    pub payload_sha256: String,
    pub payload_size: i64,
    pub summary: Option<String>,
    pub dependencies: Vec<String>,
    pub repo_gpg_check: bool,
    pub repo_signature_required: bool,
    pub repo_gpg_key_url: Option<String>,
}

pub struct Resolver<'a> {
    pub db: &'a Db,
    pub arch: String,
}

impl<'a> Resolver<'a> {
    pub fn new(db: &'a Db, arch: impl Into<String>) -> Self {
        Self {
            db,
            arch: arch.into(),
        }
    }

    pub fn resolve(&self, name: &str) -> Result<ResolvedPackage, RepoError> {
        self.resolve_inner(name, None)
    }

    pub fn resolve_from_repo_url(
        &self,
        name: &str,
        repo_url: &str,
    ) -> Result<ResolvedPackage, RepoError> {
        self.resolve_inner(name, Some(repo_url))
    }

    fn resolve_inner(
        &self,
        name: &str,
        repo_url: Option<&str>,
    ) -> Result<ResolvedPackage, RepoError> {
        let mut conn = self.db.conn()?;
        type Row = (
            i32,            // packages_available.id
            i32,            // repo_id
            String,         // name
            String,         // version
            String,         // release
            i32,            // epoch
            String,         // arch
            String,         // payload_path
            String,         // payload_sha256
            i64,            // payload_size
            Option<String>, // summary
            String,         // deps_json
            String,         // repo_name
            String,         // repo_base_url
            i32,            // repo_priority
            bool,           // repo_gpg_check
            bool,           // repo_signature_required
            Option<String>, // repo_gpg_key_url
        );

        if let Some(url) = repo_url {
            let cached = r::repositories
                .filter(r::base_url.eq(url))
                .filter(r::enabled.eq(true))
                .count()
                .get_result::<i64>(&mut conn)?;
            if cached == 0 {
                return Err(RepoError::RepositoryNotCached {
                    base_url: url.to_string(),
                });
            }
        }

        let mut query = pa::packages_available
            .inner_join(r::repositories.on(r::id.eq(pa::repo_id)))
            .filter(pa::name.eq(name))
            .filter(pa::arch.eq(&self.arch))
            .filter(r::enabled.eq(true))
            .into_boxed();

        if let Some(url) = repo_url {
            query = query.filter(r::base_url.eq(url));
        }

        let candidates: Vec<Row> = query
            .select((
                pa::id,
                pa::repo_id,
                pa::name,
                pa::version,
                pa::release,
                pa::epoch,
                pa::arch,
                pa::payload_path,
                pa::payload_sha256,
                pa::payload_size,
                pa::summary,
                pa::dependencies_json,
                r::name,
                r::base_url,
                r::priority,
                r::gpg_check,
                r::signature_required,
                r::gpg_key_url,
            ))
            .load::<Row>(&mut conn)?;

        let best = candidates
            .into_iter()
            .min_by(|a, b| {
                a.14.cmp(&b.14) // priority asc
                    .then(version_cmp(&b.3, &a.3)) // version desc
                    .then(version_cmp(&b.4, &a.4)) // release desc
            })
            .ok_or_else(|| RepoError::PackageNotFound {
                name: name.to_string(),
                arch: self.arch.clone(),
            })?;

        let deps: Vec<String> =
            serde_json::from_str(&best.11).map_err(|e| RepoError::Serialize(e.to_string()))?;

        Ok(ResolvedPackage {
            name: best.2,
            version: best.3,
            release: best.4,
            epoch: best.5,
            arch: best.6,
            repo_id: best.1,
            repo_name: best.12,
            repo_base_url: best.13,
            repo_priority: best.14,
            payload_path: best.7,
            payload_sha256: best.8,
            payload_size: best.9,
            summary: best.10,
            dependencies: deps,
            repo_gpg_check: best.15,
            repo_signature_required: best.16,
            repo_gpg_key_url: best.17,
        })
    }

    pub fn resolve_many(&self, names: &[&str]) -> Result<Vec<ResolvedPackage>, RepoError> {
        names.iter().map(|n| self.resolve(n)).collect()
    }
}

fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let av = a.split('.').map(parse_or_zero).collect::<Vec<_>>();
    let bv = b.split('.').map(parse_or_zero).collect::<Vec<_>>();
    av.cmp(&bv)
}

fn parse_or_zero(s: &str) -> u64 {
    s.chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}
