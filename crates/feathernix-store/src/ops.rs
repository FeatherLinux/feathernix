use std::path::{Path, PathBuf};

use feathernix_db::Db;
use feathernix_db::generations::{self, GenerationDetail};
use feathernix_db::models::{InstalledPackage, StorePath};

use crate::profile::{build_generation, point_current};
use crate::{StoreError, StoreResult};

pub fn create_generation(
    db: &Db,
    profile_dir: &Path,
    profile_name: &str,
    description: Option<&str>,
    pkgs: &[(InstalledPackage, StorePath)],
) -> StoreResult<GenerationDetail> {
    let entries: Vec<(i32, i32)> = pkgs.iter().map(|(i, s)| (i.id, s.id)).collect();
    let detail = generations::create_next(db, profile_name, description, &entries)?;
    let paths: Vec<PathBuf> = pkgs
        .iter()
        .map(|(_, s)| PathBuf::from(&s.full_path))
        .collect();
    build_generation(profile_dir, detail.generation.generation, &paths)?;
    point_current(profile_dir, detail.generation.generation)?;
    Ok(detail)
}

pub fn switch_generation(
    db: &Db,
    profile_dir: &Path,
    profile_name: &str,
    generation_num: i32,
) -> StoreResult<GenerationDetail> {
    let detail = generations::set_current(db, profile_name, generation_num)?;
    let paths: Vec<PathBuf> = detail
        .packages
        .iter()
        .map(|(_, s)| PathBuf::from(&s.full_path))
        .collect();
    build_generation(profile_dir, detail.generation.generation, &paths)?;
    point_current(profile_dir, detail.generation.generation)?;
    Ok(detail)
}

pub fn rollback(db: &Db, profile_dir: &Path, profile_name: &str) -> StoreResult<GenerationDetail> {
    let prev =
        generations::previous(db, profile_name)?.ok_or(StoreError::NoPreviousGeneration)?;
    switch_generation(db, profile_dir, profile_name, prev.generation)
}
