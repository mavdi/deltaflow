//! # Deltaflow
//!
//! The embeddable workflow engine.
//!
//! ![Pipeline visualization](https://raw.githubusercontent.com/mavdi/deltaflow/master/docs/assets/example.png)
//!
//! Type-safe, Elixir-inspired pipelines that run in your process. No infrastructure required.
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
//!     .start_with(ParseInput)
//!     .then(ProcessData)
//!     .then(FormatOutput)
//!     .with_retry(RetryPolicy::exponential(3))
//!     .with_recorder(NoopRecorder)
//!     .build();
//!
//! let result = pipeline.run(input).await?;
//! ```
//!
//! ## Forking and Fan-out
//!
//! Route output to multiple downstream pipelines:
//!
//! ```rust,ignore
//! let pipeline = Pipeline::new("router")
//!     .start_with(ValidateStep)
//!     .fork_when(|data| data.priority == "high", "fast_track")
//!     .fork_when_desc(|data| data.needs_review, "review", "needs_review")
//!     .fan_out(&["analytics", "archival"])
//!     .spawn_from("notifications", |data| vec![Notification::from(data)])
//!     .build();
//! ```
//!
//! ## Web Visualizer
//!
//! Use `deltaflow-harness` for interactive pipeline visualization:
//!
//! ```rust,ignore
//! use deltaflow_harness::RunnerHarnessExt;
//!
//! let runner = RunnerBuilder::new(store)
//!     .pipeline(my_pipeline)
//!     .with_visualizer(3000)
//!     .build();
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
    BuiltPipeline, EmitNode, FanOutNode, ForkNode, HasEntityId, Metadata, Pipeline,
    PipelineError, PipelineGraph, SpawnRule, StepNode,
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
