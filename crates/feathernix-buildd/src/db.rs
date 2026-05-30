use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};

use feathernix_buildd_proto::{JobInfo, JobState};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS build_jobs (
    id TEXT PRIMARY KEY,
    package_name TEXT NOT NULL,
    package_version TEXT NOT NULL,
    package_arch TEXT NOT NULL,
    recipe_toml TEXT NOT NULL,
    state TEXT NOT NULL,
    assigned_worker_id TEXT NULL,
    error TEXT NULL,
    log TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    assigned_at TEXT NULL,
    completed_at TEXT NULL,
    source_tree_sha256 TEXT NULL,
    note TEXT NULL
);

CREATE INDEX IF NOT EXISTS idx_build_jobs_state ON build_jobs(state);

CREATE TABLE IF NOT EXISTS build_workers (
    id TEXT PRIMARY KEY,
    hostname TEXT NOT NULL,
    arch TEXT NOT NULL,
    token TEXT NOT NULL,
    capabilities TEXT NOT NULL DEFAULT '[]',
    registered_at TEXT NOT NULL,
    last_heartbeat_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS build_artifacts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL,
    filename TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    stored_path TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(job_id) REFERENCES build_jobs(id) ON DELETE CASCADE
);
"#;

#[derive(Clone)]
pub struct BuilddDb {
    conn: Arc<Mutex<Connection>>,
}

