# Architecture

FeatherNix is a single Cargo workspace split into three roles: a consumer
side (the `fn` binary and its supporting libraries), a producer side
(maintainer tooling for building and signing packages), and a distributed
build daemon. They share types directly via path dependencies — there is no
crates.io coordination tax.

## Crates

### Consumer-side

| Crate | Responsibility |
|---|---|
| `feathernix-cli` | The `fn` binary. Clap subcommands, output formatting, ties together everything below. |
| `feathernix-config` | Loads `feathernix.toml` + every `repos/*.toml`. Owns `Paths`, the struct that resolves all on-disk locations from `$FEATHERNIX_ROOT`. |
| `feathernix-db` | The Diesel layer: schema, migrations, models, r2d2 pool. **All consumer DB access funnels through here.** |
| `feathernix-repo` | HTTP repo client: fetcher, sync, greedy resolver, GPG verification (`gpg.rs`). |
| `feathernix-archive` | `sevenz-rust2` wrappers; pack/unpack and `manifest.toml` extraction from `.fnpkg.7z`. |
| `feathernix-store` | `/fnix/store` path layout, atomic install, profile symlink trees. |
| `feathernix-flake` | `flake.toml` + `flake.lock.toml` parsing and input resolution. |
| `feathernix-builder` | Producer-agnostic install orchestrator. `Realizer` trait + `RepositoryRealizer` impl (download → verify → extract → register). |

### Producer-side

| Crate | Responsibility |
|---|---|
| `feathernix-maint-cli` | The `fnmaint` binary. Recipe → build → sign → publish/submit. |
| `feathernix-recipe` | Parses and validates `feathernix.recipe.toml`. Sibling of `feathernix-flake` on the producer side. |
| `feathernix-source` | Fetches source: `git` clone with rev pin + sha256 verify, tarball over HTTP, local `path://`. Caches under `<cache_dir>/sources/<hash>/`. |
| `feathernix-sandbox` | Bubblewrap orchestration. Builds a `bwrap` argv from a `SandboxSpec`. |
| `feathernix-build` | The producer-side orchestrator: recipe → source fetch → sandbox setup → run steps → stage `$OUT` → `feathernix-archive::pack`. |
| `feathernix-sign` | Public signing API: load secret keys, sign archives and indices. Built on the same `pgp` primitives the verifier uses. |
| `feathernix-publish` | Generate `repodata/index.toml` from a directory of `.fnpkg.7z` files, sign it, atomically swap into place. |

### Buildd

| Crate | Responsibility |
|---|---|
| `feathernix-buildd-proto` | Shared types between controller and worker (`Job`, `JobStatus`, `WorkerHeartbeat`) plus HTTP route constants. No impl. |
| `feathernix-buildd` | The daemon binary. Two modes: `controller` (axum HTTP server + SQLite job queue) and `worker` (polls controller, runs `feathernix-build`, uploads artifact). Also ships `feathernix-buildctl` as a second `[[bin]]`. |

## Data flow: `fn install <pkg>`

This is the full consumer pipeline. Every other consumer command is a subset.

1. `cli::commands::install::run` parses targets.
2. `config::Config::load` reads `feathernix.toml` + all `repos/*.toml`.
3. `db::Db::open` opens the r2d2 pool and runs embedded migrations.
4. `repo::RepoSync::refresh_if_stale` — for each enabled repo past its
   `metadata_expire_seconds`, GETs `repodata/index.toml`, verifies signature
   if required, sha256-checks the body, upserts `packages_available`.
5. `repo::Resolver::resolve` — greedy resolver (lowest priority × highest
   version) returning a `Vec<ResolvedPackage>` with transitive deps.
6. For each resolved package:
   - `repo::Fetcher::download` to `<cache>/payloads/<sha256>.fnpkg.7z`,
     sha256-verified.
   - `store::StorePath::compute` builds `/fnix/store/<40hex>-<name>-<ver>/`.
   - `archive::extract` decompresses via `sevenz_rust2`.
   - `store::install` atomic-renames from temp, inserts `store_paths` and
     `installed_packages` rows.
7. `db::generations::create_next` in a transaction: bump
   `MAX(generation)+1`, set `is_current` flags, insert `generation_entries`.
8. `store::profile::update_current_symlink` rebuilds
   `/var/lib/feathernix/profiles/<profile>/generations/<n>/` and swings
   `current → generations/<n>`.

## Data flow: `fnmaint build`

1. `feathernix-recipe::Recipe::load` parses and validates
   `feathernix.recipe.toml`. Unpinned git refs are rejected.
2. `feathernix-source` fetches the source into `<cache>/sources/<hash>/` and
   verifies its sha256.
3. `feathernix-build` resolves `build_requires` through the existing repo
   client and collects the store paths to bind-mount read-only.
4. `feathernix-sandbox::SandboxSpec` produces a `bwrap` argv with
   `--unshare-all`, `--die-with-parent`, ro-binds for each dep store path,
   rw `/build` and `/out`, env (`SRC`, `OUT`, `JOBS`).
5. Each `[build].prepare/build/install` step is one `bwrap` invocation
   against the now-mutated `/build`.
6. After the final step, `<work>/out` is the staging area passed to
   `feathernix-archive::pack`, producing
   `<name>-<version>-<release>-<arch>.fnpkg.7z`.

## Data flow: buildd

1. Maintainer runs `fnmaint submit --controller <url>`. The recipe directory
   is bundled as `tar.gz` and POSTed to `/v1/jobs`.
2. Controller persists the job in SQLite (`build_jobs` table) and returns
   `{ job_id }`.
3. Workers `POST /v1/workers/register` on startup and heartbeat every 30s.
   The controller reaps workers silent for >2min.
4. A worker long-polls `GET /v1/workers/{id}/lease` for an assigned job and
   receives the recipe blob.
5. Worker invokes `feathernix-build` (the same library `fnmaint build`
   uses), streams logs back via `POST .../log`, and uploads the produced
   `.fnpkg.7z` + `.sig` via `POST .../artifact`.
6. On `POST .../complete`, the controller invokes `feathernix-publish`
   against the configured `--publish-repo` directory: regenerate
   `repodata/index.toml`, re-sign, atomically swap.

## Why this shape

- **`feathernix-db` owns the connection pool** so transaction boundaries are
  obvious and the schema doesn't sprawl across crates.
- **`feathernix-builder` is a thin orchestrator** today (binary-only) but
  reserves the surface area for future source-build integration without
  another refactor.
- **Producer crates are separate from consumer crates** so the `fn` binary
  stays small. Cargo only links what it imports.
- **Buildd is its own crate with its own DB file** because controller state
  has nothing to do with end-user installs; co-locating in one workspace is
  about shared types, not shared DBs.
