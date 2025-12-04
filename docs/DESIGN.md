# Delta Pipeline Engine - Design Document

A lightweight, type-safe pipeline engine for Rust.

**Repository:** `github.com/mavdi/delta`

## Overview & Goals

Delta is a standalone crate for composable, observable workflows where steps transform typed inputs to outputs. Inspired by Elixir's pipe operator, it brings declarative pipeline definitions to Rust with compile-time type safety.

**Goals:**
- **Explicit pipelines** - Declared via method chaining, not implicit code paths
- **Type safety** - Compiler enforces step output matches next step's input
- **Observability** - Every run and step recorded to SQLite for debugging
- **Zero external runtime dependencies** - Just SQLite for persistence, tokio for async

**Non-goals (for v1):**
- Resumability from mid-pipeline (restart from beginning on failure)
- Per-step retry policies (pipeline-level only)
- Distributed execution (single worker)
- Dynamic pipeline modification at runtime

**Example usage:**

```rust
let pipeline = Pipeline::new("video_processing")
    .start_with(FetchTranscript::new(ytdlp))
    .then(AnalyzeSentiment::new(ollama))
    .then(GenerateSignals::new(strategy))
    .with_retry(RetryPolicy::exponential(5))
    .with_recorder(SqliteRecorder::new(pool))
    .build();

let result = pipeline.run(video).await?;
```

## Core Architecture

### Step Trait

The fundamental building block. Each step defines typed input/output:

```rust
#[async_trait]
pub trait Step: Send + Sync {
    type Input: Send + Clone;
    type Output: Send;

    fn name(&self) -> &'static str;
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError>;
}
```

### StepError

```rust
pub enum StepError {
    Retryable(anyhow::Error),    // Transient failure, worth retrying
    Permanent(anyhow::Error),    // Won't succeed on retry, fail immediately
}
```

### Pipeline Builder

Type-safe chaining that captures the transformation sequence:

```rust
impl Pipeline<()> {
    pub fn new(name: &'static str) -> Self;
    pub fn start_with<S: Step>(self, step: S) -> Pipeline<S::Output>;
}

impl<T> Pipeline<T> {
    pub fn then<S: Step<Input = T>>(self, step: S) -> Pipeline<S::Output>;
    pub fn with_retry(self, policy: RetryPolicy) -> Self;
    pub fn with_recorder(self, recorder: impl Recorder) -> Self;
    pub fn build(self) -> BuiltPipeline<Input, T>;
}
```

The key insight: `.then()` only accepts steps whose `Input` matches the previous step's `Output`. Compile-time enforcement.

### Recorder Trait

Optional persistence layer (SQLite provided, but pluggable):

```rust
#[async_trait]
pub trait Recorder: Send + Sync {
    async fn start_run(&self, pipeline: &str, entity_id: &str) -> Result<RunId>;
    async fn start_step(&self, run: RunId, step: &str) -> Result<StepId>;
    async fn complete_step(&self, step: StepId, status: StepStatus) -> Result<()>;
    async fn complete_run(&self, run: RunId, status: RunStatus) -> Result<()>;
}
```

## Executor & Retry Logic

### Pipeline Execution

The `BuiltPipeline` runs steps sequentially, recording each transition:

```rust
impl<I, O> BuiltPipeline<I, O> {
    pub async fn run(&self, input: I) -> Result<O, PipelineError> {
        let run_id = self.recorder.start_run(&self.name, &input.entity_id()).await?;

        let mut current = input;
        for step in &self.steps {
            let step_id = self.recorder.start_step(run_id, step.name()).await?;

            match self.execute_with_retry(step, current).await {
                Ok(output) => {
                    self.recorder.complete_step(step_id, StepStatus::Completed).await?;
                    current = output;
                }
                Err(e) => {
                    self.recorder.complete_step(step_id, StepStatus::Failed(e.to_string())).await?;
                    self.recorder.complete_run(run_id, RunStatus::Failed).await?;
                    return Err(e);
                }
            }
        }

        self.recorder.complete_run(run_id, RunStatus::Completed).await?;
        Ok(current)
    }
}
```

### Retry Policy

```rust
pub enum RetryPolicy {
    None,
    Fixed { attempts: u32, delay: Duration },
    Exponential { attempts: u32, initial: Duration, max: Duration },
}

impl RetryPolicy {
    pub fn exponential(attempts: u32) -> Self {
        Self::Exponential {
            attempts,
            initial: Duration::from_secs(1),
            max: Duration::from_secs(300),
        }
    }

    pub fn delay_for_attempt(&self, attempt: u32) -> Option<Duration> {
        // Returns None if max attempts exceeded
    }
}
```

