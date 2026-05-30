# Flakes

A FeatherNix flake describes a set of inputs (repositories or other flakes)
and outputs (packages, dev shells). The on-disk format is **TOML-based**:
`flake.toml` for the source of truth, `flake.lock.toml` for pinned input
state.

This is a deliberate deviation from Nix's `flake.nix` lambda-language
syntax. Implementing a Nix evaluator is out of scope for the binary-first
MVP; TOML gives us a stable schema today without a parser/evaluator.

## `flake.toml`

```toml
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

### Input kinds

| Kind | Fields | Use case |
|---|---|---|
| `repo` | `url` | A FeatherNix HTTP repository (signed index + payloads). |
| `flake` | `url` | Another flake on disk or over HTTP. |
| `path` | `path` | A local directory. |

### Output references

Package references use `<input>#<package>` syntax, e.g.
`feathernix-stable#ripgrep`. The input name must be declared in `[inputs.*]`
in this flake.

## `flake.lock.toml`

Generated and updated by `fn flake lock`. Mirrors the inputs from
`flake.toml` and records the resolved state of each at lock time:

```toml
[inputs.feathernix-stable]
kind            = "repo"
url             = "https://repo.feathernix.org/stable/x86_64-linux"
locked_revision = "2026-05-21T10:00:00Z"
locked_sha256   = "abc123..."
```

The lockfile is the source of truth for what gets written into the
`flake_inputs` SQLite table when a flake is registered.

## CLI

```sh
fn flake init           # write a stub flake.toml in cwd
fn flake show           # parse + resolve <input>#<pkg> against cached indices
fn flake lock           # fetch each input's current state, write flake.lock.toml
```

`fn flake init` will not overwrite an existing `flake.toml`.
