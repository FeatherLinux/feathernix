use std::path::{Path, PathBuf};
use std::process::Command;

use feathernix_archive::{Manifest, pack};
use feathernix_repo::{IndexMeta, PackageEntry, RepositoryIndex, sha256_hex};

use pgp::composed::{
    ArmorOptions, DetachedSignature, EncryptionCaps, KeyType, SecretKeyParamsBuilder,
    SignedPublicKey, SignedSecretKey, SubkeyParamsBuilder,
};
use pgp::crypto::hash::HashAlgorithm;
use pgp::types::Password;
use rand::rngs::OsRng;

fn fn_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fn"))
}

fn run(cmd: &mut Command, env_root: &Path) -> (bool, String, String) {
    let out = cmd
        .env("FEATHERNIX_ROOT", env_root)
        .env("NO_COLOR", "1")
        .output()
        .expect("spawn fn");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn build_package(workdir: &Path, name: &str, version: &str, bin_payload: &[u8]) -> PathBuf {
    let src = workdir.join(format!("src-{name}-{version}"));
    std::fs::create_dir_all(src.join("payload/bin")).unwrap();
    let manifest = Manifest::from_str(&format!(
        r#"
[package]
name = "{name}"
version = "{version}"
release = "1"
arch = "x86_64-linux"
summary = "fixture {name}"
"#
    ))
    .unwrap();
    std::fs::write(src.join("manifest.toml"), manifest.to_pretty_string().unwrap()).unwrap();
    std::fs::write(src.join(format!("payload/bin/{name}")), bin_payload).unwrap();
    let archive = workdir.join(format!("{name}-{version}-1-x86_64-linux.fnpkg.7z"));
    pack(&src, &archive).unwrap();
    archive
}

fn make_signing_keypair() -> (SignedSecretKey, Vec<u8>) {
    let mut signkey = SubkeyParamsBuilder::default();
    signkey
        .key_type(KeyType::Ed25519Legacy)
        .can_sign(true)
        .can_encrypt(EncryptionCaps::None)
        .can_authenticate(false);
    let mut key_params = SecretKeyParamsBuilder::default();
    key_params
        .key_type(KeyType::Ed25519Legacy)
        .can_certify(true)
        .can_sign(true)
        .can_encrypt(EncryptionCaps::None)
        .primary_user_id("FeatherNix Integration Test <it@example.com>".into())
        .subkeys(vec![signkey.build().unwrap()]);
    let params = key_params.build().unwrap();
    let sk = params.generate(OsRng).unwrap();
    let pk = SignedPublicKey::from(sk.clone());
    let mut armor = Vec::new();
    pk.to_armored_writer(&mut armor, ArmorOptions::default())
        .unwrap();
    (sk, armor)
}

fn sign_detached(data: &[u8], sk: &SignedSecretKey) -> Vec<u8> {
    let sig = DetachedSignature::sign_binary_data(
        OsRng,
        &sk.primary_key,
        &Password::empty(),
        HashAlgorithm::Sha256,
        data,
    )
    .unwrap();
    sig.to_armored_bytes(ArmorOptions::default()).unwrap()
}

fn build_signed_sample_repo(workdir: &Path) -> (PathBuf, PathBuf) {
    let repo_root = build_sample_repo(workdir);
    let (sk, pk_armor) = make_signing_keypair();
    let key_path = workdir.join("devrepo.asc");
    std::fs::write(&key_path, &pk_armor).unwrap();

    let index_bytes = std::fs::read(repo_root.join("repodata/index.toml")).unwrap();
    std::fs::write(
        repo_root.join("repodata/index.toml.sig"),
        sign_detached(&index_bytes, &sk),
    )
    .unwrap();

    let packages_dir = repo_root.join("packages");
    for entry in std::fs::read_dir(&packages_dir).unwrap() {
        let entry = entry.unwrap();
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) == Some("7z") {
            let bytes = std::fs::read(&p).unwrap();
            let sig_bytes = sign_detached(&bytes, &sk);
            let mut sig_path = p.clone();
            sig_path.as_mut_os_string().push(".sig");
            std::fs::write(&sig_path, sig_bytes).unwrap();
        }
    }
    (repo_root, key_path)
}