impl BuilddDb {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path).context("open buildd db")?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        conn.execute_batch(SCHEMA).context("apply schema")?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn insert_job(
        &self,
        id: &str,
        package_name: &str,
        package_version: &str,
        package_arch: &str,
        recipe_toml: &str,
        note: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.lock().unwrap().execute(
            "INSERT INTO build_jobs (id, package_name, package_version, package_arch, recipe_toml, state, created_at, note) \
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?7)",
            params![id, package_name, package_version, package_arch, recipe_toml, now, note],
        )?;
        Ok(())
    }

    pub fn get_job(&self, id: &str) -> Result<Option<JobInfo>> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, state, package_name, package_version, package_arch, created_at, \
                        assigned_worker_id, completed_at, error \
                 FROM build_jobs WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        row.get::<_, Option<String>>(8)?,
                    ))
                },
            )
            .optional()?;
        Ok(row.map(|(id, state, name, version, arch, created_at, worker, completed, err)| JobInfo {
            job_id: id,
            state: JobState::from_str(&state).unwrap_or(JobState::Pending),
            package_name: name,
            package_version: version,
            package_arch: arch,
            created_at,
            assigned_worker: worker,
            completed_at: completed,
            error: err,
        }))
    }

    pub fn list_jobs(&self, state_filter: Option<JobState>) -> Result<Vec<JobInfo>> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT id, state, package_name, package_version, package_arch, created_at, \
                    assigned_worker_id, completed_at, error \
             FROM build_jobs",
        );
        let mut params_str: Vec<String> = Vec::new();
        if let Some(s) = state_filter {
            sql.push_str(" WHERE state = ?1");
            params_str.push(s.as_str().to_string());
        }
        sql.push_str(" ORDER BY created_at DESC");
        let mut stmt = conn.prepare(&sql)?;
        let rows = if params_str.is_empty() {
            stmt.query_map([], row_to_jobinfo)?.collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(params![params_str[0]], row_to_jobinfo)?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(rows)
    }

    /// Atomically claim the oldest pending job for `worker_id`. Returns the job's id+recipe if any.
    pub fn lease_next_job(&self, worker_id: &str) -> Result<Option<(String, String)>> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let row: Option<(String, String)> = tx
            .query_row(
                "SELECT id, recipe_toml FROM build_jobs \
                 WHERE state = 'pending' ORDER BY created_at ASC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((ref id, _)) = row {
            let now = Utc::now().to_rfc3339();
            tx.execute(
                "UPDATE build_jobs SET state='assigned', assigned_worker_id=?1, assigned_at=?2 WHERE id=?3",
                params![worker_id, now, id],
            )?;
        }
        tx.commit()?;
        Ok(row)
    }

    pub fn mark_running(&self, job_id: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE build_jobs SET state='running' WHERE id=?1",
            params![job_id],
        )?;
        Ok(())
    }

    pub fn complete_job(
        &self,
        job_id: &str,
        state: JobState,
        error: Option<&str>,
        source_tree_sha256: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.lock().unwrap().execute(
            "UPDATE build_jobs SET state=?1, error=?2, completed_at=?3, source_tree_sha256=?4 WHERE id=?5",
            params![state.as_str(), error, now, source_tree_sha256, job_id],
        )?;
        Ok(())
    }

    pub fn append_log(&self, job_id: &str, chunk: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE build_jobs SET log = log || ?1 WHERE id = ?2",
            params![chunk, job_id],
        )?;
        Ok(())
    }

    pub fn get_log(&self, job_id: &str) -> Result<String> {
        let conn = self.conn.lock().unwrap();
        let log: String = conn
            .query_row(
                "SELECT log FROM build_jobs WHERE id = ?1",
                params![job_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or_default();
        Ok(log)
    }

    pub fn register_worker(
        &self,
        id: &str,
        hostname: &str,
        arch: &str,
        token: &str,
        capabilities_json: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.lock().unwrap().execute(
            "INSERT INTO build_workers (id, hostname, arch, token, capabilities, registered_at, last_heartbeat_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![id, hostname, arch, token, capabilities_json, now],
        )?;
        Ok(())
    }

    pub fn worker_heartbeat(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.lock().unwrap().execute(
            "UPDATE build_workers SET last_heartbeat_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    pub fn worker_token_matches(&self, id: &str, token: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM build_workers WHERE id = ?1 AND token = ?2",
                params![id, token],
                |r| r.get(0),
            )
            .unwrap_or(0);
        Ok(count > 0)
    }

    pub fn record_artifact(
        &self,
        job_id: &str,
        filename: &str,
        sha256: &str,
        size_bytes: i64,
        stored_path: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.lock().unwrap().execute(
            "INSERT INTO build_artifacts (job_id, filename, sha256, size_bytes, stored_path, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![job_id, filename, sha256, size_bytes, stored_path, now],
        )?;
        Ok(())
    }

    pub fn get_recipe(&self, job_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT recipe_toml FROM build_jobs WHERE id = ?1",
                params![job_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(row)
    }

    pub fn list_workers(&self) -> Result<Vec<(String, String, String, String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, hostname, arch, registered_at, last_heartbeat_at FROM build_workers \
             ORDER BY registered_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

fn row_to_jobinfo(row: &rusqlite::Row<'_>) -> rusqlite::Result<JobInfo> {
    Ok(JobInfo {
        job_id: row.get(0)?,
        state: JobState::from_str(&row.get::<_, String>(1)?).unwrap_or(JobState::Pending),
        package_name: row.get(2)?,
        package_version: row.get(3)?,
        package_arch: row.get(4)?,
        created_at: row.get(5)?,
        assigned_worker: row.get(6)?,
        completed_at: row.get(7)?,
        error: row.get(8)?,
    })
}

pub fn now_rfc3339() -> String {
    let now: DateTime<Utc> = Utc::now();
    now.to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get_job_round_trip() {
        let db = BuilddDb::open_in_memory().unwrap();
        db.insert_job("j1", "hello", "1.0.0", "x86_64-linux", "<recipe>", None).unwrap();
        let info = db.get_job("j1").unwrap().unwrap();
        assert_eq!(info.state, JobState::Pending);
        assert_eq!(info.package_name, "hello");
    }

    #[test]
    fn lease_next_atomically_assigns_one_job() {
        let db = BuilddDb::open_in_memory().unwrap();
        db.insert_job("j1", "a", "1", "x", "r1", None).unwrap();
        db.insert_job("j2", "b", "1", "x", "r2", None).unwrap();
        let leased = db.lease_next_job("w1").unwrap().unwrap();
        assert_eq!(leased.0, "j1");
        let info = db.get_job("j1").unwrap().unwrap();
        assert_eq!(info.state, JobState::Assigned);
        let next = db.lease_next_job("w2").unwrap().unwrap();
        assert_eq!(next.0, "j2");
        // No more pending jobs.
        assert!(db.lease_next_job("w3").unwrap().is_none());
    }

    #[test]
    fn worker_registration_and_token_check() {
        let db = BuilddDb::open_in_memory().unwrap();
        db.register_worker("w1", "host1", "x86_64-linux", "secret-token", "[]").unwrap();
        assert!(db.worker_token_matches("w1", "secret-token").unwrap());
        assert!(!db.worker_token_matches("w1", "wrong").unwrap());
    }
}
