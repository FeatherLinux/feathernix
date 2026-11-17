# Buildd

`feathernix-buildd` is the distributed build daemon. It is modelled on
Debian's buildd: a **controller** schedules jobs and persists state; one or
more **workers** pull jobs, build inside Bubblewrap via `feathernix-build`,
and upload signed artifacts back. The controller re-publishes the
repository index when an artifact lands.

The same binary runs both roles; mode is chosen by the first subcommand.

## Modes

```sh
feathernix-buildd controller \
    --listen 0.0.0.0:7070 \
    --state /var/lib/feathernix-buildd \
    --publish-repo /srv/devrepo

feathernix-buildd worker \
    --controller https://buildd.feathernix.org \
    --token /etc/feathernix/worker.token \
    --workdir /var/lib/feathernix-worker
```

Operators interact with the controller via `feathernix-buildctl`:

```sh
feathernix-buildctl jobs list [--state running]
feathernix-buildctl jobs cancel <id>
feathernix-buildctl workers list
feathernix-buildctl workers drain <id>
```

Maintainers submit jobs with `fnmaint submit --controller <url>` from a
recipe directory.

## Storage

The controller uses its **own** SQLite file under `--state`, separate from
the end-user `db.sqlite`. Schema lives in `0002_buildd/up.sql` and adds
three tables:

| Table | Purpose |
|---|---|
| `build_jobs` | id, recipe blob path, state, assigned worker id, attempts, created/completed timestamps. |
| `build_workers` | id, hostname, capabilities, last-heartbeat. |
| `build_artifacts` | id, job_id, path, sha256, signed (bool). |

Access is via `rusqlite` directly (not Diesel). Single writer; the version
pin (`rusqlite = "0.43"` / `libsqlite3-sys = "0.40"`) is enforced
workspace-wide — see [AGENTS.md](../AGENTS.md#dependency-landmines).

## Protocol

HTTP + JSON between controller and worker. Workers authenticate with a
long-lived bearer token: `Authorization: Bearer <token>`. Route names live
in `feathernix-buildd-proto` so the two sides cannot drift.

| Method + path | Purpose |
|---|---|
| `POST /v1/jobs` | Submit a recipe (tar.gz of recipe dir, or git URL + ref). Returns `{ job_id }`. |
| `GET /v1/jobs/{id}` | State: `pending \| assigned \| running \| succeeded \| failed \| cancelled`. |
| `GET /v1/jobs?state=…` | List (for `feathernix-buildctl`). |
| `POST /v1/workers/register` | Worker announces itself; returns worker id + capabilities ack. |
| `POST /v1/workers/{id}/heartbeat` | Every 30s. Controller reaps workers silent >2min. |
| `GET /v1/workers/{id}/lease` | Long-poll up to 30s for the next assigned job; returns recipe blob. |
| `POST /v1/workers/{id}/jobs/{job_id}/log` | Streaming log append. |
| `POST /v1/workers/{id}/jobs/{job_id}/artifact` | Multipart upload of `.fnpkg.7z` + `.sig`. |
| `POST /v1/workers/{id}/jobs/{job_id}/complete` | Finalize with status. |

## Auto-publish

When a worker completes a job successfully, the controller invokes
`feathernix-publish` against `--publish-repo`: regenerate
`repodata/index.toml` from the directory contents, sign with the
controller's signing key, atomically swap. End-to-end demo:

```sh
fnmaint submit --controller http://127.0.0.1:7070
feathernix-buildctl jobs list             # job → succeeded
fn repo refresh && fn install <pkg>       # served via the buildd-produced artifact
```

## Operational notes

- The controller runs `feathernix-publish` synchronously on the job-complete
  path. If publishing fails, the job is marked succeeded but the publish is
  retried; the artifact is still durably stored.
- Workers must have `bwrap`, `git`, and `tar` on `$PATH`. The same
  requirements as `fnmaint build` apply, because workers invoke
  `feathernix-build` directly.
- Worker network access is required for source fetch and artifact upload.
  The build sandbox itself is network-isolated regardless of worker network
  posture.

## Out of scope (today)

- Reproducibility: bit-exact rebuilds, source-date-epoch wiring,
  deterministic compression.
- Multi-arch: cross-build support and per-arch worker pools.
- A web UI for the controller (`feathernix-buildctl` + JSON output covers
  ops needs initially).
- Source rev watcher (cron-driven rebuild against `HEAD` of an upstream).
- Postgres backend for the controller — SQLite is sufficient until job
  volume demands more.