fn build_sample_repo(workdir: &Path) -> PathBuf {
    let repo_root = workdir.join("devrepo");
    let packages_dir = repo_root.join("packages");
    let repodata_dir = repo_root.join("repodata");
    std::fs::create_dir_all(&packages_dir).unwrap();
    std::fs::create_dir_all(&repodata_dir).unwrap();

    let mut entries = Vec::new();
    for (name, payload) in [
        ("hello", b"#!/bin/sh\necho hello\n".as_slice()),
        ("ripgrep", b"#!/bin/sh\necho rg\n".as_slice()),
        ("fd", b"#!/bin/sh\necho fd\n".as_slice()),
    ] {
        let archive = build_package(workdir, name, "1.0.0", payload);
        let bytes = std::fs::read(&archive).unwrap();
        let payload_rel = format!("packages/{}", archive.file_name().unwrap().to_string_lossy());
        std::fs::copy(&archive, packages_dir.join(archive.file_name().unwrap())).unwrap();
        entries.push(PackageEntry {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            release: "1".to_string(),
            epoch: 0,
            arch: "x86_64-linux".to_string(),
            payload_path: payload_rel,
            payload_sha256: sha256_hex(&bytes),
            payload_size: bytes.len() as i64,
            summary: Some(format!("fixture {name}")),
            dependencies: vec![],
            provides: vec![format!("{name} = 1.0.0")],
        });
    }

    let index = RepositoryIndex {
        index: IndexMeta {
            format_version: 1,
            generated_at: "2026-05-21T10:00:00Z".to_string(),
            repository: "devrepo".to_string(),
        },
        packages: entries,
    };
    std::fs::write(
        repodata_dir.join("index.toml"),
        index.to_pretty_string().unwrap(),
    )
    .unwrap();
    repo_root
}

#[test]
fn full_install_rollback_flow() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    let env_root = tmp.path().join("fnroot");
    std::fs::create_dir_all(&work).unwrap();

    let repo_root = build_sample_repo(&work);
    let base_url = format!("file://{}", repo_root.display());

    // fn repo add devrepo file:///.../devrepo
    let (ok, _out, err) = run(
        fn_bin().args(["repo", "add", "devrepo", &base_url]),
        &env_root,
    );
    assert!(ok, "repo add failed:\nstderr:\n{err}");

    // fn repo refresh
    let (ok, out, err) = run(fn_bin().arg("repo").arg("refresh"), &env_root);
    assert!(ok, "repo refresh failed:\nstderr:\n{err}");
    assert!(out.contains("devrepo"), "expected refresh report:\n{out}");
    assert!(out.contains("3 packages indexed"), "got:\n{out}");

    // fn repo list
    let (ok, out, _) = run(fn_bin().arg("repo").arg("list"), &env_root);
    assert!(ok);
    assert!(out.contains("devrepo"));

    // fn install hello
    let (ok, out, err) = run(fn_bin().args(["install", "hello"]), &env_root);
    assert!(ok, "install hello failed:\nstdout:\n{out}\nstderr:\n{err}");
    assert!(out.contains("installed hello-1.0.0"));
    assert!(out.contains("generation 1 active"));

    // fn install ripgrep fd
    let (ok, out, err) = run(fn_bin().args(["install", "ripgrep", "fd"]), &env_root);
    assert!(ok, "install ripgrep fd failed:\nstdout:\n{out}\nstderr:\n{err}");
    assert!(out.contains("generation 2 active"));

    // fn generation list
    let (ok, out, _) = run(fn_bin().args(["generation", "list"]), &env_root);
    assert!(ok);
    assert!(out.contains("current"), "list should mark current:\n{out}");
    let gen_lines: Vec<&str> = out
        .lines()
        .filter(|l| l.split_whitespace().next() == Some("1") || l.split_whitespace().next() == Some("2"))
        .collect();
    assert_eq!(gen_lines.len(), 2, "expected two generations:\n{out}");
    assert!(
        gen_lines.iter().any(|l| l.contains("current")),
        "expected one row to be current:\n{out}"
    );

    // Verify profile current symlink contains all 3 binaries
    let profile = env_root.join("var/lib/feathernix/profiles/system");
    let current = profile.join("current");
    assert!(current.symlink_metadata().unwrap().file_type().is_symlink());
    assert!(profile.join("generations/2/bin/hello").exists());
    assert!(profile.join("generations/2/bin/ripgrep").exists());
    assert!(profile.join("generations/2/bin/fd").exists());

    // fn generation rollback → gen 1
    let (ok, out, err) = run(fn_bin().args(["generation", "rollback"]), &env_root);
    assert!(ok, "rollback failed:\nstderr:\n{err}");
    assert!(out.contains("generation 1"));
    let target = current.read_link().unwrap();
    assert!(
        target.ends_with("1"),
        "current should point at gen 1, got {target:?}"
    );
    // After rollback gen 1 only had hello
    assert!(profile.join("generations/1/bin/hello").exists());
    assert!(!profile.join("generations/1/bin/ripgrep").exists());

    // fn store list
    let (ok, out, _) = run(fn_bin().args(["store", "list"]), &env_root);
    assert!(ok);
    assert!(out.contains("hello-1.0.0"));
    assert!(out.contains("ripgrep-1.0.0"));
}

