//! Task runner module for executing pipelines from a persistent queue.
//!
//! # Overview
//!
//! The runner provides a way to execute pipelines asynchronously from a durable
//! task queue. Tasks are stored in SQLite and processed with configurable
//! concurrency.
//!
//! # Example
//!
//! ```ignore
//! let store = SqliteTaskStore::new(pool);
//! store.run_migrations().await?;
//!
//! let pipeline = Pipeline::new("my_pipeline")
//!     .start_with(MyStep)
//!     .build();
//!
//! let runner = RunnerBuilder::new(store)
//!     .pipeline(pipeline)
//!     .poll_interval(Duration::from_secs(1))
//!     .max_concurrent(4)
//!     .build();
//!
//! runner.submit("my_pipeline", my_input).await?;
//! runner.run().await;
//! ```

pub mod erased;
pub mod executor;
pub mod sqlite_store;
pub mod store;

pub use erased::{ErasedPipeline, SpawnedTask};
pub use executor::{Runner, RunnerBuilder};
pub use sqlite_store::SqliteTaskStore;
pub use store::{StoredTask, TaskError, TaskId, TaskStore};
