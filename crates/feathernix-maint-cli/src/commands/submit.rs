use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use colored::Colorize;

use feathernix_buildd_proto::{JobInfo, JobState, SubmitJobRequest, SubmitJobResponse};
use feathernix_recipe::{RECIPE_FILENAME, load};

pub fn run(controller: &str, root: &Path) -> Result<()> {
    let recipe = load(root).context("load recipe")?;
    let recipe_toml = std::fs::read_to_string(root.join(RECIPE_FILENAME))
        .context("read recipe file")?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;
    let base = controller.trim_end_matches('/');
    let req = SubmitJobRequest { recipe_toml, note: None };
    let resp: SubmitJobResponse = client
        .post(format!("{base}/v1/jobs"))
        .json(&req)
        .send()
        .context("POST /v1/jobs")?
        .error_for_status()?
        .json()?;
    println!(
        "{} {} (package {}-{})",
        "submitted".green().bold(),
        resp.job_id,
        recipe.package.name,
        recipe.package.version
    );
    // Optionally poll until terminal.
    let poll = std::env::var("FNMAINT_SUBMIT_WAIT").ok().as_deref() == Some("1");
    if !poll {
        println!("(use FNMAINT_SUBMIT_WAIT=1 to block until terminal)");
        return Ok(());
    }
    loop {
        std::thread::sleep(Duration::from_secs(2));
        let info: JobInfo = client
            .get(format!("{base}/v1/jobs/{}", resp.job_id))
            .send()?
            .error_for_status()?
            .json()?;
        println!("  state: {}", info.state.as_str());
        if info.state.is_terminal() {
            if info.state == JobState::Succeeded {
                println!("{} {}", "succeeded".green().bold(), resp.job_id);
            } else {
                let msg = info.error.unwrap_or_default();
                anyhow::bail!("job {} ended {}: {}", resp.job_id, info.state.as_str(), msg);
            }
            break;
        }
    }
    Ok(())
}
