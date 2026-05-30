# Filesystem layout

All paths below are the **system defaults**. Every one of them is re-rooted
under `$FEATHERNIX_ROOT` when that env var is set — this is how dev and the
integration tests run without root.

## Store and state

| Path | Purpose |
|---|---|
| `/fnix/store/<hash>-<name>-<version>/` | Immutable, content-addressed package contents. Never edited in place. |
| `/var/lib/feathernix/db.sqlite` | Main SQLite database (consumer side). |
| `/var/lib/feathernix/profiles/<profile>/current` | Symlink → `generations/<n>` for the active generation. |
| `/var/lib/feathernix/profiles/<profile>/generations/<n>/` | A generation's symlink tree into `/fnix/store/*`. |

## Cache

| Path | Purpose |
|---|---|
| `/var/cache/feathernix/payloads/<sha256>.fnpkg.7z` | Downloaded package payloads, sha256-verified on fetch. |
| `/var/cache/feathernix/repodata/<repo>/index.toml` | Last successfully fetched repo index. |
| `/var/cache/feathernix/keyrings/<repo>.asc` | Trusted public key for a GPG-checked repo (TOFU). |
| `/var/cache/feathernix/keyrings/<repo>.fpr` | Fingerprint hex for sanity check / display. |
| `/var/cache/feathernix/sources/<hash>/` | Producer-side source cache (clones, tarballs). |

## Config

| Path | Purpose |
|---|---|
| `/etc/feathernix/feathernix.toml` | System-wide settings. |
| `/etc/feathernix/repos/<name>.toml` | One file per system repo (filename stem must match `[repository].name`). |
| `~/.config/feathernix/feathernix.toml` | Per-user overrides. |
| `~/.config/feathernix/repos/<name>.toml` | Per-user repo overrides / additions. |
| `~/.local/state/feathernix/db.sqlite` | Per-user state for rootless mode. |

## Producer workdirs

| Path | Purpose |
|---|---|
| `<cache_dir>/sources/<hash>/` | Verified source trees from `feathernix-source`. |
| `<state_dir>/build/<recipe>-<rev>/` | `fnmaint build` working area (`/build` + `/out` inside the sandbox). |
| `<config_dir>/signing/` | Secret signing keys for `fnmaint sign` / `publish`. |

## `$FEATHERNIX_ROOT`

When `$FEATHERNIX_ROOT=/tmp/testroot`, every path above is rebased: e.g.
`/fnix/store/...` becomes `/tmp/testroot/fnix/store/...`,
`/var/lib/feathernix/...` becomes `/tmp/testroot/var/lib/feathernix/...`,
and so on. `feathernix-config::Paths` is the single source of truth for this
mapping; nothing in the code should hardcode `/fnix` or `/var/lib/feathernix`
directly.