#[test]
fn flake_init_and_show() {
    let tmp = tempfile::tempdir().unwrap();
    let env_root = tmp.path().join("fnroot");
    let flake_root = tmp.path().join("flake-root");
    std::fs::create_dir_all(&flake_root).unwrap();

    let (ok, _out, err) = run(
        fn_bin()
            .args(["flake", "init", "--root"])
            .arg(&flake_root)
            .args(["--description", "test"]),
        &env_root,
    );
    assert!(ok, "flake init failed:\nstderr:\n{err}");
    assert!(flake_root.join("flake.toml").exists());

    let (ok, out, err) = run(
        fn_bin().args(["flake", "show", "--root"]).arg(&flake_root),
        &env_root,
    );
    assert!(ok, "flake show failed:\nstderr:\n{err}");
    assert!(out.contains("test"));
    assert!(out.contains("feathernix-stable"));
}

#[test]
fn signed_repo_refresh_install_and_tamper_detection() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    let env_root = tmp.path().join("fnroot");
    std::fs::create_dir_all(&work).unwrap();

    let (repo_root, key_path) = build_signed_sample_repo(&work);
    let base_url = format!("file://{}", repo_root.display());
    let key_url = format!("file://{}", key_path.display());

    // add with --gpg-key-url --signature-required
    let (ok, out, err) = run(
        fn_bin().args([
            "repo",
            "add",
            "devrepo",
            &base_url,
            "--gpg-key-url",
            &key_url,
            "--signature-required",
        ]),
        &env_root,
    );
    assert!(ok, "repo add failed:\nstdout:\n{out}\nstderr:\n{err}");
    assert!(
        out.contains("signed by"),
        "expected fingerprint in output:\n{out}"
    );
    let keyring_path = env_root.join("var/cache/feathernix/keyrings/devrepo.asc");
    assert!(keyring_path.exists(), "key should have been TOFU-cached");
    let fpr_path = env_root.join("var/cache/feathernix/keyrings/devrepo.fpr");
    assert!(fpr_path.exists(), "fingerprint should have been recorded");

    // refresh again — uses cache
    let (ok, out, _) = run(fn_bin().args(["repo", "refresh"]), &env_root);
    assert!(ok);
    assert!(out.contains("signed by"));

    // install hello — payload sig verified
    let (ok, out, err) = run(fn_bin().args(["install", "hello"]), &env_root);
    assert!(ok, "install failed:\nstdout:\n{out}\nstderr:\n{err}");
    assert!(
        out.contains("signature verified"),
        "expected verified note:\n{out}"
    );

    // Tamper: append garbage to the published index, blow away its sig too if we want a
    // signature-missing test — but we want signature-INVALID, so leave the .sig alone.
    let index_path = repo_root.join("repodata/index.toml");
    let mut tampered = std::fs::read(&index_path).unwrap();
    tampered.extend_from_slice(b"\n# tampered\n");
    std::fs::write(&index_path, tampered).unwrap();

    let (ok, _out, err) = run(fn_bin().args(["repo", "refresh"]), &env_root);
    assert!(!ok, "refresh should fail after tampering");
    assert!(
        err.contains("signature invalid") || err.contains("SignatureInvalid"),
        "expected signature-invalid error, got stderr:\n{err}"
    );
}

