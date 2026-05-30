use anyhow::{Context as _, Result, anyhow};
use colored::Colorize;

use feathernix_config::{RepoBlock, RepoFile, SigningBlock, load_all};
use feathernix_repo::{RepoSync, SignatureStatus};

use crate::context::Context;

pub fn list(ctx: &Context) -> Result<()> {
    if ctx.repos.is_empty() {
        println!("(no repositories configured)");
        return Ok(());
    }
    println!(
        "{:<24} {:<8} {:<8} {}",
        "NAME".bold(),
        "PRI".bold(),
        "STATE".bold(),
        "BASE URL".bold()
    );
    for r in &ctx.repos {
        let state = if r.repo.enabled {
            "enabled".green().to_string()
        } else {
            "disabled".dimmed().to_string()
        };
        println!(
            "{:<24} {:<8} {:<8} {}",
            r.repo.name, r.repo.priority, state, r.repo.base_url
        );
    }
    Ok(())
}

pub fn refresh(ctx: &Context) -> Result<()> {
    let sync = RepoSync::new(&ctx.db, ctx.paths.keyrings_dir.clone()).context("init repo sync")?;
    let reports = sync
        .refresh_all(&ctx.repos)
        .context("refresh repositories")?;
    for r in &reports {
        let sig_note = match &r.signature_status {
            SignatureStatus::NotChecked => String::new(),
            SignatureStatus::Verified { fingerprint } => {
                format!(" {} {}", "(signed by".dimmed(), format!("{fingerprint})").dimmed())
            }
            SignatureStatus::MissingButOptional => " (sig missing, optional)".yellow().to_string(),
        };
        println!(
            "{} {}: {} packages indexed{}",
            "✓".green(),
            r.repository_name,
            r.packages_indexed,
            sig_note
        );
    }
    if reports.is_empty() {
        println!("(no enabled repositories to refresh)");
    }
    Ok(())
}

pub fn add(
    ctx: &Context,
    name: &str,
    url: &str,
    gpg_key_url: Option<&str>,
    signature_required: bool,
) -> Result<()> {
    let path = ctx.paths.user_repos_dir.join(format!("{name}.toml"));
    if path.exists() {
        return Err(anyhow!("repo {name} already exists at {}", path.display()));
    }
    std::fs::create_dir_all(&ctx.paths.user_repos_dir).ok();

    let signing = gpg_key_url.map(|k| SigningBlock {
        gpg_check: true,
        gpg_key_url: Some(k.to_string()),
        signature_required,
    });

    let file = RepoFile {
        repository: RepoBlock {
            name: name.to_string(),
            description: None,
            base_url: url.to_string(),
            enabled: true,
            priority: 50,
            signing,
            refresh: None,
        },
    };
    let body = toml::to_string_pretty(&file)?;
    std::fs::write(&path, body)?;
    println!("{} wrote {}", "+".green(), path.display());

    let repos = load_all(&ctx.paths.repos_dir, &ctx.paths.user_repos_dir)?;
    let target = repos
        .into_iter()
        .find(|r| r.repo.name == name)
        .ok_or_else(|| anyhow!("could not find newly added repo {name}"))?;
    let sync = RepoSync::new(&ctx.db, ctx.paths.keyrings_dir.clone())?;
    let repo_row = sync.upsert_repo(&target)?;
    let report = sync.refresh(&repo_row)?;
    let sig_note = match &report.signature_status {
        SignatureStatus::NotChecked => String::new(),
        SignatureStatus::Verified { fingerprint } => {
            format!(" (signed by {fingerprint})")
        }
        SignatureStatus::MissingButOptional => " (sig missing, optional)".to_string(),
    };
    println!(
        "{} {}: {} packages indexed{}",
        "✓".green(),
        report.repository_name,
        report.packages_indexed,
        sig_note
    );
    Ok(())
}

pub fn remove(ctx: &Context, name: &str) -> Result<()> {
    let user_path = ctx.paths.user_repos_dir.join(format!("{name}.toml"));
    let system_path = ctx.paths.repos_dir.join(format!("{name}.toml"));

    if user_path.exists() {
        std::fs::remove_file(&user_path)?;
        println!("{} removed {}", "-".yellow(), user_path.display());
    } else if system_path.exists() {
        return Err(anyhow!(
            "refusing to delete system repo file {} (remove it manually)",
            system_path.display()
        ));
    } else {
        return Err(anyhow!("no repo named {name}"));
    }
    Ok(())
}
