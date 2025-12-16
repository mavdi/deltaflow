//! Demo: Run this to test the visualizer
//!
//! ```bash
//! cargo run --example visualizer_demo --features sqlite
//! ```
//!
//! Then open http://localhost:3000 in your browser.

use async_trait::async_trait;
use deltaflow::{
    HasEntityId, NoopRecorder, Pipeline, RunnerBuilder, SqliteTaskStore, Step, StepError,
};
use deltaflow_harness::RunnerHarnessExt;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct Order {
    id: String,
    amount: f64,
}

impl HasEntityId for Order {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct ValidatedOrder {
    id: String,
    amount: f64,
}

impl HasEntityId for ValidatedOrder {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

struct ValidateOrder;
struct ProcessPayment;
struct SendConfirmation;

#[async_trait]
impl Step for ValidateOrder {
    type Input = Order;
    type Output = ValidatedOrder;

    fn name(&self) -> &'static str {
        "ValidateOrder"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(ValidatedOrder {
            id: input.id,
            amount: input.amount,
        })
    }
}

#[async_trait]
impl Step for ProcessPayment {
    type Input = ValidatedOrder;
    type Output = ValidatedOrder;

    fn name(&self) -> &'static str {
        "ProcessPayment"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for SendConfirmation {
    type Input = ValidatedOrder;
    type Output = String;

    fn name(&self) -> &'static str {
        "SendConfirmation"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(format!("Order {} confirmed!", input.id))
    }
}

#[tokio::main]
async fn main() {
    println!("Starting visualizer demo...");
    println!("Open http://localhost:3000 in your browser");
    println!("Press Ctrl+C to stop\n");

    // Create in-memory SQLite store
    let pool = sqlx::SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();

    // Build order processing pipeline
    let order_pipeline = Pipeline::new("process_order")
        .start_with(ValidateOrder)
        .then(ProcessPayment)
        .then(SendConfirmation)
        .with_recorder(NoopRecorder)
        .build();

    // Build notification pipeline (to show multiple pipelines)
    let notify_pipeline = Pipeline::new("send_notification")
        .start_with(SendConfirmation)
        .with_recorder(NoopRecorder)
        .build();

    // Build runner with visualizer on port 3000
    let _runner = RunnerBuilder::new(store)
        .pipeline(order_pipeline)
        .pipeline(notify_pipeline)
        .with_visualizer(3000)
        .build();

    // Keep running until Ctrl+C
    tokio::signal::ctrl_c().await.unwrap();
    println!("\nShutting down...");
}
