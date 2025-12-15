//! Tests demonstrating stateful routing patterns.
//!
//! These tests show that users can implement sophisticated routing patterns
//! like circuit breakers and accumulators using the existing fork/fan-out primitives.

use async_trait::async_trait;
use deltaflow::{HasEntityId, NoopRecorder, Pipeline, RunnerBuilder, SqliteTaskStore, Step, StepError};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DataItem {
    id: u64,
    value: u64,
}

impl HasEntityId for DataItem {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
}

struct PassthroughStep;

#[async_trait]
impl Step for PassthroughStep {
    type Input = DataItem;
    type Output = DataItem;

    fn name(&self) -> &'static str {
        "passthrough"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

struct RecordingStep {
    name: &'static str,
    recorded: Arc<Mutex<Vec<u64>>>,
}

#[async_trait]
impl Step for RecordingStep {
    type Input = DataItem;
    type Output = DataItem;

    fn name(&self) -> &'static str {
        self.name
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        self.recorded.lock().await.push(input.id);
        Ok(input)
    }
}

struct FailingStep {
    fail_on_ids: Vec<u64>,
    failure_counter: Arc<AtomicUsize>,
}

#[async_trait]
impl Step for FailingStep {
    type Input = DataItem;
    type Output = DataItem;

    fn name(&self) -> &'static str {
        "failing_step"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        if self.fail_on_ids.contains(&input.id) {
            self.failure_counter.fetch_add(1, Ordering::SeqCst);
            return Err(StepError::Permanent(anyhow::anyhow!("simulated failure for id {}", input.id)));
        }
        Ok(input)
    }
}
