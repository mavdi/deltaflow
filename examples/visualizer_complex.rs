//! Complex visualizer demo with multiple pipelines and connections
//!
//! ```bash
//! cargo run --example visualizer_complex --features sqlite
//! ```
//!
//! Then open http://localhost:3000 in your browser.

use async_trait::async_trait;
use deltaflow::{
    HasEntityId, NoopRecorder, Pipeline, RunnerBuilder, SqliteTaskStore, Step, StepError,
};
use deltaflow_harness::RunnerHarnessExt;
use serde::{Deserialize, Serialize};

// ============================================================================
// Domain types
// ============================================================================

#[derive(Clone, Serialize, Deserialize)]
struct Order {
    id: String,
    customer_id: String,
    amount: f64,
    is_priority: bool,
}

impl HasEntityId for Order {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct ValidatedOrder {
    id: String,
    customer_id: String,
    amount: f64,
    is_priority: bool,
}

impl HasEntityId for ValidatedOrder {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct ProcessedOrder {
    id: String,
    customer_id: String,
    amount: f64,
    payment_ref: String,
}

impl HasEntityId for ProcessedOrder {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Notification {
    id: String,
    recipient: String,
    message: String,
}

impl HasEntityId for Notification {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct InventoryUpdate {
    id: String,
    order_id: String,
}

impl HasEntityId for InventoryUpdate {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct AnalyticsEvent {
    id: String,
    event_type: String,
}

impl HasEntityId for AnalyticsEvent {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

// ============================================================================
// Order Processing Pipeline Steps
// ============================================================================

struct ValidateOrder;
struct CheckInventory;
struct ProcessPayment;
struct FulfillOrder;

#[async_trait]
impl Step for ValidateOrder {
    type Input = Order;
    type Output = ValidatedOrder;
    fn name(&self) -> &'static str { "ValidateOrder" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(ValidatedOrder {
            id: input.id,
            customer_id: input.customer_id,
            amount: input.amount,
            is_priority: input.is_priority,
        })
    }
}

#[async_trait]
impl Step for CheckInventory {
    type Input = ValidatedOrder;
    type Output = ValidatedOrder;
    fn name(&self) -> &'static str { "CheckInventory" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for ProcessPayment {
    type Input = ValidatedOrder;
    type Output = ProcessedOrder;
    fn name(&self) -> &'static str { "ProcessPayment" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(ProcessedOrder {
            id: input.id,
            customer_id: input.customer_id,
            amount: input.amount,
            payment_ref: "PAY-12345".to_string(),
        })
    }
}

