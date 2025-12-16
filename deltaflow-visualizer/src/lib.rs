//! Web-based pipeline visualization for Deltaflow.
//!
//! This crate provides a web-based UI for visualizing pipeline topology in Deltaflow applications.
//! The visualizer shows pipeline steps as boxes with connections between them, rendered on an
//! interactive canvas that supports panning.
//!
//! # Features
//!
//! - Automatic pipeline topology extraction
//! - Interactive canvas with pan support
//! - Shows fork, fan-out, and dynamic spawn connections
//! - Minimal performance overhead (HTTP server runs in background)
//!
//! # Example
//!
//! ```no_run
//! use deltaflow::{Pipeline, Step, StepError, RunnerBuilder, SqliteTaskStore, HasEntityId};
//! use deltaflow_visualizer::RunnerVisualizerExt;
//! use async_trait::async_trait;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Clone, Serialize, Deserialize)]
//! struct MyInput {
//!     id: String,
//! }
//!
//! impl HasEntityId for MyInput {
//!     fn entity_id(&self) -> String {
//!         self.id.clone()
//!     }
//! }
//!
//! struct MyStep;
//!
//! #[async_trait]
//! impl Step for MyStep {
//!     type Input = MyInput;
//!     type Output = MyInput;
//!
//!     fn name(&self) -> &'static str {
//!         "MyStep"
//!     }
//!
//!     async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
//!         Ok(input)
//!     }
//! }
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! # let pool = sqlx::SqlitePool::connect(":memory:").await?;
//! let store = SqliteTaskStore::new(pool);
//!
//! let pipeline = Pipeline::new("my_pipeline")
//!     .start_with(MyStep)
//!     .build();
//!
//! let runner = RunnerBuilder::new(store)
//!     .pipeline(pipeline)
//!     .with_visualizer(3000)  // Start visualizer at http://localhost:3000
//!     .build();
//!
//! // Visit http://localhost:3000 in your browser to see the pipeline topology
//! # Ok(())
//! # }
//! ```

mod ext;
mod graph;
mod server;

pub use ext::RunnerVisualizerExt;