#[test]
fn signed_repo_missing_signature_with_required_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    let env_root = tmp.path().join("fnroot");
    std::fs::create_dir_all(&work).unwrap();

    let (repo_root, key_path) = build_signed_sample_repo(&work);
    // delete the index sig
    std::fs::remove_file(repo_root.join("repodata/index.toml.sig")).unwrap();
    let base_url = format!("file://{}", repo_root.display());
    let key_url = format!("file://{}", key_path.display());

    let (ok, _out, err) = run(
        fn_bin().args([
            "repo",
            "add",
            "devrepo",
            &base_url,
            "--gpg-key-url",
            &key_url,
            "--signature-required",
        ]),
        &env_root,
    );
    assert!(!ok, "expected add+refresh to fail with required sig missing");
    assert!(
        err.contains("signature missing") || err.contains("SignatureMissing"),
        "stderr:\n{err}"
    );
}

#[test]
fn signed_repo_missing_signature_optional_passes() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    let env_root = tmp.path().join("fnroot");
    std::fs::create_dir_all(&work).unwrap();

    let (repo_root, key_path) = build_signed_sample_repo(&work);
    std::fs::remove_file(repo_root.join("repodata/index.toml.sig")).unwrap();
    let base_url = format!("file://{}", repo_root.display());
    let key_url = format!("file://{}", key_path.display());

    // No --signature-required → best-effort
    let (ok, out, err) = run(
        fn_bin().args([
            "repo",
            "add",
            "devrepo",
            &base_url,
            "--gpg-key-url",
            &key_url,
        ]),
        &env_root,
    );
    assert!(ok, "add should succeed without sig when not required:\nstdout:\n{out}\nstderr:\n{err}");
    assert!(out.contains("sig missing"));
}

#[test]
fn idempotent_reinstall_does_not_duplicate_store_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    let env_root = tmp.path().join("fnroot");
    std::fs::create_dir_all(&work).unwrap();
    let repo_root = build_sample_repo(&work);
    let base_url = format!("file://{}", repo_root.display());

    let (ok, _o, _e) = run(
        fn_bin().args(["repo", "add", "devrepo", &base_url]),
        &env_root,
    );
    assert!(ok);

    let (ok, _o, _e) = run(fn_bin().args(["install", "hello"]), &env_root);
    assert!(ok);
    let (ok, _o, _e) = run(fn_bin().args(["install", "hello"]), &env_root);
    assert!(ok);

    let (ok, out, _) = run(fn_bin().args(["store", "list"]), &env_root);
    assert!(ok);
    let hello_lines: Vec<&str> = out.lines().filter(|l| l.contains("hello-1.0.0")).collect();
    assert_eq!(
        hello_lines.len(),
        1,
        "expected one store path for hello, got: {hello_lines:?}\nfull:\n{out}"
    );

    let (ok, out, _) = run(fn_bin().args(["generation", "list"]), &env_root);
    assert!(ok);
    let gen_count = out
        .lines()
        .filter(|l| {
            let first = l.split_whitespace().next();
            first == Some("1") || first == Some("2")
        })
        .count();
    assert_eq!(gen_count, 2, "expected two generations:\n{out}");
}
