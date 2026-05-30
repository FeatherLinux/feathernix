# FeatherNix — Hybrid Nix/RPM Package Manager in Rust

## Context

FeatherNix is a new package manager being built fresh at `/home/eric/RustroverProjects/feathernix` (currently empty). It hybridizes Nix and RPM:

- **From Nix**: content-addressed `/fnix/store/<hash>-<name>/` paths, flakes, generations with rollback, reproducible installs.
- **From RPM**: pre-built binary packages fetched from configured repositories — **no on-machine builds in v1**. Repos defined RPM-style, one TOML file per repo at `/etc/feathernix/repos/<name>.toml`.

A prior attempt exists at `/home/eric/RustroverProjects/feathernix-backup` with ~70% of the architecture sketched out (10 crates, SQLite schema, 7z + RPM metadata, flake loader). We are starting fresh and using it only as a **reference**, not copying code. Key improvements over the backup: per-repo TOML files (replacing INI-style `feathernix.conf`), Diesel ORM (replacing raw rusqlite), real HTTP repo fetching (backup only did `path://`), pure-Rust `sevenz-rust2` (no shell-out to `7z`).

Outcome: by the end of phase 10, `fn install <pkg>` works end-to-end against a local HTTP repo, generations record correctly, and `fn generation rollback` swaps the active profile back.

## Technology choices (locked in)

| Concern | Choice |
|---|---|
| Language | Rust, edition 2024, Cargo workspace |
| CLI | `clap` 4.x (derive) — binary name `fn` |
| 7z | `sevenz-rust2` (pure-Rust, no shell-out) |
| TOML | `toml` + `serde` |
| HTTP | `reqwest` blocking mode (pairs with sync Diesel) |
| DB | SQLite via **Diesel** + `diesel_migrations` + r2d2 pool |
| Hashing | `sha2` (SHA-256 for store paths, payloads), `blake3` optional |
| Errors | `anyhow` at CLI boundary, `thiserror` per crate |

## Workspace layout (8 crates under `crates/`)

| Crate | Purpose |
|---|---|
| `feathernix-cli` | `fn` binary; clap subcommands; output formatting |
| `feathernix-config` | System/user config + per-repo TOML reader (`/etc/feathernix/repos/*.toml`) |
| `feathernix-db` | Diesel schema, migrations, models, r2d2 pool. **All DB access funnels through here.** |
| `feathernix-repo` | HTTP repo client, index parsing, signature verification, greedy resolver (SAT stub) |
| `feathernix-archive` | `sevenz-rust2` wrappers; manifest.toml extraction from inside `.fnpkg.7z` |
| `feathernix-store` | `/fnix/store` path layout, atomic install, profile symlink trees |
| `feathernix-flake` | `flake.toml` + `flake.lock.toml` parsing; input resolution via repo crate |
| `feathernix-builder` | `Realizer` trait + `RepositoryRealizer` impl (download → extract → register). Source-builds deferred. |

Justification: each crate has one job; `feathernix-db` owns the connection pool so transaction boundaries are obvious; `feathernix-builder` exists today as a thin orchestrator but reserves the surface area for future source builds. Daemon/sandbox/derivation/eval crates from the backup are deferred — they aren't needed for binary-first MVP.

## SQLite schema (one `0001_init` migration)

Tables (all timestamps `TIMESTAMP NOT NULL`):

- `repositories` (id, name UNIQUE, base_url, enabled, priority, gpg_check, gpg_key_url NULL, signature_required, last_refresh_at NULL, etag NULL)
- `packages_available` (id, repo_id FK, name, version, release, epoch, arch, payload_path, payload_sha256, payload_size, summary NULL, dependencies_json, provides_json) — UNIQUE(repo_id, name, version, release, arch), INDEX(name, arch)
- `installed_packages` (id, name, version, release, epoch, arch, repo_id FK NULL, store_path_id FK, installed_at, summary NULL) — UNIQUE(name, arch)
- `generations` (id, profile, generation, created_at, description NULL, is_current BOOL) — UNIQUE(profile, generation)
- `generation_entries` (id, generation_id FK, installed_package_id FK, store_path_id FK)
- `flakes` (id, root_path UNIQUE, description NULL, last_locked_at NULL)
- `flake_inputs` (id, flake_id FK, name, kind [repo/flake/path], url, locked_revision NULL, locked_sha256 NULL) — UNIQUE(flake_id, name)
- `store_paths` (id, hash UNIQUE, name, full_path, sha256, blake3 NULL, size_bytes, origin, origin_ref NULL, realized_at, gc_root_count DEFAULT 0)

