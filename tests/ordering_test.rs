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

#[tokio::test]
async fn test_sequence_preservation_across_branches() {
    // Demonstrates: items maintain order within each branch after fan-out
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();

    let branch_a_order = Arc::new(Mutex::new(Vec::new()));
    let branch_b_order = Arc::new(Mutex::new(Vec::new()));

    let main_pipeline = Pipeline::new("main")
        .start_with(PassthroughOrderedStep)
        .fan_out(&["branch_a", "branch_b"])
        .with_recorder(NoopRecorder)
        .build();

    let branch_a = Pipeline::new("branch_a")
        .start_with(OrderRecordingStep {
            name: "recorder_a",
            recorded: branch_a_order.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    let branch_b = Pipeline::new("branch_b")
        .start_with(OrderRecordingStep {
            name: "recorder_b",
            recorded: branch_b_order.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    let runner = RunnerBuilder::new(store)
        .pipeline(main_pipeline)
        .pipeline(branch_a)
        .pipeline(branch_b)
        .poll_interval(Duration::from_millis(50))
        .max_concurrent(1) // Sequential processing ensures FIFO
        .build();

    // Submit items 1-10 in order
    for seq in 1..=10 {
        runner
            .submit("main", OrderedItem { id: seq, sequence: seq })
            .await
            .unwrap();
    }

    tokio::select! {
        _ = runner.run() => {}
        _ = tokio::time::sleep(Duration::from_millis(1500)) => {}
    }

    let a_order = branch_a_order.lock().await;
    let b_order = branch_b_order.lock().await;

    // Verify FIFO ordering is preserved
    let expected: Vec<u64> = (1..=10).collect();
    assert_eq!(*a_order, expected, "Branch A should maintain sequence order");
    assert_eq!(*b_order, expected, "Branch B should maintain sequence order");
}
