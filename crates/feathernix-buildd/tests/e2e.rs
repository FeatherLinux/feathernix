//! End-to-end integration test: spin up controller + worker, submit a recipe,
//! assert the artifact lands in the repo and the index is signed.

use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use feathernix_buildd_proto::{JobInfo, JobState, SubmitJobRequest, SubmitJobResponse};

fn buildd_bin() -> &'static str {
    env!("CARGO_BIN_EXE_feathernix-buildd")
}

fn have_bwrap() -> bool {
    Command::new("bwrap")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn pick_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    listener.local_addr().unwrap().port()
}

fn wait_until_listening(url: &str, deadline_ms: u64) -> bool {
    let client = reqwest::blocking::Client::new();
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(deadline_ms) {
        if client.get(url).send().map(|r| r.status().is_success()).unwrap_or(false) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

struct Reaper(Child);

impl Drop for Reaper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn controller_routes_start_and_match_job_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let listen = format!("127.0.0.1:{}", pick_free_port());
    let base = format!("http://{listen}");
    let _controller = Reaper(
        Command::new(buildd_bin())
            .args(["controller", "--listen", &listen, "--state"])
            .arg(tmp.path())
            .stdout(Stdio::null())
            .spawn()
            .expect("spawn controller"),
    );
    assert!(wait_until_listening(&format!("{base}/healthz"), 10_000));

    let client = reqwest::blocking::Client::new();
    let response = client.get(format!("{base}/v1/jobs")).send().unwrap();
    assert!(response.status().is_success());
    let response = client.get(format!("{base}/v1/jobs/missing-job")).send().unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    assert_eq!(response.text().unwrap(), "job missing-job not found");
}

#[test]
fn submit_lease_build_publish_install_roundtrip() {
    if !have_bwrap() {
        eprintln!("skipping: bwrap not available");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let state_dir = root.join("state");
    let worker_wd = root.join("worker-wd");
    let repo_dir = root.join("repo");
    let src_dir = root.join("src");
    let recipe_dir = root.join("recipe");
    let keys_dir = root.join("keys");
    for d in [&state_dir, &worker_wd, &src_dir, &recipe_dir, &keys_dir] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::create_dir_all(repo_dir.join("packages")).unwrap();

    // Source
    std::fs::write(src_dir.join("hello.sh"), b"#!/bin/sh\necho hi from e2e\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        src_dir.join("hello.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    // Recipe
    let recipe_path = recipe_dir.join("feathernix.recipe.toml");
    let recipe_toml = format!(
        r#"
[recipe]
format_version = 1
[package]
name    = "hello"
version = "9.9.9"
release = "1"
arch    = "x86_64-linux"
summary = "e2e"
[source]
kind = "path"
path = "{}"
[build]
install = ["mkdir -p $OUT/bin && cp $SRC/hello.sh $OUT/bin/hello && chmod 755 $OUT/bin/hello"]
[dependencies]
provides = ["hello = 9.9.9"]
"#,
        src_dir.display()
    );
    std::fs::write(&recipe_path, &recipe_toml).unwrap();

    // Keypair (in-process via feathernix-sign)
    let (sk, pub_armor) =
        feathernix_sign::generate_ephemeral("e2e <e@x>").unwrap();
    let sk_path = keys_dir.join("repo.sk.asc");
    feathernix_sign::save_secret_key(&sk, &sk_path).unwrap();
    std::fs::write(keys_dir.join("repo.pub.asc"), &pub_armor).unwrap();

    // Spawn controller
    let port = pick_free_port();
    let listen = format!("127.0.0.1:{port}");
    let base = format!("http://{listen}");
    let mut controller = Command::new(buildd_bin());
    controller
        .arg("controller")
        .arg("--listen")
        .arg(&listen)
        .arg("--state")
        .arg(&state_dir)
        .arg("--publish-repo")
        .arg(&repo_dir)
        .arg("--signing-key")
        .arg(&sk_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let controller = Reaper(controller.spawn().expect("spawn controller"));
    assert!(
        wait_until_listening(&format!("{base}/healthz"), 10_000),
        "controller did not come up"
    );

    // Spawn worker
    let mut worker = Command::new(buildd_bin());
    worker
        .arg("worker")
        .arg("--controller")
        .arg(&base)
        .arg("--workdir")
        .arg(&worker_wd)
        .env("FEATHERNIX_WORKER_SIGNING_KEY", &sk_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _worker = Reaper(worker.spawn().expect("spawn worker"));

    // Submit a job
    let client = reqwest::blocking::Client::new();
    let resp: SubmitJobResponse = client
        .post(format!("{base}/v1/jobs"))
        .json(&SubmitJobRequest { recipe_toml, note: Some("e2e".into()) })
        .send()
        .expect("submit")
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    let job_id = resp.job_id;

    // Poll until terminal (max 60s).
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut final_state = JobState::Pending;
    let mut err_msg: Option<String> = None;
    while Instant::now() < deadline {
        let info: JobInfo = client
            .get(format!("{base}/v1/jobs/{job_id}"))
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap();
        if info.state.is_terminal() {
            final_state = info.state;
            err_msg = info.error;
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    assert_eq!(
        final_state,
        JobState::Succeeded,
        "job did not succeed: {:?}",
        err_msg
    );

    // Repo should now have the package + index + sig.
    let pkg = repo_dir
        .join("packages")
        .join("hello-9.9.9-1-x86_64-linux.fnpkg.7z");
    let pkg_sig = repo_dir
        .join("packages")
        .join("hello-9.9.9-1-x86_64-linux.fnpkg.7z.sig");
    let index = repo_dir.join("repodata/index.toml");
    let index_sig = repo_dir.join("repodata/index.toml.sig");
    assert!(pkg.exists(), "package missing: {}", pkg.display());
    assert!(pkg_sig.exists(), "payload .sig missing: {}", pkg_sig.display());
    assert!(index.exists(), "index missing: {}", index.display());
    assert!(index_sig.exists(), "index .sig missing");
    assert!(
        looks_like_manifest_in_archive(&pkg),
        "archive doesn't contain a manifest"
    );

    drop(controller);
}

fn looks_like_manifest_in_archive(path: &Path) -> bool {
    feathernix_archive::read_manifest(path)
        .map(|m| m.package.name == "hello" && m.package.version == "9.9.9")
        .unwrap_or(false)
}
