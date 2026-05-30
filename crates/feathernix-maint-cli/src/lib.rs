pub mod commands;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "fnmaint", about = "FeatherNix maintainer toolchain", version)]
struct Cli {
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[arg(long, global = true)]
    no_color: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Recipe authoring (init, lint, show)
    Recipe {
        #[command(subcommand)]
        cmd: RecipeCmd,
    },
    /// Build a recipe into a .fnpkg.7z
    Build {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },
    /// Sign a .fnpkg.7z (Phase 4 — currently stub)
    Sign {
        archive: PathBuf,
        #[arg(long)]
        key: PathBuf,
    },
    /// Publish artifacts to a repo dir (Phase 4 — currently stub)
    Publish {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        key: PathBuf,
        archives: Vec<PathBuf>,
    },
    /// Submit a recipe to a buildd controller (Phase 6+ — currently stub)
    Submit {
        #[arg(long)]
        controller: String,
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Manage signing keys
    Key {
        #[command(subcommand)]
        cmd: KeyCmd,
    },
}

#[derive(Subcommand, Debug)]
enum KeyCmd {
    /// Generate an ephemeral Ed25519 signing keypair.
    Gen {
        #[arg(long)]
        user_id: String,
        #[arg(long)]
        secret_out: PathBuf,
        #[arg(long)]
        public_out: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum RecipeCmd {
    /// Write a stub feathernix.recipe.toml in the current directory.
    Init {
        #[arg(long)]
        name: String,
        #[arg(long)]
        version: String,
        #[arg(long, default_value = "x86_64-linux")]
        arch: String,
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Validate a feathernix.recipe.toml.
    Lint {
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Print the parsed recipe.
    Show {
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

    match cli.cmd {
        Cmd::Recipe { cmd } => match cmd {
            RecipeCmd::Init { name, version, arch, root } => {
                commands::recipe::init(&root, &name, &version, &arch)
            }
            RecipeCmd::Lint { root } => commands::recipe::lint(&root),
            RecipeCmd::Show { root } => commands::recipe::show(&root),
        },
        Cmd::Build { root, out } => commands::build::run(&root, &out),
        Cmd::Sign { archive, key } => commands::sign::run(&archive, &key),
        Cmd::Publish { repo, key, archives } => commands::publish::run(&repo, &key, &archives),
        Cmd::Submit { controller, root } => commands::submit::run(&controller, &root),
        Cmd::Key { cmd } => match cmd {
            KeyCmd::Gen { user_id, secret_out, public_out } => {
                commands::key::generate(&user_id, &secret_out, &public_out)
            }
        },
    }
}
