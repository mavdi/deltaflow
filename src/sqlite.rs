//! SQLite-based recorder implementation.

use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::recorder::{Recorder, RunId, RunStatus, StepId, StepStatus};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS delta_runs (
    id INTEGER PRIMARY KEY,
    pipeline_name TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT,
    status TEXT NOT NULL DEFAULT 'running',
    error_message TEXT
);

CREATE TABLE IF NOT EXISTS delta_steps (
    id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL REFERENCES delta_runs(id),
    step_name TEXT NOT NULL,
    step_index INTEGER NOT NULL,
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT,
    status TEXT NOT NULL DEFAULT 'running',
    attempt INTEGER NOT NULL DEFAULT 1,
    error_message TEXT
);

CREATE INDEX IF NOT EXISTS idx_delta_runs_pipeline ON delta_runs(pipeline_name, started_at);
CREATE INDEX IF NOT EXISTS idx_delta_runs_entity ON delta_runs(entity_id);
CREATE INDEX IF NOT EXISTS idx_delta_steps_run ON delta_steps(run_id);
"#;

/// SQLite-based recorder for pipeline execution history.
#[derive(Clone)]
pub struct SqliteRecorder {
    pool: SqlitePool,
}

impl SqliteRecorder {
    /// Create a new SQLite recorder with the given connection pool.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Run database migrations to create required tables.
    pub async fn run_migrations(&self) -> anyhow::Result<()> {
        for statement in SCHEMA.split(';') {
            let trimmed = statement.trim();
            if !trimmed.is_empty() {
                sqlx::query(trimmed).execute(&self.pool).await?;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Recorder for SqliteRecorder {
    async fn start_run(&self, pipeline_name: &str, entity_id: &str) -> anyhow::Result<RunId> {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO delta_runs (pipeline_name, entity_id) VALUES (?, ?) RETURNING id",
        )
        .bind(pipeline_name)
        .bind(entity_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(RunId(id))
    }

    async fn start_step(
        &self,
        run_id: RunId,
        step_name: &str,
        step_index: u32,
    ) -> anyhow::Result<StepId> {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO delta_steps (run_id, step_name, step_index) VALUES (?, ?, ?) RETURNING id",
        )
        .bind(run_id.0)
        .bind(step_name)
        .bind(step_index as i32)
        .fetch_one(&self.pool)
        .await?;

        Ok(StepId(id))
    }

    async fn complete_step(&self, step_id: StepId, status: StepStatus) -> anyhow::Result<()> {
        let (status_str, error_msg, attempt) = match status {
            StepStatus::Completed => ("completed", None, 1u32),
            StepStatus::Failed { error, attempt } => ("failed", Some(error), attempt),
        };

        sqlx::query(
            "UPDATE delta_steps SET completed_at = datetime('now'), status = ?, error_message = ?, attempt = ? WHERE id = ?",
        )
        .bind(status_str)
        .bind(error_msg)
        .bind(attempt as i32)
        .bind(step_id.0)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn complete_run(&self, run_id: RunId, status: RunStatus) -> anyhow::Result<()> {
        let (status_str, error_msg) = match status {
            RunStatus::Completed => ("completed", None),
            RunStatus::Failed { error } => ("failed", Some(error)),
        };

        sqlx::query(
            "UPDATE delta_runs SET completed_at = datetime('now'), status = ?, error_message = ? WHERE id = ?",
        )
        .bind(status_str)
        .bind(error_msg)
        .bind(run_id.0)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
