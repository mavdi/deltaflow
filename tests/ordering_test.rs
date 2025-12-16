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

struct TimestampPassthroughStep;

#[async_trait]
impl Step for TimestampPassthroughStep {
    type Input = TimestampedItem;
    type Output = TimestampedItem;

    fn name(&self) -> &'static str {
        "timestamp_passthrough"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

struct TimestampRecordingStep {
    name: &'static str,
    recorded: Arc<Mutex<Vec<u64>>>,
}

#[async_trait]
impl Step for TimestampRecordingStep {
    type Input = TimestampedItem;
    type Output = TimestampedItem;

    fn name(&self) -> &'static str {
        self.name
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        self.recorded.lock().await.push(input.id);
        Ok(input)
    }
}

#[tokio::test]
async fn test_late_arrival_routing() {
    // Demonstrates: items with old timestamps route to late handler
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();

    let normal_recorded = Arc::new(Mutex::new(Vec::new()));
    let late_recorded = Arc::new(Mutex::new(Vec::new()));

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let cutoff = now - 60; // 60 seconds ago

    let main_pipeline = Pipeline::new("main")
        .start_with(TimestampPassthroughStep)
        .fork_when(
            move |item: &TimestampedItem| item.timestamp >= cutoff,
            "normal",
        )
        .fork_when(
            move |item: &TimestampedItem| item.timestamp < cutoff,
            "late_handler",
        )
        .with_recorder(NoopRecorder)
        .build();

    let normal_pipeline = Pipeline::new("normal")
        .start_with(TimestampRecordingStep {
            name: "normal",
            recorded: normal_recorded.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    let late_pipeline = Pipeline::new("late_handler")
        .start_with(TimestampRecordingStep {
            name: "late",
            recorded: late_recorded.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    let runner = RunnerBuilder::new(store)
        .pipeline(main_pipeline)
        .pipeline(normal_pipeline)
        .pipeline(late_pipeline)
        .poll_interval(Duration::from_millis(50))
        .max_concurrent(2)
        .build();

    // Submit mix: items 1-3 are on-time, items 4-6 are late
    let items = vec![
        TimestampedItem { id: 1, timestamp: now },
        TimestampedItem { id: 2, timestamp: now - 30 },
        TimestampedItem { id: 3, timestamp: now - 50 },
        TimestampedItem { id: 4, timestamp: now - 120 }, // Late
        TimestampedItem { id: 5, timestamp: now - 300 }, // Late
        TimestampedItem { id: 6, timestamp: now - 3600 }, // Very late
    ];

    for item in items {
        runner.submit("main", item).await.unwrap();
    }

    tokio::select! {
        _ = runner.run() => {}
        _ = tokio::time::sleep(Duration::from_millis(800)) => {}
    }

    let normal = normal_recorded.lock().await;
    let late = late_recorded.lock().await;

    // Items 1-3 should be on-time
    assert!(normal.contains(&1), "Item 1 should be on-time");
    assert!(normal.contains(&2), "Item 2 should be on-time");
    assert!(normal.contains(&3), "Item 3 should be on-time");

    // Items 4-6 should be late
    assert!(late.contains(&4), "Item 4 should be late");
    assert!(late.contains(&5), "Item 5 should be late");
    assert!(late.contains(&6), "Item 6 should be late");
}

struct PriorityPassthroughStep;

#[async_trait]
impl Step for PriorityPassthroughStep {
    type Input = PriorityItem;
    type Output = PriorityItem;

    fn name(&self) -> &'static str {
        "priority_passthrough"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

struct PriorityRecordingStep {
    name: &'static str,
    recorded: Arc<Mutex<Vec<u64>>>,
}

#[async_trait]
impl Step for PriorityRecordingStep {
    type Input = PriorityItem;
    type Output = PriorityItem;

    fn name(&self) -> &'static str {
        self.name
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        self.recorded.lock().await.push(input.id);
        Ok(input)
    }
}

#[tokio::test]
async fn test_priority_routing() {
    // Demonstrates: high priority items route to urgent pipeline
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();

    let urgent_recorded = Arc::new(Mutex::new(Vec::new()));
    let standard_recorded = Arc::new(Mutex::new(Vec::new()));

    let main_pipeline = Pipeline::new("main")
        .start_with(PriorityPassthroughStep)
        .fork_when(
            |item: &PriorityItem| item.priority == Priority::High,
            "urgent",
        )
        .fork_when(
            |item: &PriorityItem| item.priority == Priority::Normal,
            "standard",
        )
        .with_recorder(NoopRecorder)
        .build();

    let urgent_pipeline = Pipeline::new("urgent")
        .start_with(PriorityRecordingStep {
            name: "urgent",
            recorded: urgent_recorded.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    let standard_pipeline = Pipeline::new("standard")
        .start_with(PriorityRecordingStep {
            name: "standard",
            recorded: standard_recorded.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    let runner = RunnerBuilder::new(store)
        .pipeline(main_pipeline)
        .pipeline(urgent_pipeline)
        .pipeline(standard_pipeline)
        .poll_interval(Duration::from_millis(50))
        .max_concurrent(2)
        .build();

    // Interleaved priorities: normal, high, normal, high, normal
    let items = vec![
        PriorityItem { id: 1, priority: Priority::Normal },
        PriorityItem { id: 2, priority: Priority::High },
        PriorityItem { id: 3, priority: Priority::Normal },
        PriorityItem { id: 4, priority: Priority::High },
        PriorityItem { id: 5, priority: Priority::Normal },
    ];

    for item in items {
        runner.submit("main", item).await.unwrap();
    }

    tokio::select! {
        _ = runner.run() => {}
        _ = tokio::time::sleep(Duration::from_millis(800)) => {}
    }

    let urgent = urgent_recorded.lock().await;
    let standard = standard_recorded.lock().await;

    // High priority items (2, 4) go to urgent
    assert!(urgent.contains(&2), "Item 2 should go to urgent");
    assert!(urgent.contains(&4), "Item 4 should go to urgent");
    assert_eq!(urgent.len(), 2, "Only high priority items in urgent");

    // Normal priority items (1, 3, 5) go to standard
    assert!(standard.contains(&1), "Item 1 should go to standard");
    assert!(standard.contains(&3), "Item 3 should go to standard");
    assert!(standard.contains(&5), "Item 5 should go to standard");
    assert_eq!(standard.len(), 3, "Only normal priority items in standard");
}

struct BatchPassthroughStep;

#[async_trait]
impl Step for BatchPassthroughStep {
    type Input = BatchedItem;
    type Output = BatchedItem;

    fn name(&self) -> &'static str {
        "batch_passthrough"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

struct BatchRecordingStep {
    name: &'static str,
    recorded: Arc<Mutex<Vec<(u64, String)>>>,
}

#[async_trait]
impl Step for BatchRecordingStep {
    type Input = BatchedItem;
    type Output = BatchedItem;

    fn name(&self) -> &'static str {
        self.name
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        self.recorded.lock().await.push((input.id, input.batch_id.clone()));
        Ok(input)
    }
}

#[tokio::test]
async fn test_batch_boundary_routing() {
    // Demonstrates: routing decision based on batch_id, all items in same batch go same route
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();

    let route_a_recorded = Arc::new(Mutex::new(Vec::<(u64, String)>::new()));
    let route_b_recorded = Arc::new(Mutex::new(Vec::<(u64, String)>::new()));

    // Shared state: once a batch is seen, remember its route
    let batch_routes: Arc<Mutex<HashMap<String, &'static str>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let batch_routes_a = batch_routes.clone();
    let batch_routes_b = batch_routes.clone();

    let main_pipeline = Pipeline::new("main")
        .start_with(BatchPassthroughStep)
        // First batch seen goes to route_a, alternating
        .fork_when(
            move |item: &BatchedItem| {
                let mut routes = futures::executor::block_on(batch_routes_a.lock());
                let current_len = routes.len();
                let route = routes
                    .entry(item.batch_id.clone())
                    .or_insert_with(|| if current_len % 2 == 0 { "route_a" } else { "route_b" });
                *route == "route_a"
            },
            "route_a",
        )
        .fork_when(
            move |item: &BatchedItem| {
                let routes = futures::executor::block_on(batch_routes_b.lock());
                routes.get(&item.batch_id).map_or(false, |r| *r == "route_b")
            },
            "route_b",
        )
        .with_recorder(NoopRecorder)
        .build();

    let route_a = Pipeline::new("route_a")
        .start_with(BatchRecordingStep {
            name: "route_a",
            recorded: route_a_recorded.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    let route_b = Pipeline::new("route_b")
        .start_with(BatchRecordingStep {
            name: "route_b",
            recorded: route_b_recorded.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    let runner = RunnerBuilder::new(store)
        .pipeline(main_pipeline)
        .pipeline(route_a)
        .pipeline(route_b)
        .poll_interval(Duration::from_millis(50))
        .max_concurrent(1) // Sequential for deterministic batch assignment
        .build();

    // Items from batches A, B, A, B, A - should route consistently per batch
    let items = vec![
        BatchedItem { id: 1, batch_id: "batch_A".to_string() },
        BatchedItem { id: 2, batch_id: "batch_B".to_string() },
        BatchedItem { id: 3, batch_id: "batch_A".to_string() },
        BatchedItem { id: 4, batch_id: "batch_B".to_string() },
        BatchedItem { id: 5, batch_id: "batch_A".to_string() },
    ];

    for item in items {
        runner.submit("main", item).await.unwrap();
    }

    tokio::select! {
        _ = runner.run() => {}
        _ = tokio::time::sleep(Duration::from_millis(1000)) => {}
    }

    let route_a = route_a_recorded.lock().await;
    let route_b = route_b_recorded.lock().await;

    // All batch_A items should go to same route
    let batch_a_in_a: Vec<_> = route_a.iter().filter(|(_, b)| b == "batch_A").collect();
    let batch_a_in_b: Vec<_> = route_b.iter().filter(|(_, b)| b == "batch_A").collect();

    // Either all in A or all in B, not split
    assert!(
        batch_a_in_a.len() == 3 || batch_a_in_b.len() == 3,
        "All batch_A items should go to same route"
    );

    // All batch_B items should go to same route
    let batch_b_in_a: Vec<_> = route_a.iter().filter(|(_, b)| b == "batch_B").collect();
    let batch_b_in_b: Vec<_> = route_b.iter().filter(|(_, b)| b == "batch_B").collect();

    assert!(
        batch_b_in_a.len() == 2 || batch_b_in_b.len() == 2,
        "All batch_B items should go to same route"
    );
}
