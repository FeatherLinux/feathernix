pub mod commands;
pub mod context;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::context::{Context, ContextOpts};

#[derive(Parser, Debug)]
#[command(name = "fn", about = "FeatherNix hybrid package manager", version)]
struct Cli {
    #[arg(long, global = true, value_name = "DIR")]
    store: Option<PathBuf>,

    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[arg(long, global = true)]
    no_color: bool,

    #[arg(long, global = true)]
    json: bool,

    #[arg(long, global = true, default_value = "system")]
    profile: String,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Install one or more packages and create a new generation
    Install { packages: Vec<String> },

    /// Manage repositories
    Repo {
        #[command(subcommand)]
        cmd: RepoCmd,
    },

    /// Manage generations for the active profile
    Generation {
        #[command(subcommand)]
        cmd: GenerationCmd,
    },

    /// Inspect the /fnix/store
    Store {
        #[command(subcommand)]
        cmd: StoreCmd,
    },

    /// Flake operations
    Flake {
        #[command(subcommand)]
        cmd: FlakeCmd,
    },
}

#[derive(Subcommand, Debug)]
enum RepoCmd {
    /// List configured repositories
    List,
    /// Add a new repository (writes user-level TOML)
    Add {
        name: String,
        url: String,
        /// URL to the armored GPG public key (.asc) that signs this repo.
        /// Implies gpg_check = true.
        #[arg(long)]
        gpg_key_url: Option<String>,
        /// If set, refresh/install fail when a signature is missing or invalid.
        /// Default is best-effort verification.
        #[arg(long)]
        signature_required: bool,
    },
    /// Remove a user-level repository
    Remove { name: String },
    /// Refresh all enabled repositories
    Refresh,
}

#[derive(Subcommand, Debug)]
enum GenerationCmd {
    /// List all generations for the active profile
    List,
    /// Switch to an explicit generation number
    Switch { generation: i32 },
    /// Roll back to the previous generation
    Rollback,
}

#[derive(Subcommand, Debug)]
enum StoreCmd {
    /// List all store paths
    List,
    /// Show info for a store path by hash prefix
    PathInfo { hash: String },
}

#[derive(Subcommand, Debug)]
enum FlakeCmd {
    /// Create a default flake.toml in CWD
    Init {
        #[arg(long)]
        description: Option<String>,
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Display the parsed flake.toml in CWD
    Show {
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Refresh flake.lock.toml against current input state (stub)
    Lock {
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    if cli.no_color {
        colored::control::set_override(false);
    }

    let filter = match cli.verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_new(filter).unwrap())
        .with_target(false)
        .try_init();

    // Flake commands don't need the full Context (no DB / config).
    if let Cmd::Flake { cmd } = &cli.cmd {
        return match cmd {
            FlakeCmd::Init { description, root } => {
                commands::flake::init(root.clone(), description.clone())
            }
            FlakeCmd::Show { root } => commands::flake::show(root.clone()),
            FlakeCmd::Lock { root } => commands::flake::lock(root.clone()),
        };
    }

    let ctx = Context::load(ContextOpts {
        override_store: cli.store,
        override_config: cli.config,
        profile: cli.profile,
        json: cli.json,
        verbose: cli.verbose,
        ensure_dirs: true,
    })?;

    match cli.cmd {
        Cmd::Install { packages } => commands::install::run(&ctx, &packages),
        Cmd::Repo { cmd } => match cmd {
            RepoCmd::List => commands::repo::list(&ctx),
            RepoCmd::Add {
                name,
                url,
                gpg_key_url,
                signature_required,
            } => commands::repo::add(&ctx, &name, &url, gpg_key_url.as_deref(), signature_required),
            RepoCmd::Remove { name } => commands::repo::remove(&ctx, &name),
            RepoCmd::Refresh => commands::repo::refresh(&ctx),
        },
        Cmd::Generation { cmd } => match cmd {
            GenerationCmd::List => commands::generation::list(&ctx),
            GenerationCmd::Switch { generation } => commands::generation::switch(&ctx, generation),
            GenerationCmd::Rollback => commands::generation::rollback(&ctx),
        },
        Cmd::Store { cmd } => match cmd {
            StoreCmd::List => commands::store::list(&ctx),
            StoreCmd::PathInfo { hash } => commands::store::path_info(&ctx, &hash),
        },
        Cmd::Flake { .. } => unreachable!("handled earlier"),
    }
}
