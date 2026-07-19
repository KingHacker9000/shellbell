use crate::{Session, activity::ActivityAction, ipc::DaemonStatus};
use anyhow::{Context, Result};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{fs, path::Path, time::Duration};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedRing {
    pub event_id: Uuid,
    pub message: Option<String>,
    pub targets: Vec<String>,
    pub created_at_ms: i64,
    pub attempts: u32,
}

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    pub async fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            crate::config::ensure_private_dir(parent)?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        sqlx::raw_sql(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
              id TEXT PRIMARY KEY,
              data_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS ring_queue (
              event_id TEXT PRIMARY KEY,
              message TEXT,
              targets_json TEXT NOT NULL,
              created_at_ms INTEGER NOT NULL,
              attempts INTEGER NOT NULL DEFAULT 0,
              next_attempt_ms INTEGER NOT NULL,
              status TEXT NOT NULL CHECK(status IN ('pending', 'authorization_failure')),
              last_error TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_ring_queue_due
              ON ring_queue(status, next_attempt_ms, created_at_ms);
            CREATE TABLE IF NOT EXISTS local_meta (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );
            "#,
        )
        .execute(&pool)
        .await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(Self { pool })
    }

    pub async fn save_sessions<'a>(
        &self,
        sessions: impl Iterator<Item = &'a Session>,
    ) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM sessions")
            .execute(&mut *transaction)
            .await?;
        for session in sessions {
            sqlx::query("INSERT INTO sessions(id,data_json) VALUES(?,?)")
                .bind(session.id.to_string())
                .bind(serde_json::to_string(session)?)
                .execute(&mut *transaction)
                .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn load_sessions(&self) -> Result<Vec<Session>> {
        let rows = sqlx::query("SELECT data_json FROM sessions")
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                serde_json::from_str(row.get("data_json"))
                    .context("invalid persisted shell session")
            })
            .collect()
    }

    pub async fn enqueue(
        &self,
        action: &ActivityAction,
        now_ms: i64,
        queue_limit: usize,
        event_max_age: Duration,
    ) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        prune_queue_in(&mut transaction, now_ms, event_max_age).await?;
        enqueue_in(&mut transaction, action, now_ms, queue_limit).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn save_sessions_and_enqueue<'a>(
        &self,
        sessions: impl Iterator<Item = &'a Session>,
        actions: &[ActivityAction],
        now_ms: i64,
        queue_limit: usize,
        event_max_age: Duration,
    ) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM sessions")
            .execute(&mut *transaction)
            .await?;
        for session in sessions {
            sqlx::query("INSERT INTO sessions(id,data_json) VALUES(?,?)")
                .bind(session.id.to_string())
                .bind(serde_json::to_string(session)?)
                .execute(&mut *transaction)
                .await?;
        }
        prune_queue_in(&mut transaction, now_ms, event_max_age).await?;
        for action in actions {
            enqueue_in(&mut transaction, action, now_ms, queue_limit).await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn next_due(
        &self,
        now_ms: i64,
        event_max_age: Duration,
    ) -> Result<Option<QueuedRing>> {
        self.expire_old(now_ms, event_max_age).await?;
        let row = sqlx::query("SELECT event_id,message,targets_json,created_at_ms,attempts FROM ring_queue WHERE status='pending' AND next_attempt_ms<=? ORDER BY created_at_ms LIMIT 1")
            .bind(now_ms)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| {
            Ok(QueuedRing {
                event_id: Uuid::parse_str(row.get("event_id"))?,
                message: row.get("message"),
                targets: serde_json::from_str(row.get("targets_json"))?,
                created_at_ms: row.get("created_at_ms"),
                attempts: u32::try_from(row.get::<i64, _>("attempts")).unwrap_or(u32::MAX),
            })
        })
        .transpose()
    }

    pub async fn mark_delivered(&self, event_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM ring_queue WHERE event_id=?")
            .bind(event_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn mark_retry(
        &self,
        event_id: Uuid,
        next_attempt_ms: i64,
        diagnostic: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE ring_queue SET attempts=attempts+1,next_attempt_ms=?,last_error=? WHERE event_id=?")
            .bind(next_attempt_ms)
            .bind(truncate_diagnostic(diagnostic))
            .bind(event_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn mark_authorization_failure(&self, event_id: Uuid, diagnostic: &str) -> Result<()> {
        sqlx::query("UPDATE ring_queue SET status='authorization_failure',attempts=attempts+1,last_error=? WHERE event_id=?")
            .bind(truncate_diagnostic(diagnostic))
            .bind(event_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn discard_permanent(&self, event_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM ring_queue WHERE event_id=?")
            .bind(event_id.to_string())
            .execute(&self.pool)
            .await?;
        self.increment_meta("dropped_events", 1).await
    }

    pub async fn expire_old(&self, now_ms: i64, event_max_age: Duration) -> Result<u64> {
        let cutoff =
            now_ms.saturating_sub(i64::try_from(event_max_age.as_millis()).unwrap_or(i64::MAX));
        let removed = sqlx::query("DELETE FROM ring_queue WHERE created_at_ms < ?")
            .bind(cutoff)
            .execute(&self.pool)
            .await?
            .rows_affected();
        self.increment_meta("dropped_events", removed).await?;
        Ok(removed)
    }

    pub async fn daemon_status(&self, active_sessions: usize) -> Result<DaemonStatus> {
        let pending_events: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM ring_queue WHERE status='pending'")
                .fetch_one(&self.pool)
                .await?;
        let authorization_failures: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM ring_queue WHERE status='authorization_failure'",
        )
        .fetch_one(&self.pool)
        .await?;
        let dropped_events = self.meta_u64("dropped_events").await?;
        Ok(DaemonStatus {
            pending_events: u32::try_from(pending_events).unwrap_or(u32::MAX),
            authorization_failures: u32::try_from(authorization_failures).unwrap_or(u32::MAX),
            dropped_events,
            active_sessions: u32::try_from(active_sessions).unwrap_or(u32::MAX),
        })
    }

    async fn increment_meta(&self, key: &str, amount: u64) -> Result<()> {
        if amount == 0 {
            return Ok(());
        }
        sqlx::query("INSERT INTO local_meta(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=CAST(local_meta.value AS INTEGER)+CAST(excluded.value AS INTEGER)")
            .bind(key)
            .bind(amount.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn meta_u64(&self, key: &str) -> Result<u64> {
        let value: Option<String> = sqlx::query_scalar("SELECT value FROM local_meta WHERE key=?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(value
            .and_then(|value| value.parse().ok())
            .unwrap_or_default())
    }
}

async fn increment_meta_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    key: &str,
    amount: u64,
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    sqlx::query("INSERT INTO local_meta(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=CAST(local_meta.value AS INTEGER)+CAST(excluded.value AS INTEGER)")
        .bind(key)
        .bind(amount.to_string())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn prune_queue_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    now_ms: i64,
    event_max_age: Duration,
) -> Result<()> {
    let max_age_ms = i64::try_from(event_max_age.as_millis()).unwrap_or(i64::MAX);
    let expired = sqlx::query("DELETE FROM ring_queue WHERE created_at_ms < ?")
        .bind(now_ms.saturating_sub(max_age_ms))
        .execute(&mut **transaction)
        .await?
        .rows_affected();
    increment_meta_in(transaction, "dropped_events", expired).await
}

async fn enqueue_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    action: &ActivityAction,
    now_ms: i64,
    queue_limit: usize,
) -> Result<()> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ring_queue")
        .fetch_one(&mut **transaction)
        .await?;
    if count >= i64::try_from(queue_limit).unwrap_or(i64::MAX) {
        let removed = sqlx::query(
            "DELETE FROM ring_queue WHERE event_id=(SELECT event_id FROM ring_queue ORDER BY created_at_ms LIMIT 1)",
        )
        .execute(&mut **transaction)
        .await?
        .rows_affected();
        increment_meta_in(transaction, "dropped_events", removed).await?;
    }
    sqlx::query("INSERT OR IGNORE INTO ring_queue(event_id,message,targets_json,created_at_ms,next_attempt_ms,status) VALUES(?,?,?,?,?,'pending')")
        .bind(action.event_id.to_string())
        .bind(&action.message)
        .bind(serde_json::to_string(&action.targets)?)
        .bind(now_ms)
        .bind(now_ms)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

fn truncate_diagnostic(value: &str) -> String {
    value.chars().take(240).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn store() -> (tempfile::TempDir, Store) {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(&directory.path().join("state.db"))
            .await
            .unwrap();
        (directory, store)
    }

    fn action(id: Uuid) -> ActivityAction {
        ActivityAction {
            event_id: id,
            session_id: None,
            message: Some("Shell is ready".into()),
            targets: vec!["phone".into()],
            automatic: true,
            active_duration_ms: Some(120_000),
        }
    }

    #[tokio::test]
    async fn queue_persists_event_id_and_retry_metadata() {
        let (directory, first) = store().await;
        let id = Uuid::new_v4();
        first
            .enqueue(&action(id), 1_000, 100, Duration::from_secs(86_400))
            .await
            .unwrap();
        first.mark_retry(id, 5_000, "offline").await.unwrap();
        drop(first);
        let reopened = Store::open(&directory.path().join("state.db"))
            .await
            .unwrap();
        assert!(
            reopened
                .next_due(4_999, Duration::from_secs(86_400))
                .await
                .unwrap()
                .is_none()
        );
        let queued = reopened
            .next_due(5_000, Duration::from_secs(86_400))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(queued.event_id, id);
        assert_eq!(queued.attempts, 1);
    }

    #[tokio::test]
    async fn authorization_failure_is_not_retried_and_is_reported() {
        let (_directory, store) = store().await;
        let id = Uuid::new_v4();
        store
            .enqueue(&action(id), 1_000, 100, Duration::from_secs(86_400))
            .await
            .unwrap();
        store
            .mark_authorization_failure(id, "unauthorized")
            .await
            .unwrap();
        assert!(
            store
                .next_due(99_000, Duration::from_secs(86_400))
                .await
                .unwrap()
                .is_none()
        );
        let status = store.daemon_status(0).await.unwrap();
        assert_eq!(status.authorization_failures, 1);
    }

    #[tokio::test]
    async fn queue_limit_evicts_oldest_and_age_expiry_is_bounded() {
        let (_directory, store) = store().await;
        let oldest = Uuid::new_v4();
        let newest = Uuid::new_v4();
        store
            .enqueue(&action(oldest), 1_000, 1, Duration::from_secs(60))
            .await
            .unwrap();
        store
            .enqueue(&action(newest), 2_000, 1, Duration::from_secs(60))
            .await
            .unwrap();
        assert_eq!(
            store
                .next_due(2_000, Duration::from_secs(60))
                .await
                .unwrap()
                .unwrap()
                .event_id,
            newest
        );
        assert_eq!(store.daemon_status(0).await.unwrap().dropped_events, 1);
        assert_eq!(
            store
                .expire_old(62_001, Duration::from_secs(60))
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn sessions_round_trip_without_live_monotonic_timestamps() {
        let (_directory, store) = store().await;
        let session = Session::new(
            Uuid::new_v4(),
            42,
            crate::ShellType::Zsh,
            None,
            10,
            5,
            vec![],
            100,
        );
        store.save_sessions([&session].into_iter()).await.unwrap();
        assert_eq!(store.load_sessions().await.unwrap(), vec![session]);
    }
}
