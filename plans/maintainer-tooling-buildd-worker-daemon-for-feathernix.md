# Maintainer tooling + buildd worker daemon for FeatherNix

## Context

FeatherNix today is consumption-only: the `fn` client can install pre-built `.fnpkg.7z` packages from HTTP repos, verify GPG signatures, and manage generations. There is **no way for a maintainer to produce those packages** — no source build, no recipe format, no signing CLI, no index generation, no publish step. The only authoring primitive that exists is `feathernix-archive::pack()` (zips a pre-built directory tree) and a test-only `sign_detached_armor()` helper in `crates/feathernix-repo/src/gpg.rs:109-143`.

FeatherLinux will be a rolling-release distro, which means continuous, repeatable rebuilds against drifting upstream sources. We need:

1. **A recipe format** describing how to fetch and build a source tree into a package.
2. **A local maintainer CLI** (`fnmaint`) for the full pipeline: clone → build (sandboxed) → pack → sign → publish.
3. **A distributed build service** (`feathernix-buildd`) modelled on Debian's buildd: a controller schedules jobs, workers pull from git, build inside bubblewrap, upload signed artifacts back, and the controller atomically rotates the repo index.

**Decisions locked in** (from clarification): single Cargo workspace (new crates, new binaries — not new repo), bubblewrap sandbox, TOML recipe format consistent with `flake.toml` and `repos/*.toml`, full stack (CLI + buildd) covered in this plan.

The shared types argument is decisive: the recipe-side `Manifest`, the publisher-side `RepositoryIndex`, the path layout in `feathernix-config::Paths`, and the GPG primitives in `feathernix-repo::gpg` are all already in this workspace. New maintainer crates depend on them directly without a coordination tax. The end-user `fn` binary stays lean because Cargo only links what it imports — maintainer crates are only depended on by the new binaries.

Outcome: a maintainer can write `feathernix.recipe.toml`, run `fnmaint build`, get a signed `.fnpkg.7z`, and either `fnmaint publish --repo /srv/repo` locally or `fnmaint submit --controller https://buildd.feathernix.org` to dispatch the build to a worker fleet. The controller persists state in SQLite, hands jobs to workers over HTTPS+token, and republishes the index when artifacts land.

## Workspace additions

Eight new crates, three new binaries. Existing crates extended in three small ways.

### New crates (all under `crates/`)

| Crate | Purpose |
|---|---|
| `feathernix-recipe` | Parse/validate `feathernix.recipe.toml`. Mirrors how `feathernix-flake` handles `flake.toml`. |
| `feathernix-source` | Fetch source: `git2`-based clone with rev pin + sha256 verify; tarball fetch via `reqwest`; local `path://`. Caches under `<cache_dir>/sources/<hash>/`. |
| `feathernix-sandbox` | Bubblewrap orchestration. Builds a `bwrap` argv from a `SandboxSpec` (read-only binds for `/fnix/store/*` build deps, fresh rw `/build`, `--unshare-all` plus optional `--share-net` for the fetch phase only). |
| `feathernix-build` | The orchestrator: recipe → source fetch → sandbox setup → run steps → stage `$OUT` → call `feathernix-archive::pack`. Mirrors `feathernix-builder`'s `Realizer` pattern but on the producer side. |
| `feathernix-sign` | Promote the test-only `make_keypair` / `sign_detached_armor` helpers from `feathernix-repo::gpg` into a public signing API. Add secret-key loading from disk and agent-style passphrase handling. |
| `feathernix-publish` | Generate `repodata/index.toml` from a directory of `.fnpkg.7z` files, sign it, atomically swap into place via tempfile + rename. Reuses `feathernix-repo::index::RepositoryIndex` for the type. |
| `feathernix-buildd-proto` | Shared types between controller and worker: `Job`, `JobStatus`, `WorkerHeartbeat`, plus the HTTP route definitions. Zero impl, just types. |
| `feathernix-buildd` | The daemon. Single binary, two modes: `feathernix-buildd controller` (HTTP server + SQLite job queue) and `feathernix-buildd worker` (polls controller, builds via `feathernix-build`, uploads artifact). |

### New binaries

