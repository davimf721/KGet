//! Persistent queue of the daemon (`jobs.db`).
//!
//! Every job the daemon knows about is saved as a JSON snapshot
//! ([`JobInfo`]) in a SQLite table, updated on each state change. On start
//! the daemon [restores](crate::jobs::JobManager::restore) them: queued and
//! running jobs are queued again and continue from their partial data,
//! paused/cancelled/failed/completed ones keep their state until removed.
//!
//! The file is private to the owner (`0600`): snapshots contain the full job,
//! including URL credentials and HTTP headers needed to resume it. The schema
//! is versioned with `PRAGMA user_version` like `history.db`.

use crate::jobs::{JobId, JobInfo};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::path::{Path, PathBuf};
use std::time::Duration;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Finished jobs kept across restarts (newest first); older ones are pruned.
pub const MAX_FINISHED: usize = 500;

/// Schema migrations; entry `n` upgrades `user_version` `n` → `n + 1`.
/// Only ever append.
const MIGRATIONS: &[&str] = &[
    // v1: initial schema (KGet 1.8)
    "CREATE TABLE IF NOT EXISTS jobs (
        id          INTEGER PRIMARY KEY,
        state       TEXT NOT NULL,
        info        TEXT NOT NULL,
        updated_at  INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS jobs_state ON jobs(state, updated_at);",
];

/// Handle on `jobs.db`.
pub struct JobStore {
    conn: Connection,
    path: PathBuf,
}

impl std::fmt::Debug for JobStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobStore")
            .field("path", &self.path)
            .finish()
    }
}

impl JobStore {
    /// Open (creating and migrating if needed) the database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BoxError> {
        let path = path.as_ref().to_path_buf();
        if let Some(p) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(p)?;
        }
        let conn = Connection::open(&path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        super::restrict_to_owner(&path)?;
        let mut store = Self { conn, path };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> rusqlite::Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i32 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        for sql in MIGRATIONS.iter().skip(version.max(0) as usize) {
            tx.execute_batch(sql)?;
        }
        if (version as usize) < MIGRATIONS.len() {
            tx.pragma_update(None, "user_version", MIGRATIONS.len() as i32)?;
        }
        tx.commit()
    }

    /// Path of the database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Insert or replace the snapshot of a job.
    pub fn save(&self, info: &JobInfo) -> Result<(), BoxError> {
        self.conn.execute(
            "INSERT INTO jobs (id, state, info, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET state = ?2, info = ?3, updated_at = ?4",
            params![
                info.id as i64,
                info.state.as_str(),
                serde_json::to_string(info)?,
                super::unix_now() as i64
            ],
        )?;
        Ok(())
    }

    /// Forget a job.
    pub fn delete(&self, id: JobId) -> Result<(), BoxError> {
        self.conn
            .execute("DELETE FROM jobs WHERE id = ?1", params![id as i64])?;
        Ok(())
    }

    /// The snapshot of one job.
    pub fn get(&self, id: JobId) -> Result<Option<JobInfo>, BoxError> {
        let text: Option<String> = self
            .conn
            .query_row(
                "SELECT info FROM jobs WHERE id = ?1",
                params![id as i64],
                |r| r.get(0),
            )
            .optional()?;
        Ok(text.and_then(|t| serde_json::from_str(&t).ok()))
    }

    /// Every saved job, oldest id first. Rows that cannot be parsed (written
    /// by a newer, incompatible KGet) are skipped.
    pub fn load_all(&self) -> Result<Vec<JobInfo>, BoxError> {
        let mut stmt = self.conn.prepare("SELECT info FROM jobs ORDER BY id")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for text in rows {
            if let Ok(info) = serde_json::from_str::<JobInfo>(&text?) {
                out.push(info);
            }
        }
        Ok(out)
    }

    /// Delete completed/failed/cancelled jobs beyond the newest `keep`.
    pub fn prune_finished(&self, keep: usize) -> Result<usize, BoxError> {
        Ok(self.conn.execute(
            "DELETE FROM jobs WHERE state IN ('completed', 'failed', 'cancelled')
             AND id NOT IN (SELECT id FROM jobs
                            WHERE state IN ('completed', 'failed', 'cancelled')
                            ORDER BY updated_at DESC, id DESC LIMIT ?1)",
            params![keep as i64],
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::DownloadState;
    use crate::runner::DownloadJob;

    fn info(id: JobId, state: DownloadState) -> JobInfo {
        let job = DownloadJob::new(format!("https://example.com/{id}"));
        JobInfo {
            id,
            protocol: job.protocol(),
            job,
            state,
            created_at: 1,
            started_at: None,
            finished_at: None,
            progress: Default::default(),
            path: None,
            error: None,
            handed_off: false,
        }
    }

    #[test]
    fn save_load_delete_prune() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        {
            let s = JobStore::open(&path).unwrap();
            s.save(&info(1, DownloadState::Queued)).unwrap();
            s.save(&info(2, DownloadState::Completed)).unwrap();
            s.save(&info(3, DownloadState::Failed)).unwrap();
            s.save(&info(1, DownloadState::Paused)).unwrap();
        }
        let s = JobStore::open(&path).unwrap();
        let all = s.load_all().unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].state, DownloadState::Paused);
        assert_eq!(s.prune_finished(1).unwrap(), 1);
        s.delete(1).unwrap();
        assert!(s.get(1).unwrap().is_none());
        assert_eq!(s.load_all().unwrap().len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
