//! Tests for SqliteTaskStore.

use deltaflow::{SqliteTaskStore, TaskStore};
use sqlx::SqlitePool;

async fn setup_store() -> SqliteTaskStore {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();
    store
}

#[tokio::test]
async fn test_enqueue_and_claim() {
    let store = setup_store().await;

    // Enqueue a task
    let input = serde_json::json!({"video_id": 123});
    let id = store.enqueue("process_video", input.clone()).await.unwrap();

    // Claim it
    let tasks = store.claim(10).await.unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, id);
    assert_eq!(tasks[0].pipeline, "process_video");
    assert_eq!(tasks[0].input, input);

    // Claiming again should return empty (task is running)
    let tasks2 = store.claim(10).await.unwrap();
    assert!(tasks2.is_empty());
}

#[tokio::test]
async fn test_complete_task() {
    let store = setup_store().await;

    let input = serde_json::json!({"ticker": "AAPL"});
    let id = store.enqueue("fetch_price", input).await.unwrap();

    let tasks = store.claim(1).await.unwrap();
    assert_eq!(tasks.len(), 1);

    // Complete the task
    store.complete(id).await.unwrap();

    // Should not be claimable
    let tasks2 = store.claim(10).await.unwrap();
    assert!(tasks2.is_empty());
}

#[tokio::test]
async fn test_fail_task() {
    let store = setup_store().await;

    let input = serde_json::json!({"data": "test"});
    let id = store.enqueue("some_pipeline", input).await.unwrap();

    let tasks = store.claim(1).await.unwrap();
    assert_eq!(tasks.len(), 1);

    // Fail the task
    store.fail(id, "something went wrong").await.unwrap();

    // Should not be claimable
    let tasks2 = store.claim(10).await.unwrap();
    assert!(tasks2.is_empty());
}

#[tokio::test]
async fn test_claim_respects_limit() {
    let store = setup_store().await;

    // Enqueue 5 tasks
    for i in 0..5 {
        let input = serde_json::json!({"n": i});
        store.enqueue("pipeline", input).await.unwrap();
    }

    // Claim only 2
    let tasks = store.claim(2).await.unwrap();
    assert_eq!(tasks.len(), 2);

    // Claim remaining 3
    let tasks2 = store.claim(10).await.unwrap();
    assert_eq!(tasks2.len(), 3);
}

#[tokio::test]
async fn test_claim_fifo_order() {
    let store = setup_store().await;

    let id1 = store.enqueue("p", serde_json::json!({"n": 1})).await.unwrap();
    let id2 = store.enqueue("p", serde_json::json!({"n": 2})).await.unwrap();
    let id3 = store.enqueue("p", serde_json::json!({"n": 3})).await.unwrap();

    let tasks = store.claim(2).await.unwrap();
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].id, id1);
    assert_eq!(tasks[1].id, id2);

    let tasks2 = store.claim(1).await.unwrap();
    assert_eq!(tasks2[0].id, id3);
}

#[tokio::test]
async fn test_claim_for_pipeline() {
    let store = setup_store().await;

    // Enqueue tasks for different pipelines
    store.enqueue("pipeline_a", serde_json::json!({"n": 1})).await.unwrap();
    store.enqueue("pipeline_b", serde_json::json!({"n": 2})).await.unwrap();
    store.enqueue("pipeline_a", serde_json::json!({"n": 3})).await.unwrap();
    store.enqueue("pipeline_b", serde_json::json!({"n": 4})).await.unwrap();

    // Claim only pipeline_a tasks
    let tasks = store.claim_for_pipeline("pipeline_a", 10).await.unwrap();
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().all(|t| t.pipeline == "pipeline_a"));

    // pipeline_b tasks should still be pending
    let remaining = store.claim_for_pipeline("pipeline_b", 10).await.unwrap();
    assert_eq!(remaining.len(), 2);
    assert!(remaining.iter().all(|t| t.pipeline == "pipeline_b"));
}
