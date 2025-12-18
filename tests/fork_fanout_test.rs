//! Tests for fork and fan-out functionality.

use async_trait::async_trait;
use deltaflow::{
    HasEntityId, NoopRecorder, Pipeline, RunnerBuilder, SqliteTaskStore, Step, StepError,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct MarketData {
    symbol: String,
    asset_class: String,
    price: f64,
}

impl HasEntityId for MarketData {
    fn entity_id(&self) -> String {
        self.symbol.clone()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AlertRequest {
    symbol: String,
    alert_type: String,
}

struct NormalizeStep;

#[async_trait]
impl Step for NormalizeStep {
    type Input = MarketData;
    type Output = MarketData;

    fn name(&self) -> &'static str {
        "normalize"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[tokio::test]
async fn test_fork_when_predicate_matches() {
    let pipeline = Pipeline::new("market_data")
        .start_with(NormalizeStep)
        .fork_when(
            |d: &MarketData| d.asset_class == "crypto",
            "crypto_pipeline",
        )
        .with_recorder(NoopRecorder)
        .build();

    let input = MarketData {
        symbol: "BTC".to_string(),
        asset_class: "crypto".to_string(),
        price: 50000.0,
    };

    let output = pipeline.run(input).await.unwrap();
    let spawned = pipeline.get_spawned(&output);

    assert_eq!(spawned.len(), 1);
    assert_eq!(spawned[0].0, "crypto_pipeline");
}

#[tokio::test]
async fn test_fork_when_predicate_does_not_match() {
    let pipeline = Pipeline::new("market_data")
        .start_with(NormalizeStep)
        .fork_when(
            |d: &MarketData| d.asset_class == "crypto",
            "crypto_pipeline",
        )
        .with_recorder(NoopRecorder)
        .build();

    let input = MarketData {
        symbol: "AAPL".to_string(),
        asset_class: "equity".to_string(),
        price: 150.0,
    };

    let output = pipeline.run(input).await.unwrap();
    let spawned = pipeline.get_spawned(&output);

    assert_eq!(spawned.len(), 0);
}

#[tokio::test]
async fn test_fan_out_spawns_to_all_targets() {
    let pipeline = Pipeline::new("market_data")
        .start_with(NormalizeStep)
        .fan_out(&["ml_pipeline", "stats_pipeline", "alerts_pipeline"])
        .with_recorder(NoopRecorder)
        .build();

    let input = MarketData {
        symbol: "AAPL".to_string(),
        asset_class: "equity".to_string(),
        price: 150.0,
    };

    let output = pipeline.run(input).await.unwrap();
    let spawned = pipeline.get_spawned(&output);

    assert_eq!(spawned.len(), 3);

    let targets: Vec<&str> = spawned.iter().map(|(t, _)| *t).collect();
    assert!(targets.contains(&"ml_pipeline"));
    assert!(targets.contains(&"stats_pipeline"));
    assert!(targets.contains(&"alerts_pipeline"));
}

#[tokio::test]
async fn test_multiple_forks_all_matching_fire() {
    let pipeline = Pipeline::new("market_data")
        .start_with(NormalizeStep)
        .fork_when(
            |d: &MarketData| d.asset_class == "crypto",
            "crypto_pipeline",
        )
        .fork_when(|d: &MarketData| d.price > 10000.0, "high_value_pipeline")
        .fork_when(
            |d: &MarketData| d.symbol.len() <= 4,
            "short_symbol_pipeline",
        )
        .with_recorder(NoopRecorder)
        .build();

    let input = MarketData {
        symbol: "BTC".to_string(),
        asset_class: "crypto".to_string(),
        price: 50000.0,
    };

    let output = pipeline.run(input).await.unwrap();
    let spawned = pipeline.get_spawned(&output);

    // All three predicates match
    assert_eq!(spawned.len(), 3);

    let targets: Vec<&str> = spawned.iter().map(|(t, _)| *t).collect();
    assert!(targets.contains(&"crypto_pipeline"));
    assert!(targets.contains(&"high_value_pipeline"));
    assert!(targets.contains(&"short_symbol_pipeline"));
}

#[tokio::test]
async fn test_combined_fork_fanout_spawn_from() {
    let pipeline = Pipeline::new("market_data")
        .start_with(NormalizeStep)
        .fork_when(
            |d: &MarketData| d.asset_class == "crypto",
            "crypto_pipeline",
        )
        .fan_out(&["audit_pipeline"])
        .emit("alert_pipeline", |d: &MarketData| {
            if d.price > 40000.0 {
                vec![AlertRequest {
                    symbol: d.symbol.clone(),
                    alert_type: "high_price".to_string(),
                }]
            } else {
                vec![]
            }
        })
        .with_recorder(NoopRecorder)
        .build();

    let input = MarketData {
        symbol: "BTC".to_string(),
        asset_class: "crypto".to_string(),
        price: 50000.0,
    };

    let output = pipeline.run(input).await.unwrap();
    let spawned = pipeline.get_spawned(&output);

    // crypto_pipeline (fork matches) + audit_pipeline (fan-out) + alert_pipeline (spawn_from)
    assert_eq!(spawned.len(), 3);

    let targets: Vec<&str> = spawned.iter().map(|(t, _)| *t).collect();
    assert!(targets.contains(&"crypto_pipeline"));
    assert!(targets.contains(&"audit_pipeline"));
    assert!(targets.contains(&"alert_pipeline"));
}

#[tokio::test]
async fn test_to_graph_exports_structure() {
    let pipeline = Pipeline::new("market_data")
        .start_with(NormalizeStep)
        .fork_when(
            |d: &MarketData| d.asset_class == "crypto",
            "crypto_pipeline",
        )
        .fan_out(&["ml_pipeline", "stats_pipeline"])
        .with_recorder(NoopRecorder)
        .build();

    let graph = pipeline.to_graph();

    assert_eq!(graph.name, "market_data");
    assert_eq!(graph.steps.len(), 1);
    assert_eq!(graph.steps[0].name, "normalize");

    assert_eq!(graph.forks.len(), 1);
    assert_eq!(graph.forks[0].target_pipeline, "crypto_pipeline");

    assert_eq!(graph.fan_outs.len(), 1);
    assert_eq!(
        graph.fan_outs[0].targets,
        vec!["ml_pipeline", "stats_pipeline"]
    );
}

#[tokio::test]
async fn test_graph_serializes_to_json() {
    let pipeline = Pipeline::new("market_data")
        .start_with(NormalizeStep)
        .fork_when(
            |d: &MarketData| d.asset_class == "crypto",
            "crypto_pipeline",
        )
        .fan_out(&["ml_pipeline"])
        .with_recorder(NoopRecorder)
        .build();

    let graph = pipeline.to_graph();
    let json = serde_json::to_string_pretty(&graph).unwrap();

    // Verify it's valid JSON that can be parsed back
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["name"], "market_data");
    assert!(parsed["steps"].is_array());
    assert!(parsed["forks"].is_array());
    assert!(parsed["fan_outs"].is_array());
}

#[tokio::test]
async fn test_fork_when_with_description() {
    let pipeline = Pipeline::new("market_data")
        .start_with(NormalizeStep)
        .fork_when(
            |d: &MarketData| d.asset_class == "crypto",
            "crypto_pipeline",
        )
        .desc("routes crypto assets")
        .with_recorder(NoopRecorder)
        .build();

    let graph = pipeline.to_graph();
    assert_eq!(graph.forks[0].condition, "routes crypto assets");
}

// ============================================================================
// Runner Integration Tests
// ============================================================================

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ProcessedData {
    symbol: String,
    processed: bool,
}

impl HasEntityId for ProcessedData {
    fn entity_id(&self) -> String {
        self.symbol.clone()
    }
}

struct CryptoProcessor {
    processed: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl Step for CryptoProcessor {
    type Input = MarketData;
    type Output = ProcessedData;

    fn name(&self) -> &'static str {
        "crypto_processor"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        self.processed.lock().await.push(input.symbol.clone());
        Ok(ProcessedData {
            symbol: input.symbol,
            processed: true,
        })
    }
}

#[tokio::test]
async fn test_runner_executes_forked_tasks() {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();

    let crypto_processed = Arc::new(Mutex::new(Vec::new()));

    let main_pipeline = Pipeline::new("market_data")
        .start_with(NormalizeStep)
        .fork_when(
            |d: &MarketData| d.asset_class == "crypto",
            "crypto_pipeline",
        )
        .with_recorder(NoopRecorder)
        .build();

    let crypto_pipeline = Pipeline::new("crypto_pipeline")
        .start_with(CryptoProcessor {
            processed: crypto_processed.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    let runner = RunnerBuilder::new(store)
        .pipeline(main_pipeline)
        .pipeline(crypto_pipeline)
        .poll_interval(Duration::from_millis(50))
        .max_concurrent(2)
        .build();

    // Submit crypto data - should fork
    runner
        .submit(
            "market_data",
            MarketData {
                symbol: "BTC".to_string(),
                asset_class: "crypto".to_string(),
                price: 50000.0,
            },
        )
        .await
        .unwrap();

    // Submit equity data - should NOT fork
    runner
        .submit(
            "market_data",
            MarketData {
                symbol: "AAPL".to_string(),
                asset_class: "equity".to_string(),
                price: 150.0,
            },
        )
        .await
        .unwrap();

    // Let runner process
    tokio::select! {
        _ = runner.run() => {}
        _ = tokio::time::sleep(Duration::from_millis(500)) => {}
    }

    // Only BTC should have been processed by crypto_pipeline
    let processed = crypto_processed.lock().await;
    assert_eq!(processed.len(), 1);
    assert!(processed.contains(&"BTC".to_string()));
}

struct RecordingStep {
    name: &'static str,
    recorded: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl Step for RecordingStep {
    type Input = MarketData;
    type Output = MarketData;

    fn name(&self) -> &'static str {
        self.name
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        self.recorded
            .lock()
            .await
            .push(format!("{}:{}", self.name, input.symbol));
        Ok(input)
    }
}

#[tokio::test]
async fn test_runner_executes_fan_out_to_all_targets() {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();

    let recorded = Arc::new(Mutex::new(Vec::new()));

    let main_pipeline = Pipeline::new("market_data")
        .start_with(NormalizeStep)
        .fan_out(&["ml_pipeline", "stats_pipeline"])
        .with_recorder(NoopRecorder)
        .build();

    let ml_pipeline = Pipeline::new("ml_pipeline")
        .start_with(RecordingStep {
            name: "ml_processor",
            recorded: recorded.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    let stats_pipeline = Pipeline::new("stats_pipeline")
        .start_with(RecordingStep {
            name: "stats_processor",
            recorded: recorded.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    let runner = RunnerBuilder::new(store)
        .pipeline(main_pipeline)
        .pipeline(ml_pipeline)
        .pipeline(stats_pipeline)
        .poll_interval(Duration::from_millis(50))
        .max_concurrent(4)
        .build();

    runner
        .submit(
            "market_data",
            MarketData {
                symbol: "AAPL".to_string(),
                asset_class: "equity".to_string(),
                price: 150.0,
            },
        )
        .await
        .unwrap();

    tokio::select! {
        _ = runner.run() => {}
        _ = tokio::time::sleep(Duration::from_millis(500)) => {}
    }

    let recorded = recorded.lock().await;
    assert_eq!(recorded.len(), 2);
    assert!(recorded.contains(&"ml_processor:AAPL".to_string()));
    assert!(recorded.contains(&"stats_processor:AAPL".to_string()));
}

#[tokio::test]
async fn test_fork_to_unknown_pipeline_fails_gracefully() {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let pool_clone = pool.clone();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();

    let main_pipeline = Pipeline::new("market_data")
        .start_with(NormalizeStep)
        .fork_when(|_: &MarketData| true, "nonexistent_pipeline")
        .with_recorder(NoopRecorder)
        .build();

    let runner = RunnerBuilder::new(store)
        .pipeline(main_pipeline)
        .poll_interval(Duration::from_millis(50))
        .max_concurrent(1)
        .build();

    runner
        .submit(
            "market_data",
            MarketData {
                symbol: "BTC".to_string(),
                asset_class: "crypto".to_string(),
                price: 50000.0,
            },
        )
        .await
        .unwrap();

    tokio::select! {
        _ = runner.run() => {}
        _ = tokio::time::sleep(Duration::from_millis(300)) => {}
    }

    // The spawned task to nonexistent_pipeline should be failed
    let failed_tasks: Vec<(String, String)> =
        sqlx::query_as("SELECT pipeline, status FROM delta_tasks WHERE status = 'failed'")
            .fetch_all(&pool_clone)
            .await
            .unwrap();

    assert!(failed_tasks
        .iter()
        .any(|(p, _)| p == "nonexistent_pipeline"));
}
