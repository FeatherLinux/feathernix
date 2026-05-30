use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "feathernix-buildd", about = "FeatherNix buildd controller/worker", version)]
struct Cli {
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run the controller (HTTP server + SQLite job queue).
    Controller {
        #[arg(long, default_value = "127.0.0.1:7070")]
        listen: String,
        #[arg(long)]
        state: PathBuf,
        #[arg(long)]
        publish_repo: Option<PathBuf>,
        #[arg(long)]
        signing_key: Option<PathBuf>,
    },
    /// Run a build worker that polls a controller for jobs.
    Worker {
        #[arg(long)]
        controller: String,
        #[arg(long)]
        token: Option<String>,
        #[arg(long)]
        workdir: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let filter = match cli.verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_new(filter).unwrap())
        .with_target(false)
        .try_init();

    match cli.cmd {
        Cmd::Controller {
            listen,
            state,
            publish_repo,
            signing_key,
        } => feathernix_buildd::controller::run(listen, state, publish_repo, signing_key),
        Cmd::Worker {
            controller,
            token,
            workdir,
        } => feathernix_buildd::worker::run(controller, token, workdir),
    }
}
