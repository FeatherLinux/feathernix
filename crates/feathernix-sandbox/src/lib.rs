use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use thiserror::Error;

pub const BWRAP_BIN: &str = "bwrap";

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("bubblewrap not available on PATH: {0}")]
    BwrapMissing(String),
    #[error("sandbox step exited {status}: {step}")]
    StepFailed { step: String, status: ExitStatus },
    #[error("sandbox config error: {0}")]
    Config(String),
}

pub type SandboxResult<T> = Result<T, SandboxError>;

/// Declarative description of a single bwrap-isolated process.
#[derive(Debug, Clone)]
pub struct SandboxSpec {
    pub workdir: PathBuf,
    pub out_dir: PathBuf,
    pub src_dir: Option<PathBuf>,
    pub ro_binds: Vec<(PathBuf, PathBuf)>,
    pub rw_binds: Vec<(PathBuf, PathBuf)>,
    pub env: BTreeMap<String, String>,
    pub network: bool,
    pub chdir: PathBuf,
}

impl SandboxSpec {
    pub fn new(workdir: PathBuf, out_dir: PathBuf) -> Self {
        let chdir = workdir.clone();
        Self {
            workdir,
            out_dir,
            src_dir: None,
            ro_binds: Vec::new(),
            rw_binds: Vec::new(),
            env: BTreeMap::new(),
            network: false,
            chdir,
        }
    }

    pub fn with_src(mut self, src: PathBuf) -> Self {
        self.src_dir = Some(src);
        self
    }

    pub fn with_network(mut self, enabled: bool) -> Self {
        self.network = enabled;
        self
    }

    pub fn ro_bind(mut self, host: PathBuf, guest: PathBuf) -> Self {
        self.ro_binds.push((host, guest));
        self
    }

    pub fn env(mut self, k: &str, v: &str) -> Self {
        self.env.insert(k.to_string(), v.to_string());
        self
    }

    /// Build the argv (minus the trailing `--` and command). Public for tests.
    pub fn build_args(&self) -> Vec<String> {
        let mut a: Vec<String> = Vec::new();
        a.push("--die-with-parent".into());
        a.push("--new-session".into());
        a.push("--unshare-pid".into());
        a.push("--unshare-uts".into());
        a.push("--unshare-ipc".into());
        a.push("--unshare-cgroup-try".into());
        if !self.network {
            a.push("--unshare-net".into());
        }
        a.push("--proc".into());
        a.push("/proc".into());
        a.push("--dev".into());
        a.push("/dev".into());
        a.push("--tmpfs".into());
        a.push("/tmp".into());

        for ro in &[
            "/usr",
            "/etc/resolv.conf",
            "/etc/ssl",
            "/etc/ca-certificates",
            "/etc/pki",
        ] {
            let p = Path::new(ro);
            if p.exists() {
                a.push("--ro-bind-try".into());
                a.push(ro.to_string());
                a.push(ro.to_string());
            }
        }
        // /bin and /lib are often symlinks to /usr — bind them if they exist.
        for ro in &["/bin", "/sbin", "/lib", "/lib64"] {
            let p = Path::new(ro);
            if p.exists() {
                a.push("--ro-bind-try".into());
                a.push(ro.to_string());
                a.push(ro.to_string());
            }
        }

        for (host, guest) in &self.ro_binds {
            a.push("--ro-bind".into());
            a.push(host.to_string_lossy().into());
            a.push(guest.to_string_lossy().into());
        }
        for (host, guest) in &self.rw_binds {
            a.push("--bind".into());
            a.push(host.to_string_lossy().into());
            a.push(guest.to_string_lossy().into());
        }

        a.push("--bind".into());
        a.push(self.workdir.to_string_lossy().into());
        a.push("/build".into());

        a.push("--bind".into());
        a.push(self.out_dir.to_string_lossy().into());
        a.push("/out".into());

        if let Some(src) = &self.src_dir {
            a.push("--ro-bind".into());
            a.push(src.to_string_lossy().into());
            a.push("/src".into());
        }

        a.push("--chdir".into());
        a.push(
            if self.chdir == self.workdir {
                "/build".into()
            } else {
                self.chdir.to_string_lossy().into()
            },
        );

        // Clear environment, set only what we want.
        a.push("--clearenv".into());
        for (k, v) in &self.env {
            a.push("--setenv".into());
            a.push(k.clone());
            a.push(v.clone());
        }
        a.push("--setenv".into());
        a.push("PATH".into());
        a.push("/usr/bin:/usr/local/bin:/bin".into());
        a.push("--setenv".into());
        a.push("HOME".into());
        a.push("/build".into());
        if self.src_dir.is_some() {
            a.push("--setenv".into());
            a.push("SRC".into());
            a.push("/src".into());
        }
        a.push("--setenv".into());
        a.push("OUT".into());
        a.push("/out".into());

        a
    }
}

