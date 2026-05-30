pub mod install;
pub mod ops;
pub mod path;
pub mod profile;

use chrono::Utc;
use diesel::prelude::*;
use thiserror::Error;

use feathernix_db::models::{NewStorePath, StorePath};
use feathernix_db::schema::store_paths::dsl as sp;
use feathernix_db::{Db, DbError};

pub use install::{InstalledStorePath, install_via_temp};
pub use path::{HASH_LEN, compute_store_path, store_path};

pub const ORIGIN_REPOSITORY: &str = "repository";
pub const ORIGIN_BUILT: &str = "built";
pub const ORIGIN_IMPORTED: &str = "imported";

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("walkdir error: {0}")]
    WalkDir(String),
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("diesel error: {0}")]
    Diesel(#[from] diesel::result::Error),
    #[error("no previous generation to roll back to")]
    NoPreviousGeneration,
}

pub type StoreResult<T> = Result<T, StoreError>;

pub fn register_store_path(
    db: &Db,
    installed: &InstalledStorePath,
    name: &str,
    sha256: &str,
    origin: &str,
    origin_ref: Option<&str>,
) -> StoreResult<StorePath> {
    let mut conn = db.conn()?;
    if let Ok(existing) = sp::store_paths
        .filter(sp::hash.eq(&installed.hash))
        .first::<StorePath>(&mut conn)
    {
        return Ok(existing);
    }
    let now = Utc::now().naive_utc();
    let new_row = NewStorePath {
        hash: &installed.hash,
        name,
        full_path: &installed.full_path.to_string_lossy(),
        sha256,
        blake3: None,
        size_bytes: installed.size_bytes as i64,
        origin,
        origin_ref,
        realized_at: now,
        created_at: now,
    };
    diesel::insert_into(sp::store_paths)
        .values(&new_row)
        .execute(&mut conn)?;
    let row: StorePath = sp::store_paths
        .filter(sp::hash.eq(&installed.hash))
        .first(&mut conn)?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_payload(target: &std::path::Path) -> Result<(), StoreError> {
        std::fs::create_dir_all(target.join("payload/bin"))?;
        std::fs::write(target.join("payload/bin/hello"), b"hello\n")?;
        std::fs::write(target.join("manifest.toml"), b"")?;
        Ok(())
    }

    #[test]
    fn install_via_temp_creates_store_path() {
        let tmp = tempfile::tempdir().unwrap();
        let store_root = tmp.path().join("store");
        let installed = install_via_temp(&store_root, "hello", "1.0.0", "deadbeef", make_payload)
            .expect("install");
        assert!(installed.freshly_installed);
        assert!(installed.full_path.starts_with(&store_root));
        assert!(installed.full_path.join("payload/bin/hello").exists());

        let installed2 =
            install_via_temp(&store_root, "hello", "1.0.0", "deadbeef", make_payload).expect("re-install");
        assert!(!installed2.freshly_installed);
        assert_eq!(installed.full_path, installed2.full_path);
    }

    #[test]
    fn register_inserts_row() {
        let tmp = tempfile::tempdir().unwrap();
        let store_root = tmp.path().join("store");
        let installed = install_via_temp(&store_root, "hello", "1.0.0", "abc", make_payload).unwrap();
        let db = Db::open_in_memory().unwrap();
        let row = register_store_path(&db, &installed, "hello", "abc", ORIGIN_REPOSITORY, None).unwrap();
        assert_eq!(row.name, "hello");
        assert_eq!(row.hash, installed.hash);
        let row2 = register_store_path(&db, &installed, "hello", "abc", ORIGIN_REPOSITORY, None).unwrap();
        assert_eq!(row.id, row2.id);
    }

    #[test]
    fn profile_generation_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let store_root = tmp.path().join("store");
        let i1 = install_via_temp(&store_root, "a", "1", "aa", |d| {
            std::fs::create_dir_all(d.join("payload/bin"))?;
            std::fs::write(d.join("payload/bin/a"), b"a\n")?;
            Ok(())
        })
        .unwrap();
        let i2 = install_via_temp(&store_root, "b", "1", "bb", |d| {
            std::fs::create_dir_all(d.join("payload/bin"))?;
            std::fs::write(d.join("payload/bin/b"), b"b\n")?;
            Ok(())
        })
        .unwrap();
        let profile_dir = tmp.path().join("profile");
        let paths: Vec<PathBuf> = vec![i1.full_path.clone(), i2.full_path.clone()];
        let gen_dir = profile::build_generation(&profile_dir, 1, &paths).unwrap();
        assert!(gen_dir.join("bin/a").symlink_metadata().unwrap().file_type().is_symlink());
        assert!(gen_dir.join("bin/b").symlink_metadata().unwrap().file_type().is_symlink());

        profile::point_current(&profile_dir, 1).unwrap();
        let cur = profile_dir.join("current");
        assert!(cur.symlink_metadata().unwrap().file_type().is_symlink());
        assert!(cur.read_link().unwrap().ends_with("generations/1"));
    }

    fn install_and_record(
        db: &Db,
        store_root: &std::path::Path,
        name: &str,
        version: &str,
        payload_sha: &str,
        bin_name: &str,
    ) -> (
        feathernix_db::models::InstalledPackage,
        feathernix_db::models::StorePath,
    ) {
        use chrono::Utc;
        use feathernix_db::models::{InstalledPackage, NewInstalledPackage};
        use feathernix_db::schema::installed_packages::dsl as ip;
        let installed_store = install_via_temp(store_root, name, version, payload_sha, |d| {
            std::fs::create_dir_all(d.join("payload/bin"))?;
            std::fs::write(d.join(format!("payload/bin/{bin_name}")), b"x\n")?;
            Ok(())
        })
        .unwrap();
        let sp_row =
            register_store_path(db, &installed_store, name, payload_sha, ORIGIN_REPOSITORY, None)
                .unwrap();
        let now = Utc::now().naive_utc();
        let mut conn = db.conn().unwrap();
        diesel::insert_into(ip::installed_packages)
            .values(&NewInstalledPackage {
                name,
                version,
                release: "1",
                epoch: 0,
                arch: "x86_64-linux",
                repo_id: None,
                store_path_id: sp_row.id,
                installed_at: now,
                summary: None,
                created_at: now,
            })
            .execute(&mut conn)
            .unwrap();
        let ip_row: InstalledPackage = ip::installed_packages
            .filter(ip::name.eq(name))
            .filter(ip::arch.eq("x86_64-linux"))
            .first(&mut conn)
            .unwrap();
        (ip_row, sp_row)
    }

    #[test]
    fn create_switch_and_rollback_generation() {
        use feathernix_db::generations as gens;

        let tmp = tempfile::tempdir().unwrap();
        let store_root = tmp.path().join("store");
        let profile_dir = tmp.path().join("profiles/system");
        let db = Db::open_in_memory().unwrap();

        let a = install_and_record(&db, &store_root, "a", "1.0.0", "aa", "a");
        let g1 = ops::create_generation(&db, &profile_dir, "system", None, &[a.clone()]).unwrap();
        assert_eq!(g1.generation.generation, 1);
        assert!(g1.generation.is_current);
        assert!(profile_dir.join("current").read_link().unwrap().ends_with("1"));
        assert!(profile_dir.join("generations/1/bin/a").symlink_metadata().unwrap().file_type().is_symlink());

        let b = install_and_record(&db, &store_root, "b", "1.0.0", "bb", "b");
        let g2 =
            ops::create_generation(&db, &profile_dir, "system", None, &[a.clone(), b.clone()])
                .unwrap();
        assert_eq!(g2.generation.generation, 2);
        assert!(profile_dir.join("current").read_link().unwrap().ends_with("2"));
        assert!(profile_dir.join("generations/2/bin/a").exists());
        assert!(profile_dir.join("generations/2/bin/b").exists());

        let cur = gens::current(&db, "system").unwrap().unwrap();
        assert_eq!(cur.generation, 2);

        let rolled = ops::rollback(&db, &profile_dir, "system").unwrap();
        assert_eq!(rolled.generation.generation, 1);
        assert!(profile_dir.join("current").read_link().unwrap().ends_with("1"));
        let cur = gens::current(&db, "system").unwrap().unwrap();
        assert_eq!(cur.generation, 1);
    }
}
