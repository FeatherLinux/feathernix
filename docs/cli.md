# CLI reference

FeatherNix ships four binaries: `fn` (end-user), `fnmaint` (maintainer),
`feathernix-buildd` (daemon), and `feathernix-buildctl` (ops). Commands
below are organized by binary.

Global flags accepted by `fn` and `fnmaint`:

| Flag | Purpose |
|---|---|
| `--store <path>` | Override the store root (default: `/fnix/store`). |
| `--config <path>` | Override the config root. |
| `--verbose` / `-v` | Increase log verbosity (`tracing` env-filter). |
| `--json` | Emit machine-readable output where supported. |

The `$FEATHERNIX_ROOT` env var re-roots all standard paths under a single
prefix — preferred for dev and tests.

## `fn` — end-user package manager

### `fn install <pkg>…`

Resolve, download, verify, extract, register, and activate one or more
packages. Creates a new generation.

```sh
fn install ripgrep
fn install ripgrep fd bat
```

### `fn repo`

```sh
fn repo list
fn repo add <name> <url> [--gpg-key-url <url>]
fn repo remove <name>
fn repo refresh
```

`fn repo add` writes to the user repo directory. `fn repo remove` refuses
to delete system repo files.

### `fn generation`

```sh
fn generation list
fn generation switch <n>
fn generation rollback
```

`rollback` is sugar for `switch <current - 1>`.

### `fn store`

```sh
fn store list
fn store path-info <path>
```

### `fn flake`

```sh
fn flake init           # write flake.toml
fn flake show           # parse and resolve package refs from cached repo metadata
fn flake lock           # lock repo inputs into flake.lock.toml
```

`fn flake show` does not refresh repositories. Add and refresh the matching
repo first so the input URL is present in the local SQLite cache.

See [flakes.md](flakes.md).

## `fnmaint` — maintainer CLI

### `fnmaint recipe`

```sh
fnmaint recipe init             # write a stub feathernix.recipe.toml
fnmaint recipe lint [path]      # validate format, check pinning rules
```

### `fnmaint build`

```sh
fnmaint build [path]
```

End-to-end local build: fetch source into the cache (verifying sha256),
spin up `bwrap`, run each `[build].prepare/build/install` step, pack
`$OUT` into `<name>-<version>-<release>-<arch>.fnpkg.7z` in the current
directory.

Requires `bwrap`, `git`, and `tar` on `$PATH`. See [recipes.md](recipes.md).

### `fnmaint sign`

```sh
fnmaint sign <archive> --key <path>
```

Produces `<archive>.sig`. The key may also come from
`$FNMAINT_SIGNING_KEY`.

### `fnmaint publish`

```sh
fnmaint publish --repo <dir> [--key <path>] [<archive>…]
```

Copies the named archives (or every `*.fnpkg.7z` in cwd if none are given)
into `<repo>/packages/`, regenerates `<repo>/repodata/index.toml` from the
directory contents, signs the new index, and atomically swaps it into
place.

### `fnmaint submit`

```sh
fnmaint submit --controller <url> [path]
```

Bundles the recipe directory as `tar.gz`, POSTs to the controller's
`/v1/jobs`, streams logs to stdout, and exits non-zero if the build fails.

## `feathernix-buildd` — distributed build daemon

```sh
feathernix-buildd controller \
    --listen 0.0.0.0:7070 \
    --state <dir> \
    --publish-repo <dir>

feathernix-buildd worker \
    --controller <url> \
    --token <file> \
    --workdir <dir>
```

See [buildd.md](buildd.md).

## `feathernix-buildctl` — buildd ops CLI

```sh
feathernix-buildctl jobs list [--state pending|assigned|running|succeeded|failed|cancelled]
feathernix-buildctl jobs cancel <id>
feathernix-buildctl jobs rebuild --recipe <name>

feathernix-buildctl workers list
feathernix-buildctl workers drain <id>
```

All commands accept `--controller <url>` (default:
`$FEATHERNIX_BUILDD_URL`).
