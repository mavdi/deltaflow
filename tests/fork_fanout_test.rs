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
