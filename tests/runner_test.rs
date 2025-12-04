//! Integration tests for the task runner.

use async_trait::async_trait;
use delta::{
    HasEntityId, NoopRecorder, Pipeline, RunnerBuilder,
    SqliteTaskStore, Step, StepError,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct VideoInput {
    id: String,
    title: String,
}

impl HasEntityId for VideoInput {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

#[derive(Clone, Debug)]
struct VideoOutput {
    id: String,
    tickers: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PriceRequest {
    ticker: String,
}

impl HasEntityId for PriceRequest {
    fn entity_id(&self) -> String {
        self.ticker.clone()
    }
}

// Step that extracts tickers from video
struct ExtractTickers;

#[async_trait]
impl Step for ExtractTickers {
    type Input = VideoInput;
    type Output = VideoOutput;

    fn name(&self) -> &'static str {
        "extract_tickers"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        // Simulate finding tickers
        Ok(VideoOutput {
            id: input.id,
            tickers: vec!["AAPL".to_string(), "GOOGL".to_string()],
        })
    }
}

// Step that "fetches" a price
struct FetchPriceStep {
    fetched: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl Step for FetchPriceStep {
    type Input = PriceRequest;
    type Output = ();

    fn name(&self) -> &'static str {
        "fetch_price"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        self.fetched.lock().await.push(input.ticker);
        Ok(())
    }
}

#[tokio::test]
async fn test_runner_executes_pipeline() {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();

    let video_pipeline = Pipeline::new("process_video")
        .start_with(ExtractTickers)
        .with_recorder(NoopRecorder)
        .build();

    let runner = RunnerBuilder::new(store)
        .pipeline(video_pipeline)
        .poll_interval(Duration::from_millis(50))
        .max_concurrent(1)
        .build();

    // Submit a task
    let input = VideoInput {
        id: "v1".to_string(),
        title: "Test Video".to_string(),
    };
    runner.submit("process_video", input).await.unwrap();

    // Let runner process (run briefly then cancel)
    tokio::select! {
        _ = runner.run() => {}
        _ = tokio::time::sleep(Duration::from_millis(200)) => {}
    }

    // Task should be completed (we can check the store directly)
    // For now, just verify no panic occurred
}

#[tokio::test]
async fn test_runner_spawns_followup_tasks() {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool.clone());
    store.run_migrations().await.unwrap();

    let fetched = Arc::new(Mutex::new(Vec::new()));
    let fetched_clone = fetched.clone();

    let video_pipeline = Pipeline::new("process_video")
        .start_with(ExtractTickers)
        .with_recorder(NoopRecorder)
        .spawns("fetch_price", |output: &VideoOutput| {
            output.tickers.iter()
                .map(|t| PriceRequest { ticker: t.clone() })
                .collect()
        })
        .build();

    let price_pipeline = Pipeline::new("fetch_price")
        .start_with(FetchPriceStep { fetched: fetched_clone })
        .with_recorder(NoopRecorder)
        .build();

    let runner = RunnerBuilder::new(store)
        .pipeline(video_pipeline)
        .pipeline(price_pipeline)
        .poll_interval(Duration::from_millis(50))
        .max_concurrent(2)
        .build();

    // Submit initial task
    let input = VideoInput {
        id: "v1".to_string(),
        title: "Test Video".to_string(),
    };
    runner.submit("process_video", input).await.unwrap();

    // Let runner process
    tokio::select! {
        _ = runner.run() => {}
        _ = tokio::time::sleep(Duration::from_millis(500)) => {}
    }

    // Check that price fetches were executed
    let fetched_tickers = fetched.lock().await;
    assert!(fetched_tickers.contains(&"AAPL".to_string()));
    assert!(fetched_tickers.contains(&"GOOGL".to_string()));
}