Diesel schema is generated to `crates/feathernix-db/src/schema.rs`; models live in `crates/feathernix-db/src/models.rs`; migrations are embedded via `diesel_migrations::embed_migrations!`. Migrations directory: `crates/feathernix-db/migrations/0001_init/{up.sql,down.sql}`.

## Repository TOML schema

One file per repo at `/etc/feathernix/repos/<name>.toml` (system) or `~/.config/feathernix/repos/<name>.toml` (user override). The filename stem must match `[repository].name`.

```toml
[repository]
name        = "feathernix-stable"
description = "FeatherNix Stable for x86_64-linux"
base_url    = "https://repo.feathernix.org/stable/x86_64-linux"
enabled     = true
priority    = 50                       # lower wins, DNF convention

[repository.signing]
gpg_check          = true
gpg_key_url        = "https://repo.feathernix.org/keys/release.asc"
signature_required = true

[repository.refresh]
metadata_expire_seconds = 21600        # 6h
```

`base_url` supports `https://`, `http://`, `file:///abs/path`, or `path:/abs/path`. Loader lives in `feathernix-config::repo`.

## Package format

Each package is `<name>-<version>-<release>-<arch>.fnpkg.7z` containing:

```
manifest.toml         # required at archive root
payload/              # files to install (rooted at the future store path)
metadata/files.toml   # optional per-file mode/uid/gid/caps (RPM-style)
metadata/scripts/     # optional install hooks (not run in MVP)
```

`manifest.toml`:

```toml
[package]
name = "ripgrep"; version = "14.1.0"; release = "1"; epoch = 0
arch = "x86_64-linux"; summary = "..."; license = "..."; url = "..."

[[dependencies.requires]]
name = "glibc"; version_constraint = ">= 2.17"

[[dependencies.provides]]
name = "rg"; version = "14.1.0"

[outputs]
default = "out"
```

Repository index at `<base_url>/repodata/index.toml` (detached sig at `index.toml.sig`):

```toml
[index]
format_version = 1
generated_at   = "2026-05-21T10:00:00Z"
repository     = "feathernix-stable"

[[packages]]
name = "ripgrep"; version = "14.1.0"; release = "1"; epoch = 0
arch = "x86_64-linux"
payload_path   = "packages/ripgrep-14.1.0-1-x86_64-linux.fnpkg.7z"
payload_sha256 = "abc123…"
payload_size   = 1234567
dependencies = ["glibc >= 2.17"]
provides     = ["rg = 14.1.0"]
```

## Flake format

**TOML-based for MVP** (`flake.toml` + `flake.lock.toml`). This is a deliberate deviation from Nix's `flake.nix` Lambda-language syntax — implementing a Nix evaluator is out of scope for binary-first v1. The legacy `flake.fnix` from the backup is parked until a real evaluator exists.

```toml
# flake.toml
description = "My system flake"

[inputs.feathernix-stable]
kind = "repo"
url  = "https://repo.feathernix.org/stable/x86_64-linux"

[outputs.packages.default]
packages = ["feathernix-stable#ripgrep", "feathernix-stable#fd"]

[outputs.devShells.default]
packages  = ["feathernix-stable#rustup", "feathernix-stable#git"]
shellHook = "echo entered dev shell"
```

Lockfile mirrors inputs with `locked_revision`/`locked_sha256` and is the source of truth for what `flake_inputs` rows are populated with.

## Core flows

**`fn install <pkg>`** — the full pipeline; everything else is a subset.

