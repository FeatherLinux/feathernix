# FeatherNix — GPG signature verification

## Context

FeatherNix v0.1 ships repository metadata and binary packages over HTTP but does not verify their authenticity. The repo TOML schema already has a `[repository.signing]` block (`gpg_check`, `gpg_key_url`, `signature_required`), and the `repositories` SQLite table already has matching columns — but nothing enforces them. The refresh path blindly trusts whatever bytes come back from the repo URL, and the realizer trusts whatever payload it downloads (it does check sha256 against the index, but a tampered index would just record an attacker's hash).

This change makes those config fields actually do something. After this, an attacker who controls the network between a user and `repo.feathernix.org` cannot serve a modified `index.toml` or a swapped `.fnpkg.7z` to a user whose repo file declares `gpg_check = true`, because the locally-cached public key won't match the forged signature.

Design choices already made:
- **Library**: the pure-Rust `pgp` crate (consistent with `sevenz-rust2` — no `gpg` CLI dependency, no Nettle/OpenSSL).
- **Scope**: both index AND every payload are signed (matches RPM/DNF). The index's sha256 already chains trust to payloads, but verifying payload sigs independently catches a compromised mirror that swaps a payload between index publication and a user's install.
- **Trust model**: TOFU — on first refresh with `gpg_check = true`, fetch the key from `gpg_key_url`, store under `<cache_dir>/keyrings/<repo>.asc`, log the fingerprint. Subsequent refreshes use the cached key; we never silently re-fetch from the URL (rotating keys is a deliberate, future `fn repo trust` command, out of scope here).

## Repository signature layout

```
<base_url>/repodata/index.toml
<base_url>/repodata/index.toml.sig          # detached, ASCII-armored OpenPGP signature
<base_url>/packages/<pkg>.fnpkg.7z
<base_url>/packages/<pkg>.fnpkg.7z.sig       # detached signature, sibling
```

Both `.sig` files are produced offline by the repo publisher (`gpg --detach-sign --armor`). FeatherNix does not yet ship a `fn archive sign` command — that's a follow-up; users today can use `gpg` directly.

## Local key cache layout

```
<cache_dir>/keyrings/<repo_name>.asc        # the trusted public key for this repo
<cache_dir>/keyrings/<repo_name>.fpr        # fingerprint hex (sanity check / display)
```

Under `$FEATHERNIX_ROOT=$ROOT`, cache_dir = `$ROOT/var/cache/feathernix`. The keyrings directory is created on demand and added to `Paths`.

## Policy semantics

Effective behavior is the product of `gpg_check` × `signature_required`:

| gpg_check | signature_required | Behavior |
|---|---|---|
| false | (any) | No verification. Today's behavior. |
| true | false | Best-effort: if a `.sig` is present, verify it; if missing, warn and continue. Fetch & cache the key on first encounter. |
| true | true | Mandatory: missing or invalid `.sig` is a hard error. Index refresh aborts; payload install aborts. |

## Files to create / modify

### New

- `crates/feathernix-repo/src/gpg.rs` — the verifier module:
  - `pub struct Keyring(pgp::composed::SignedPublicKey)` — single trusted key per repo (multi-key armors get the first, with a warning)
  - `pub fn from_armored_bytes(bytes: &[u8]) -> RepoResult<Keyring>`
  - `pub fn fingerprint_hex(&self) -> String`
  - `pub fn verify_detached(&self, data: &[u8], sig_armor: &[u8]) -> RepoResult<()>`
  - `pub fn load_or_fetch(fetcher: &Fetcher, repo: &Repository, paths: &Paths) -> RepoResult<Keyring>` — TOFU: read cache if present; else fetch from `repo.gpg_key_url`, persist to cache, return.

### Modified

- `crates/feathernix-config/src/paths.rs`
  - Add `pub keyrings_dir: PathBuf` to `Paths`. Compute as `cache_dir.join("keyrings")` in both `system()` and `rooted()`. Add to `ensure_dirs()` list.

- `crates/feathernix-repo/src/lib.rs`
  - Re-export `gpg::Keyring`.
  - New error variants:
    - `RepoError::SignatureMissing { url }`
    - `RepoError::SignatureInvalid { url, reason: String }`
    - `RepoError::KeyloadFailed { repo: String, reason: String }`
    - `RepoError::GpgCheckButNoKeyUrl { repo: String }`

- `crates/feathernix-repo/src/sync.rs::refresh`
  - After fetching `repodata/index.toml`, if the persisted `Repository` row has `gpg_check = true`:
    1. `Keyring::load_or_fetch(&self.fetcher, repo, paths)` — needs Paths plumbed through (see signature change below).
    2. Try `self.fetcher.fetch_bytes(&repo.base_url, "repodata/index.toml.sig")`.
       - On success → `keyring.verify_detached(&index_bytes, &sig_bytes)`. On verify failure → return `SignatureInvalid`.
       - On 404/missing → if `repo.signature_required`, return `SignatureMissing`; otherwise `tracing::warn!` and continue.
    3. Only on success do we upsert into `packages_available`.
  - `RepoSync::new` (and downstream `refresh`/`refresh_all`) gains a `paths: Paths` field so it can write to `<cache_dir>/keyrings/`. Update all call sites.

- `crates/feathernix-builder/src/lib.rs::RepositoryRealizer`
  - Constructor takes a fully-loaded `Repository` row lookup (or we pass `Paths` plus the resolved `repo_gpg_check` / `repo_signature_required` / `repo_gpg_key_url` on the resolved package — see below).
  - Simpler: extend `ResolvedPackage` (in `resolver.rs`) with `pub repo_gpg_check: bool, pub repo_signature_required: bool, pub repo_gpg_key_url: Option<String>` and select them in the join. Then `realize()` checks the flags without an extra DB hop.
  - In `realize()`, after `fetch_to_file` for the payload, when `pkg.repo_gpg_check`:
    1. `Keyring::load_or_fetch(&self.fetcher, &fake_repo_view, &self.paths)` — or refactor `load_or_fetch` to take `(repo_name, gpg_key_url, keyrings_dir)` directly. The fn signature should be flag-free.
    2. Try `fetcher.fetch_bytes(repo_base_url, format!("{payload_path}.sig"))`.
    3. Same missing/invalid policy as the index check.
  - Realizer constructor gains a `paths: Paths` so it can locate the keyrings directory; update `commands::install::run` to pass `ctx.paths.clone()`.

- `crates/feathernix-repo/src/resolver.rs`
  - Extend the `Row` tuple and `ResolvedPackage` struct with the three GPG fields from `repositories`. Update the SELECT.

- `crates/feathernix-cli/src/commands/install.rs` & `repo.rs::refresh/add`
  - No behavior change beyond passing `ctx.paths.clone()` into `RepoSync::new` and `RepositoryRealizer::new`.

## Dependency additions

- `crates/feathernix-repo/Cargo.toml`: add `pgp = "0.14"` (latest at the time of writing; pin to whatever resolves on first `cargo build`).
- For the test helper that generates an ephemeral signing key: add `pgp` + `rand` to `[dev-dependencies]` (rand for the key generator's RNG). Same for `feathernix-builder` dev-deps if its integration test uses signed fixtures.

## Tests

### Unit (in `feathernix-repo`)

- `gpg::tests::ephemeral_key_signs_and_verifies` — generate a `SecretKeyParamsBuilder`-produced keypair, sign a known byte buffer, round-trip the armored public key through `Keyring::from_armored_bytes`, verify the detached signature succeeds.
- `gpg::tests::wrong_key_fails` — sign with key A, verify with key B → `SignatureInvalid`.
- `gpg::tests::tampered_data_fails` — sign data D, flip one byte, verify → `SignatureInvalid`.
- `gpg::tests::tofu_caches_key` — temp dir as cache root; first `load_or_fetch` writes `<repo>.asc`; second `load_or_fetch` with the URL pointing at `/dev/null` still succeeds (proving cache, not URL, is the source).

### Integration (extend `crates/feathernix-cli/tests/integration_test.rs`)

- Build a fixture repo as before, then **also**: generate a keypair, write the armored public key to `<work>/keys/devrepo.asc`, sign `repodata/index.toml` and each `.fnpkg.7z`, drop the sigs as `.sig` siblings.
- `fn repo add devrepo file://.../devrepo` — already works.
- Manually write a user repo TOML with `[repository.signing] gpg_check = true, gpg_key_url = "file://.../keys/devrepo.asc", signature_required = true` — OR add a `--gpg-key-url` flag to `fn repo add` (recommend the latter for ergonomics; small CLI change).
- `fn repo refresh` should succeed; assert the keyring file exists at `$FEATHERNIX_ROOT/var/cache/feathernix/keyrings/devrepo.asc`.
- `fn install hello` should succeed (payload sig present).
- Negative test: delete `index.toml.sig`, `fn repo refresh` returns nonzero with a "signature missing" message in stderr.

### Existing tests

- All current passing tests stay green. Repos without `gpg_check` see no behavior change.

## Verification

```
export FEATHERNIX_ROOT=$PWD/testroot && rm -rf "$FEATHERNIX_ROOT"

# Run all unit + integration tests
cargo test --workspace

# Manual: signed fixture
# (the integration test does this automatically; for a manual run use the
# helper it exposes, or `gpg --detach-sign --armor` against a real fixture.)
fn repo refresh             # should print "✓ devrepo: ... verified with key <fpr>"
fn install hello            # should print "verified payload signature"

# Tamper detection
echo broken >> "$REPO/repodata/index.toml"
fn repo refresh             # exit 1, stderr "signature invalid for repodata/index.toml"
```

DB-level assertion:

```
sqlite3 "$FEATHERNIX_ROOT/var/lib/feathernix/db.sqlite" \
  "SELECT name, gpg_check, signature_required FROM repositories WHERE name='devrepo'"
# → devrepo|1|1
```

## Deliberately out of scope (future)

- `fn repo trust <name> --fingerprint <fpr>` for explicit key rotation.
- `fn archive sign` (built-in signer for package publishers).
- Multi-key keyrings per repo (rotate-by-overlap).
- Subkey expiry / revocation checks beyond what `pgp` enforces by default.
- Signature on `flake.lock.toml` inputs.
