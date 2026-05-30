# FeatherNix

A hybrid Nix/RPM-style package manager written in Rust.

FeatherNix combines content-addressed `/fnix/store/<hash>-<name>/` paths,
flakes, and generations with rollback (from Nix) with pre-built binary
packages fetched from configured repositories (from RPM). The consumer side
ships as the `fn` binary; the producer side (`fnmaint`, `feathernix-buildd`)
lets maintainers fetch sources, build inside a Bubblewrap sandbox, sign, and
publish to a repo.

## Status

Early development. The MVP consumer pipeline (`fn install` end-to-end against
an HTTP repo, generations with rollback, signed indices and payloads) is in
place, and the maintainer toolchain (`fnmaint`, `feathernix-buildd`) is being
built out in this same Cargo workspace. APIs, CLI flags, and the on-disk
schema are not stable yet.

## Features

- **Content-addressed store** — `/fnix/store/<hash>-<name>-<version>/`,
  immutable, atomically installed.
- **Binary-first repositories** — TOML-defined, fetched over HTTP, with
  signed indices and signed payloads (TOFU key cache).
- **Generations** — every install/upgrade/rollback is a new generation;
  switching is a symlink swap and a DB transaction.
- **Flakes** — `flake.toml` + `flake.lock.toml` describe inputs and outputs
  (TOML-based; no Nix-language evaluator).
- **7z payloads** — pure-Rust archive handling (`sevenz-rust2`), no shell-out
  to `7z`.
- **Sandboxed builds** — `fnmaint build` and the buildd worker run each step
  inside Bubblewrap with network-isolated build phases.
- **Distributed builder** — `feathernix-buildd` controller schedules jobs to
  workers and re-publishes the repo index when artifacts land.

## Installation

FeatherNix is not yet packaged. To build from source:

```sh
git clone https://github.com/featherlinux/feathernix.git
cd feathernix
cargo build --release --workspace
```

Binaries land in `target/release/`:

| Binary | Purpose |
|---|---|
| `fn` | End-user package manager |
| `fnmaint` | Maintainer CLI: recipe → build → sign → publish |
| `feathernix-buildd` | Distributed build daemon (controller or worker mode) |
| `feathernix-buildctl` | Ops CLI for the buildd controller |

### Runtime requirements

- Linux with user namespaces enabled.
- `bwrap` (Bubblewrap) on `PATH` — required for `fnmaint build` and buildd
  workers. The consumer-side `fn` does **not** need it.
- `git` and `tar` on `PATH` for source fetching.

## Quickstart

Run against an isolated root so nothing touches your real system:

```sh
export FEATHERNIX_ROOT=$PWD/testroot

fn repo add devrepo http://localhost:8080/devrepo
fn repo refresh
fn install hello
fn generation list
fn generation rollback
```

See [docs/cli.md](docs/cli.md) for the full command reference and
[docs/development.md](docs/development.md) for the local-fixture repo setup
used in the integration tests.

## Workspace layout

This repo is a single Cargo workspace. See [docs/architecture.md](docs/architecture.md)
for the full crate-by-crate breakdown. At a glance:

- **Consumer-side**: `feathernix-cli` (`fn`), `feathernix-config`, `feathernix-db`,
  `feathernix-repo`, `feathernix-archive`, `feathernix-store`, `feathernix-flake`,
  `feathernix-builder`.
- **Producer-side**: `feathernix-maint-cli` (`fnmaint`), `feathernix-recipe`,
  `feathernix-source`, `feathernix-sandbox`, `feathernix-build`,
  `feathernix-sign`, `feathernix-publish`.
- **Buildd**: `feathernix-buildd-proto`, `feathernix-buildd` (controller +
  worker + `feathernix-buildctl`).

## Documentation

- [Architecture](docs/architecture.md) — workspace, crate responsibilities,
  data flow.
- [Filesystem layout](docs/filesystem.md) — where state, cache, and config
  live.
- [Package format](docs/package-format.md) — `.fnpkg.7z`, `manifest.toml`,
  `repodata/index.toml`.
- [Repository config](docs/repository-config.md) — one TOML file per repo.
- [Flakes](docs/flakes.md) — `flake.toml` and `flake.lock.toml`.
- [Recipes](docs/recipes.md) — `feathernix.recipe.toml` for source builds.
- [Signing](docs/signing.md) — GPG verification policy and signing keys.
- [Buildd](docs/buildd.md) — controller/worker daemon and protocol.
- [CLI reference](docs/cli.md) — `fn`, `fnmaint`, buildd commands.
- [Development](docs/development.md) — building, testing, sandbox
  requirements.

## Contributing

See [AGENTS.md](AGENTS.md) for orientation when working in this codebase
(this applies to AI agents and to humans onboarding for the first time).

## License

[MIT](LICENSE.md).
