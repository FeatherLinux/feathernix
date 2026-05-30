use std::path::PathBuf;

use anyhow::{Context as _, Result};

use feathernix_config::{LoadedRepo, Paths, SystemConfig, load_all};
use feathernix_db::Db;

pub struct Context {
    pub paths: Paths,
    pub config: SystemConfig,
    pub repos: Vec<LoadedRepo>,
    pub db: Db,
    pub profile: String,
    pub json: bool,
    pub verbose: u8,
}

pub struct ContextOpts {
    pub override_store: Option<PathBuf>,
    pub override_config: Option<PathBuf>,
    pub profile: String,
    pub json: bool,
    pub verbose: u8,
    pub ensure_dirs: bool,
}

impl Context {
    pub fn load(opts: ContextOpts) -> Result<Self> {
        let mut paths = Paths::from_env();
        if let Some(store) = opts.override_store {
            paths.store_dir = store;
        }
        if opts.ensure_dirs {
            paths.ensure_dirs().ok();
        }

        let config_path = opts
            .override_config
            .unwrap_or_else(|| paths.config_dir.join("feathernix.toml"));
        let config = SystemConfig::load(&config_path)
            .with_context(|| format!("loading {}", config_path.display()))?;

        let repos = load_all(&paths.repos_dir, &paths.user_repos_dir)
            .with_context(|| "loading repositories")?;

        let db = Db::open(&paths.db_path)
            .with_context(|| format!("opening database at {}", paths.db_path.display()))?;

        Ok(Self {
            paths,
            config,
            repos,
            db,
            profile: opts.profile,
            json: opts.json,
            verbose: opts.verbose,
        })
    }

    pub fn profile_dir(&self) -> PathBuf {
        self.paths.profiles_dir.join(&self.profile)
    }
}
