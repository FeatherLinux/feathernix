use axum::Json;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use feathernix_buildd_proto::{
    AppendLogRequest, CompleteJobRequest, JobInfo, JobState, LeaseResponse, ListJobsResponse,
    ListWorkersResponse, RebuildJobRequest, SubmitJobRequest, SubmitJobResponse, WorkerInfo,
    WorkerRegisterRequest, WorkerRegisterResponse,
};
use feathernix_recipe::Recipe;

use super::state::ControllerState;

fn err(code: StatusCode, msg: impl Into<String>) -> (StatusCode, String) {
    (code, msg.into())
}

fn random_id(prefix: &str) -> String {
    let bytes: [u8; 12] = rand::random();
    format!("{prefix}-{}", hex::encode(bytes))
}

pub async fn submit_job(
    State(s): State<ControllerState>,
    Json(req): Json<SubmitJobRequest>,
) -> Result<Json<SubmitJobResponse>, (StatusCode, String)> {
    let recipe = Recipe::from_str(&req.recipe_toml)
        .map_err(|e| err(StatusCode::BAD_REQUEST, format!("recipe parse: {e}")))?;
    let id = random_id("job");
    s.db
        .insert_job(
            &id,
            &recipe.package.name,
            &recipe.package.version,
            &recipe.package.arch,
            &req.recipe_toml,
            req.note.as_deref(),
        )
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(SubmitJobResponse { job_id: id }))
}

pub async fn get_job(
    State(s): State<ControllerState>,
    Path(id): Path<String>,
) -> Result<Json<JobInfo>, (StatusCode, String)> {
    let info = s
        .db
        .get_job(&id)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, format!("job {id} not found")))?;
    Ok(Json(info))
}

#[derive(Debug, Deserialize)]
pub struct ListJobsQuery {
    pub state: Option<String>,
}

pub async fn list_jobs(
    State(s): State<ControllerState>,
    Query(q): Query<ListJobsQuery>,
) -> Result<Json<ListJobsResponse>, (StatusCode, String)> {
    let state_filter = q.state.as_deref().and_then(JobState::from_str);
    let jobs = s
        .db
        .list_jobs(state_filter)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(ListJobsResponse { jobs }))
}

pub async fn rebuild_job(
    State(s): State<ControllerState>,
    Path(id): Path<String>,
    Json(req): Json<RebuildJobRequest>,
) -> Result<Json<SubmitJobResponse>, (StatusCode, String)> {
    let recipe_toml = s
        .db
        .get_recipe(&id)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, format!("job {id} not found")))?;
    let recipe = Recipe::from_str(&recipe_toml)
        .map_err(|e| err(StatusCode::BAD_REQUEST, format!("stored recipe: {e}")))?;
    let new_id = random_id("job");
    s.db
        .insert_job(
            &new_id,
            &recipe.package.name,
            &recipe.package.version,
            &recipe.package.arch,
            &recipe_toml,
            req.note.as_deref().or(Some("rebuild")),
        )
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(SubmitJobResponse { job_id: new_id }))
}

pub async fn list_workers(
    State(s): State<ControllerState>,
) -> Result<Json<ListWorkersResponse>, (StatusCode, String)> {
    let rows = s
        .db
        .list_workers()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let workers = rows
        .into_iter()
        .map(|(id, hostname, arch, registered_at, last_heartbeat_at)| WorkerInfo {
            worker_id: id,
            hostname,
            arch,
            registered_at,
            last_heartbeat_at,
        })
        .collect();
    Ok(Json(ListWorkersResponse { workers }))
}

pub async fn register_worker(
    State(s): State<ControllerState>,
    Json(req): Json<WorkerRegisterRequest>,
) -> Result<Json<WorkerRegisterResponse>, (StatusCode, String)> {
    let id = random_id("w");
    let token = random_id("tok");
    let caps = serde_json::to_string(&req.capabilities).unwrap_or_else(|_| "[]".into());
    s.db
        .register_worker(&id, &req.hostname, &req.arch, &token, &caps)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(WorkerRegisterResponse { worker_id: id, token }))
}