1. `cli::commands::install::run(args)` parses targets.
2. `config::Config::load()` reads `feathernix.toml` + all `repos/*.toml`.
3. `db::Db::open(config.db_path)` opens pool, runs embedded migrations.
4. `repo::RepoSync::refresh_if_stale(&db, &config)` — for each enabled repo past its `metadata_expire_seconds`, GETs `repodata/index.toml`, verifies sig if required, sha256-checks the body, upserts `packages_available`.
5. `repo::Resolver::resolve(&db, targets)` — greedy resolver (lowest priority × highest version) returning `Vec<ResolvedPackage>` with transitive deps. `DependencySolver` trait stubbed for future SAT.
6. For each resolved package:
   - `repo::Fetcher::download(&pkg, cache_dir)` — blocking GET to `/var/cache/feathernix/payloads/<sha256>.fnpkg.7z`, sha256-verified.
   - `store::StorePath::compute(name, version, sha256)` builds `/fnix/store/<40hex>-<name>-<version>/`.
   - `archive::extract(payload, &store_path)` via `sevenz_rust2::decompress_file`.
   - `store::install(store_path)` atomic-renames from temp, inserts `store_paths` + `installed_packages`.
7. `db::generations::create_next(&db, profile)` in a transaction: bump `MAX(generation)+1`, set `is_current` flags, insert `generation_entries` rows.
8. `store::profile::update_current_symlink(profile, generation)` rebuilds `/var/lib/feathernix/profiles/<profile>/generations/<n>/` as symlinks into store paths and swings `current → generations/<n>`.

**`fn repo {list,refresh,add,remove}`** — owned by `feathernix-config` + `feathernix-repo`. `add` writes to user dir; `remove` refuses to delete system files; both update `repositories` table.

**`fn generation {list,switch <n>,rollback}`** — pure DB transaction + symlink update. Past-generation store paths stay on disk until `fn gc` (future).

**`fn flake {init,show,lock}`** — `init` writes a stub `flake.toml`; `show` parses + resolves `<input>#<pkg>` against cached indices; `lock` fetches each input's current state and writes `flake.lock.toml`.

## Filesystem layout

| Path | Purpose |
|---|---|
| `/fnix/store/<hash>-<name>-<version>/` | Immutable content-addressed package contents |
| `/var/lib/feathernix/db.sqlite` | Main SQLite DB |
| `/var/lib/feathernix/profiles/<profile>/{current,generations/<n>/}` | Active generation symlinks |
| `/var/cache/feathernix/payloads/<sha256>.fnpkg.7z` | Downloaded packages |
| `/var/cache/feathernix/repodata/<repo>/index.toml` | Last fetched index |
| `/etc/feathernix/feathernix.toml` | System-wide settings |
| `/etc/feathernix/repos/<name>.toml` | One file per system repo |
| `~/.config/feathernix/{feathernix.toml,repos/<name>.toml}` | Per-user overrides |
| `~/.local/state/feathernix/db.sqlite` | Per-user state for rootless mode |

`$FEATHERNIX_ROOT` env var overrides all of the above for dev/testing (defaults handled in `feathernix-config`).

## Phased implementation order

Each phase ends with a passing test or runnable command.

