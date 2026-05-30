use std::path::PathBuf;
use std::sync::Arc;

use feathernix_sign::SecretKey;

use crate::db::BuilddDb;

#[derive(Clone)]
pub struct ControllerState {
    pub db: BuilddDb,
    pub artifacts_dir: PathBuf,
    pub publish_repo: Option<PathBuf>,
    pub signer: Option<Arc<SecretKey>>,
}

impl ControllerState {
    pub fn new(
        db: BuilddDb,
        artifacts_dir: PathBuf,
        publish_repo: Option<PathBuf>,
        signer: Option<SecretKey>,
    ) -> Self {
        Self {
            db,
            artifacts_dir,
            publish_repo,
            signer: signer.map(Arc::new),
        }
    }
}
