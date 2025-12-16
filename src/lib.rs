//! # Deltaflow
//!
//! The embeddable workflow engine.
//!
//! Deltaflow brings Elixir-style pipeline composition to Rust with compile-time
//! type safety. Build observable workflows where each step transforms typed
//! inputs to outputs - no infrastructure required.
//!
//! ## Why Deltaflow?
//!
//! - **Type-safe composition** - Compiler enforces step output matches next step's input
//! - **Elixir-inspired** - Declarative pipelines via method chaining, not scattered callbacks
//! - **Observable by default** - Every run and step recorded for debugging
//! - **Embeddable** - A library, not a service. Runs in your process.
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use deltaflow::{Pipeline, Step, StepError, RetryPolicy, NoopRecorder};
//!
//! let pipeline = Pipeline::new("my_workflow")
//!     .start_with(ParseInput)       // String -> ParsedData
//!     .then(ProcessData)            // ParsedData -> ProcessedData
//!     .then(FormatOutput)           // ProcessedData -> Output
//!     .with_retry(RetryPolicy::exponential(3))
//!     .with_recorder(NoopRecorder)
//!     .build();
//!
//! let result = pipeline.run(input).await?;
//! ```
//!
//! ## Feature Flags
//!
//! - `sqlite` - Enable SQLite-backed recording and task storage

pub mod pipeline;
pub mod recorder;
pub mod retry;
pub mod step;

pub use pipeline::{
    BuiltPipeline, DynamicSpawnNode, FanOutNode, ForkNode, HasEntityId, Pipeline, PipelineError,
    PipelineGraph, SpawnRule, StepNode,
};
pub use recorder::{NoopRecorder, Recorder, RunId, RunStatus, StepId, StepStatus};
pub use retry::RetryPolicy;
pub use step::{Step, StepError};

#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "sqlite")]
pub mod runner;

#[cfg(feature = "sqlite")]
pub mod scheduler;

#[cfg(feature = "sqlite")]
pub use sqlite::SqliteRecorder;

#[cfg(feature = "sqlite")]
pub use runner::{
    ErasedPipeline, Runner, RunnerBuilder, SpawnedTask, SqliteTaskStore, StoredTask, TaskError,
    TaskId, TaskStore,
};

#[cfg(feature = "sqlite")]
pub use scheduler::{PeriodicScheduler, SchedulerBuilder};
