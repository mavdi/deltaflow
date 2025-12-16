//! Tests demonstrating ordering and time-sensitive patterns.
//!
//! These tests verify sequence preservation, late arrival handling,
//! priority routing, and batch boundary behavior.

use async_trait::async_trait;
use deltaflow::{HasEntityId, NoopRecorder, Pipeline, RunnerBuilder, SqliteTaskStore, Step, StepError};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct OrderedItem {
    id: u64,
    sequence: u64,
}

impl HasEntityId for OrderedItem {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TimestampedItem {
    id: u64,
    timestamp: u64, // Unix timestamp in seconds
}

impl HasEntityId for TimestampedItem {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
enum Priority {
    High,
    Normal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PriorityItem {
    id: u64,
    priority: Priority,
}

impl HasEntityId for PriorityItem {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BatchedItem {
    id: u64,
    batch_id: String,
}

impl HasEntityId for BatchedItem {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
}

struct OrderRecordingStep {
    name: &'static str,
    recorded: Arc<Mutex<Vec<u64>>>,
}

#[async_trait]
impl Step for OrderRecordingStep {
    type Input = OrderedItem;
    type Output = OrderedItem;

    fn name(&self) -> &'static str {
        self.name
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        self.recorded.lock().await.push(input.sequence);
        Ok(input)
    }
}

struct PassthroughOrderedStep;

#[async_trait]
impl Step for PassthroughOrderedStep {
    type Input = OrderedItem;
    type Output = OrderedItem;

    fn name(&self) -> &'static str {
        "passthrough"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}
