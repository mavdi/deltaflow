//! Web-based pipeline visualization for Deltaflow.
//!
//! This crate adds a `.with_visualizer(port)` method to `RunnerBuilder` that
//! spawns a web UI showing your pipeline topology.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use deltaflow::{RunnerBuilder, SqliteTaskStore};
//! use deltaflow_harness::RunnerHarnessExt;
//!
//! let runner = RunnerBuilder::new(store)
//!     .pipeline(my_pipeline)
//!     .with_visualizer(3000)
//!     .build();
//!
//! // Open http://localhost:3000 to view pipeline topology
//! ```
//!
//! # What It Shows
//!
//! - Pipeline steps as connected boxes
//! - Fork, fan-out, and spawn connections between pipelines
//! - Interactive canvas with pan support
//!
//! For full documentation, see [deltaflow](https://docs.rs/deltaflow).

mod ext;
mod graph;
mod server;

pub use ext::RunnerHarnessExt;
