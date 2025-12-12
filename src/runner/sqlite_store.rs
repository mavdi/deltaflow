//! SQLite implementation of TaskStore.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use super::store::{StoredTask, TaskError, TaskId, TaskStore};

/// SQLite-backed task store.
pub struct SqliteTaskStore {
    pool: SqlitePool,
}

impl SqliteTaskStore {
    /// Create a new SqliteTaskStore.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Run migrations to create the tasks table.
    pub async fn run_migrations(&self) -> Result<(), TaskError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS delta_tasks (
                id INTEGER PRIMARY KEY,
                pipeline TEXT NOT NULL,
                input TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                error_message TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                started_at TEXT,
                completed_at TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| TaskError::StorageError(e.to_string()))?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_delta_tasks_status
            ON delta_tasks(status, created_at)
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| TaskError::StorageError(e.to_string()))?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_delta_tasks_pipeline
            ON delta_tasks(pipeline, status)
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| TaskError::StorageError(e.to_string()))?;

        Ok(())
    }
}

#[async_trait]
impl TaskStore for SqliteTaskStore {
    async fn enqueue(&self, pipeline: &str, input: serde_json::Value) -> Result<TaskId, TaskError> {
        let input_str = serde_json::to_string(&input)
            .map_err(|e| TaskError::SerializationError(e.to_string()))?;

        let result = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO delta_tasks (pipeline, input)
            VALUES (?, ?)
            RETURNING id
            "#,
        )
        .bind(pipeline)
        .bind(input_str)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| TaskError::StorageError(e.to_string()))?;

        Ok(TaskId(result))
    }

    async fn claim(&self, limit: usize) -> Result<Vec<StoredTask>, TaskError> {
        // SQLite doesn't support UPDATE ... LIMIT with RETURNING directly,
        // so we do it in two steps within a transaction
        let mut tx = self.pool
            .begin()
            .await
            .map_err(|e| TaskError::StorageError(e.to_string()))?;

        // Get IDs of tasks to claim
        let ids: Vec<i64> = sqlx::query_scalar(
            r#"
            SELECT id FROM delta_tasks
            WHERE status = 'pending'
            ORDER BY created_at
            LIMIT ?
            "#,
        )
        .bind(limit as i64)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| TaskError::StorageError(e.to_string()))?;

        if ids.is_empty() {
            tx.commit()
                .await
                .map_err(|e| TaskError::StorageError(e.to_string()))?;
            return Ok(vec![]);
        }

        // Build placeholders for IN clause
        let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
        let in_clause = placeholders.join(",");

        // Update status
        let update_query = format!(
            "UPDATE delta_tasks SET status = 'running', started_at = datetime('now') WHERE id IN ({})",
            in_clause
        );
        let mut query = sqlx::query(&update_query);
        for id in &ids {
            query = query.bind(id);
        }
        query
            .execute(&mut *tx)
            .await
            .map_err(|e| TaskError::StorageError(e.to_string()))?;

        // Fetch the updated tasks
        let select_query = format!(
            "SELECT id, pipeline, input, created_at FROM delta_tasks WHERE id IN ({})",
            in_clause
        );
        let mut select = sqlx::query_as::<_, (i64, String, String, String)>(&select_query);
        for id in &ids {
            select = select.bind(id);
        }
        let rows = select
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| TaskError::StorageError(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| TaskError::StorageError(e.to_string()))?;

        let tasks = rows
            .into_iter()
            .map(|(id, pipeline, input, created_at)| {
                let input_value: serde_json::Value = serde_json::from_str(&input)
                    .map_err(|e| TaskError::DeserializationError(e.to_string()))?;
                let created = DateTime::parse_from_rfc3339(&format!("{}Z", created_at.replace(' ', "T")))
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                Ok(StoredTask {
                    id: TaskId(id),
                    pipeline,
                    input: input_value,
                    created_at: created,
                })
            })
            .collect::<Result<Vec<_>, TaskError>>()?;

        Ok(tasks)
    }

    async fn claim_for_pipeline(&self, pipeline: &str, limit: usize) -> Result<Vec<StoredTask>, TaskError> {
        let mut tx = self.pool
            .begin()
            .await
            .map_err(|e| TaskError::StorageError(e.to_string()))?;

        // Get IDs of tasks to claim for this specific pipeline
        let ids: Vec<i64> = sqlx::query_scalar(
            r#"
            SELECT id FROM delta_tasks
            WHERE status = 'pending' AND pipeline = ?
            ORDER BY created_at
            LIMIT ?
            "#,
        )
        .bind(pipeline)
        .bind(limit as i64)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| TaskError::StorageError(e.to_string()))?;

        if ids.is_empty() {
            tx.commit()
                .await
                .map_err(|e| TaskError::StorageError(e.to_string()))?;
            return Ok(vec![]);
        }

        // Build placeholders for IN clause
        let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
        let in_clause = placeholders.join(",");

        // Update status
        let update_query = format!(
            "UPDATE delta_tasks SET status = 'running', started_at = datetime('now') WHERE id IN ({})",
            in_clause
        );
        let mut query = sqlx::query(&update_query);
        for id in &ids {
            query = query.bind(id);
        }
        query
            .execute(&mut *tx)
            .await
            .map_err(|e| TaskError::StorageError(e.to_string()))?;

        // Fetch the updated tasks
        let select_query = format!(
            "SELECT id, pipeline, input, created_at FROM delta_tasks WHERE id IN ({})",
            in_clause
        );
        let mut select = sqlx::query_as::<_, (i64, String, String, String)>(&select_query);
        for id in &ids {
            select = select.bind(id);
        }
        let rows = select
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| TaskError::StorageError(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| TaskError::StorageError(e.to_string()))?;

        let tasks = rows
            .into_iter()
            .map(|(id, pipeline, input, created_at)| {
                let input_value: serde_json::Value = serde_json::from_str(&input)
                    .map_err(|e| TaskError::DeserializationError(e.to_string()))?;
                let created = DateTime::parse_from_rfc3339(&format!("{}Z", created_at.replace(' ', "T")))
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                Ok(StoredTask {
                    id: TaskId(id),
                    pipeline,
                    input: input_value,
                    created_at: created,
                })
            })
            .collect::<Result<Vec<_>, TaskError>>()?;

        Ok(tasks)
    }

    async fn recover_orphans(&self) -> Result<usize, TaskError> {
        let result = sqlx::query(
            r#"
            UPDATE delta_tasks
            SET status = 'pending', started_at = NULL
            WHERE status = 'running'
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| TaskError::StorageError(e.to_string()))?;

        Ok(result.rows_affected() as usize)
    }

    async fn claim_excluding(&self, limit: usize, exclude_pipelines: &[&str]) -> Result<Vec<StoredTask>, TaskError> {
        if exclude_pipelines.is_empty() {
            return self.claim(limit).await;
        }

        let mut tx = self.pool
            .begin()
            .await
            .map_err(|e| TaskError::StorageError(e.to_string()))?;

        // Build NOT IN clause for excluded pipelines
        let exclude_placeholders: Vec<String> = exclude_pipelines.iter().map(|_| "?".to_string()).collect();
        let exclude_clause = exclude_placeholders.join(",");

        let select_query = format!(
            r#"
            SELECT id FROM delta_tasks
            WHERE status = 'pending' AND pipeline NOT IN ({})
            ORDER BY created_at
            LIMIT ?
            "#,
            exclude_clause
        );

        let mut query = sqlx::query_scalar::<_, i64>(&select_query);
        for pipeline in exclude_pipelines {
            query = query.bind(*pipeline);
        }
        query = query.bind(limit as i64);

        let ids: Vec<i64> = query
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| TaskError::StorageError(e.to_string()))?;

        if ids.is_empty() {
            tx.commit()
                .await
                .map_err(|e| TaskError::StorageError(e.to_string()))?;
            return Ok(vec![]);
        }

        // Build placeholders for IN clause
        let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
        let in_clause = placeholders.join(",");

        // Update status
        let update_query = format!(
            "UPDATE delta_tasks SET status = 'running', started_at = datetime('now') WHERE id IN ({})",
            in_clause
        );
        let mut update = sqlx::query(&update_query);
        for id in &ids {
            update = update.bind(id);
        }
        update
            .execute(&mut *tx)
            .await
            .map_err(|e| TaskError::StorageError(e.to_string()))?;

        // Fetch the updated tasks
        let fetch_query = format!(
            "SELECT id, pipeline, input, created_at FROM delta_tasks WHERE id IN ({})",
            in_clause
        );
        let mut fetch = sqlx::query_as::<_, (i64, String, String, String)>(&fetch_query);
        for id in &ids {
            fetch = fetch.bind(id);
        }
        let rows = fetch
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| TaskError::StorageError(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| TaskError::StorageError(e.to_string()))?;

        let tasks = rows
            .into_iter()
            .map(|(id, pipeline, input, created_at)| {
                let input_value: serde_json::Value = serde_json::from_str(&input)
                    .map_err(|e| TaskError::DeserializationError(e.to_string()))?;
                let created = DateTime::parse_from_rfc3339(&format!("{}Z", created_at.replace(' ', "T")))
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                Ok(StoredTask {
                    id: TaskId(id),
                    pipeline,
                    input: input_value,
                    created_at: created,
                })
            })
            .collect::<Result<Vec<_>, TaskError>>()?;

        Ok(tasks)
    }

    async fn complete(&self, id: TaskId) -> Result<(), TaskError> {
        sqlx::query(
            r#"
            UPDATE delta_tasks
            SET status = 'completed', completed_at = datetime('now')
            WHERE id = ?
            "#,
        )
        .bind(id.0)
        .execute(&self.pool)
        .await
        .map_err(|e| TaskError::StorageError(e.to_string()))?;

        Ok(())
    }

    async fn fail(&self, id: TaskId, error: &str) -> Result<(), TaskError> {
        let truncated_error = if error.len() > 2000 {
            &error[..2000]
        } else {
            error
        };

        sqlx::query(
            r#"
            UPDATE delta_tasks
            SET status = 'failed', completed_at = datetime('now'), error_message = ?
            WHERE id = ?
            "#,
        )
        .bind(truncated_error)
        .bind(id.0)
        .execute(&self.pool)
        .await
        .map_err(|e| TaskError::StorageError(e.to_string()))?;

        Ok(())
    }
}
