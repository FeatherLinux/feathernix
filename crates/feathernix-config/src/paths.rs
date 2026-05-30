use std::env;
use std::path::{Path, PathBuf};

pub const FEATHERNIX_ROOT_ENV: &str = "FEATHERNIX_ROOT";

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: Option<PathBuf>,
    pub config_dir: PathBuf,
    pub repos_dir: PathBuf,
    pub user_config_dir: PathBuf,
    pub user_repos_dir: PathBuf,
    pub state_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub store_dir: PathBuf,
    pub db_path: PathBuf,
    pub profiles_dir: PathBuf,
    pub payloads_cache_dir: PathBuf,
    pub repodata_cache_dir: PathBuf,
    pub keyrings_dir: PathBuf,
}

impl Paths {
    pub fn from_env() -> Self {
        match env::var_os(FEATHERNIX_ROOT_ENV).map(PathBuf::from) {
            Some(root) => Self::rooted(&root),
            None => Self::system(),
        }
    }

    pub fn system() -> Self {
        let user_config_dir = dirs::config_dir()
            .map(|d| d.join("feathernix"))
            .unwrap_or_else(|| PathBuf::from("/etc/feathernix"));
        let user_repos_dir = user_config_dir.join("repos");

        Self {
            root: None,
            config_dir: PathBuf::from("/etc/feathernix"),
            repos_dir: PathBuf::from("/etc/feathernix/repos"),
            user_config_dir,
            user_repos_dir,
            state_dir: PathBuf::from("/var/lib/feathernix"),
            cache_dir: PathBuf::from("/var/cache/feathernix"),
            store_dir: PathBuf::from("/fnix/store"),
            db_path: PathBuf::from("/var/lib/feathernix/db.sqlite"),
            profiles_dir: PathBuf::from("/var/lib/feathernix/profiles"),
            payloads_cache_dir: PathBuf::from("/var/cache/feathernix/payloads"),
            repodata_cache_dir: PathBuf::from("/var/cache/feathernix/repodata"),
            keyrings_dir: PathBuf::from("/var/cache/feathernix/keyrings"),
        }
    }

    pub fn rooted(root: &Path) -> Self {
        let config_dir = root.join("etc/feathernix");
        let user_config_dir = root.join("home/.config/feathernix");
        Self {
            root: Some(root.to_path_buf()),
            repos_dir: config_dir.join("repos"),
            user_repos_dir: user_config_dir.join("repos"),
            config_dir,
            user_config_dir,
            state_dir: root.join("var/lib/feathernix"),
            cache_dir: root.join("var/cache/feathernix"),
            store_dir: root.join("fnix/store"),
            db_path: root.join("var/lib/feathernix/db.sqlite"),
            profiles_dir: root.join("var/lib/feathernix/profiles"),
            payloads_cache_dir: root.join("var/cache/feathernix/payloads"),
            repodata_cache_dir: root.join("var/cache/feathernix/repodata"),
            keyrings_dir: root.join("var/cache/feathernix/keyrings"),
        }
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for d in [
            &self.config_dir,
            &self.repos_dir,
            &self.user_config_dir,
            &self.user_repos_dir,
            &self.state_dir,
            &self.cache_dir,
            &self.store_dir,
            &self.profiles_dir,
            &self.payloads_cache_dir,
            &self.repodata_cache_dir,
            &self.keyrings_dir,
        ] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rooted_paths_are_under_root() {
        let p = Paths::rooted(Path::new("/tmp/fnroot"));
        assert_eq!(p.config_dir, PathBuf::from("/tmp/fnroot/etc/feathernix"));
        assert_eq!(p.repos_dir, PathBuf::from("/tmp/fnroot/etc/feathernix/repos"));
        assert_eq!(p.store_dir, PathBuf::from("/tmp/fnroot/fnix/store"));
        assert_eq!(p.db_path, PathBuf::from("/tmp/fnroot/var/lib/feathernix/db.sqlite"));
    }
}
