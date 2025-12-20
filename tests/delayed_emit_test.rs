//! Integration test for delayed emit functionality.

use async_trait::async_trait;
use deltaflow::{HasEntityId, NoopRecorder, Pipeline, SqliteTaskStore, Step, StepError, TaskStore};
use serde::Serialize;
use sqlx::SqlitePool;

// Test input type
#[derive(Clone, Debug, Serialize)]
struct TestInput {
    id: String,
    value: u32,
}

impl HasEntityId for TestInput {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

// Simple passthrough step that returns input unchanged
struct PassthroughStep;

#[async_trait]
impl Step for PassthroughStep {
    type Input = TestInput;
    type Output = TestInput;

    fn name(&self) -> &'static str {
        "passthrough"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

async fn setup_store() -> (SqliteTaskStore, SqlitePool) {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool.clone());
    store.run_migrations().await.unwrap();
    (store, pool)
}

#[tokio::test]
async fn test_delayed_emit_schedules_for_future() {
    let (store, pool) = setup_store().await;

    // Build pipeline with delayed emit
    let pipeline = Pipeline::new("source_pipeline")
        .start_with(PassthroughStep)
        .emit("delayed_target", |_output: &TestInput| {
            vec![serde_json::json!({"id": "delayed-1", "value": 42})]
        })
        .delay(std::time::Duration::from_secs(3600)) // 1 hour delay
        .with_recorder(NoopRecorder)
        .build();

    // Run the pipeline
    let input = TestInput {
        id: "test-1".to_string(),
        value: 10,
    };

    let result = pipeline.run(input).await;
    assert!(result.is_ok());

    // Get spawned tasks from the result
    let spawned = pipeline.get_spawned_from_result(&Ok(result.unwrap()));
    assert_eq!(spawned.len(), 1, "Should have one spawned task");

    let (target, input, delay) = &spawned[0];
    assert_eq!(*target, "delayed_target");
    assert!(delay.is_some(), "Delay should be set");
    assert_eq!(delay.unwrap().as_secs(), 3600);

    // Manually enqueue the spawned task with delay
    let now = chrono::Utc::now();
    let scheduled_for = now + chrono::Duration::seconds(3600);
    store
        .enqueue_scheduled("delayed_target", input.clone(), scheduled_for)
        .await
        .unwrap();

    // Verify the task is NOT claimable immediately
    let claimed = store.claim(10).await.unwrap();
    assert_eq!(
        claimed.len(),
        0,
        "Delayed task should not be claimable yet"
    );

    // Verify via direct database query that scheduled_for is set
    let scheduled_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM delta_tasks WHERE scheduled_for > datetime('now')"
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(scheduled_count, 1, "Should have one task scheduled for future");
}

#[tokio::test]
async fn test_delayed_emit_claimable_after_scheduled_time() {
    let (store, _pool) = setup_store().await;

    let now = chrono::Utc::now();
    let past = now - chrono::Duration::seconds(10); // 10 seconds ago

    // Enqueue task scheduled for the past - should be claimable immediately
    store
        .enqueue_scheduled(
            "delayed_target",
            serde_json::json!({"id": "past-task", "value": 99}),
            past,
        )
        .await
        .unwrap();

    // Should be claimable since scheduled_for is in the past
    let claimed = store.claim(10).await.unwrap();
    assert_eq!(claimed.len(), 1, "Task scheduled in past should be claimable");
    assert_eq!(claimed[0].pipeline, "delayed_target");
}

#[tokio::test]
async fn test_immediate_emit_has_no_delay() {
    // Build pipeline with immediate emit (no delay)
    let pipeline = Pipeline::new("source_pipeline")
        .start_with(PassthroughStep)
        .emit("immediate_target", |_output: &TestInput| {
            vec![serde_json::json!({"id": "immediate-1", "value": 42})]
        })
        .with_recorder(NoopRecorder)
        .build();

    let input = TestInput {
        id: "test-2".to_string(),
        value: 20,
    };

    let result = pipeline.run(input).await;
    assert!(result.is_ok());

    // Get spawned tasks
    let spawned = pipeline.get_spawned_from_result(&Ok(result.unwrap()));
    assert_eq!(spawned.len(), 1);

    let (_target, _input, delay) = &spawned[0];
    assert!(delay.is_none(), "Immediate emit should have no delay");
}

#[tokio::test]
async fn test_mixed_immediate_and_delayed_emits() {
    let (store, _pool) = setup_store().await;

    let now = chrono::Utc::now();
    let future = now + chrono::Duration::hours(1);

    // Enqueue immediate task
    store
        .enqueue("pipeline_a", serde_json::json!({"id": "immediate"}))
        .await
        .unwrap();

    // Enqueue delayed task
    store
        .enqueue_scheduled("pipeline_b", serde_json::json!({"id": "delayed"}), future)
        .await
        .unwrap();

    // Only immediate task should be claimable
    let claimed = store.claim(10).await.unwrap();
    assert_eq!(claimed.len(), 1, "Only immediate task should be claimable");
    assert_eq!(
        claimed[0].input.get("id").unwrap().as_str().unwrap(),
        "immediate"
    );
}

#[tokio::test]
async fn test_pipeline_graph_includes_delay() {
    // Build pipeline with delayed emit
    let pipeline = Pipeline::new("test_graph")
        .start_with(PassthroughStep)
        .emit("delayed_target", |_output: &TestInput| {
            vec![serde_json::json!({"value": 1})]
        })
        .delay(std::time::Duration::from_secs(7200)) // 2 hours
        .with_recorder(NoopRecorder)
        .build();

    // Get the graph representation
    let graph = pipeline.to_graph();

    // Verify the emit node includes delay information
    assert_eq!(graph.emits.len(), 1, "Should have one emit");
    assert_eq!(graph.emits[0].target_pipeline, "delayed_target");
    assert_eq!(
        graph.emits[0].delay_seconds,
        Some(7200),
        "Emit should show 2 hour delay"
    );
}
