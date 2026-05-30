use std::path::PathBuf;

use anyhow::{Context as _, Result};
use colored::Colorize;

use feathernix_flake as fl;

pub fn init(root: PathBuf, description: Option<String>) -> Result<()> {
    let p = fl::init(&root, description.as_deref()).context("init flake")?;
    println!("{} wrote {}", "+".green(), p.display());
    Ok(())
}

pub fn show(root: PathBuf) -> Result<()> {
    let f = fl::load(&root).context("load flake.toml")?;
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
            }
        }
    }
    if !f.outputs.dev_shells.is_empty() {
        println!("{}:", "outputs.devShells".bold());
        for (k, v) in &f.outputs.dev_shells {
            println!("  {}: {} package(s)", k.cyan(), v.packages.len());
            for p in &v.packages {
                println!("    - {p}");
            }
        }
    }
    Ok(())
}

pub fn lock(_root: PathBuf) -> Result<()> {
    // MVP stub. Real implementation walks inputs, fetches current state, writes flake.lock.toml.
    println!("{} flake lock not yet implemented", "(stub)".dimmed());
    Ok(())
}