/// Run one shell command inside the sandbox. Streams stdout/stderr through to the parent.
pub fn run_step(spec: &SandboxSpec, shell_cmd: &str) -> SandboxResult<()> {
    ensure_bwrap_available()?;
    std::fs::create_dir_all(&spec.workdir)?;
    std::fs::create_dir_all(&spec.out_dir)?;

    let mut cmd = Command::new(BWRAP_BIN);
    cmd.args(spec.build_args());
    cmd.arg("--");
    cmd.arg("/bin/sh");
    cmd.arg("-c");
    cmd.arg(shell_cmd);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    tracing::debug!("bwrap step: {}", shell_cmd);
    let status = cmd.status()?;
    if !status.success() {
        return Err(SandboxError::StepFailed {
            step: shell_cmd.to_string(),
            status,
        });
    }
    Ok(())
}

pub fn ensure_bwrap_available() -> SandboxResult<()> {
    match Command::new(BWRAP_BIN).arg("--version").output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(SandboxError::BwrapMissing(format!(
            "bwrap --version exited {}: {}",
            o.status,
            String::from_utf8_lossy(&o.stderr).trim()
        ))),
        Err(e) => Err(SandboxError::BwrapMissing(format!("bwrap not on PATH: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn have_bwrap() -> bool {
        ensure_bwrap_available().is_ok()
    }

    #[test]
    fn build_args_includes_clearenv_and_proc() {
        let tmp = tempfile::tempdir().unwrap();
        let workdir = tmp.path().join("build");
        let out_dir = tmp.path().join("out");
        std::fs::create_dir_all(&workdir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let spec = SandboxSpec::new(workdir, out_dir).env("JOBS", "2");
        let args = spec.build_args();
        assert!(args.iter().any(|a| a == "--clearenv"));
        assert!(args.iter().any(|a| a == "--proc"));
        assert!(args.iter().any(|a| a == "/proc"));
        assert!(args.iter().any(|a| a == "JOBS"));
    }

    #[test]
    fn round_trip_writes_to_out() {
        if !have_bwrap() {
            eprintln!("skipping: bwrap not available");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let workdir = tmp.path().join("build");
        let out_dir = tmp.path().join("out");
        std::fs::create_dir_all(&workdir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let spec = SandboxSpec::new(workdir.clone(), out_dir.clone());
        run_step(&spec, "echo hello > $OUT/hi.txt").unwrap();
        let content = std::fs::read_to_string(out_dir.join("hi.txt")).unwrap();
        assert_eq!(content.trim(), "hello");
    }

    #[test]
    fn network_is_isolated_by_default() {
        if !have_bwrap() {
            eprintln!("skipping: bwrap not available");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let workdir = tmp.path().join("build");
        let out_dir = tmp.path().join("out");
        std::fs::create_dir_all(&workdir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let spec = SandboxSpec::new(workdir, out_dir);
        // `getent hosts localhost` requires resolver/network — should fail when isolated.
        // We can't depend on getent being present in every image, so just assert
        // build_args asked for unshare-net.
        let args = spec.build_args();
        assert!(args.iter().any(|a| a == "--unshare-net"));
    }
}
