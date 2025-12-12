//! Task storage trait and types.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;

/// Unique identifier for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(pub i64);

/// A task retrieved from storage.
#[derive(Debug, Clone)]
pub struct StoredTask {
    pub id: TaskId,
    pub pipeline: String,
    pub input: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// Error type for task operations.
#[derive(Error, Debug)]
pub enum TaskError {
    #[error("storage error: {0}")]
    StorageError(String),

    #[error("serialization error: {0}")]
    SerializationError(String),

    #[error("unknown pipeline: {0}")]
    UnknownPipeline(String),

    #[error("deserialization error: {0}")]
    DeserializationError(String),

    #[error("pipeline error: {0}")]
    PipelineError(String),
}

/// Trait for task storage backends.
#[async_trait]
pub trait TaskStore: Send + Sync {
    /// Enqueue a task for a pipeline with JSON input.
    async fn enqueue(&self, pipeline: &str, input: serde_json::Value) -> Result<TaskId, TaskError>;

    /// Claim up to `limit` pending tasks atomically.
    async fn claim(&self, limit: usize) -> Result<Vec<StoredTask>, TaskError>;

    /// Claim up to `limit` pending tasks for a specific pipeline atomically.
    /// Default implementation filters from `claim()` - override for efficiency.
    async fn claim_for_pipeline(&self, pipeline: &str, limit: usize) -> Result<Vec<StoredTask>, TaskError> {
        let tasks = self.claim(limit).await?;
        Ok(tasks.into_iter().filter(|t| t.pipeline == pipeline).collect())
    }

    /// Mark a task as completed.
    async fn complete(&self, id: TaskId) -> Result<(), TaskError>;

    /// Mark a task as failed with an error message.
    async fn fail(&self, id: TaskId, error: &str) -> Result<(), TaskError>;
}
