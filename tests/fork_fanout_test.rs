//! Tests for fork and fan-out functionality.

use async_trait::async_trait;
use deltaflow::{HasEntityId, NoopRecorder, Pipeline, Step, StepError};
use serde::{Deserialize, Serialize};

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
        .fork_when(|d: &MarketData| d.asset_class == "crypto", "crypto_pipeline")
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
        .fork_when(|d: &MarketData| d.asset_class == "crypto", "crypto_pipeline")
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
        .fork_when(|d: &MarketData| d.asset_class == "crypto", "crypto_pipeline")
        .fork_when(|d: &MarketData| d.price > 10000.0, "high_value_pipeline")
        .fork_when(|d: &MarketData| d.symbol.len() <= 4, "short_symbol_pipeline")
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
        .fork_when(|d: &MarketData| d.asset_class == "crypto", "crypto_pipeline")
        .fan_out(&["audit_pipeline"])
        .spawn_from("alert_pipeline", |d: &MarketData| {
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
        .fork_when(|d: &MarketData| d.asset_class == "crypto", "crypto_pipeline")
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
    assert_eq!(graph.fan_outs[0].targets, vec!["ml_pipeline", "stats_pipeline"]);
}

#[tokio::test]
async fn test_graph_serializes_to_json() {
    let pipeline = Pipeline::new("market_data")
        .start_with(NormalizeStep)
        .fork_when(|d: &MarketData| d.asset_class == "crypto", "crypto_pipeline")
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
