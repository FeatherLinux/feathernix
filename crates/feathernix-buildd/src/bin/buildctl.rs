use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;

use feathernix_buildd_proto::{
    JobInfo, ListJobsResponse, ListWorkersResponse, RebuildJobRequest, SubmitJobRequest,
    SubmitJobResponse,
};

#[derive(Parser, Debug)]
#[command(name = "feathernix-buildctl", about = "Ops CLI for the FeatherNix buildd controller", version)]
struct Cli {
    #[arg(long, default_value = "http://127.0.0.1:7070")]
    controller: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Job operations
    Jobs {
        #[command(subcommand)]
        cmd: JobsCmd,
    },
    /// Worker operations
    Workers {
        #[command(subcommand)]
        cmd: WorkersCmd,
    },
}

#[derive(Subcommand, Debug)]
enum JobsCmd {
    /// Submit a recipe (read from `--recipe`) to the controller.
    Submit {
        #[arg(long)]
        recipe: PathBuf,
        #[arg(long)]
        note: Option<String>,
    },
    /// List jobs, optionally filtered by state.
    List {
        #[arg(long)]
        state: Option<String>,
    },
    /// Show details for one job by id.
    Get { id: String },
    /// Re-queue a job using its stored recipe.
    Rebuild {
        id: String,
        #[arg(long)]
        note: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum WorkersCmd {
    /// List registered workers.
    List,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = reqwest::blocking::Client::new();
    match cli.cmd {
        Cmd::Jobs { cmd } => match cmd {
            JobsCmd::Submit { recipe, note } => {
                let recipe_toml =
                    std::fs::read_to_string(&recipe).with_context(|| format!("read {}", recipe.display()))?;
                let req = SubmitJobRequest { recipe_toml, note };
                let resp: SubmitJobResponse = client
                    .post(format!("{}/v1/jobs", cli.controller.trim_end_matches('/')))
                    .json(&req)
                    .send()?
                    .error_for_status()?
                    .json()?;
                println!("{} {}", "submitted".green().bold(), resp.job_id);
            }
            JobsCmd::List { state } => {
                let mut url = format!("{}/v1/jobs", cli.controller.trim_end_matches('/'));
                if let Some(s) = state {
                    url.push_str(&format!("?state={s}"));
                }
                let resp: ListJobsResponse = client.get(url).send()?.error_for_status()?.json()?;
                if resp.jobs.is_empty() {
                    println!("(no jobs)");
                } else {
                    println!("{:<22} {:<10} {:<20} {:<12} CREATED", "JOB", "STATE", "PACKAGE", "ARCH");
                    for j in resp.jobs {
                        println!(
                            "{:<22} {:<10} {:<20} {:<12} {}",
                            j.job_id,
                            j.state.as_str(),
                            format!("{}-{}", j.package_name, j.package_version),
                            j.package_arch,
                            j.created_at
                        );
                    }
                }
            }
            JobsCmd::Get { id } => {
                let url = format!("{}/v1/jobs/{}", cli.controller.trim_end_matches('/'), id);
                let info: JobInfo = client.get(url).send()?.error_for_status()?.json()?;
                println!("{}", serde_json::to_string_pretty(&info)?);
            }
            JobsCmd::Rebuild { id, note } => {
                let url = format!(
                    "{}/v1/jobs/{}/rebuild",
                    cli.controller.trim_end_matches('/'),
                    id
                );
                let resp: SubmitJobResponse = client
                    .post(url)
                    .json(&RebuildJobRequest { note })
                    .send()?
                    .error_for_status()?
                    .json()?;
                println!("{} {} (from {})", "rebuild queued".green().bold(), resp.job_id, id);
            }
        },
        Cmd::Workers { cmd } => match cmd {
            WorkersCmd::List => {
                let url = format!("{}/v1/workers", cli.controller.trim_end_matches('/'));
                let resp: ListWorkersResponse = client.get(url).send()?.error_for_status()?.json()?;
                if resp.workers.is_empty() {
                    println!("(no workers)");
                } else {
                    println!(
                        "{:<26} {:<20} {:<14} {:<32} LAST HEARTBEAT",
                        "WORKER", "HOSTNAME", "ARCH", "REGISTERED"
                    );
                    for w in resp.workers {
                        println!(
                            "{:<26} {:<20} {:<14} {:<32} {}",
                            w.worker_id, w.hostname, w.arch, w.registered_at, w.last_heartbeat_at
                        );
                    }
                }
            }
        },
    }
    Ok(())
}
