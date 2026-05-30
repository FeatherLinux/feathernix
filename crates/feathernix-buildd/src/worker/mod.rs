use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use feathernix_buildd_proto::{
    AppendLogRequest, CompleteJobRequest, HEADER_WORKER_TOKEN, JobState, LeaseResponse,
    WorkerRegisterRequest, WorkerRegisterResponse,
};
use feathernix_recipe::Recipe;
use reqwest::blocking::{Client, multipart};

pub fn run(controller: String, token: Option<String>, workdir: PathBuf) -> Result<()> {
    std::fs::create_dir_all(&workdir)?;
    let signing_key_path = std::env::var_os("FEATHERNIX_WORKER_SIGNING_KEY")
        .map(PathBuf::from);
    let signer = match signing_key_path.as_deref() {
        Some(p) => Some(feathernix_sign::SecretKey::load(p, "").context("load signing key")?),
        None => None,
    };

    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent("feathernix-buildd-worker/0.1")
        .build()?;

    let base = controller.trim_end_matches('/').to_string();

    let (worker_id, worker_token) = match token {
        Some(t) => {
            let id = std::env::var("FEATHERNIX_WORKER_ID").context(
                "FEATHERNIX_WORKER_ID env var required when --token is supplied",
            )?;
            (id, t)
        }
        None => register(&client, &base)?,
    };
    tracing::info!("worker {} registered", worker_id);

    loop {
        heartbeat(&client, &base, &worker_id, &worker_token).unwrap_or_else(|e| {
            tracing::warn!("heartbeat: {e}");
        });
        let lease = lease_one(&client, &base, &worker_id, &worker_token)?;
        match lease {
            Some(l) => {
                tracing::info!("leased job {}", l.job_id);
                if let Err(e) = handle_job(
                    &client,
                    &base,
                    &worker_id,
                    &worker_token,
                    &workdir,
                    &l,
                    signer.as_ref(),
                ) {
                    tracing::error!("job {} failed: {}", l.job_id, e);
                    let _ = complete_job(
                        &client,
                        &base,
                        &worker_id,
                        &worker_token,
                        &l.job_id,
                        JobState::Failed,
                        Some(e.to_string()),
                        None,
                    );
                }
            }
            None => {
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

fn register(client: &Client, base: &str) -> Result<(String, String)> {
    let hostname = hostname_or_default();
    let arch = current_arch();
    let req = WorkerRegisterRequest {
        hostname,
        arch,
        capabilities: vec!["bwrap".into()],
    };
    let resp: WorkerRegisterResponse = client
        .post(format!("{base}/v1/workers/register"))
        .json(&req)
        .send()?
        .error_for_status()?
        .json()?;
    Ok((resp.worker_id, resp.token))
}

fn heartbeat(client: &Client, base: &str, worker_id: &str, token: &str) -> Result<()> {
    client
        .post(format!("{base}/v1/workers/{worker_id}/heartbeat"))
        .header(HEADER_WORKER_TOKEN, token)
        .send()?
        .error_for_status()?;
    Ok(())
}

fn lease_one(
    client: &Client,
    base: &str,
    worker_id: &str,
    token: &str,
) -> Result<Option<LeaseResponse>> {
    let resp = client
        .get(format!("{base}/v1/workers/{worker_id}/lease"))
        .header(HEADER_WORKER_TOKEN, token)
        .send()?;
    if resp.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(None);
    }
    let resp = resp.error_for_status()?;
    Ok(Some(resp.json()?))
}

fn handle_job(
    client: &Client,
    base: &str,
    worker_id: &str,
    token: &str,
    workdir: &Path,
    lease: &LeaseResponse,
    signer: Option<&feathernix_sign::SecretKey>,
) -> Result<()> {
    let job_dir = workdir.join(&lease.job_id);
    let work_root = job_dir.join("work");
    let source_cache = job_dir.join("sources");
    let out_dir = job_dir.join("out");
    std::fs::create_dir_all(&out_dir)?;

    let recipe = Recipe::from_str(&lease.recipe_toml).context("parse recipe")?;
    append_log(
        client,
        base,
        worker_id,
        token,
        &lease.job_id,
        &format!("[worker] building {}-{}\n", recipe.package.name, recipe.package.version),
    )
    .ok();

    let builder = feathernix_build::Builder::new(work_root, source_cache).context("init builder")?;
    let artifact = match builder.build(&recipe, &out_dir) {
        Ok(a) => a,
        Err(e) => {
            append_log(client, base, worker_id, token, &lease.job_id, &format!("[worker] build error: {e}\n")).ok();
            return Err(e.into());
        }
    };

    if let Some(sk) = signer {
        sk.sign_file(&artifact.archive_path).context("sign artifact")?;
        append_log(
            client,
            base,
            worker_id,
            token,
            &lease.job_id,
            "[worker] artifact signed\n",
        )
        .ok();
    }

    upload_artifact(
        client,
        base,
        worker_id,
        token,
        &lease.job_id,
        &artifact.archive_path,
    )?;
    if let Some(sk) = signer {
        let sig_path = artifact.archive_path.with_extension(
            artifact
                .archive_path
                .extension()
                .map(|e| {
                    let mut s = e.to_string_lossy().into_owned();
                    s.push_str(".sig");
                    s
                })
                .unwrap_or_else(|| "sig".to_string()),
        );
        if sig_path.exists() {
            upload_artifact(client, base, worker_id, token, &lease.job_id, &sig_path)?;
        }
        let _ = sk; // referenced above
    }

    complete_job(
        client,
        base,
        worker_id,
        token,
        &lease.job_id,
        JobState::Succeeded,
        None,
        Some(artifact.source_tree_sha256),
    )?;
    append_log(
        client,
        base,
        worker_id,
        token,
        &lease.job_id,
        "[worker] job complete\n",
    )
    .ok();
    Ok(())
}

fn upload_artifact(
    client: &Client,
    base: &str,
    worker_id: &str,
    token: &str,
    job_id: &str,
    path: &Path,
) -> Result<()> {
    let filename = path.file_name().unwrap().to_string_lossy().into_owned();
    let bytes = std::fs::read(path)?;
    let part = multipart::Part::bytes(bytes).file_name(filename.clone());
    let form = multipart::Form::new().part("artifact", part);
    client
        .post(format!("{base}/v1/workers/{worker_id}/jobs/{job_id}/artifact"))
        .header(HEADER_WORKER_TOKEN, token)
        .multipart(form)
        .send()?
        .error_for_status()?;
    Ok(())
}

fn append_log(
    client: &Client,
    base: &str,
    worker_id: &str,
    token: &str,
    job_id: &str,
    chunk: &str,
) -> Result<()> {
    client
        .post(format!("{base}/v1/workers/{worker_id}/jobs/{job_id}/log"))
        .header(HEADER_WORKER_TOKEN, token)
        .json(&AppendLogRequest { data: chunk.to_string() })
        .send()?
        .error_for_status()?;
    Ok(())
}

fn complete_job(
    client: &Client,
    base: &str,
    worker_id: &str,
    token: &str,
    job_id: &str,
    state: JobState,
    error: Option<String>,
    source_tree_sha256: Option<String>,
) -> Result<()> {
    let req = CompleteJobRequest { state, error, source_tree_sha256 };
    client
        .post(format!("{base}/v1/workers/{worker_id}/jobs/{job_id}/complete"))
        .header(HEADER_WORKER_TOKEN, token)
        .json(&req)
        .send()?
        .error_for_status()?;
    Ok(())
}

fn hostname_or_default() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn current_arch() -> String {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    format!("{arch}-{os}")
}
