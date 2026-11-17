use serde::{Deserialize, Serialize};

pub const API_VERSION: &str = "v1";
pub const HEADER_AUTH: &str = "Authorization";
pub const HEADER_WORKER_TOKEN: &str = "X-Feathernix-Worker-Token";

pub mod routes {
    pub const SUBMIT_JOB: &str = "/v1/jobs";
    pub const GET_JOB: &str = "/v1/jobs/{id}";
    pub const LIST_JOBS: &str = "/v1/jobs";
    pub const REBUILD_JOB: &str = "/v1/jobs/{id}/rebuild";
    pub const REGISTER_WORKER: &str = "/v1/workers/register";
    pub const LIST_WORKERS: &str = "/v1/workers";
    pub const HEARTBEAT: &str = "/v1/workers/{id}/heartbeat";
    pub const LEASE: &str = "/v1/workers/{id}/lease";
    pub const APPEND_LOG: &str = "/v1/workers/{id}/jobs/{job_id}/log";
    pub const UPLOAD_ARTIFACT: &str = "/v1/workers/{id}/jobs/{job_id}/artifact";
    pub const COMPLETE_JOB: &str = "/v1/workers/{id}/jobs/{job_id}/complete";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Pending,
    Assigned,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            JobState::Pending => "pending",
            JobState::Assigned => "assigned",
            JobState::Running => "running",
            JobState::Succeeded => "succeeded",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => JobState::Pending,
            "assigned" => JobState::Assigned,
            "running" => JobState::Running,
            "succeeded" => JobState::Succeeded,
            "failed" => JobState::Failed,
            "cancelled" => JobState::Cancelled,
            _ => return None,
        })
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, JobState::Succeeded | JobState::Failed | JobState::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitJobRequest {
    pub recipe_toml: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitJobResponse {
    pub job_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobInfo {
    pub job_id: String,
    pub state: JobState,
    pub package_name: String,
    pub package_version: String,
    pub package_arch: String,
    pub created_at: String,
    #[serde(default)]
    pub assigned_worker: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerRegisterRequest {
    pub hostname: String,
    pub arch: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerRegisterResponse {
    pub worker_id: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseResponse {
    pub job_id: String,
    pub recipe_toml: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteJobRequest {
    pub state: JobState,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub source_tree_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendLogRequest {
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListJobsResponse {
    pub jobs: Vec<JobInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerInfo {
    pub worker_id: String,
    pub hostname: String,
    pub arch: String,
    pub registered_at: String,
    pub last_heartbeat_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListWorkersResponse {
    pub workers: Vec<WorkerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildJobRequest {
    #[serde(default)]
    pub note: Option<String>,
}