#[async_trait]
impl Step for FulfillOrder {
    type Input = ProcessedOrder;
    type Output = ProcessedOrder;
    fn name(&self) -> &'static str { "FulfillOrder" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

// ============================================================================
// Notification Pipeline Steps
// ============================================================================

struct BuildEmail;
struct SendEmail;

#[async_trait]
impl Step for BuildEmail {
    type Input = Notification;
    type Output = Notification;
    fn name(&self) -> &'static str { "BuildEmail" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for SendEmail {
    type Input = Notification;
    type Output = Notification;
    fn name(&self) -> &'static str { "SendEmail" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

// ============================================================================
// Inventory Pipeline Steps
// ============================================================================

struct ReserveStock;
struct UpdateWarehouse;
struct NotifySupplier;

#[async_trait]
impl Step for ReserveStock {
    type Input = InventoryUpdate;
    type Output = InventoryUpdate;
    fn name(&self) -> &'static str { "ReserveStock" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for UpdateWarehouse {
    type Input = InventoryUpdate;
    type Output = InventoryUpdate;
    fn name(&self) -> &'static str { "UpdateWarehouse" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for NotifySupplier {
    type Input = InventoryUpdate;
    type Output = InventoryUpdate;
    fn name(&self) -> &'static str { "NotifySupplier" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

// ============================================================================
// Analytics Pipeline Steps
// ============================================================================

struct EnrichEvent;
struct StoreMetrics;
struct UpdateDashboard;

#[async_trait]
impl Step for EnrichEvent {
    type Input = AnalyticsEvent;
    type Output = AnalyticsEvent;
    fn name(&self) -> &'static str { "EnrichEvent" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for StoreMetrics {
    type Input = AnalyticsEvent;
    type Output = AnalyticsEvent;
    fn name(&self) -> &'static str { "StoreMetrics" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for UpdateDashboard {
    type Input = AnalyticsEvent;
    type Output = AnalyticsEvent;
    fn name(&self) -> &'static str { "UpdateDashboard" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

// ============================================================================
// Priority Order Pipeline Steps
// ============================================================================

struct ExpediteShipping;
struct AssignDedicatedSupport;

#[async_trait]
impl Step for ExpediteShipping {
    type Input = ProcessedOrder;
    type Output = ProcessedOrder;
    fn name(&self) -> &'static str { "ExpediteShipping" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for AssignDedicatedSupport {
    type Input = ProcessedOrder;
    type Output = ProcessedOrder;
    fn name(&self) -> &'static str { "AssignDedicatedSupport" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[tokio::main]
async fn main() {
    println!("==============================================");
    println!("  Deltaflow Visualizer - Complex Demo");
    println!("==============================================");
    println!();
    println!("Open http://localhost:3000 in your browser");
    println!("Press Ctrl+C to stop");
    println!();
    println!("Pipelines:");
    println!("  - process_order (4 steps) -> forks to priority_fulfillment");
    println!("  - priority_fulfillment (2 steps)");
    println!("  - send_notification (2 steps)");
    println!("  - update_inventory (3 steps)");
    println!("  - track_analytics (3 steps)");
    println!();

    // Create in-memory SQLite store
    let pool = sqlx::SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();

    // Main order processing pipeline
    // Forks to TWO pipelines based on conditions
    let order_pipeline = Pipeline::new("process_order")
        .start_with(ValidateOrder)
        .then(CheckInventory)
        .then(ProcessPayment)
        .then(FulfillOrder)
        .fork_when_desc(
            |order: &ProcessedOrder| order.amount > 1000.0,
            "priority_fulfillment",
            "amount > 1000",
        )
        .fork_when_desc(
            |_: &ProcessedOrder| true,
            "send_notification",
            "always",
        )
        .with_recorder(NoopRecorder)
        .build();

    // Priority fulfillment pipeline (target of fork)
    let priority_pipeline = Pipeline::new("priority_fulfillment")
        .start_with(ExpediteShipping)
        .then(AssignDedicatedSupport)
        .with_recorder(NoopRecorder)
        .build();

    // Notification pipeline
    let notification_pipeline = Pipeline::new("send_notification")
        .start_with(BuildEmail)
        .then(SendEmail)
        .with_recorder(NoopRecorder)
        .build();

    // Inventory management pipeline
    let inventory_pipeline = Pipeline::new("update_inventory")
        .start_with(ReserveStock)
        .then(UpdateWarehouse)
        .then(NotifySupplier)
        .with_recorder(NoopRecorder)
        .build();

    // Analytics pipeline
    let analytics_pipeline = Pipeline::new("track_analytics")
        .start_with(EnrichEvent)
        .then(StoreMetrics)
        .then(UpdateDashboard)
        .with_recorder(NoopRecorder)
        .build();

    // Build runner with all pipelines and visualizer
    let _runner = RunnerBuilder::new(store)
        .pipeline(order_pipeline)
        .pipeline(priority_pipeline)
        .pipeline(notification_pipeline)
        .pipeline(inventory_pipeline)
        .pipeline(analytics_pipeline)
        .with_visualizer(3000)
        .build();

    // Keep running until Ctrl+C
    tokio::signal::ctrl_c().await.unwrap();
    println!("\nShutting down...");
}
