//! Recording interface for pipeline execution history.

use async_trait::async_trait;

/// Unique identifier for a pipeline run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RunId(pub i64);

/// Unique identifier for a step execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StepId(pub i64);

/// Status of a completed pipeline run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunStatus {
    /// Run completed successfully.
    Completed,
    /// Run failed.
    Failed { error: String },
}

/// Status of a completed step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepStatus {
    /// Step completed successfully.
    Completed,
    /// Step failed.
    Failed { error: String, attempt: u32 },
}

/// Records pipeline execution for observability.
#[async_trait]
pub trait Recorder: Send + Sync {
    /// Record the start of a pipeline run.
    async fn start_run(&self, pipeline_name: &str, entity_id: &str) -> anyhow::Result<RunId>;

    /// Record the start of a step execution.
    async fn start_step(
        &self,
        run_id: RunId,
        step_name: &str,
        step_index: u32,
    ) -> anyhow::Result<StepId>;

    /// Record step completion.
    async fn complete_step(&self, step_id: StepId, status: StepStatus) -> anyhow::Result<()>;

    /// Record run completion.
    async fn complete_run(&self, run_id: RunId, status: RunStatus) -> anyhow::Result<()>;
}

/// A no-op recorder that discards all events.
///
/// Useful for testing or when persistence is not needed.
#[derive(Debug, Clone, Default)]
pub struct NoopRecorder;

impl NoopRecorder {
    /// Create a new no-op recorder.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Recorder for NoopRecorder {
    async fn start_run(&self, _pipeline_name: &str, _entity_id: &str) -> anyhow::Result<RunId> {
        Ok(RunId(0))
    }

    async fn start_step(
        &self,
        _run_id: RunId,
        _step_name: &str,
        _step_index: u32,
    ) -> anyhow::Result<StepId> {
        Ok(StepId(0))
    }

    async fn complete_step(&self, _step_id: StepId, _status: StepStatus) -> anyhow::Result<()> {
        Ok(())
    }

    async fn complete_run(&self, _run_id: RunId, _status: RunStatus) -> anyhow::Result<()> {
        Ok(())
    }
}