1. **Workspace skeleton** — root `Cargo.toml` (workspace + `[workspace.package]` shared metadata), stub `lib.rs` per crate, `main.rs` in `feathernix-cli`. `cargo build` succeeds.
2. **`feathernix-db` core** — Diesel + `diesel_migrations`, write `0001_init/up.sql` with all 8 tables, generate `schema.rs`, derive models, unit-test in-memory DB.
3. **`feathernix-config` + repo TOML** — load `feathernix.toml` + `repos/*.toml` (system + user). Round-trip the example in section 3.
4. **`feathernix-archive` (read path)** — wrap `sevenz-rust2` for extraction; helper to stream `manifest.toml` out of a `.7z`. Test with a fixture archive.
5. **`feathernix-store`** — path computation, atomic install, `store_paths` registration. Test against a fake extracted directory.
6. **`feathernix-repo` with HTTP** — `RepoSync::refresh` via `reqwest::blocking`, `Resolver::resolve` greedy impl. Integration-test with `httpmock` serving a fixture index + tiny `.fnpkg.7z`.
7. **`feathernix-builder`** — `Realizer` trait, `RepositoryRealizer` chaining download → verify → extract → register. End-to-end test ending with a row in `installed_packages`.
8. **Generations** — `create_next`, `switch`, `rollback`, profile symlink updater. Test two installs and a rollback.
9. **CLI wiring** — implement subcommands `install`, `repo {list,add,remove,refresh}`, `generation {list,switch,rollback}`, `store {list,path-info}`, `flake {init,show,lock}`. Wire globals (`--store`, `--config`, `--verbose`, `--json`).
10. **Integration tests + polish** — `tests/` at workspace root spins up a local HTTP server against `tests/fixtures/sample-repo/`, runs `fn` as a subprocess against `FEATHERNIX_ROOT=/tmp/xyz`, asserts the full flow.

## Critical files to create

- `/home/eric/RustroverProjects/feathernix/Cargo.toml` — workspace root
- `/home/eric/RustroverProjects/feathernix/crates/feathernix-db/migrations/0001_init/up.sql` — full schema
- `/home/eric/RustroverProjects/feathernix/crates/feathernix-db/src/{schema.rs,models.rs,lib.rs}` — Diesel layer
- `/home/eric/RustroverProjects/feathernix/crates/feathernix-config/src/{lib.rs,repo.rs}` — TOML loaders
- `/home/eric/RustroverProjects/feathernix/crates/feathernix-repo/src/{lib.rs,sync.rs,resolver.rs,fetcher.rs}` — repo client
- `/home/eric/RustroverProjects/feathernix/crates/feathernix-archive/src/lib.rs` — 7z extract/pack
- `/home/eric/RustroverProjects/feathernix/crates/feathernix-store/src/{lib.rs,path.rs,profile.rs}` — store mgmt
- `/home/eric/RustroverProjects/feathernix/crates/feathernix-flake/src/{lib.rs,types.rs,lock.rs}` — flake TOML
- `/home/eric/RustroverProjects/feathernix/crates/feathernix-builder/src/{lib.rs,realizer.rs}` — install orchestrator
- `/home/eric/RustroverProjects/feathernix/crates/feathernix-cli/src/{main.rs,commands/*.rs}` — `fn` CLI
- `/home/eric/RustroverProjects/feathernix/tests/integration_test.rs` — end-to-end harness

Reference (not copy) from the backup for design decisions:
- `feathernix-backup/crates/feathernix-store/src/state.rs` — original SQLite schema patterns
- `feathernix-backup/crates/feathernix-repo/src/lib.rs` — repo metadata shape
- `feathernix-backup/crates/feathernix-archive/src/lib.rs` — 7z + RPM sidecar approach
- `feathernix-backup/crates/feathernix-flake/src/{loader.rs,types.rs}` — flake structure

## Verification

End-to-end smoke (with `FEATHERNIX_ROOT=$PWD/testroot` and a local repo on port 8080):

```
fn repo add devrepo http://localhost:8080/devrepo
fn repo refresh                # "devrepo: N packages indexed"
fn repo list                   # table view
fn install hello               # generation 1 active, /fnix/store/<hash>-hello-1.0.0/
fn install ripgrep fd          # generation 2 active
fn generation list             # two rows, gen 2 current
fn generation rollback         # gen 1 active again
fn flake init && fn flake show # flake.toml created and parsed
```

DB-level assertions (via `sqlite3 $FEATHERNIX_ROOT/var/lib/feathernix/db.sqlite`):

- `SELECT name, version FROM installed_packages` returns expected rows
- Reinstalling the same package produces no duplicate `store_paths` row but a new `generations` row
- After rollback, `installed_packages` reflects gen 1's contents and the profile `current` symlink points at `generations/1`

Per-crate unit tests: in-memory Diesel DB for db crate; httpmock for repo crate; tempdir for archive/store/builder crates.
