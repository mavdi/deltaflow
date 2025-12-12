//! Failing tests demonstrating concurrency bugs.
//!
//! These tests document the bugs reported:
//! 1. Per-pipeline concurrency limits not enforced
//! 2. No orphan recovery for crashed tasks

use async_trait::async_trait;
use deltaflow::{
    HasEntityId, NoopRecorder, Pipeline, RunnerBuilder,
    SqliteTaskStore, Step, StepError, TaskStore,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SlowInput {
    id: String,
}

impl HasEntityId for SlowInput {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

/// A step that tracks concurrent executions and takes time to complete.
struct SlowStep {
    /// Current number of concurrent executions
    concurrent: Arc<AtomicUsize>,
    /// Maximum concurrent executions observed
    max_observed: Arc<AtomicUsize>,
    /// How long each execution takes
    duration: Duration,
}

#[async_trait]
impl Step for SlowStep {
    type Input = SlowInput;
    type Output = ();

    fn name(&self) -> &'static str {
        "slow_step"
    }

    async fn execute(&self, _input: Self::Input) -> Result<Self::Output, StepError> {
        // Increment concurrent counter
        let current = self.concurrent.fetch_add(1, Ordering::SeqCst) + 1;

        // Update max if this is a new high
        self.max_observed.fetch_max(current, Ordering::SeqCst);

        // Simulate work
        tokio::time::sleep(self.duration).await;

        // Decrement concurrent counter
        self.concurrent.fetch_sub(1, Ordering::SeqCst);

        Ok(())
    }
}

/// BUG: Per-pipeline concurrency limit is not enforced.
///
/// When using `pipeline_with_concurrency(pipeline, 1)`, the runner should
/// only execute 1 task for that pipeline at a time. However, the current
/// implementation marks tasks as "running" in the database BEFORE acquiring
/// the semaphore permit, allowing many more tasks to be claimed than the
/// concurrency limit allows.
///
/// Expected: max_observed == 1
/// Actual: max_observed >> 1
#[tokio::test]
async fn test_pipeline_concurrency_limit_enforced() {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool.clone());
    store.run_migrations().await.unwrap();

    let concurrent = Arc::new(AtomicUsize::new(0));
    let max_observed = Arc::new(AtomicUsize::new(0));

    let pipeline = Pipeline::new("slow_pipeline")
        .start_with(SlowStep {
            concurrent: concurrent.clone(),
            max_observed: max_observed.clone(),
            duration: Duration::from_millis(100),
        })
        .with_recorder(NoopRecorder)
        .build();

    let runner = RunnerBuilder::new(store)
        .max_concurrent(10) // Global limit is high
        .pipeline_with_concurrency(pipeline, 1) // But this pipeline should only run 1 at a time
        .poll_interval(Duration::from_millis(10))
        .build();

    // Submit 10 tasks
    for i in 0..10 {
        runner
            .submit("slow_pipeline", SlowInput { id: format!("task_{}", i) })
            .await
            .unwrap();
    }

    // Run for enough time to process several tasks
    tokio::select! {
        _ = runner.run() => {}
        _ = tokio::time::sleep(Duration::from_secs(2)) => {}
    }

    let max = max_observed.load(Ordering::SeqCst);

    // This assertion SHOULD pass but currently FAILS
    // The bug causes max to be much higher than 1
    assert_eq!(
        max, 1,
        "Pipeline concurrency limit violated: observed {} concurrent executions, expected max 1",
        max
    );
}

/// BUG: Tasks in "running" state block the database count but may exceed semaphore.
///
/// This test checks the database state to verify that at most N tasks
/// are marked as "running" for a pipeline with concurrency limit N.
#[tokio::test]
async fn test_database_running_count_respects_concurrency() {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool.clone());
    store.run_migrations().await.unwrap();

    let concurrent = Arc::new(AtomicUsize::new(0));
    let max_observed = Arc::new(AtomicUsize::new(0));

    let pipeline = Pipeline::new("limited_pipeline")
        .start_with(SlowStep {
            concurrent: concurrent.clone(),
            max_observed: max_observed.clone(),
            duration: Duration::from_millis(200),
        })
        .with_recorder(NoopRecorder)
        .build();

    let runner = RunnerBuilder::new(store)
        .max_concurrent(5)
        .pipeline_with_concurrency(pipeline, 1)
        .poll_interval(Duration::from_millis(10))
        .build();

    // Submit many tasks
    for i in 0..20 {
        runner
            .submit("limited_pipeline", SlowInput { id: format!("task_{}", i) })
            .await
            .unwrap();
    }

    // Start runner in background
    let runner_handle = tokio::spawn(async move {
        tokio::select! {
            _ = runner.run() => {}
            _ = tokio::time::sleep(Duration::from_secs(3)) => {}
        }
    });

    // Check database state multiple times while runner is processing
    let mut max_running_in_db = 0i64;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(100)).await;

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM delta_tasks WHERE pipeline = 'limited_pipeline' AND status = 'running'"
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        if count > max_running_in_db {
            max_running_in_db = count;
        }
    }

    runner_handle.abort();

    // This assertion SHOULD pass but currently FAILS
    // The bug causes many more than 1 task to be marked "running"
    assert!(
        max_running_in_db <= 1,
        "Database shows {} tasks as 'running' for pipeline with concurrency limit 1",
        max_running_in_db
    );
}

/// BUG: No orphan recovery - tasks stuck in "running" are never recovered.
///
/// If the runner crashes or restarts, tasks that were in "running" state
/// remain stuck forever. They should be reset to "pending" on startup.
#[tokio::test]
async fn test_orphan_tasks_recovered_on_startup() {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool.clone());
    store.run_migrations().await.unwrap();

    // Simulate a crashed runner by directly inserting a task in "running" state
    sqlx::query(
        r#"
        INSERT INTO delta_tasks (pipeline, input, status, started_at)
        VALUES ('orphan_pipeline', '{"id": "orphan_1"}', 'running', datetime('now', '-5 minutes'))
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    // Also add a pending task
    store
        .enqueue("orphan_pipeline", serde_json::json!({"id": "pending_1"}))
        .await
        .unwrap();

    // Verify setup: 1 running, 1 pending
    let running_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM delta_tasks WHERE status = 'running'"
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(running_count, 1, "Setup: should have 1 running task");

    let pending_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM delta_tasks WHERE status = 'pending'"
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pending_count, 1, "Setup: should have 1 pending task");

    // Recover orphans (simulating runner startup)
    let recovered = store.recover_orphans().await.unwrap();
    assert_eq!(recovered, 1, "Should recover 1 orphaned task");

    // Claiming should return BOTH tasks (orphan should be recovered)
    let claimed = store.claim(10).await.unwrap();

    // This assertion SHOULD pass but currently FAILS
    // The orphan task is never recovered
    assert_eq!(
        claimed.len(),
        2,
        "Should claim both orphan and pending tasks, but only got {}",
        claimed.len()
    );
}
