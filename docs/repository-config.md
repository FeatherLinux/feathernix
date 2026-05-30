# Repository configuration

FeatherNix uses **one TOML file per repository**, not a single global
`feathernix.conf`. System repos live at `/etc/feathernix/repos/<name>.toml`;
per-user repos live at `~/.config/feathernix/repos/<name>.toml`.

**The filename stem must match `[repository].name`.** A file
`devrepo.toml` must declare `name = "devrepo"`; mismatches are an error.

## Schema

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

### Fields

| Field | Required | Notes |
|---|---|---|
| `name` | yes | Must match the filename stem. |
| `description` | no | Free-form. |
| `base_url` | yes | `https://`, `http://`, `file:///abs/path`, or `path:/abs/path`. |
| `enabled` | no, default `true` | Disabled repos are skipped during refresh. |
| `priority` | no, default `50` | DNF convention: lower wins on tie. |
| `signing.gpg_check` | no, default `false` | If true, verify the index signature. |
| `signing.gpg_key_url` | required iff `gpg_check = true` | Fetched once on first refresh (TOFU), then cached. |
| `signing.signature_required` | no, default `false` | If true, missing signatures are a hard error. |
| `refresh.metadata_expire_seconds` | no, default `21600` (6h) | Refresh is skipped while cached metadata is fresher than this. |

## Precedence

User repos with the same `name` as a system repo **override** the system
file completely (no field-level merging). User repos can also add new repos
the system doesn't define.

## Adding a repo from the CLI

```sh
fn repo add devrepo http://localhost:8080/devrepo
fn repo add stable https://repo.feathernix.org/stable/x86_64-linux \
    --gpg-key-url https://repo.feathernix.org/keys/release.asc
```

`fn repo add` writes to the user directory. `fn repo remove` refuses to
delete system files — remove those by hand or via your config-management
tooling.

## TOFU key cache

When `gpg_check = true`, the first successful refresh fetches `gpg_key_url`,
records the fingerprint, and stores the armored key at
`<cache_dir>/keyrings/<name>.asc`. Subsequent refreshes use the cached key
only — FeatherNix never silently re-fetches from `gpg_key_url`. Rotating to
a new key is a deliberate operation (planned: `fn repo trust <name>
--fingerprint <fpr>`).
