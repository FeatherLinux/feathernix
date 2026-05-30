# Signing and verification

FeatherNix uses OpenPGP for repository signatures. The implementation is
pure-Rust via the `pgp` crate; there is no dependency on the `gpg` CLI at
runtime.

## What gets signed

| Object | Signature location | Produced by |
|---|---|---|
| `repodata/index.toml` | `repodata/index.toml.sig` | `fnmaint publish` (or `gpg --detach-sign --armor`). |
| `<payload>.fnpkg.7z` | `<payload>.fnpkg.7z.sig` | `fnmaint sign` (or `gpg --detach-sign --armor`). |

Both files are detached, ASCII-armored OpenPGP signatures.

Signing the index alone would technically be sufficient — the index
records each payload's sha256, so a verified index implies trusted payload
content. We sign payloads independently anyway, matching RPM/DNF, so a
compromised mirror that swaps a payload between index publication and
install is caught at install time.

## Verification policy

The behavior of a repo's `[repository.signing]` block is the product of
`gpg_check` and `signature_required`:

| `gpg_check` | `signature_required` | Behavior |
|---|---|---|
| `false` | (any) | No verification. |
| `true` | `false` | Best-effort. If a `.sig` is present, verify it; if missing, warn and continue. The key is fetched and cached on first encounter. |
| `true` | `true` | Mandatory. Missing or invalid `.sig` is a hard error: `fn repo refresh` aborts, `fn install` aborts. |

`gpg_check = true` with no `gpg_key_url` is a configuration error.

## TOFU key cache

On the first successful refresh of a repo with `gpg_check = true`:

1. Fetch the armored key from `gpg_key_url`.
2. Write it to `<cache_dir>/keyrings/<repo>.asc`.
3. Record the fingerprint at `<cache_dir>/keyrings/<repo>.fpr` for display.

Subsequent refreshes use the cached key. **FeatherNix never silently
re-fetches from `gpg_key_url`** — rotating to a new key is a deliberate
operation. A future `fn repo trust <name> --fingerprint <fpr>` will provide
the supported rotation path; today, deleting the cached key file forces a
fresh TOFU on the next refresh.

## Signing on the producer side

The signing API lives in `feathernix-sign`, built on the same `pgp`
primitives the verifier uses.

### `fnmaint sign`

```sh
fnmaint sign hello-0.1.0-1-x86_64-linux.fnpkg.7z \
    --key ~/.config/feathernix/signing/dev.asc
```

Produces `<archive>.sig` next to the archive. The key file is an
ASCII-armored OpenPGP secret key.

### `fnmaint publish`

```sh
fnmaint publish --repo /srv/devrepo \
    --key ~/.config/feathernix/signing/dev.asc \
    hello-*.fnpkg.7z
```

Copies the artifacts into `<repo>/packages/`, regenerates
`<repo>/repodata/index.toml` from the directory contents, signs the new
index, and atomically swaps it into place (write-to-temp + rename).

The signing key can also come from `$FNMAINT_SIGNING_KEY`.

## Out of scope (today)

- Multi-key keyrings per repo (rotate-by-overlap).
- Subkey expiry / revocation checks beyond what `pgp` enforces by default.
- Signature on `flake.lock.toml` inputs.
- Agent-mediated passphrase entry beyond reading from stdin.
