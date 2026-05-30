use anyhow::Result;
use colored::Colorize;

use feathernix_db::generations;
use feathernix_store::ops as store_ops;

use crate::context::Context;

pub fn list(ctx: &Context) -> Result<()> {
    let gens = generations::list(&ctx.db, &ctx.profile)?;
    if gens.is_empty() {
        println!("(no generations for profile {})", ctx.profile);
        return Ok(());
    }
    println!(
        "{:<5} {:<20} {:<8} {}",
        "GEN".bold(),
        "CREATED".bold(),
        "STATE".bold(),
        "DESCRIPTION".bold()
    );
    for g in gens {
        let state = if g.is_current {
            "current".green().to_string()
        } else {
            "".to_string()
        };
        println!(
            "{:<5} {:<20} {:<8} {}",
            g.generation,
            g.created_at,
            state,
            g.description.as_deref().unwrap_or("")
        );
    }
    Ok(())
}

pub fn switch(ctx: &Context, generation: i32) -> Result<()> {
    let d = store_ops::switch_generation(&ctx.db, &ctx.profile_dir(), &ctx.profile, generation)?;
    println!(
        "{} switched to generation {} ({} packages)",
        "✓".green(),
        d.generation.generation,
        d.packages.len()
    );
    Ok(())
}

pub fn rollback(ctx: &Context) -> Result<()> {
    let d = store_ops::rollback(&ctx.db, &ctx.profile_dir(), &ctx.profile)?;
    println!(
        "{} rolled back to generation {}",
        "✓".green(),
        d.generation.generation
    );
    Ok(())
}
