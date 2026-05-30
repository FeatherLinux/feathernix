use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use colored::Colorize;

use feathernix_publish::{ingest_archive, publish};
use feathernix_sign::SecretKey;

pub fn run(repo: &Path, key: &Path, archives: &[PathBuf]) -> Result<()> {
    let sk = SecretKey::load(key, "").context("load signing key")?;
    let repo_name = repo
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".to_string());

    for archive in archives {
        let dest = ingest_archive(repo, archive).context("ingest archive")?;
        println!("{} {}", "ingested".cyan().bold(), dest.display());
    }

    let report = publish(repo, &repo_name, Some(&sk)).context("publish")?;
    println!(
        "{} {} packages indexed → {}",
        "published".green().bold(),
        report.packages_indexed,
        report.index_path.display()
    );
    if let Some(sig) = &report.index_sig_path {
        println!("           signed: {}", sig.display());
    }
    Ok(())
}
