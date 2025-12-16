//! Complex visualizer demo demonstrating layout edge cases
//!
//! ```bash
//! cargo run --example visualizer_complex --features sqlite
//! ```
//!
//! Edge cases demonstrated:
//! - Diamond pattern: ingest -> enrich -> store AND ingest -> store directly
//! - Multiple sources to same target: enrich AND alert both fork to audit
//! - Backwards arrow: store forks back to monitor (column 2 -> column 0)
//! - Independent roots: ingest and monitor are separate root pipelines
//! - Deep chain: ingest -> enrich -> transform -> store (tests column depth)

use async_trait::async_trait;
use deltaflow::{
    HasEntityId, NoopRecorder, Pipeline, RunnerBuilder, SqliteTaskStore, Step, StepError,
};
use deltaflow_harness::RunnerHarnessExt;
use serde::{Deserialize, Serialize};

// ============================================================================
// Generic event type for all pipelines
// ============================================================================

#[derive(Clone, Serialize, Deserialize)]
struct Event {
    id: String,
    data: String,
}

impl HasEntityId for Event {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

// ============================================================================
// Ingest Pipeline: ingest -> validate -> enrich
// ============================================================================

struct Receive;
struct Validate;

#[async_trait]
impl Step for Receive {
    type Input = Event;
    type Output = Event;
    fn name(&self) -> &'static str { "Receive" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for Validate {
    type Input = Event;
    type Output = Event;
    fn name(&self) -> &'static str { "Validate" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

// ============================================================================
// Enrich Pipeline: parse -> transform
// ============================================================================

struct Parse;
struct Transform;

#[async_trait]
impl Step for Parse {
    type Input = Event;
    type Output = Event;
    fn name(&self) -> &'static str { "Parse" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for Transform {
    type Input = Event;
    type Output = Event;
    fn name(&self) -> &'static str { "Transform" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

// ============================================================================
// Store Pipeline: index -> persist -> replicate
// ============================================================================

struct Index;
struct Persist;
struct Replicate;

#[async_trait]
impl Step for Index {
    type Input = Event;
    type Output = Event;
    fn name(&self) -> &'static str { "Index" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for Persist {
    type Input = Event;
    type Output = Event;
    fn name(&self) -> &'static str { "Persist" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for Replicate {
    type Input = Event;
    type Output = Event;
    fn name(&self) -> &'static str { "Replicate" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

// ============================================================================
// Monitor Pipeline (independent root): watch -> detect
// ============================================================================

struct Watch;
struct Detect;

#[async_trait]
impl Step for Watch {
    type Input = Event;
    type Output = Event;
    fn name(&self) -> &'static str { "Watch" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for Detect {
    type Input = Event;
    type Output = Event;
    fn name(&self) -> &'static str { "Detect" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

// ============================================================================
// Alert Pipeline: evaluate -> notify -> escalate
// ============================================================================

struct Evaluate;
struct Notify;
struct Escalate;

#[async_trait]
impl Step for Evaluate {
    type Input = Event;
    type Output = Event;
    fn name(&self) -> &'static str { "Evaluate" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for Notify {
    type Input = Event;
    type Output = Event;
    fn name(&self) -> &'static str { "Notify" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for Escalate {
    type Input = Event;
    type Output = Event;
    fn name(&self) -> &'static str { "Escalate" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

// ============================================================================
// Audit Pipeline: record -> archive
// ============================================================================

struct Record;
struct Archive;

#[async_trait]
impl Step for Record {
    type Input = Event;
    type Output = Event;
    fn name(&self) -> &'static str { "Record" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[async_trait]
impl Step for Archive {
    type Input = Event;
    type Output = Event;
    fn name(&self) -> &'static str { "Archive" }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[tokio::main]
async fn main() {
    println!("==============================================");
    println!("  Deltaflow Harness - Edge Cases Demo");
    println!("==============================================");
    println!();
    println!("Open http://localhost:3000 in your browser");
    println!("Press Ctrl+C to stop");
    println!();
    println!("Edge cases demonstrated:");
    println!("  [Diamond]     ingest -> enrich -> store");
    println!("                ingest -----------> store");
    println!("  [Multi-src]   enrich -> audit");
    println!("                alert  -> audit");
    println!("  [Cycle]       store  -> ingest (backwards arrow)");
    println!("  [Indep roots] ingest and monitor are separate roots");
    println!();

    // Create in-memory SQLite store
    let pool = sqlx::SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();

    // ========================================================================
    // Pipeline: ingest (ROOT, Column 0)
    // Forks to: enrich (diamond leg 1), store (diamond leg 2 - direct)
    // ========================================================================
    let ingest = Pipeline::new("ingest")
        .start_with(Receive)
        .then(Validate)
        .fork_when_desc(|_: &Event| true, "enrich", "to_enrich")
        .fork_when_desc(|_: &Event| true, "store", "direct")  // Diamond: skip enrich
        .with_recorder(NoopRecorder)
        .build();

    // ========================================================================
    // Pipeline: enrich (Column 1)
    // Forks to: store (completes diamond), audit (multi-source test)
    // ========================================================================
    let enrich = Pipeline::new("enrich")
        .start_with(Parse)
        .then(Transform)
        .fork_when_desc(|_: &Event| true, "store", "enriched")
        .fork_when_desc(|_: &Event| true, "audit", "for_audit")  // Multi-source leg 1
        .with_recorder(NoopRecorder)
        .build();

    // ========================================================================
    // Pipeline: store (Column 2)
    // Forks to: ingest (BACKWARDS/CYCLE - column 2 back to column 0)
    // ========================================================================
    let store_pipeline = Pipeline::new("store")
        .start_with(Index)
        .then(Persist)
        .then(Replicate)
        .fork_when_desc(|_: &Event| true, "ingest", "retry")  // Cycle back to start!
        .with_recorder(NoopRecorder)
        .build();

    // ========================================================================
    // Pipeline: monitor (ROOT, Column 0 - independent of ingest)
    // Forks to: alert
    // ========================================================================
    let monitor = Pipeline::new("monitor")
        .start_with(Watch)
        .then(Detect)
        .fork_when_desc(|_: &Event| true, "alert", "detected")
        .with_recorder(NoopRecorder)
        .build();

    // ========================================================================
    // Pipeline: alert (Column 1, child of monitor)
    // Forks to: audit (multi-source leg 2)
    // ========================================================================
    let alert = Pipeline::new("alert")
        .start_with(Evaluate)
        .then(Notify)
        .then(Escalate)
        .fork_when_desc(|_: &Event| true, "audit", "alert_log")  // Multi-source leg 2
        .with_recorder(NoopRecorder)
        .build();

    // ========================================================================
    // Pipeline: audit (Column 2 - receives from enrich AND alert)
    // ========================================================================
    let audit = Pipeline::new("audit")
        .start_with(Record)
        .then(Archive)
        .with_recorder(NoopRecorder)
        .build();

    // Build runner with all pipelines and visualizer
    let _runner = RunnerBuilder::new(store)
        .pipeline(ingest)
        .pipeline(enrich)
        .pipeline(store_pipeline)
        .pipeline(monitor)
        .pipeline(alert)
        .pipeline(audit)
        .with_visualizer(3000)
        .build();

    // Keep running until Ctrl+C
    tokio::signal::ctrl_c().await.unwrap();
    println!("\nShutting down...");
}
