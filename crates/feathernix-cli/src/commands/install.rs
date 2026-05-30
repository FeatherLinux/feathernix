use anyhow::{Context as _, Result, anyhow};
use colored::Colorize;

use feathernix_builder::{Realizer, RepositoryRealizer};
use feathernix_repo::{RepoSync, Resolver};
use feathernix_store::ops as store_ops;

use crate::context::Context;

pub fn run(ctx: &Context, packages: &[String]) -> Result<()> {
    if packages.is_empty() {
        return Err(anyhow!("no packages specified"));
    }

    let sync = RepoSync::new(&ctx.db, ctx.paths.keyrings_dir.clone()).context("init repo sync")?;
    let reports = sync
        .refresh_all(&ctx.repos)
        .context("refresh repositories")?;
    for r in &reports {
        if ctx.verbose > 0 {
            println!(
                "{} {}: {} packages indexed",
                "→".cyan(),
                r.repository_name,
                r.packages_indexed
            );
        }
    }

    let arch = ctx.config.general.default_arch.clone();
    let resolver = Resolver::new(&ctx.db, arch);

    let mut resolved = Vec::with_capacity(packages.len());
    for name in packages {
        let pkg = resolver
            .resolve(name)
            .with_context(|| format!("resolve {name}"))?;
        resolved.push(pkg);
    }

    let realizer = RepositoryRealizer::new(
        &ctx.db,
        ctx.paths.store_dir.clone(),
        ctx.paths.payloads_cache_dir.clone(),
        ctx.paths.keyrings_dir.clone(),
    )?;

    let mut realized = Vec::with_capacity(resolved.len());
    for pkg in &resolved {
        let r = realizer
            .realize(pkg)
            .with_context(|| format!("install {}", pkg.name))?;
        let sig_note = if r.signature_verified {
            " (signature verified)".dimmed().to_string()
        } else if pkg.repo_gpg_check {
            " (signature missing, optional)".yellow().to_string()
        } else {
            String::new()
        };
        if r.freshly_installed {
            println!(
                "{} {}-{} → {}{}",
                "installed".green().bold(),
                pkg.name,
                pkg.version,
                r.store_path.full_path,
                sig_note
            );
        } else {
            println!(
                "{} {}-{} (already in store){}",
                "ready".yellow(),
                pkg.name,
                pkg.version,
                sig_note
            );
        }
        realized.push(r);
    }

    // Merge with existing current generation's contents so install adds rather than replaces.
    use feathernix_db::generations;
    let prior_entries = if let Some(cur) = generations::current(&ctx.db, &ctx.profile)? {
        generations::entries_for(&ctx.db, cur.id)?
    } else {
        Vec::new()
    };

    let new_names: std::collections::HashSet<String> = realized
        .iter()
        .map(|r| r.installed.name.clone())
        .collect();
    let mut to_record: Vec<_> = prior_entries
        .into_iter()
        .filter(|(p, _)| !new_names.contains(&p.name))
        .collect();
    for r in &realized {
        to_record.push((r.installed.clone(), r.store_path.clone()));
    }

    let detail = store_ops::create_generation(
        &ctx.db,
        &ctx.profile_dir(),
        &ctx.profile,
        Some(&format!(
            "install: {}",
            packages
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
        &to_record,
    )?;

    println!(
        "{} generation {} active for profile {}",
        "✓".green(),
        detail.generation.generation,
        ctx.profile
    );
    Ok(())
}
