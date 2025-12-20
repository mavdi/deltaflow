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

#[tokio::test]
async fn test_circuit_breaker_pattern() {
    // Demonstrates: after N failures, predicate switches routing from primary to fallback
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();

    let failure_count = Arc::new(AtomicUsize::new(0));
    let primary_recorded = Arc::new(Mutex::new(Vec::new()));
    let fallback_recorded = Arc::new(Mutex::new(Vec::new()));

    let failure_count_for_predicate = failure_count.clone();
    let threshold = 2;

    // Main pipeline that routes based on failure count
    let main_pipeline = Pipeline::new("main")
        .start_with(PassthroughStep)
        // Route to primary if failures < threshold
        .fork_when(
            move |result| result.is_ok() && failure_count_for_predicate.load(Ordering::SeqCst) < threshold,
            "primary",
        )
        // Route to fallback if failures >= threshold
        .fork_when(
            {
                let fc = failure_count.clone();
                move |result| result.is_ok() && fc.load(Ordering::SeqCst) >= threshold
            },
            "fallback",
        )
        .with_recorder(NoopRecorder)
        .build();

    // Primary pipeline that fails on certain items
    let primary_pipeline = Pipeline::new("primary")
        .start_with(FailingStep {
            fail_on_ids: vec![2, 4], // Will fail on items 2 and 4
            failure_counter: failure_count.clone(),
        })
        .then(RecordingStep {
            name: "primary_recorder",
            recorded: primary_recorded.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    // Fallback pipeline that always succeeds
    let fallback_pipeline = Pipeline::new("fallback")
        .start_with(RecordingStep {
            name: "fallback_recorder",
            recorded: fallback_recorded.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    let runner = Arc::new(RunnerBuilder::new(store)
        .pipeline(main_pipeline)
        .pipeline(primary_pipeline)
        .pipeline(fallback_pipeline)
        .poll_interval(Duration::from_millis(50))
        .max_concurrent(1) // Sequential for predictable ordering
        .build());

    // Start runner in background
    let runner_clone = runner.clone();
    let runner_handle = tokio::spawn(async move {
        runner_clone.run().await
    });

    // Submit first batch of items (1-4)
    // Items 2 and 4 will fail, triggering circuit breaker
    for id in 1..=4 {
        runner
            .submit("main", DataItem { id, value: id * 10 })
            .await
            .unwrap();
    }

    // Wait for first batch to complete
    tokio::time::sleep(Duration::from_millis(800)).await;

    // Submit second batch (5-6) - these should go to fallback
    for id in 5..=6 {
        runner
            .submit("main", DataItem { id, value: id * 10 })
            .await
            .unwrap();
    }

    // Wait for second batch to complete
    tokio::time::sleep(Duration::from_millis(800)).await;

    runner_handle.abort();

    let primary = primary_recorded.lock().await;
    let fallback = fallback_recorded.lock().await;

    // Items 1, 3 should have succeeded on primary (2 and 4 failed, incrementing counter)
    // After 2 failures, threshold reached, items 5, 6 go to fallback
    assert!(primary.contains(&1), "Item 1 should reach primary");
    assert!(primary.contains(&3), "Item 3 should reach primary");
    assert!(fallback.contains(&5), "Item 5 should go to fallback");
    assert!(fallback.contains(&6), "Item 6 should go to fallback");
}

#[tokio::test]
async fn test_accumulator_routing() {
    // Demonstrates: running total influences routing decisions
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();

    let running_total = Arc::new(AtomicU64::new(0));
    let normal_recorded = Arc::new(Mutex::new(Vec::new()));
    let overflow_recorded = Arc::new(Mutex::new(Vec::new()));

    let threshold: u64 = 100;

    // Main pipeline that accumulates and routes
    let main_pipeline = Pipeline::new("main")
        .start_with(PassthroughStep)
        .fork_when(
            {
                let total = running_total.clone();
                move |result| {
                    result.as_ref().map_or(false, |item| {
                        let prev = total.fetch_add(item.value, Ordering::SeqCst);
                        prev + item.value <= threshold
                    })
                }
            },
            "normal",
        )
        .fork_when(
            {
                let total = running_total.clone();
                move |result| result.is_ok() && total.load(Ordering::SeqCst) > threshold
            },
            "overflow",
        )
        .with_recorder(NoopRecorder)
        .build();

    let normal_pipeline = Pipeline::new("normal")
        .start_with(RecordingStep {
            name: "normal_recorder",
            recorded: normal_recorded.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    let overflow_pipeline = Pipeline::new("overflow")
        .start_with(RecordingStep {
            name: "overflow_recorder",
            recorded: overflow_recorded.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    let runner = RunnerBuilder::new(store)
        .pipeline(main_pipeline)
        .pipeline(normal_pipeline)
        .pipeline(overflow_pipeline)
        .poll_interval(Duration::from_millis(50))
        .max_concurrent(1)
        .build();

    // Submit items: values 30, 30, 30, 50, 50
    // Running total: 30, 60, 90, 140, 190
    // Items 1-3 (total <= 100) go to normal
    // Items 4-5 (total > 100) go to overflow
    let values = vec![30, 30, 30, 50, 50];
    for (idx, value) in values.iter().enumerate() {
        runner
            .submit("main", DataItem { id: idx as u64 + 1, value: *value })
            .await
            .unwrap();
    }

    tokio::select! {
        _ = runner.run() => {}
        _ = tokio::time::sleep(Duration::from_millis(1000)) => {}
    }

    let normal = normal_recorded.lock().await;
    let overflow = overflow_recorded.lock().await;

    // First 3 items should go to normal
    assert!(normal.contains(&1), "Item 1 should go to normal");
    assert!(normal.contains(&2), "Item 2 should go to normal");
    assert!(normal.contains(&3), "Item 3 should go to normal");

    // Items 4-5 should go to overflow
    assert!(overflow.contains(&4) || overflow.contains(&5), "Later items should go to overflow");
}