### Retry Execution

```rust
async fn execute_with_retry<S: Step>(&self, step: &S, input: S::Input) -> Result<S::Output, PipelineError> {
    let mut attempt = 0;
    loop {
        match step.execute(input.clone()).await {
            Ok(output) => return Ok(output),
            Err(StepError::Permanent(e)) => return Err(PipelineError::StepFailed(e)),
            Err(StepError::Retryable(e)) => {
                attempt += 1;
                match self.retry_policy.delay_for_attempt(attempt) {
                    Some(delay) => tokio::time::sleep(delay).await,
                    None => return Err(PipelineError::RetriesExhausted(e)),
                }
            }
        }
    }
}
```

Note: This requires `Input: Clone` for retry support.

## Persistence & SQLite Recorder

### Schema

Delta creates prefixed tables in the host application's database:

```sql
CREATE TABLE delta_runs (
    id INTEGER PRIMARY KEY,
    pipeline_name TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT,
    status TEXT NOT NULL DEFAULT 'running',  -- 'running', 'completed', 'failed'
    error_message TEXT
);

CREATE TABLE delta_steps (
    id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL REFERENCES delta_runs(id),
    step_name TEXT NOT NULL,
    step_index INTEGER NOT NULL,
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT,
    status TEXT NOT NULL DEFAULT 'running',  -- 'running', 'completed', 'failed'
    attempt INTEGER NOT NULL DEFAULT 1,
    error_message TEXT
);

CREATE INDEX idx_delta_runs_pipeline ON delta_runs(pipeline_name, started_at);
CREATE INDEX idx_delta_runs_entity ON delta_runs(entity_id);
CREATE INDEX idx_delta_steps_run ON delta_steps(run_id);
```

### SqliteRecorder

```rust
pub struct SqliteRecorder {
    pool: SqlitePool,
}

impl SqliteRecorder {
    pub fn new(pool: SqlitePool) -> Self { Self { pool } }

    pub async fn run_migrations(&self) -> Result<()> {
        // Create tables if not exists
    }
}
```

### NoopRecorder

For testing or when persistence isn't needed:

```rust
pub struct NoopRecorder;

#[async_trait]
impl Recorder for NoopRecorder {
    async fn start_run(&self, _: &str, _: &str) -> Result<RunId> { Ok(RunId(0)) }
    async fn start_step(&self, _: RunId, _: &str) -> Result<StepId> { Ok(StepId(0)) }
    async fn complete_step(&self, _: StepId, _: StepStatus) -> Result<()> { Ok(()) }
    async fn complete_run(&self, _: RunId, _: RunStatus) -> Result<()> { Ok(()) }
}
```

## Crate Structure

### File Layout

```
delta/
├── Cargo.toml
├── src/
│   ├── lib.rs           # Re-exports public API
│   ├── step.rs          # Step trait, StepError
│   ├── pipeline.rs      # Pipeline builder, BuiltPipeline
│   ├── executor.rs      # Execution logic, retry handling
│   ├── retry.rs         # RetryPolicy
│   ├── recorder.rs      # Recorder trait, NoopRecorder
│   └── sqlite.rs        # SqliteRecorder (behind feature flag)
└── examples/
    └── basic.rs
```

### Cargo.toml

```toml
[package]
name = "delta"
version = "0.1.0"
edition = "2021"
description = "A lightweight, type-safe pipeline engine for Rust"
license = "MIT"
repository = "https://github.com/mavdi/delta"

[features]
default = []
sqlite = ["sqlx"]

[dependencies]
async-trait = "0.1"
thiserror = "2"
tokio = { version = "1", features = ["time"] }
anyhow = "1"

# Optional
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"], optional = true }
```

### Public API

```rust
pub use step::{Step, StepError};
pub use pipeline::Pipeline;
pub use executor::{BuiltPipeline, PipelineError};
pub use retry::RetryPolicy;
pub use recorder::{Recorder, NoopRecorder, RunId, StepId, RunStatus, StepStatus};

#[cfg(feature = "sqlite")]
pub use sqlite::SqliteRecorder;
```

## Design Decisions Summary

| Aspect | Decision | Rationale |
|--------|----------|-----------|
| Composition | Method chaining (`.then()`) | Idiomatic Rust, no macro magic |
| Context passing | Typed transforms | Compile-time safety, explicit data flow |
| Resumability | Restart from beginning | Simplicity for v1 |
| Retry policy | Pipeline-level only | Keeps API simple |
| Persistence | Prefixed tables in host DB | Enables joins with domain data |
| SQLite | Optional feature flag | Zero overhead if not needed |
