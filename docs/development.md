# Development

## Toolchain

- Rust **1.95+** (`edition = "2024"`) — set in the workspace `Cargo.toml`.
- Linux with user namespaces enabled (kernel default on every modern
  distro).
- For `fnmaint build` and the buildd worker:
  - `bwrap` (Bubblewrap)
  - `git`
  - `tar`

The consumer-side `fn` binary does **not** need `bwrap`.

## Build

```sh
cargo build --workspace
cargo build --workspace --release
```

Binaries land in `target/debug/` or `target/release/`. The interesting ones
are `fn`, `fnmaint`, `feathernix-buildd`, and `feathernix-buildctl`.

## Test

```sh
cargo test --workspace
cargo test -p feathernix-repo       # one crate
cargo test -p feathernix-cli --test integration_test
```

### Sandbox-dependent tests

Any test that exercises real sandboxing must guard with `have_bwrap()` and
**skip** when missing — they must not fail. This keeps the suite green on
minimal CI containers without paving over a real misconfiguration.

```rust
if !have_bwrap() {
    return;
}
```

### Integration tests

The workspace-level end-to-end harness lives at
`crates/feathernix-cli/tests/integration_test.rs`. It:

1. Builds a fixture repo in a tempdir.
2. Generates an ephemeral signing keypair and signs index + payloads.
3. Spins up a local HTTP server.
4. Runs `fn` as a subprocess against `FEATHERNIX_ROOT=<tempdir>`.
5. Asserts the full flow — refresh, install, generation rollback.

New end-to-end scenarios belong here. Per-crate unit tests cover narrower
behavior:

- `feathernix-repo` uses `httpmock` for HTTP fixtures.
- `feathernix-db` uses in-memory SQLite via Diesel.
- `feathernix-archive`, `feathernix-store`, `feathernix-builder` use
  `tempfile` for working directories.

## Running locally

Always use `$FEATHERNIX_ROOT` so nothing touches the real system:

```sh
export FEATHERNIX_ROOT=$PWD/testroot
rm -rf "$FEATHERNIX_ROOT"

# Serve a local fixture repo
(cd tests/fixtures/sample-repo && python3 -m http.server 8080 &)

fn repo add devrepo http://localhost:8080/devrepo
fn repo refresh
fn install hello
sqlite3 "$FEATHERNIX_ROOT/var/lib/feathernix/db.sqlite" \
    "SELECT name, version FROM installed_packages"
```

## Inspecting state

```sh
sqlite3 "$FEATHERNIX_ROOT/var/lib/feathernix/db.sqlite" .schema
sqlite3 "$FEATHERNIX_ROOT/var/lib/feathernix/db.sqlite" \
    "SELECT generation, is_current FROM generations ORDER BY generation"

ls "$FEATHERNIX_ROOT/fnix/store"
ls -l "$FEATHERNIX_ROOT/var/lib/feathernix/profiles/default"
```

## Logging

All crates use `tracing`. The CLI binaries set up
`tracing_subscriber::EnvFilter`, so the standard env var works:

```sh
RUST_LOG=info fn install hello
RUST_LOG=feathernix_repo=debug,feathernix_store=trace fn install hello
```

## Dependency landmines

- **SQLite versions must match across the workspace.** `feathernix-buildd`
  pins `rusqlite = "0.43"`, which pulls `libsqlite3-sys = 0.40`, the same
  version Diesel uses on the consumer side. Cargo's `links = "sqlite3"`
  rule forbids two versions of the same native library in one binary. When
  bumping either crate, look up the exact `libsqlite3-sys` version each
  pulls in before running `cargo build`.

## Don't do these

- Don't commit `testroot/` or `target/` — both are in `.gitignore`.
- Don't reach into `rusqlite`/`diesel` from a crate other than
  `feathernix-db` (consumer side) or `feathernix-buildd` (its own DB).
- Don't shell out to `7z`, `gpg`, or anything else where a pure-Rust crate
  is already in the workspace. `bwrap` is the one intentional exception.
- Don't add `--no-verify` or `--no-gpg-sign` workarounds in tests. If a
  signature check is in the way, fix the fixture, not the verifier.
