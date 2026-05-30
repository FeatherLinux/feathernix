use std::path::Path;

use anyhow::{Context, Result};
use colored::Colorize;

use feathernix_build::Builder;
use feathernix_config::Paths;
use feathernix_recipe::load;

pub fn run(root: &Path, out_dir: &Path) -> Result<()> {
    let recipe = load(root).context("load recipe")?;
    let paths = Paths::from_env();
    let work_root = paths.cache_dir.join("build-work");
    let source_cache = paths.cache_dir.join("sources");

    println!(
        "{} {} {}-{} ({})",
        "building".cyan().bold(),
        recipe.package.name,
        recipe.package.version,
        recipe.package.release,
        recipe.package.arch
    );

    let builder = Builder::new(work_root, source_cache).context("init builder")?;
    let artifact = builder.build(&recipe, out_dir).context("run build")?;
    println!(
        "{} {} ({} source sha256: {})",
        "wrote".green().bold(),
        artifact.archive_path.display(),
        recipe.source.kind_label(),
        &artifact.source_tree_sha256[..16]
    );
    Ok(())
}