fn worker_token_check(
    s: &ControllerState,
    worker_id: &str,
    headers: &axum::http::HeaderMap,
) -> Result<(), (StatusCode, String)> {
    let token = headers
        .get(feathernix_buildd_proto::HEADER_WORKER_TOKEN)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "missing worker token"))?;
    let ok = s
        .db
        .worker_token_matches(worker_id, token)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !ok {
        return Err(err(StatusCode::UNAUTHORIZED, "invalid worker token"));
    }
    Ok(())
}

pub async fn heartbeat(
    State(s): State<ControllerState>,
    Path(worker_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<StatusCode, (StatusCode, String)> {
    worker_token_check(&s, &worker_id, &headers)?;
    s.db
        .worker_heartbeat(&worker_id)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn lease(
    State(s): State<ControllerState>,
    Path(worker_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<axum::response::Response, (StatusCode, String)> {
    worker_token_check(&s, &worker_id, &headers)?;
    let leased = s
        .db
        .lease_next_job(&worker_id)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    match leased {
        Some((job_id, recipe_toml)) => {
            Ok(Json(LeaseResponse { job_id, recipe_toml }).into_response())
        }
        None => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

pub async fn append_log(
    State(s): State<ControllerState>,
    Path((worker_id, job_id)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AppendLogRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    worker_token_check(&s, &worker_id, &headers)?;
    s.db
        .append_log(&job_id, &req.data)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn upload_artifact(
    State(s): State<ControllerState>,
    Path((worker_id, job_id)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    mut multipart: Multipart,
) -> Result<StatusCode, (StatusCode, String)> {
    worker_token_check(&s, &worker_id, &headers)?;
    s.db
        .mark_running(&job_id)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let job_dir = s.artifacts_dir.join(&job_id);
    std::fs::create_dir_all(&job_dir)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("mkdir: {e}")))?;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| err(StatusCode::BAD_REQUEST, format!("multipart: {e}")))?
    {
        let filename = field
            .file_name()
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "field missing filename"))?
            .to_string();
        let bytes = field
            .bytes()
            .await
            .map_err(|e| err(StatusCode::BAD_REQUEST, format!("read field: {e}")))?;
        let sha = hex::encode(Sha256::digest(&bytes));
        let dest = job_dir.join(&filename);
        std::fs::write(&dest, &bytes)
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("write: {e}")))?;
        s.db
            .record_artifact(&job_id, &filename, &sha, bytes.len() as i64, &dest.to_string_lossy())
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn complete_job(
    State(s): State<ControllerState>,
    Path((worker_id, job_id)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CompleteJobRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    worker_token_check(&s, &worker_id, &headers)?;
    let final_state = match req.state {
        JobState::Succeeded | JobState::Failed | JobState::Cancelled => req.state,
        _ => return Err(err(StatusCode::BAD_REQUEST, "non-terminal state in complete")),
    };
    s.db
        .complete_job(&job_id, final_state, req.error.as_deref(), req.source_tree_sha256.as_deref())
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if final_state == JobState::Succeeded
        && let Some(repo) = &s.publish_repo
    {
        let job_dir = s.artifacts_dir.join(&job_id);
        if job_dir.exists() {
            for entry in std::fs::read_dir(&job_dir).into_iter().flatten().flatten() {
                let p = entry.path();
                let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                if name.ends_with(".fnpkg.7z") {
                    if let Err(e) = feathernix_publish::ingest_archive(repo, &p) {
                        tracing::warn!("ingest_archive failed for {}: {}", p.display(), e);
                    }
                }
            }
        }
        let repo_name = repo
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "repo".to_string());
        let signer_ref = s.signer.as_ref().map(|arc| arc.as_ref());
        if let Err(e) = feathernix_publish::publish(repo, &repo_name, signer_ref) {
            tracing::warn!("publish after job {}: {}", job_id, e);
        } else {
            tracing::info!("republished repo at {}", repo.display());
        }
    }
    Ok(StatusCode::NO_CONTENT)
}