- `fnmaint` — the maintainer CLI. Lives in a new `crates/feathernix-maint-cli` (separate from `feathernix-cli` so the end-user `fn` binary doesn't grow).
- `feathernix-buildd` — the daemon binary (in `crates/feathernix-buildd`).
- `feathernix-buildctl` — small ops CLI for poking the controller (submit job, list jobs, drain worker). Lives in `crates/feathernix-buildd` as a second `[[bin]]`.

### Existing crates extended

- `feathernix-archive` — already exposes `pack()`; no change.
- `feathernix-repo::gpg` — make `make_keypair`, `sign_detached_armor`, and a new `load_secret_key_from_armor` `pub` (currently test-gated). Move them into a new `signing` submodule so the verify path stays clean.
- `feathernix-repo::index::RepositoryIndex` — already has `to_pretty_string()`. Add a `from_packages(packages: &[PackageEntry], meta: IndexMeta) -> Self` constructor for the publish path. Pure struct work.
- `feathernix-config::Paths` — add `recipes_dir`, `sources_dir`, `build_workdir`, `secret_keyring_dir`. Compute under `cache_dir`/`state_dir` the same way `keyrings_dir` does at `crates/feathernix-config/src/paths.rs`.
- `feathernix-db` — add three tables for the buildd controller: `build_jobs`, `build_workers`, `build_artifacts`. New migration `0002_buildd/up.sql`. End-user installs never touch these tables; they live in the same DB only when running in controller mode (controller uses a separate DB file by default).

## Recipe format (`feathernix.recipe.toml`)

```toml
[recipe]
format_version = 1

[package]
name    = "ripgrep"
version = "14.1.0"
release = "1"
epoch   = 0
arch    = "x86_64-linux"
summary = "Fast recursive grep"
license = "MIT OR Unlicense"
url     = "https://github.com/BurntSushi/ripgrep"

[source]
kind   = "git"
url    = "https://github.com/BurntSushi/ripgrep.git"
rev    = "14.1.0"                            # tag, branch, or sha — pinned
sha256 = "abc123…"                           # verifies the post-checkout tree

# Alt: kind = "tarball" with `url` + `sha256`, or kind = "path" with `path`.

[build]
# Each step is one argv passed to /bin/sh -c inside bwrap.
# Available env: $SRC (read-only source), $OUT (rw staging), $JOBS.
prepare = []
build   = ["cargo build --release --locked"]
install = ["install -Dm755 target/release/rg $OUT/bin/rg"]

[build.env]
RUSTFLAGS = "-C target-cpu=x86-64-v2"

[dependencies]
requires       = ["glibc >= 2.17"]
provides       = ["rg = 14.1.0"]
build_requires = ["rustc >= 1.85", "cargo", "git"]
```

`build_requires` resolve through the existing repo client and are mounted read-only into the sandbox at their store paths. `requires`/`provides` flow straight into the produced `manifest.toml`. Parser lives in `feathernix-recipe::Recipe`; validation rejects unpinned git refs (no floating branches in published builds).

## Sandbox details

`feathernix-sandbox::SandboxSpec` produces a bwrap argv along these lines:

```
bwrap \
  --unshare-all \
  --share-net          # fetch phase only
  --die-with-parent \
  --new-session \
  --proc /proc --dev /dev --tmpfs /tmp \
  --ro-bind /fnix/store/<dep1> /fnix/store/<dep1> \
  --ro-bind /fnix/store/<dep2> /fnix/store/<dep2> \
  --ro-bind <source-cache>/<hash> /src \
  --bind <work>/build /build \
  --bind <work>/out /out \
  --setenv SRC /src --setenv OUT /out --setenv JOBS 8 \
  --chdir /build \
  /bin/sh -c "<step>"
```

Source fetch runs in a sibling sandbox with network enabled, into the source cache. The build sandbox itself is network-isolated. Each build step is one `bwrap` invocation; on success the next step runs against the now-mutated `/build`. After the final step, `<work>/out` is the staging area for `feathernix-archive::pack`.

We shell out to the `bwrap` binary rather than reimplementing namespaces. `feathernix-sandbox::ensure_bwrap_available()` checks at startup and returns a clear error if missing.

## Buildd protocol

HTTP+JSON between controller and worker. Worker auth: long-lived bearer token issued by controller, set in `Authorization: Bearer …`.

Routes (defined as constants in `feathernix-buildd-proto`):

- `POST   /v1/jobs` — submit recipe (body: tar.gz of recipe dir, or git URL + ref). Returns `{ job_id }`.
- `GET    /v1/jobs/{id}` — status: `pending | assigned | running | succeeded | failed | cancelled`.
- `GET    /v1/jobs?state=…` — list (for `feathernix-buildctl`).
- `POST   /v1/workers/register` — worker announces itself; returns worker_id + capabilities ack.
- `POST   /v1/workers/{id}/heartbeat` — every 30s; controller reaps workers silent >2min.
- `GET    /v1/workers/{id}/lease` — long-poll up to 30s for the next assigned job; returns recipe blob.
- `POST   /v1/workers/{id}/jobs/{job_id}/log` — streaming log append.
- `POST   /v1/workers/{id}/jobs/{job_id}/artifact` — multipart upload of `.fnpkg.7z` + `.sig`.
- `POST   /v1/workers/{id}/jobs/{job_id}/complete` — finalize with status.

Controller persists jobs in SQLite (`build_jobs` table: id, recipe_blob_path, state, assigned_worker_id, attempts, created_at, completed_at). Single SQLite writer via the existing r2d2 pool pattern in `feathernix-db`. No Postgres yet.

On artifact upload to a configured repo path, the controller invokes `feathernix-publish` to regenerate and re-sign the index in place.

## CLI surface

`fnmaint`:

- `fnmaint recipe init` — write a `feathernix.recipe.toml` stub in cwd.
- `fnmaint recipe lint [path]` — validate format, check pinning rules.
- `fnmaint build [path]` — local end-to-end: fetch source, sandbox build, pack. Produces `<name>-<version>-<release>-<arch>.fnpkg.7z` in cwd.
- `fnmaint sign <archive> --key <path>` — produce `<archive>.sig` via `feathernix-sign`.
- `fnmaint publish --repo <dir> [<archive>…]` — copy artifacts into `<dir>/packages/`, regenerate `<dir>/repodata/index.toml`, sign it. Uses `--key` or `$FNMAINT_SIGNING_KEY`.
- `fnmaint submit --controller <url> [path]` — bundle recipe dir as tar.gz, POST to `/v1/jobs`, stream logs.

`feathernix-buildd`:

- `feathernix-buildd controller --listen 0.0.0.0:7070 --state <dir> --publish-repo <dir>`
- `feathernix-buildd worker --controller <url> --token <file> --workdir <dir>`

`feathernix-buildctl`:

- `feathernix-buildctl jobs list [--state running]`
- `feathernix-buildctl jobs cancel <id>`
- `feathernix-buildctl workers list`
- `feathernix-buildctl workers drain <id>`

## Critical files to create or modify

- `crates/feathernix-recipe/src/{lib.rs,types.rs}` — `Recipe`, `SourceSpec`, `BuildSpec`, parser.
- `crates/feathernix-source/src/{lib.rs,git.rs,tarball.rs}` — fetch + cache + hash verify. Reuses `feathernix-repo::fetcher` for HTTP.
- `crates/feathernix-sandbox/src/lib.rs` — `SandboxSpec`, `SandboxRun`, `ensure_bwrap_available`.
- `crates/feathernix-build/src/lib.rs` — `Builder::run(recipe, paths) -> BuildResult`. The producer-side analogue of `feathernix-builder::Realizer`.
- `crates/feathernix-sign/src/lib.rs` — `SecretKey`, `sign_file`, re-exports for the moved primitives.
- `crates/feathernix-repo/src/gpg.rs` — split into `verify.rs` and `signing.rs`; promote test helpers to `pub`.
- `crates/feathernix-publish/src/lib.rs` — `IndexBuilder::from_dir`, `publish(dir, signing_key)`.
- `crates/feathernix-buildd-proto/src/{lib.rs,routes.rs,types.rs}`.
- `crates/feathernix-buildd/src/{main.rs,controller/mod.rs,worker/mod.rs,db.rs}` plus a `[[bin]]` for `feathernix-buildctl`.
- `crates/feathernix-maint-cli/src/{main.rs,commands/{recipe.rs,build.rs,sign.rs,publish.rs,submit.rs}}`.
- `crates/feathernix-db/migrations/0002_buildd/{up.sql,down.sql}` — three new tables; only run when the binary is `feathernix-buildd controller`.
- `crates/feathernix-config/src/paths.rs` — add `recipes_dir`, `sources_dir`, `build_workdir`, `secret_keyring_dir`.
- `Cargo.toml` (workspace) — add the eight new members and any new shared deps (`git2`, `axum`, `tower-http`, `tokio`, `reqwest` multipart feature).

## Phased rollout

Each phase ends with a runnable, demoable artifact.

1. **Recipe + source fetch** — `feathernix-recipe` + `feathernix-source`. `fnmaint recipe init`, `fnmaint recipe lint`, source clone with sha256 verify into the cache. Unit-test with a local git repo fixture.
2. **Sandbox** — `feathernix-sandbox`. Smoke test that runs `echo hello > $OUT/hi` inside bwrap and reads it back.
3. **Local build end-to-end** — `feathernix-build` + `fnmaint build`. Build a trivial "hello" recipe (just `install -Dm755 hello.sh $OUT/bin/hello`). Produces a `.fnpkg.7z` you can install with `fn install`.
4. **Sign + publish** — `feathernix-sign` + `feathernix-publish` + `fnmaint sign` + `fnmaint publish`. Round-trip: produce a signed package, publish to a local repo dir, `fn install` it through the verified path.
5. **buildd proto + skeleton** — `feathernix-buildd-proto` types, controller boots with SQLite, accepts `POST /v1/jobs`, persists, returns id. Worker registers + heartbeats but does not yet build.
6. **buildd build flow** — worker leases a job, invokes `feathernix-build`, uploads artifact + sig + log. Controller updates state.
7. **Auto-publish on artifact land** — controller invokes `feathernix-publish` against the configured repo dir on each successful job. End-to-end demo: `fnmaint submit` → wait → `fn refresh && fn install`.
8. **Rolling-release ergonomics** — `feathernix-buildctl jobs rebuild --recipe <name>` to trigger a rebuild against a moving upstream tag. Defer the full cron/watcher to a later plan; the API hook is in place.

## Verification

End-to-end smoke (with `FEATHERNIX_ROOT=$PWD/testroot` and a local bwrap install):

```
# Phase 3
fnmaint recipe init
$EDITOR feathernix.recipe.toml
fnmaint build                          # → hello-0.1.0-1-x86_64-linux.fnpkg.7z

# Phase 4
fnmaint sign hello-*.fnpkg.7z --key ~/.config/feathernix/signing/dev.asc
fnmaint publish --repo /tmp/devrepo --key ~/.config/feathernix/signing/dev.asc
fn repo add devrepo file:///tmp/devrepo --gpg-key-url file://…/dev.pub.asc
fn repo refresh                        # signed index verifies
fn install hello                       # signed payload verifies

# Phase 6
feathernix-buildd controller --listen 127.0.0.1:7070 --state /tmp/buildd \
                             --publish-repo /tmp/devrepo &
feathernix-buildd worker --controller http://127.0.0.1:7070 --token /tmp/token \
                          --workdir /tmp/worker-wd &
fnmaint submit --controller http://127.0.0.1:7070
feathernix-buildctl jobs list          # shows job → succeeded
fn repo refresh && fn install hello    # served via the buildd-produced artifact
```

Test layers:

- Per-crate unit tests as in existing crates: `feathernix-recipe` round-trips a fixture; `feathernix-source` clones from a local bare repo fixture; `feathernix-sandbox` smoke-tests `bwrap --version` then a no-op run; `feathernix-build` builds the trivial hello recipe in tempdir.
- Integration test at workspace root: extends the pattern in `crates/feathernix-cli/tests/integration_test.rs`. Spawns controller+worker as subprocesses against a `tempdir` state, submits a recipe, asserts artifact lands and index re-publishes correctly.
- The existing `cargo test --workspace` stays green — all changes to consumer-side crates are additive.

## Deliberately out of scope (follow-up plans)

- Reproducibility: bit-exact rebuilds, source-date-epoch wiring, deterministic compression. Recipe spec leaves room for it.
- Multi-arch: cross-build support and per-arch worker pools.
- A web UI for the controller (buildctl + JSON output covers ops needs initially).
- Source rev watcher (cron-driven rebuild against `HEAD` of an upstream).
- Postgres backend for the controller — SQLite is sufficient until job volume demands more.
- Multi-key keyrings, key rotation flows (already deferred by the verification plan).
