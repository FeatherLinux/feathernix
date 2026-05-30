use std::path::Path;

use anyhow::{Context, Result};
use colored::Colorize;

use feathernix_recipe::{init as recipe_init, load};

pub fn init(root: &Path, name: &str, version: &str, arch: &str) -> Result<()> {
    let p = recipe_init(root, name, version, arch).context("write recipe")?;
    println!("{} {}", "wrote".green().bold(), p.display());
    Ok(())
}

pub fn lint(root: &Path) -> Result<()> {
    let r = load(root).context("load recipe")?;
    println!(
        "{} {} {}-{} ({})",
        "ok".green().bold(),
        r.package.name,
        r.package.version,
        r.package.release,
        r.package.arch
    );
    Ok(())
}

pub fn show(root: &Path) -> Result<()> {
    let r = load(root).context("load recipe")?;
    println!("{}", r.to_pretty_string()?);
    Ok(())
}
