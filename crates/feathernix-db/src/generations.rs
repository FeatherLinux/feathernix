use chrono::Utc;
use diesel::prelude::*;

use crate::models::{
    Generation, InstalledPackage, NewGeneration, NewGenerationEntry, StorePath,
};
use crate::schema::{
    generation_entries::dsl as ge, generations::dsl as g, installed_packages::dsl as ip,
    store_paths::dsl as sp,
};
use crate::{Db, DbError, DbResult};

#[derive(Debug, Clone)]
pub struct GenerationDetail {
    pub generation: Generation,
    pub packages: Vec<(InstalledPackage, StorePath)>,
}

pub fn list(db: &Db, profile: &str) -> DbResult<Vec<Generation>> {
    let mut conn = db.conn()?;
    Ok(g::generations
        .filter(g::profile.eq(profile))
        .order(g::generation.desc())
        .load::<Generation>(&mut conn)?)
}

pub fn current(db: &Db, profile: &str) -> DbResult<Option<Generation>> {
    let mut conn = db.conn()?;
    Ok(g::generations
        .filter(g::profile.eq(profile))
        .filter(g::is_current.eq(true))
        .first::<Generation>(&mut conn)
        .optional()?)
}

pub fn entries_for(db: &Db, generation_id: i32) -> DbResult<Vec<(InstalledPackage, StorePath)>> {
    let mut conn = db.conn()?;
    let rows: Vec<(InstalledPackage, StorePath)> = ge::generation_entries
        .inner_join(ip::installed_packages.on(ip::id.eq(ge::installed_package_id)))
        .inner_join(sp::store_paths.on(sp::id.eq(ge::store_path_id)))
        .filter(ge::generation_id.eq(generation_id))
        .select((InstalledPackage::as_select(), StorePath::as_select()))
        .load(&mut conn)?;
    Ok(rows)
}

pub fn detail(db: &Db, profile: &str, generation_num: i32) -> DbResult<Option<GenerationDetail>> {
    let mut conn = db.conn()?;
    let row: Option<Generation> = g::generations
        .filter(g::profile.eq(profile))
        .filter(g::generation.eq(generation_num))
        .first(&mut conn)
        .optional()?;
    drop(conn);
    let Some(row) = row else {
        return Ok(None);
    };
    let entries = entries_for(db, row.id)?;
    Ok(Some(GenerationDetail {
        generation: row,
        packages: entries,
    }))
}

pub fn create_next(
    db: &Db,
    profile: &str,
    description: Option<&str>,
    entries: &[(i32, i32)],
) -> DbResult<GenerationDetail> {
    let result = {
        let mut conn = db.conn()?;
        conn.transaction::<_, DbError, _>(|conn| {
            let max_existing: Option<i32> = g::generations
                .filter(g::profile.eq(profile))
                .select(diesel::dsl::max(g::generation))
                .first(conn)?;
            let next = max_existing.unwrap_or(0) + 1;

            diesel::update(g::generations.filter(g::profile.eq(profile)))
                .set(g::is_current.eq(false))
                .execute(conn)?;

            let now = Utc::now().naive_utc();
            diesel::insert_into(g::generations)
                .values(&NewGeneration {
                    profile,
                    generation: next,
                    description,
                    is_current: true,
                    created_at: now,
                })
                .execute(conn)?;
            let row: Generation = g::generations
                .filter(g::profile.eq(profile))
                .filter(g::generation.eq(next))
                .first(conn)?;

            for (installed_id, store_id) in entries {
                diesel::insert_into(ge::generation_entries)
                    .values(&NewGenerationEntry {
                        generation_id: row.id,
                        installed_package_id: *installed_id,
                        store_path_id: *store_id,
                        created_at: now,
                    })
                    .execute(conn)?;
            }
            Ok(row)
        })?
    };

    let entries = entries_for(db, result.id)?;
    Ok(GenerationDetail {
        generation: result,
        packages: entries,
    })
}

pub fn set_current(db: &Db, profile: &str, generation_num: i32) -> DbResult<GenerationDetail> {
    let row: Generation = {
        let mut conn = db.conn()?;
        conn.transaction::<_, DbError, _>(|conn| {
            let row: Generation = g::generations
                .filter(g::profile.eq(profile))
                .filter(g::generation.eq(generation_num))
                .first(conn)?;
            diesel::update(g::generations.filter(g::profile.eq(profile)))
                .set(g::is_current.eq(false))
                .execute(conn)?;
            diesel::update(g::generations.filter(g::id.eq(row.id)))
                .set(g::is_current.eq(true))
                .execute(conn)?;
            Ok(row)
        })?
    };
    let entries = entries_for(db, row.id)?;
    Ok(GenerationDetail {
        generation: row,
        packages: entries,
    })
}

pub fn previous(db: &Db, profile: &str) -> DbResult<Option<Generation>> {
    let Some(cur) = current(db, profile)? else {
        return Ok(None);
    };
    let mut conn = db.conn()?;
    Ok(g::generations
        .filter(g::profile.eq(profile))
        .filter(g::generation.lt(cur.generation))
        .order(g::generation.desc())
        .first(&mut conn)
        .optional()?)
}
