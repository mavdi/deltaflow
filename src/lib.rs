//! Delta - A lightweight, type-safe pipeline engine for Rust.

pub mod pipeline;
pub mod recorder;
pub mod retry;
pub mod step;

pub use pipeline::{BuiltPipeline, HasEntityId, Pipeline, PipelineError};
pub use recorder::{NoopRecorder, Recorder, RunId, RunStatus, StepId, StepStatus};
pub use retry::RetryPolicy;
pub use step::{Step, StepError};

#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "sqlite")]
pub mod runner;

#[cfg(feature = "sqlite")]
pub use sqlite::SqliteRecorder;

#[cfg(feature = "sqlite")]
pub use runner::{
    ErasedPipeline, Runner, RunnerBuilder, SqliteTaskStore, SpawnedTask, StoredTask, TaskError,
    TaskId, TaskStore,
};
