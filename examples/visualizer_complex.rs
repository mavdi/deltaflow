//! Order Processing Pipeline - Visualizer Demo
//!
//! ```bash
//! cargo run --example visualizer_complex --features sqlite
//! ```
//!
//! A realistic e-commerce order processing pipeline demonstrating:
//! - Single entry point (orders)
//! - Conditional branching (valid/invalid, in-stock/backorder)
//! - Convergence (multiple paths lead to notifications)
//! - Retry cycle (backorder -> fulfillment when restocked)
//!
//! Flow:
//!   orders -> fulfillment -> shipping -> notify
//!          -> returns -> notify
//!          -> fulfillment -> backorder -> fulfillment (retry cycle)

use async_trait::async_trait;
use deltaflow::{
    HasEntityId, NoopRecorder, Pipeline, RunnerBuilder, SqliteTaskStore, Step, StepError,
};
use deltaflow_harness::RunnerHarnessExt;
use serde::{Deserialize, Serialize};

// ============================================================================
// Order type used across all pipelines
// ============================================================================

#[derive(Clone, Serialize, Deserialize)]
struct Order {
    id: String,
    customer: String,
    items: Vec<String>,
}

impl HasEntityId for Order {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

// ============================================================================
// Orders Pipeline (ENTRY POINT): Receive -> Validate
// Branches to: fulfillment (valid), returns (invalid)
// ============================================================================

struct Receive;
struct Validate;

#[async_trait]
impl Step for Receive {
    type Input = Order;
    type Output = Order;
    fn name(&self) -> &'static str { "Receive" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for Validate {
    type Input = Order;
    type Output = Order;
    fn name(&self) -> &'static str { "Validate" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

// ============================================================================
// Fulfillment Pipeline: CheckStock -> Reserve -> Pack
// Branches to: shipping (in stock), backorder (out of stock)
// ============================================================================

struct CheckStock;
struct Reserve;
struct Pack;

#[async_trait]
impl Step for CheckStock {
    type Input = Order;
    type Output = Order;
    fn name(&self) -> &'static str { "CheckStock" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for Reserve {
    type Input = Order;
    type Output = Order;
    fn name(&self) -> &'static str { "Reserve" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for Pack {
    type Input = Order;
    type Output = Order;
    fn name(&self) -> &'static str { "Pack" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

// ============================================================================
// Shipping Pipeline: Label -> Dispatch
// Branches to: notify (shipment confirmation)
// ============================================================================

struct Label;
struct Dispatch;

#[async_trait]
impl Step for Label {
    type Input = Order;
    type Output = Order;
    fn name(&self) -> &'static str { "Label" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for Dispatch {
    type Input = Order;
    type Output = Order;
    fn name(&self) -> &'static str { "Dispatch" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

// ============================================================================
// Backorder Pipeline: Queue -> Source
// Branches to: fulfillment (RETRY CYCLE - when items restocked)
// ============================================================================

struct Queue;
struct Source;

#[async_trait]
impl Step for Queue {
    type Input = Order;
    type Output = Order;
    fn name(&self) -> &'static str { "Queue" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for Source {
    type Input = Order;
    type Output = Order;
    fn name(&self) -> &'static str { "Source" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

// ============================================================================
// Returns Pipeline: Process -> Refund
// Branches to: notify (refund confirmation)
// ============================================================================

struct Process;
struct Refund;

#[async_trait]
impl Step for Process {
    type Input = Order;
    type Output = Order;
    fn name(&self) -> &'static str { "Process" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for Refund {
    type Input = Order;
    type Output = Order;
    fn name(&self) -> &'static str { "Refund" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

// ============================================================================
// Notify Pipeline (TERMINAL): Format -> Send
// Receives from: shipping, returns
// ============================================================================

struct Format;
struct Send;

#[async_trait]
impl Step for Format {
    type Input = Order;
    type Output = Order;
    fn name(&self) -> &'static str { "Format" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for Send {
    type Input = Order;
    type Output = Order;
    fn name(&self) -> &'static str { "Send" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[tokio::main]
async fn main() {
    println!("==============================================");
    println!("  Order Processing Pipeline - Visualizer Demo");
    println!("==============================================");
    println!();
    println!("Open http://localhost:3000 in your browser");
    println!("Press Ctrl+C to stop");
    println!();
    println!("Pipeline flow:");
    println!("  orders -----> fulfillment ---> shipping ---> notify");
    println!("    |               |                           ^");
    println!("    |               +---> backorder --+         |");
    println!("    |                     (retry) ----+         |");
    println!("    +---> returns --------------------------> notify");
    println!();

    // Create in-memory SQLite store
    let pool = sqlx::SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();

    // ========================================================================
    // Pipeline: orders (ENTRY POINT)
    // Receives orders, validates them, routes to fulfillment or returns
    // ========================================================================
    let orders = Pipeline::new("orders")
        .start_with(Receive)
        .then(Validate)
        .fork_when(|result| result.is_ok(), "fulfillment").desc("valid")
        .fork_when(|result| result.is_err(), "returns").desc("invalid")  // Would check validation result
        .with_recorder(NoopRecorder)
        .build();

    // ========================================================================
    // Pipeline: fulfillment
    // Checks inventory, reserves items, packs order
    // Routes to shipping (in stock) or backorder (out of stock)
    // ========================================================================
    let fulfillment = Pipeline::new("fulfillment")
        .start_with(CheckStock)
        .then(Reserve)
        .then(Pack)
        .fork_when(|result| result.is_ok(), "shipping").desc("in_stock")
        .fork_when(|result| result.is_err(), "backorder").desc("out_of_stock")  // Would check stock
        .with_recorder(NoopRecorder)
        .build();

    // ========================================================================
    // Pipeline: shipping
    // Labels and dispatches the order, then notifies customer
    // ========================================================================
    let shipping = Pipeline::new("shipping")
        .start_with(Label)
        .then(Dispatch)
        .fork_when(|result| result.is_ok(), "notify").desc("shipped")
        .with_recorder(NoopRecorder)
        .build();

    // ========================================================================
    // Pipeline: backorder
    // Queues order, sources items, then retries fulfillment
    // This creates a CYCLE back to fulfillment
    // ========================================================================
    let backorder = Pipeline::new("backorder")
        .start_with(Queue)
        .then(Source)
        .fork_when(|result| result.is_ok(), "fulfillment").desc("restocked")  // RETRY CYCLE
        .with_recorder(NoopRecorder)
        .build();

    // ========================================================================
    // Pipeline: returns
    // Processes invalid/returned orders, issues refund, notifies customer
    // ========================================================================
    let returns = Pipeline::new("returns")
        .start_with(Process)
        .then(Refund)
        .fork_when(|result| result.is_ok(), "notify").desc("refunded")
        .with_recorder(NoopRecorder)
        .build();

    // ========================================================================
    // Pipeline: notify (TERMINAL)
    // Formats and sends notifications to customers
    // Receives from: shipping (order shipped) and returns (refund issued)
    // ========================================================================
    let notify = Pipeline::new("notify")
        .start_with(Format)
        .then(Send)
        .with_recorder(NoopRecorder)
        .build();

    // Build runner with all pipelines and visualizer
    let _runner = RunnerBuilder::new(store)
        .pipeline(orders)
        .pipeline(fulfillment)
        .pipeline(shipping)
        .pipeline(backorder)
        .pipeline(returns)
        .pipeline(notify)
        .with_visualizer(3000)
        .build();

    // Keep running until Ctrl+C
    tokio::signal::ctrl_c().await.unwrap();
    println!("\nShutting down...");
}
