//! Step trait and error types.

use async_trait::async_trait;
use thiserror::Error;

/// Error returned by a step execution.
#[derive(Error, Debug)]
pub enum StepError {
    /// Transient failure - worth retrying.
    #[error("retryable: {0}")]
    Retryable(#[source] anyhow::Error),

    /// Permanent failure - won't succeed on retry.
    #[error("permanent: {0}")]
    Permanent(#[source] anyhow::Error),
}

impl StepError {
    /// Create a retryable error.
    pub fn retryable(err: impl Into<anyhow::Error>) -> Self {
        Self::Retryable(err.into())
    }

    /// Create a permanent error.
    pub fn permanent(err: impl Into<anyhow::Error>) -> Self {
        Self::Permanent(err.into())
    }

    /// Returns true if this error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable(_))
    }
}

/// A single step in a pipeline that transforms input to output.
#[async_trait]
pub trait Step: Send + Sync {
    /// The input type this step accepts.
    type Input: Send + Clone;

    /// The output type this step produces.
    type Output: Send;

    /// The name of this step for logging and recording.
    fn name(&self) -> &'static str;

    /// Execute the step with the given input.
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError>;
}
