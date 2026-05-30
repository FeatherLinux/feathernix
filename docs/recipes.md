# Recipes

A recipe (`feathernix.recipe.toml`) is the producer-side input to
`fnmaint build`. It declares where the source comes from, what the build
needs, and what commands turn the source tree into a staged `$OUT`
directory that gets packed into a `.fnpkg.7z`.

## Full example

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
rev    = "14.1.0"        # tag, branch, or sha — pinned
sha256 = "abc123..."     # verifies the post-checkout tree

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

## Sections

### `[recipe]`

| Field | Required | Notes |
|---|---|---|
| `format_version` | yes | Currently always `1`. |

### `[package]`

Mirrors the consumer-side `manifest.toml` `[package]` block. These values
end up verbatim in the produced archive's manifest.

### `[source]`

| `kind` | Required fields | Notes |
|---|---|---|
| `git` | `url`, `rev`, `sha256` | `rev` may be a tag, branch, or commit SHA but **must be pinned** — floating branch refs (e.g. `main`) are rejected by `fnmaint recipe lint`. `sha256` is computed over the post-checkout working tree (excluding `.git/`). |
| `tarball` | `url`, `sha256` | HTTPS only in practice; sha256 is of the downloaded bytes. |
| `path` | `path` | Local directory. Not allowed in recipes submitted to buildd (controllers reject `path://` sources). |

### `[build]`

`prepare`, `build`, and `install` are arrays of strings. Each string is one
shell command run as `/bin/sh -c "<string>"` inside its own `bwrap`
invocation. Steps run in order; the next step sees the previous step's
mutations to `/build`.

Available env inside each step:
- `$SRC` — read-only bind of the verified source tree (`/src`).
- `$OUT` — read-write staging area (`/out`); becomes the package payload.
- `$JOBS` — parallelism hint (default: number of CPUs).
- `$HOME` — `/build` (the rw working area).

`[build.env]` is merged into the per-step environment; values here override
defaults but cannot override `$SRC`, `$OUT`, `$JOBS`.

### `[dependencies]`

| Field | Used by | Effect |
|---|---|---|
| `requires` | Consumer | Runtime dependencies; flow into `manifest.toml` and the repo index. |
| `provides` | Consumer | Capabilities this package satisfies (e.g. `rg = 14.1.0`). |
| `build_requires` | Producer | Resolved through the existing repo client and mounted **read-only** into the sandbox at their store paths. Network is unshared during build, so any missing `build_requires` will fail loudly. |

## Sandbox guarantees

Each build step runs under `bwrap` with `--unshare-all` (no network, no
PID namespace, no user namespace leakage), `--die-with-parent`, fresh
`/proc`, `/dev`, `/tmp`, and the bind mounts above. The source fetch phase
runs in a separate sibling sandbox **with** network enabled, writing to the
source cache; the build sandbox itself has no network.

See [buildd.md](buildd.md) for how recipes flow through the distributed
builder.

## Linting

`fnmaint recipe lint [path]` validates:

- Required fields present, types correct.
- `source.rev` is pinned (no floating branches).
- `source.sha256` is the right length and hex.
- `build_requires` parse as version constraints.

A clean lint is a precondition for `fnmaint submit`.
