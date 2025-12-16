//! Tests for PeriodicScheduler.

use deltaflow::{SchedulerBuilder, SqliteTaskStore, TaskStore};
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

async fn setup_task_store() -> (SqliteTaskStore, SqlitePool) {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool.clone());
    store.run_migrations().await.unwrap();
    (store, pool)
}

#[tokio::test]
async fn test_scheduler_enqueues_items() {
    let (task_store, pool) = setup_task_store().await;

    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_clone = call_count.clone();

    let scheduler = SchedulerBuilder::new(task_store)
        .job("test_pipeline", Duration::from_millis(50), move || {
            let count = call_count_clone.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                vec!["item1".to_string(), "item2".to_string()]
            }
        })
        .run_on_start(true)
        .build();

    // Run scheduler in background
    let handle = tokio::spawn(async move { scheduler.run().await });

    // Wait for at least one execution
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify query was called
    assert!(call_count.load(Ordering::SeqCst) >= 1);

    // Verify items were enqueued - create new store instance from same pool
    let verify_store = SqliteTaskStore::new(pool);
    let tasks = verify_store.claim(10).await.unwrap();
    assert!(!tasks.is_empty());
    assert!(tasks.iter().all(|t| t.pipeline == "test_pipeline"));

    handle.abort();
}

#[tokio::test]
async fn test_scheduler_run_on_start_false() {
    let (task_store, _pool) = setup_task_store().await;

    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_clone = call_count.clone();

    let scheduler = SchedulerBuilder::new(task_store)
        .job("test_pipeline", Duration::from_secs(60), move || {
            let count = call_count_clone.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                vec!["item".to_string()]
            }
        })
        .run_on_start(false) // Don't run immediately
        .build();

    let handle = tokio::spawn(async move { scheduler.run().await });

    // Wait briefly
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Should NOT have been called yet (60s interval, no run_on_start)
    assert_eq!(call_count.load(Ordering::SeqCst), 0);

    handle.abort();
}

#[tokio::test]
async fn test_scheduler_empty_query_result() {
    let (task_store, pool) = setup_task_store().await;

    let scheduler = SchedulerBuilder::new(task_store)
        .job("test_pipeline", Duration::from_millis(50), || async {
            Vec::<String>::new() // Return empty
        })
        .run_on_start(true)
        .build();

    let handle = tokio::spawn(async move { scheduler.run().await });

    tokio::time::sleep(Duration::from_millis(100)).await;

    // No items should be enqueued
    let verify_store = SqliteTaskStore::new(pool);
    let tasks = verify_store.claim(10).await.unwrap();
    assert!(tasks.is_empty());

    handle.abort();
}

#[tokio::test]
async fn test_scheduler_multiple_jobs() {
    let (task_store, pool) = setup_task_store().await;

    let job1_count = Arc::new(AtomicUsize::new(0));
    let job2_count = Arc::new(AtomicUsize::new(0));
    let job1_clone = job1_count.clone();
    let job2_clone = job2_count.clone();

    let scheduler = SchedulerBuilder::new(task_store)
        .job("pipeline_a", Duration::from_millis(50), move || {
            let count = job1_clone.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                vec!["a".to_string()]
            }
        })
        .run_on_start(true)
        .job("pipeline_b", Duration::from_millis(50), move || {
            let count = job2_clone.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                vec!["b".to_string()]
            }
        })
        .run_on_start(true)
        .build();

    let handle = tokio::spawn(async move { scheduler.run().await });

    tokio::time::sleep(Duration::from_millis(150)).await;

    // Both jobs should have run
    assert!(job1_count.load(Ordering::SeqCst) >= 1);
    assert!(job2_count.load(Ordering::SeqCst) >= 1);

    // Both pipelines should have tasks
    let verify_store = SqliteTaskStore::new(pool);
    let tasks = verify_store.claim(20).await.unwrap();
    let pipeline_a_tasks: Vec<_> = tasks
        .iter()
        .filter(|t| t.pipeline == "pipeline_a")
        .collect();
    let pipeline_b_tasks: Vec<_> = tasks
        .iter()
        .filter(|t| t.pipeline == "pipeline_b")
        .collect();

    assert!(!pipeline_a_tasks.is_empty());
    assert!(!pipeline_b_tasks.is_empty());

    handle.abort();
}
