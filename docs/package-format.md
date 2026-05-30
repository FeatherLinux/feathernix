# Package format

A FeatherNix package is a 7z archive named
`<name>-<version>-<release>-<arch>.fnpkg.7z`. The archive root must contain a
`manifest.toml`; the `payload/` directory holds the files that will live at
the store path once installed.

## Archive layout

```
manifest.toml         # required at archive root
payload/              # files to install, rooted at the future store path
metadata/files.toml   # optional per-file mode/uid/gid/caps (RPM-style)
metadata/scripts/     # optional install hooks (not run in MVP)
```

7z payloads are handled entirely in Rust via `sevenz-rust2`. There is no
shell-out to `7z`.

## `manifest.toml`

```toml
[package]
name    = "ripgrep"
version = "14.1.0"
release = "1"
epoch   = 0
arch    = "x86_64-linux"
summary = "Fast recursive grep"
license = "MIT OR Unlicense"
url     = "https://github.com/BurntSushi/ripgrep"

[[dependencies.requires]]
name               = "glibc"
version_constraint = ">= 2.17"

[[dependencies.provides]]
name    = "rg"
version = "14.1.0"

[outputs]
default = "out"
```

## Repository index

A repository serves a TOML index at `<base_url>/repodata/index.toml`. If the
repo is signed, a detached ASCII-armored OpenPGP signature lives alongside
at `<base_url>/repodata/index.toml.sig`.

```toml
[index]
format_version = 1
generated_at   = "2026-05-21T10:00:00Z"
repository     = "feathernix-stable"

[[packages]]
name           = "ripgrep"
version        = "14.1.0"
release        = "1"
epoch          = 0
arch           = "x86_64-linux"
payload_path   = "packages/ripgrep-14.1.0-1-x86_64-linux.fnpkg.7z"
payload_sha256 = "abc123..."
payload_size   = 1234567
dependencies   = ["glibc >= 2.17"]
provides       = ["rg = 14.1.0"]
```

Each payload may have its own detached sibling signature at
`<payload_path>.sig`. The index sha256 already chains trust to payloads via
`payload_sha256`, but a per-payload signature catches a compromised mirror
that swaps a payload between index publication and install.

## Verification policy

The behavior of `[repository.signing]` flags in the repo TOML is documented
in [signing.md](signing.md). Briefly:

- `gpg_check = false` → no verification (today's default for unsigned dev
  repos).
- `gpg_check = true, signature_required = false` → best-effort.
- `gpg_check = true, signature_required = true` → mandatory; missing or
  invalid signatures abort the refresh/install.
