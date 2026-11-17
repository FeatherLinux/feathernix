use std::path::PathBuf;

use anyhow::{Context as _, Result, anyhow, bail};
use colored::Colorize;

use crate::context::Context;
use feathernix_flake as fl;
use feathernix_repo::Resolver;

pub fn init(root: PathBuf, description: Option<String>) -> Result<()> {
    let p = fl::init(&root, description.as_deref()).context("init flake")?;
    println!("{} wrote {}", "+".green(), p.display());
    Ok(())
}

pub fn show(ctx: &Context, root: PathBuf) -> Result<()> {
    let f = fl::load(&root).context("load flake.toml")?;
    let resolver = Resolver::new(&ctx.db, ctx.config.general.default_arch.clone());

    if let Some(d) = &f.description {
        println!("{}: {}", "description".bold(), d);
    }
    println!("{}:", "inputs".bold());
    for (name, input) in &f.inputs {
        println!(
            "  {} ({}) {}",
            name.cyan(),
            format!("{:?}", input.kind).to_lowercase(),
            input.url
        );
    }
    if !f.outputs.packages.is_empty() {
        println!("{}:", "outputs.packages".bold());
        for (k, v) in &f.outputs.packages {
            println!("  {}: {} package(s)", k.cyan(), v.packages.len());
            for p in &v.packages {
                println!("    - {p}");
                let resolved = resolve_ref(&f, &resolver, p)
                    .with_context(|| format!("resolve outputs.packages.{k} reference {p:?}"))?;
                println!(
                    "      {} {}-{} from {}",
                    "resolved".green(),
                    resolved.name,
                    format!("{}-{}", resolved.version, resolved.release),
                    resolved.repo_name
                );
            }
        }
    }
    if !f.outputs.dev_shells.is_empty() {
        println!("{}:", "outputs.devShells".bold());
        for (k, v) in &f.outputs.dev_shells {
            println!("  {}: {} package(s)", k.cyan(), v.packages.len());
            for p in &v.packages {
                println!("    - {p}");
                let resolved = resolve_ref(&f, &resolver, p)
                    .with_context(|| format!("resolve outputs.devShells.{k} reference {p:?}"))?;
                println!(
                    "      {} {}-{} from {}",
                    "resolved".green(),
                    resolved.name,
                    format!("{}-{}", resolved.version, resolved.release),
                    resolved.repo_name
                );
            }
        }
    }
    Ok(())
}

pub fn lock(_ctx: &Context, root: PathBuf) -> Result<()> {
    let lock = fl::build_lock(&root).context("build flake lock")?;
    fl::save_lock(&root, &lock).context("write flake.lock.toml")?;
    for (name, input) in &lock.inputs {
        let revision = input.locked_revision.as_deref().unwrap_or("-");
        let sha = input.locked_sha256.as_deref().unwrap_or("-");
        let short_sha: String = sha.chars().take(12).collect();
        println!(
            "{} {} {} {}",
            "locked".green().bold(),
            name.cyan(),
            revision,
            short_sha
        );
    }
    println!("{} wrote {}", "+".green(), fl::lock_path(&root).display());
    Ok(())
}

fn resolve_ref(
    flake: &fl::Flake,
    resolver: &Resolver<'_>,
    reference: &str,
) -> Result<feathernix_repo::ResolvedPackage> {
    let (input_name, package_name) = parse_package_ref(reference)?;
    let input = flake
        .inputs
        .get(input_name)
        .ok_or_else(|| anyhow!("unknown flake input {input_name:?} in reference {reference:?}"))?;
    if input.kind != fl::InputKind::Repo {
        bail!(
            "reference {reference:?} targets input {input_name:?} of kind {}; only repo inputs can be resolved from cached metadata",
            input.kind
        );
    }
    resolver
        .resolve_from_repo_url(package_name, &input.url)
        .with_context(|| {
            format!(
                "cached repo metadata for input {input_name:?} at {} is missing or incomplete; run `fn repo add {input_name} {}` and `fn repo refresh` first",
                input.url, input.url
            )
        })
}

fn parse_package_ref(reference: &str) -> Result<(&str, &str)> {
    let (input, package) = reference.split_once('#').ok_or_else(|| {
        anyhow!("invalid package reference {reference:?}; expected <input>#<package>")
    })?;
    if input.is_empty() || package.is_empty() || package.contains('#') {
        bail!("invalid package reference {reference:?}; expected <input>#<package>");
    }
    Ok((input, package))
}
