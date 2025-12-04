//! Delta - A lightweight, type-safe pipeline engine for Rust.

pub mod step;
pub mod retry;
pub mod recorder;
pub mod pipeline;

pub use step::{Step, StepError};
pub use retry::RetryPolicy;
pub use recorder::{Recorder, NoopRecorder, RunId, StepId, RunStatus, StepStatus};
pub use pipeline::{Pipeline, PipelineError};

#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "sqlite")]
pub use sqlite::SqliteRecorder;
