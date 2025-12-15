//! Tests demonstrating failure handling and branch isolation.
//!
//! These tests verify that failures in one branch don't affect siblings,
//! and that various error conditions are handled gracefully.

use async_trait::async_trait;
use deltaflow::{HasEntityId, NoopRecorder, Pipeline, RunnerBuilder, SqliteTaskStore, Step, StepError};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TestItem {
    id: u64,
    label: String,
}

impl HasEntityId for TestItem {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
}

struct PassthroughStep;

#[async_trait]
impl Step for PassthroughStep {
    type Input = TestItem;
    type Output = TestItem;

    fn name(&self) -> &'static str {
        "passthrough"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

struct RecordingStep {
    name: &'static str,
    recorded: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl Step for RecordingStep {
    type Input = TestItem;
    type Output = TestItem;

    fn name(&self) -> &'static str {
        self.name
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        self.recorded.lock().await.push(format!("{}:{}", self.name, input.id));
        Ok(input)
    }
}

struct AlwaysFailsStep {
    name: &'static str,
}

#[async_trait]
impl Step for AlwaysFailsStep {
    type Input = TestItem;
    type Output = TestItem;

    fn name(&self) -> &'static str {
        self.name
    }

    async fn execute(&self, _input: Self::Input) -> Result<Self::Output, StepError> {
        Err(StepError::Permanent(anyhow::anyhow!("always fails")))
    }
}

struct ConditionalFailStep {
    fail_on_label: String,
}

#[async_trait]
impl Step for ConditionalFailStep {
    type Input = TestItem;
    type Output = TestItem;

    fn name(&self) -> &'static str {
        "conditional_fail"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        if input.label == self.fail_on_label {
            return Err(StepError::Permanent(anyhow::anyhow!("conditional failure")));
        }
        Ok(input)
    }
}
