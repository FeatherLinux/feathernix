use std::path::Path;
use std::process::Command;

use crate::SourceError;

pub fn clone_into(url: &str, rev: &str, dest: &Path) -> Result<(), SourceError> {
    std::fs::create_dir_all(dest)?;
    run_git(&["init", "--quiet"], dest)?;
    run_git(&["remote", "add", "origin", url], dest)?;
    // Try a shallow fetch of the exact ref; fall back to a full fetch if the
    // server doesn't support uploadpack.allowReachableSHA1InWant for sha refs.
    if run_git(&["fetch", "--depth=1", "--quiet", "origin", rev], dest).is_err() {
        run_git(&["fetch", "--quiet", "origin"], dest)?;
    }
    run_git(&["checkout", "--quiet", "--detach", "FETCH_HEAD"], dest)?;
    Ok(())
}

fn run_git(args: &[&str], cwd: &Path) -> Result<(), SourceError> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| SourceError::Git(format!("spawn git: {e}")))?;
    if !out.status.success() {
        return Err(SourceError::Git(format!(
            "git {:?} in {} failed: {}",
            args,
            cwd.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

pub fn ensure_git_available() -> Result<(), SourceError> {
    let out = Command::new("git")
        .arg("--version")
        .output()
        .map_err(|e| SourceError::Git(format!("git not on PATH: {e}")))?;
    if !out.status.success() {
        return Err(SourceError::Git("git --version exited non-zero".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn have_git() -> bool {
        ensure_git_available().is_ok()
    }

    #[test]
    fn clone_local_bare_repo() {
        if !have_git() {
            eprintln!("skipping: git not available");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();

        // Build a small upstream repo.
        let upstream = tmp.path().join("upstream");
        std::fs::create_dir_all(&upstream).unwrap();
        run_git(&["init", "--quiet", "--initial-branch=main"], &upstream).unwrap();
        run_git(&["config", "user.email", "t@example.com"], &upstream).unwrap();
        run_git(&["config", "user.name", "Test"], &upstream).unwrap();
        std::fs::write(upstream.join("hello.txt"), b"hi\n").unwrap();
        run_git(&["add", "hello.txt"], &upstream).unwrap();
        run_git(&["commit", "-m", "init", "--quiet"], &upstream).unwrap();
        run_git(&["tag", "v0.1.0"], &upstream).unwrap();

        let dest = tmp.path().join("clone");
        let url = format!("file://{}", upstream.display());
        clone_into(&url, "v0.1.0", &dest).unwrap();
        assert!(dest.join("hello.txt").exists());
    }
}
