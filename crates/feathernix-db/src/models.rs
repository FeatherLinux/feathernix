use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::schema::{
    flake_inputs, flakes, generation_entries, generations, installed_packages, packages_available,
    repositories, store_paths,
};

#[derive(Debug, Clone, Queryable, Identifiable, Selectable, Serialize)]
#[diesel(table_name = repositories)]
pub struct Repository {
    pub id: i32,
    pub name: String,
    pub base_url: String,
    pub enabled: bool,
    pub priority: i32,
    pub gpg_check: bool,
    pub gpg_key_url: Option<String>,
    pub signature_required: bool,
    pub last_refresh_at: Option<NaiveDateTime>,
    pub etag: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Insertable, AsChangeset, Deserialize)]
#[diesel(table_name = repositories)]
pub struct NewRepository<'a> {
    pub name: &'a str,
    pub base_url: &'a str,
    pub enabled: bool,
    pub priority: i32,
    pub gpg_check: bool,
    pub gpg_key_url: Option<&'a str>,
    pub signature_required: bool,
    pub last_refresh_at: Option<NaiveDateTime>,
    pub etag: Option<&'a str>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Queryable, Identifiable, Selectable, Serialize)]
#[diesel(table_name = store_paths)]
pub struct StorePath {
    pub id: i32,
    pub hash: String,
    pub name: String,
    pub full_path: String,
    pub sha256: String,
    pub blake3: Option<String>,
    pub size_bytes: i64,
    pub origin: String,
    pub origin_ref: Option<String>,
    pub realized_at: NaiveDateTime,
    pub gc_root_count: i32,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = store_paths)]
pub struct NewStorePath<'a> {
    pub hash: &'a str,
    pub name: &'a str,
    pub full_path: &'a str,
    pub sha256: &'a str,
    pub blake3: Option<&'a str>,
    pub size_bytes: i64,
    pub origin: &'a str,
    pub origin_ref: Option<&'a str>,
    pub realized_at: NaiveDateTime,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Queryable, Identifiable, Selectable, Serialize)]
#[diesel(table_name = packages_available)]
pub struct PackageAvailable {
    pub id: i32,
    pub repo_id: i32,
    pub name: String,
    pub version: String,
    pub release: String,
    pub epoch: i32,
    pub arch: String,
    pub payload_path: String,
    pub payload_sha256: String,
    pub payload_size: i64,
    pub summary: Option<String>,
    pub dependencies_json: String,
    pub provides_json: String,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Insertable, AsChangeset)]
#[diesel(table_name = packages_available)]
pub struct NewPackageAvailable<'a> {
    pub repo_id: i32,
    pub name: &'a str,
    pub version: &'a str,
    pub release: &'a str,
    pub epoch: i32,
    pub arch: &'a str,
    pub payload_path: &'a str,
    pub payload_sha256: &'a str,
    pub payload_size: i64,
    pub summary: Option<&'a str>,
    pub dependencies_json: &'a str,
    pub provides_json: &'a str,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Queryable, Identifiable, Selectable, Serialize)]
#[diesel(table_name = installed_packages)]
pub struct InstalledPackage {
    pub id: i32,
    pub name: String,
    pub version: String,
    pub release: String,
    pub epoch: i32,
    pub arch: String,
    pub repo_id: Option<i32>,
    pub store_path_id: i32,
    pub installed_at: NaiveDateTime,
    pub summary: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Insertable, AsChangeset)]
#[diesel(table_name = installed_packages)]
pub struct NewInstalledPackage<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub release: &'a str,
    pub epoch: i32,
    pub arch: &'a str,
    pub repo_id: Option<i32>,
    pub store_path_id: i32,
    pub installed_at: NaiveDateTime,
    pub summary: Option<&'a str>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Queryable, Identifiable, Selectable, Serialize)]
#[diesel(table_name = generations)]
pub struct Generation {
    pub id: i32,
    pub profile: String,
    pub generation: i32,
    pub description: Option<String>,
    pub is_current: bool,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = generations)]
pub struct NewGeneration<'a> {
    pub profile: &'a str,
    pub generation: i32,
    pub description: Option<&'a str>,
    pub is_current: bool,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Queryable, Identifiable, Selectable, Serialize)]
#[diesel(table_name = generation_entries)]
pub struct GenerationEntry {
    pub id: i32,
    pub generation_id: i32,
    pub installed_package_id: i32,
    pub store_path_id: i32,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = generation_entries)]
pub struct NewGenerationEntry {
    pub generation_id: i32,
    pub installed_package_id: i32,
    pub store_path_id: i32,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Queryable, Identifiable, Selectable, Serialize)]
#[diesel(table_name = flakes)]
pub struct Flake {
    pub id: i32,
    pub root_path: String,
    pub description: Option<String>,
    pub last_locked_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Insertable, AsChangeset)]
#[diesel(table_name = flakes)]
pub struct NewFlake<'a> {
    pub root_path: &'a str,
    pub description: Option<&'a str>,
    pub last_locked_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Queryable, Identifiable, Selectable, Serialize)]
#[diesel(table_name = flake_inputs)]
pub struct FlakeInput {
    pub id: i32,
    pub flake_id: i32,
    pub name: String,
    pub kind: String,
    pub url: String,
    pub locked_revision: Option<String>,
    pub locked_sha256: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Insertable, AsChangeset)]
#[diesel(table_name = flake_inputs)]
pub struct NewFlakeInput<'a> {
    pub flake_id: i32,
    pub name: &'a str,
    pub kind: &'a str,
    pub url: &'a str,
    pub locked_revision: Option<&'a str>,
    pub locked_sha256: Option<&'a str>,
    pub created_at: NaiveDateTime,
}
