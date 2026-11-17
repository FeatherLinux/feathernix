# Agent instructions

Orientation for AI coding agents (and new human contributors) working in this
repository. Read this file first; everything else assumes you have.

## What this project is

FeatherNix is a hybrid Nix/RPM-style package manager. The high-level design
and rationale live in three planning documents at the repo root:

- `plans/feathernix-hybrid-nix-rpm-package-manager-in-rust.md` — original
  architecture, the 10-phase rollout, and the SQLite schema.
- `plans/feathernix-gpg-signature-verification.md` — the GPG verification design
  layered on top.
- `plans/maintainer-tooling-buildd-worker-daemon-for-feathernix.md` — the
  producer-side (`fnmaint`, `feathernix-buildd`) plan.

These are **the source of truth for intent**. If something in the code looks
wrong, check these first before "fixing" it.

For the user-facing summary see [README.md](README.md); for crate-by-crate
detail see [docs/architecture.md](docs/architecture.md).

## Workspace shape

Single Cargo workspace, `edition = "2024"`, `rust-version = "1.95"`. All
crates live under `crates/`. Three roles:

| Role | Crates | Binaries |
|---|---|---|
| Consumer | `feathernix-cli`, `feathernix-config`, `feathernix-db`, `feathernix-repo`, `feathernix-archive`, `feathernix-store`, `feathernix-flake`, `feathernix-builder` | `fn` |
| Producer | `feathernix-maint-cli`, `feathernix-recipe`, `feathernix-source`, `feathernix-sandbox`, `feathernix-build`, `feathernix-sign`, `feathernix-publish` | `fnmaint` |
| Buildd | `feathernix-buildd-proto`, `feathernix-buildd` | `feathernix-buildd`, `feathernix-buildctl` |

The maintainer/builder toolchain ships in the same workspace **on purpose**
— shared types (`Manifest`, `RepositoryIndex`, `Paths`, GPG primitives) are
used by both sides without a publish-to-crates.io coordination tax. The
end-user `fn` binary is not bloated because Cargo only links what it imports.

If you are tempted to extract maintainer code into a separate repo, don't —
that decision has been considered and rejected. The only crate worth
re-evaluating in isolation is `feathernix-buildd`, and only if it grows
dependencies (Postgres, a web UI) the rest of the workspace shouldn't carry.

Shared cryptographic utilities live in `feathernix-crypto`. Its `CompatOsRng`
bridges Rand 0.10 OS randomness to the older traits required by `pgp`. Keep
RNG compatibility dependencies contained there.

## Conventions

- **DB access funnels through `feathernix-db`.** Do not add `rusqlite` or
  `diesel` calls in other crates. The buildd controller is the one exception
  because it uses a separate DB file and uses `rusqlite` directly — keep that
  contained to `crates/feathernix-buildd/`.
- **Errors**: `anyhow` only at the CLI boundary (`feathernix-cli`,
  `feathernix-maint-cli`, the buildd binaries). Library crates use
  `thiserror` with their own crate-local error enum.
- **No shell-out** for things that have pure-Rust crates already in use:
  archives go through `sevenz-rust2`, not `7z`. GPG goes through the `pgp`
  crate, not `gpg`. The one exception is `bwrap` — see below.
- **Bubblewrap is shelled out on purpose.** `feathernix-sandbox` spawns
  `/usr/bin/bwrap`. Tests that exercise the real sandbox must guard with
  `if !have_bwrap() { return; }` and **skip** when missing — they must not
  fail. This keeps CI green on minimal containers without paving over a real
  misconfiguration.
- **Repo config is one TOML file per repo** at `/etc/feathernix/repos/<name>.toml`
  (system) or `~/.config/feathernix/repos/<name>.toml` (user). The filename
  stem must match `[repository].name`. Do not reintroduce a single
  `feathernix.conf` INI file.
- **`$FEATHERNIX_ROOT`** is the env var that re-roots all of `/fnix`,
  `/var/lib/feathernix`, `/var/cache/feathernix`, and `/etc/feathernix` for
  dev and tests. Everything in `feathernix-config::Paths` must honor it.

## Dependency landmines

- **SQLite version pinning.** `feathernix-buildd` pins `rusqlite = "0.43"`,
  which pulls `libsqlite3-sys = 0.40`; this matches what Diesel uses on the
  consumer side. Cargo's `links = "sqlite3"` rule forbids two versions of
  the same native lib in one binary. Before bumping either crate, look up
  the *exact* `libsqlite3-sys` version each pulls in and confirm they match.
- **`edition = "2024"`** is required (`rust-version = "1.95"`). If `cargo
  build` complains about an unstable edition, the toolchain is too old —
  fix the toolchain, don't downgrade the edition.

## Workflow expectations

- **Always run `cargo build --workspace` and `cargo test --workspace`** for
  changes that touch more than one crate. Per-crate `cargo test -p <crate>`
  is fine for tightly scoped work.
- **Tests that need `bwrap`** check `have_bwrap()` and skip if absent. If
  you're adding a test that exercises real sandboxing, follow that pattern.
- **Integration test harness** lives at
  `crates/feathernix-cli/tests/integration_test.rs`. It spins up a local
  HTTP server against a fixture repo, runs `fn` as a subprocess under
  `FEATHERNIX_ROOT=<tempdir>`, and asserts the full flow. New end-to-end
  scenarios belong here.
- **Don't add comments that restate the code.** Comments earn their place
  by explaining a hidden constraint, an invariant, or a workaround — not by
  narrating what the next line does.
- **Don't add backwards-compat shims** for code paths that haven't shipped
  to users. This is pre-1.0; just change the code.

## When you're stuck

1. Re-read the relevant planning document (the three files listed under
   "What this project is"). The intent is usually written down.
2. Surface the question. A 30-second clarifying question beats a 20-minute
   guess that gets reverted.

## Things you must not do

- Do not commit anything in `testroot/` or `target/` — both are in
  `.gitignore` and contain throwaway state.
- Do not change the workspace `license = "MIT"` field or the `authors` list
  without an explicit ask.
- Do not silently re-fetch GPG keys from `gpg_key_url` after first TOFU; key
  rotation is a deliberate future `fn repo trust` command.
- Do not introduce a second SQLite linkage (see "Dependency landmines").
- Do not add `unsafe` to bypass a sandbox or signature check just to make a
  test pass — that's the kind of bug this project exists to prevent.
