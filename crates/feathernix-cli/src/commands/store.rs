use anyhow::{Result, anyhow};
use colored::Colorize;
use diesel::prelude::*;

use feathernix_db::models::StorePath;
use feathernix_db::schema::store_paths::dsl as sp;

use crate::context::Context;

pub fn list(ctx: &Context) -> Result<()> {
    let mut conn = ctx.db.conn()?;
    let rows: Vec<StorePath> = sp::store_paths
        .order(sp::realized_at.desc())
        .load(&mut conn)?;
    if rows.is_empty() {
        println!("(no store paths)");
        return Ok(());
    }
    println!(
        "{:<42} {:<8} {:<12} {}",
        "HASH-NAME".bold(),
        "ORIGIN".bold(),
        "SIZE".bold(),
        "PATH".bold()
    );
    for r in rows {
        let leaf = std::path::Path::new(&r.full_path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let size = format_size(r.size_bytes);
        println!("{:<42} {:<8} {:<12} {}", leaf, r.origin, size, r.full_path);
    }
    Ok(())
}

pub fn path_info(ctx: &Context, hash_or_name: &str) -> Result<()> {
    let mut conn = ctx.db.conn()?;
    let row: Option<StorePath> = sp::store_paths
        .filter(sp::hash.like(format!("{hash_or_name}%")))
        .first(&mut conn)
        .optional()?;
    let row = row.ok_or_else(|| anyhow!("no store path matching {hash_or_name}"))?;
    println!("hash:       {}", row.hash);
    println!("name:       {}", row.name);
    println!("full_path:  {}", row.full_path);
    println!("sha256:     {}", row.sha256);
    println!("size_bytes: {}", row.size_bytes);
    println!("origin:     {}", row.origin);
    if let Some(r) = row.origin_ref {
        println!("origin_ref: {r}");
    }
    println!("realized_at: {}", row.realized_at);
    Ok(())
}

fn format_size(n: i64) -> String {
    let units = ["B", "K", "M", "G"];
    let mut value = n as f64;
    let mut idx = 0;
    while value >= 1024.0 && idx < units.len() - 1 {
        value /= 1024.0;
        idx += 1;
    }
    format!("{:.1}{}", value, units[idx])
}
