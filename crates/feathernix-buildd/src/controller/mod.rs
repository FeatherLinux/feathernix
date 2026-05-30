pub mod handlers;
pub mod state;

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result};
use axum::Router;
use axum::routing::{get, post};
use feathernix_buildd_proto::routes;
use tower_http::trace::TraceLayer;

use crate::db::BuilddDb;
use state::ControllerState;

pub fn run(
    listen: String,
    state_dir: PathBuf,
    publish_repo: Option<PathBuf>,
    signing_key: Option<PathBuf>,
) -> Result<()> {
    std::fs::create_dir_all(&state_dir)?;
    let db = BuilddDb::open(&state_dir.join("buildd.sqlite"))
        .context("open buildd db")?;
    let artifacts_dir = state_dir.join("artifacts");
    std::fs::create_dir_all(&artifacts_dir)?;

    let signer = match signing_key.as_ref() {
        Some(p) => Some(feathernix_sign::SecretKey::load(p, "").context("load signing key")?),
        None => None,
    };
    let state = ControllerState::new(db, artifacts_dir, publish_repo, signer);

    let addr: SocketAddr = listen.parse().context("parse listen address")?;
    tracing::info!("controller listening on {}", addr);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("tokio runtime")?;

    rt.block_on(async move {
        let app = Router::new()
            .route(routes::SUBMIT_JOB, post(handlers::submit_job).get(handlers::list_jobs))
            .route(routes::GET_JOB, get(handlers::get_job))
            .route(routes::REBUILD_JOB, post(handlers::rebuild_job))
            .route(routes::REGISTER_WORKER, post(handlers::register_worker))
            .route(routes::LIST_WORKERS, get(handlers::list_workers))
            .route(routes::HEARTBEAT, post(handlers::heartbeat))
            .route(routes::LEASE, get(handlers::lease))
            .route(routes::APPEND_LOG, post(handlers::append_log))
            .route(routes::UPLOAD_ARTIFACT, post(handlers::upload_artifact))
            .route(routes::COMPLETE_JOB, post(handlers::complete_job))
            .route("/healthz", get(|| async { "ok" }))
            .layer(TraceLayer::new_for_http())
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .context("bind listener")?;
        axum::serve(listener, app).await.context("axum serve")
    })
}
