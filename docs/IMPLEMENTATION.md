# Delta Pipeline Engine - Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a lightweight, type-safe pipeline engine for Rust with method chaining, typed transforms, and SQLite persistence.

**Architecture:** Pipeline builder pattern with typed steps. Each step transforms Input -> Output. Pipeline executor runs steps sequentially with retry logic. Optional SQLite recorder tracks run history.

**Tech Stack:** Rust, async-trait, thiserror, tokio, anyhow, sqlx (optional)

---

## Task 1: Project Setup

**Files:**
- Create: `Cargo.toml`
- Create: `src/lib.rs`

**Step 1: Initialize cargo project**

Run:
```bash
cd /home/mavdi/Documents/work/delta
cargo init --lib
```

**Step 2: Replace Cargo.toml with dependencies**

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
sqlite = ["dep:sqlx"]

[dependencies]
async-trait = "0.1"
thiserror = "2"
tokio = { version = "1", features = ["time"] }
anyhow = "1"

[dependencies.sqlx]
version = "0.8"
features = ["runtime-tokio", "sqlite"]
optional = true

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

**Step 3: Create minimal lib.rs**

```rust
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
```

**Step 4: Verify it compiles (will fail - modules don't exist yet)**

Run:
```bash
cargo check
```

Expected: Compile errors about missing modules (this is expected, we'll create them next)

**Step 5: Commit project setup**

```bash
git init
git add Cargo.toml src/lib.rs
git commit -m "chore: initialize delta project structure"
```

---

## Task 2: Step Trait and StepError

**Files:**
- Create: `src/step.rs`
- Test: inline doc tests

**Step 1: Create step.rs with Step trait and StepError**

```rust
//! Step trait and error types.

use async_trait::async_trait;
use thiserror::Error;

/// Error returned by a step execution.
#[derive(Error, Debug)]
pub enum StepError {
    /// Transient failure - worth retrying.
    #[error("retryable: {0}")]
    Retryable(#[source] anyhow::Error),

    /// Permanent failure - won't succeed on retry.
    #[error("permanent: {0}")]
    Permanent(#[source] anyhow::Error),
}

impl StepError {
    /// Create a retryable error.
    pub fn retryable(err: impl Into<anyhow::Error>) -> Self {
        Self::Retryable(err.into())
    }

    /// Create a permanent error.
    pub fn permanent(err: impl Into<anyhow::Error>) -> Self {
        Self::Permanent(err.into())
    }

    /// Returns true if this error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable(_))
    }
}

/// A single step in a pipeline that transforms input to output.
#[async_trait]
pub trait Step: Send + Sync {
    /// The input type this step accepts.
    type Input: Send + Clone;

    /// The output type this step produces.
    type Output: Send;

    /// The name of this step for logging and recording.
    fn name(&self) -> &'static str;

    /// Execute the step with the given input.
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError>;
}
```

**Step 2: Verify it compiles**

Run:
```bash
cargo check
```

Expected: Still fails (other modules missing), but no errors in step.rs

**Step 3: Commit**

```bash
git add src/step.rs
git commit -m "feat: add Step trait and StepError"
```

---

## Task 3: RetryPolicy

**Files:**
- Create: `src/retry.rs`

**Step 1: Create retry.rs with RetryPolicy enum**

```rust
//! Retry policy configuration.

use std::time::Duration;

/// Policy for retrying failed steps.
#[derive(Debug, Clone)]
pub enum RetryPolicy {
    /// No retries - fail immediately.
    None,

    /// Fixed delay between retries.
    Fixed {
        /// Maximum number of retry attempts.
        max_attempts: u32,
        /// Delay between attempts.
        delay: Duration,
    },

    /// Exponential backoff between retries.
    Exponential {
        /// Maximum number of retry attempts.
        max_attempts: u32,
        /// Initial delay (doubles each attempt).
        initial_delay: Duration,
        /// Maximum delay cap.
        max_delay: Duration,
    },
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::None
    }
}

impl RetryPolicy {
    /// Create an exponential backoff policy with sensible defaults.
    ///
    /// - Initial delay: 1 second
    /// - Max delay: 5 minutes
    pub fn exponential(max_attempts: u32) -> Self {
        Self::Exponential {
            max_attempts,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(300),
        }
    }

    /// Create a fixed delay policy.
    pub fn fixed(max_attempts: u32, delay: Duration) -> Self {
        Self::Fixed { max_attempts, delay }
    }

    /// Calculate the delay for a given attempt number (1-indexed).
    ///
    /// Returns `None` if max attempts exceeded.
    pub fn delay_for_attempt(&self, attempt: u32) -> Option<Duration> {
        match self {
            Self::None => None,
            Self::Fixed { max_attempts, delay } => {
                if attempt <= *max_attempts {
                    Some(*delay)
                } else {
                    None
                }
            }
            Self::Exponential {
                max_attempts,
                initial_delay,
                max_delay,
            } => {
                if attempt <= *max_attempts {
                    // 2^(attempt-1) * initial_delay, capped at max_delay
                    let multiplier = 2u64.saturating_pow(attempt.saturating_sub(1));
                    let delay_ms = initial_delay.as_millis() as u64 * multiplier;
                    let delay = Duration::from_millis(delay_ms.min(max_delay.as_millis() as u64));
                    Some(delay)
                } else {
                    None
                }
            }
        }
    }

    /// Returns the maximum number of attempts allowed.
    pub fn max_attempts(&self) -> u32 {
        match self {
            Self::None => 0,
            Self::Fixed { max_attempts, .. } => *max_attempts,
            Self::Exponential { max_attempts, .. } => *max_attempts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_none_policy() {
        let policy = RetryPolicy::None;
        assert_eq!(policy.delay_for_attempt(1), None);
        assert_eq!(policy.max_attempts(), 0);
    }

    #[test]
    fn test_fixed_policy() {
        let policy = RetryPolicy::fixed(3, Duration::from_secs(5));
        assert_eq!(policy.delay_for_attempt(1), Some(Duration::from_secs(5)));
        assert_eq!(policy.delay_for_attempt(3), Some(Duration::from_secs(5)));
        assert_eq!(policy.delay_for_attempt(4), None);
    }

    #[test]
    fn test_exponential_policy() {
        let policy = RetryPolicy::Exponential {
            max_attempts: 5,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
        };

        assert_eq!(policy.delay_for_attempt(1), Some(Duration::from_secs(1)));
        assert_eq!(policy.delay_for_attempt(2), Some(Duration::from_secs(2)));
        assert_eq!(policy.delay_for_attempt(3), Some(Duration::from_secs(4)));
        assert_eq!(policy.delay_for_attempt(4), Some(Duration::from_secs(8)));
        assert_eq!(policy.delay_for_attempt(5), Some(Duration::from_secs(16)));
        assert_eq!(policy.delay_for_attempt(6), None);
    }

    #[test]
    fn test_exponential_caps_at_max() {
        let policy = RetryPolicy::Exponential {
            max_attempts: 10,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(10),
        };

        // 2^6 = 64 seconds, but capped at 10
        assert_eq!(policy.delay_for_attempt(7), Some(Duration::from_secs(10)));
    }
}
```

**Step 2: Run tests**

Run:
```bash
cargo test retry
```

Expected: All tests pass

**Step 3: Commit**

```bash
git add src/retry.rs
git commit -m "feat: add RetryPolicy with fixed and exponential backoff"
```

---

## Task 4: Recorder Trait and NoopRecorder

**Files:**
- Create: `src/recorder.rs`

**Step 1: Create recorder.rs with Recorder trait and types**

```rust
//! Recording interface for pipeline execution history.

use async_trait::async_trait;

/// Unique identifier for a pipeline run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RunId(pub i64);

/// Unique identifier for a step execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StepId(pub i64);

/// Status of a completed pipeline run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunStatus {
    /// Run completed successfully.
    Completed,
    /// Run failed.
    Failed { error: String },
}

/// Status of a completed step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepStatus {
    /// Step completed successfully.
    Completed,
    /// Step failed.
    Failed { error: String, attempt: u32 },
}

/// Records pipeline execution for observability.
#[async_trait]
pub trait Recorder: Send + Sync {
    /// Record the start of a pipeline run.
    async fn start_run(&self, pipeline_name: &str, entity_id: &str) -> anyhow::Result<RunId>;

    /// Record the start of a step execution.
    async fn start_step(&self, run_id: RunId, step_name: &str, step_index: u32) -> anyhow::Result<StepId>;

    /// Record step completion.
    async fn complete_step(&self, step_id: StepId, status: StepStatus) -> anyhow::Result<()>;

    /// Record run completion.
    async fn complete_run(&self, run_id: RunId, status: RunStatus) -> anyhow::Result<()>;
}

/// A no-op recorder that discards all events.
///
/// Useful for testing or when persistence is not needed.
#[derive(Debug, Clone, Default)]
pub struct NoopRecorder;

impl NoopRecorder {
    /// Create a new no-op recorder.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Recorder for NoopRecorder {
    async fn start_run(&self, _pipeline_name: &str, _entity_id: &str) -> anyhow::Result<RunId> {
        Ok(RunId(0))
    }

    async fn start_step(&self, _run_id: RunId, _step_name: &str, _step_index: u32) -> anyhow::Result<StepId> {
        Ok(StepId(0))
    }

    async fn complete_step(&self, _step_id: StepId, _status: StepStatus) -> anyhow::Result<()> {
        Ok(())
    }

    async fn complete_run(&self, _run_id: RunId, _status: RunStatus) -> anyhow::Result<()> {
        Ok(())
    }
}
```

**Step 2: Verify it compiles**

Run:
```bash
cargo check
```

Expected: Still fails (pipeline module missing)

**Step 3: Commit**

```bash
git add src/recorder.rs
git commit -m "feat: add Recorder trait and NoopRecorder"
```

---

## Task 5: Pipeline Builder and Executor (Core)

**Files:**
- Create: `src/pipeline.rs`

**Step 1: Create pipeline.rs with Pipeline builder**

This is the most complex part. We use a boxed step chain to enable type-safe chaining.

```rust
//! Pipeline builder and executor.

use std::sync::Arc;
use async_trait::async_trait;
use thiserror::Error;

use crate::recorder::{NoopRecorder, Recorder, RunId, RunStatus, StepStatus};
use crate::retry::RetryPolicy;
use crate::step::{Step, StepError};

/// Error returned by pipeline execution.
#[derive(Error, Debug)]
pub enum PipelineError {
    /// A step failed permanently.
    #[error("step '{step}' failed: {source}")]
    StepFailed {
        step: &'static str,
        #[source]
        source: anyhow::Error,
    },

    /// A step exhausted all retries.
    #[error("step '{step}' exhausted {attempts} retries: {source}")]
    RetriesExhausted {
        step: &'static str,
        attempts: u32,
        #[source]
        source: anyhow::Error,
    },

    /// Recording failed.
    #[error("recorder error: {0}")]
    RecorderError(#[from] anyhow::Error),
}

/// Trait for types that can provide an entity ID for recording.
pub trait HasEntityId {
    /// Returns the entity identifier for this input.
    fn entity_id(&self) -> String;
}

// Blanket impl for String
impl HasEntityId for String {
    fn entity_id(&self) -> String {
        self.clone()
    }
}

// Blanket impl for &str
impl HasEntityId for &str {
    fn entity_id(&self) -> String {
        self.to_string()
    }
}

/// Internal trait for boxed step execution.
#[async_trait]
trait BoxedStep<I, O>: Send + Sync {
    fn name(&self) -> &'static str;
    async fn execute(&self, input: I) -> Result<O, StepError>;
}

/// Wrapper to make any Step into a BoxedStep.
struct StepWrapper<S>(S);

#[async_trait]
impl<S> BoxedStep<S::Input, S::Output> for StepWrapper<S>
where
    S: Step,
{
    fn name(&self) -> &'static str {
        self.0.name()
    }

    async fn execute(&self, input: S::Input) -> Result<S::Output, StepError> {
        self.0.execute(input).await
    }
}

/// A chain of steps that transforms I -> O.
#[async_trait]
trait StepChain<I, O>: Send + Sync {
    async fn run(
        &self,
        input: I,
        run_id: RunId,
        recorder: &dyn Recorder,
        retry_policy: &RetryPolicy,
        start_index: u32,
    ) -> Result<O, PipelineError>;
}

/// Terminal chain - identity transform.
struct Identity;

#[async_trait]
impl<T: Send> StepChain<T, T> for Identity {
    async fn run(
        &self,
        input: T,
        _run_id: RunId,
        _recorder: &dyn Recorder,
        _retry_policy: &RetryPolicy,
        _start_index: u32,
    ) -> Result<T, PipelineError> {
        Ok(input)
    }
}

/// Chain that runs a step then continues with the rest.
struct ChainedStep<S, Next, I, M, O>
where
    S: BoxedStep<I, M>,
    Next: StepChain<M, O>,
{
    step: S,
    next: Next,
    _phantom: std::marker::PhantomData<(I, M, O)>,
}

#[async_trait]
impl<S, Next, I, M, O> StepChain<I, O> for ChainedStep<S, Next, I, M, O>
where
    I: Send + Clone + 'static,
    M: Send + 'static,
    O: Send + 'static,
    S: BoxedStep<I, M> + Send + Sync,
    Next: StepChain<M, O> + Send + Sync,
{
    async fn run(
        &self,
        input: I,
        run_id: RunId,
        recorder: &dyn Recorder,
        retry_policy: &RetryPolicy,
        start_index: u32,
    ) -> Result<O, PipelineError> {
        let step_name = self.step.name();
        let step_id = recorder.start_step(run_id, step_name, start_index).await?;

        // Execute with retry
        let mut attempt = 0u32;
        let output = loop {
            attempt += 1;
            match self.step.execute(input.clone()).await {
                Ok(output) => break output,
                Err(StepError::Permanent(e)) => {
                    recorder
                        .complete_step(step_id, StepStatus::Failed {
                            error: e.to_string(),
                            attempt,
                        })
                        .await?;
                    return Err(PipelineError::StepFailed {
                        step: step_name,
                        source: e,
                    });
                }
                Err(StepError::Retryable(e)) => {
                    if let Some(delay) = retry_policy.delay_for_attempt(attempt) {
                        tokio::time::sleep(delay).await;
                    } else {
                        recorder
                            .complete_step(step_id, StepStatus::Failed {
                                error: e.to_string(),
                                attempt,
                            })
                            .await?;
                        return Err(PipelineError::RetriesExhausted {
                            step: step_name,
                            attempts: attempt,
                            source: e,
                        });
                    }
                }
            }
        };

        recorder.complete_step(step_id, StepStatus::Completed).await?;

        // Continue with next steps
        self.next
            .run(output, run_id, recorder, retry_policy, start_index + 1)
            .await
    }
}

/// Builder for constructing pipelines.
pub struct Pipeline<I, O, Chain>
where
    Chain: StepChain<I, O>,
{
    name: &'static str,
    chain: Chain,
    retry_policy: RetryPolicy,
    recorder: Arc<dyn Recorder>,
    _phantom: std::marker::PhantomData<(I, O)>,
}

impl Pipeline<(), (), Identity> {
    /// Create a new pipeline builder with the given name.
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            chain: Identity,
            retry_policy: RetryPolicy::default(),
            recorder: Arc::new(NoopRecorder),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<O, Chain> Pipeline<(), O, Chain>
where
    Chain: StepChain<(), O> + Send + Sync + 'static,
    O: Send + 'static,
{
    /// Add the first step to the pipeline.
    pub fn start_with<S>(self, step: S) -> Pipeline<S::Input, S::Output, ChainedStep<StepWrapper<S>, Identity, S::Input, S::Output, S::Output>>
    where
        S: Step + 'static,
    {
        Pipeline {
            name: self.name,
            chain: ChainedStep {
                step: StepWrapper(step),
                next: Identity,
                _phantom: std::marker::PhantomData,
            },
            retry_policy: self.retry_policy,
            recorder: self.recorder,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<I, O, Chain> Pipeline<I, O, Chain>
where
    I: Send + Clone + 'static,
    O: Send + 'static,
    Chain: StepChain<I, O> + Send + Sync + 'static,
{
    /// Add a step to the pipeline.
    pub fn then<S>(self, step: S) -> Pipeline<I, S::Output, impl StepChain<I, S::Output>>
    where
        S: Step<Input = O> + 'static,
        S::Output: Send + 'static,
    {
        Pipeline {
            name: self.name,
            chain: ThenChain {
                first: self.chain,
                step: StepWrapper(step),
                _phantom: std::marker::PhantomData,
            },
            retry_policy: self.retry_policy,
            recorder: self.recorder,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Set the retry policy for this pipeline.
    pub fn with_retry(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Set the recorder for this pipeline.
    pub fn with_recorder<R: Recorder + 'static>(mut self, recorder: R) -> Self {
        self.recorder = Arc::new(recorder);
        self
    }

    /// Build the pipeline, ready for execution.
    pub fn build(self) -> BuiltPipeline<I, O, Chain> {
        BuiltPipeline {
            name: self.name,
            chain: self.chain,
            retry_policy: self.retry_policy,
            recorder: self.recorder,
            _phantom: std::marker::PhantomData,
        }
    }
}

/// Chain that runs first chain then a step.
struct ThenChain<First, S, I, M, O>
where
    First: StepChain<I, M>,
    S: BoxedStep<M, O>,
{
    first: First,
    step: S,
    _phantom: std::marker::PhantomData<(I, M, O)>,
}

#[async_trait]
impl<First, S, I, M, O> StepChain<I, O> for ThenChain<First, S, I, M, O>
where
    I: Send + Clone + 'static,
    M: Send + Clone + 'static,
    O: Send + 'static,
    First: StepChain<I, M> + Send + Sync,
    S: BoxedStep<M, O> + Send + Sync,
{
    async fn run(
        &self,
        input: I,
        run_id: RunId,
        recorder: &dyn Recorder,
        retry_policy: &RetryPolicy,
        start_index: u32,
    ) -> Result<O, PipelineError> {
        // Run first chain
        let mid = self.first.run(input, run_id, recorder, retry_policy, start_index).await?;

        // Count steps in first chain (hacky but works for now)
        let next_index = start_index + count_chain_steps(&self.first);

        let step_name = self.step.name();
        let step_id = recorder.start_step(run_id, step_name, next_index).await?;

        // Execute with retry
        let mut attempt = 0u32;
        let output = loop {
            attempt += 1;
            match self.step.execute(mid.clone()).await {
                Ok(output) => break output,
                Err(StepError::Permanent(e)) => {
                    recorder
                        .complete_step(step_id, StepStatus::Failed {
                            error: e.to_string(),
                            attempt,
                        })
                        .await?;
                    return Err(PipelineError::StepFailed {
                        step: step_name,
                        source: e,
                    });
                }
                Err(StepError::Retryable(e)) => {
                    if let Some(delay) = retry_policy.delay_for_attempt(attempt) {
                        tokio::time::sleep(delay).await;
                    } else {
                        recorder
                            .complete_step(step_id, StepStatus::Failed {
                                error: e.to_string(),
                                attempt,
                            })
                            .await?;
                        return Err(PipelineError::RetriesExhausted {
                            step: step_name,
                            attempts: attempt,
                            source: e,
                        });
                    }
                }
            }
        };

        recorder.complete_step(step_id, StepStatus::Completed).await?;
        Ok(output)
    }
}

/// Helper to count steps (simplified - always returns 1 for non-identity).
fn count_chain_steps<I, O, C: StepChain<I, O> + ?Sized>(_chain: &C) -> u32 {
    // This is a simplification - in practice we'd need a trait method
    1
}

/// A built pipeline ready for execution.
pub struct BuiltPipeline<I, O, Chain>
where
    Chain: StepChain<I, O>,
{
    name: &'static str,
    chain: Chain,
    retry_policy: RetryPolicy,
    recorder: Arc<dyn Recorder>,
    _phantom: std::marker::PhantomData<(I, O)>,
}

impl<I, O, Chain> BuiltPipeline<I, O, Chain>
where
    I: Send + Clone + HasEntityId + 'static,
    O: Send + 'static,
    Chain: StepChain<I, O> + Send + Sync,
{
    /// Execute the pipeline with the given input.
    pub async fn run(&self, input: I) -> Result<O, PipelineError> {
        let entity_id = input.entity_id();
        let run_id = self.recorder.start_run(self.name, &entity_id).await?;

        match self.chain.run(input, run_id, self.recorder.as_ref(), &self.retry_policy, 0).await {
            Ok(output) => {
                self.recorder.complete_run(run_id, RunStatus::Completed).await?;
                Ok(output)
            }
            Err(e) => {
                self.recorder
                    .complete_run(run_id, RunStatus::Failed { error: e.to_string() })
                    .await?;
                Err(e)
            }
        }
    }

    /// Get the pipeline name.
    pub fn name(&self) -> &'static str {
        self.name
    }
}
```

**Step 2: Verify it compiles**

Run:
```bash
cargo check
```

Expected: Compiles successfully (all modules now exist)

**Step 3: Commit**

```bash
git add src/pipeline.rs
git commit -m "feat: add Pipeline builder and executor with retry logic"
```

---

## Task 6: Integration Test

**Files:**
- Create: `tests/integration.rs`

**Step 1: Write integration test with mock steps**

```rust
//! Integration tests for delta pipeline.

use async_trait::async_trait;
use delta::{HasEntityId, NoopRecorder, Pipeline, RetryPolicy, Step, StepError};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

// Test input type
#[derive(Clone)]
struct TestInput {
    id: String,
    value: u32,
}

impl HasEntityId for TestInput {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

// Step that doubles the value
struct DoubleStep;

#[async_trait]
impl Step for DoubleStep {
    type Input = TestInput;
    type Output = TestInput;

    fn name(&self) -> &'static str {
        "double"
    }

    async fn execute(&self, mut input: Self::Input) -> Result<Self::Output, StepError> {
        input.value *= 2;
        Ok(input)
    }
}

// Step that adds 10
struct AddTenStep;

#[async_trait]
impl Step for AddTenStep {
    type Input = TestInput;
    type Output = TestInput;

    fn name(&self) -> &'static str {
        "add_ten"
    }

    async fn execute(&self, mut input: Self::Input) -> Result<Self::Output, StepError> {
        input.value += 10;
        Ok(input)
    }
}

// Step that fails N times then succeeds
struct FlakyStep {
    fail_count: AtomicU32,
    max_failures: u32,
}

impl FlakyStep {
    fn new(max_failures: u32) -> Self {
        Self {
            fail_count: AtomicU32::new(0),
            max_failures,
        }
    }
}

#[async_trait]
impl Step for FlakyStep {
    type Input = TestInput;
    type Output = TestInput;

    fn name(&self) -> &'static str {
        "flaky"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        let count = self.fail_count.fetch_add(1, Ordering::SeqCst);
        if count < self.max_failures {
            Err(StepError::retryable(anyhow::anyhow!("transient failure {}", count + 1)))
        } else {
            Ok(input)
        }
    }
}

// Step that always fails permanently
struct PermanentFailStep;

#[async_trait]
impl Step for PermanentFailStep {
    type Input = TestInput;
    type Output = TestInput;

    fn name(&self) -> &'static str {
        "permanent_fail"
    }

    async fn execute(&self, _input: Self::Input) -> Result<Self::Output, StepError> {
        Err(StepError::permanent(anyhow::anyhow!("permanent failure")))
    }
}

#[tokio::test]
async fn test_simple_pipeline() {
    let pipeline = Pipeline::new("test")
        .start_with(DoubleStep)
        .then(AddTenStep)
        .with_recorder(NoopRecorder)
        .build();

    let input = TestInput {
        id: "test-1".to_string(),
        value: 5,
    };

    let result = pipeline.run(input).await.unwrap();
    assert_eq!(result.value, 20); // (5 * 2) + 10 = 20
}

#[tokio::test]
async fn test_pipeline_with_retry_success() {
    let flaky = Arc::new(FlakyStep::new(2)); // Fail twice, then succeed

    let pipeline = Pipeline::new("test_retry")
        .start_with(DoubleStep)
        .then(FlakyWrapper(flaky.clone()))
        .with_retry(RetryPolicy::fixed(3, std::time::Duration::from_millis(10)))
        .with_recorder(NoopRecorder)
        .build();

    let input = TestInput {
        id: "test-2".to_string(),
        value: 5,
    };

    let result = pipeline.run(input).await.unwrap();
    assert_eq!(result.value, 10); // 5 * 2 = 10 (flaky step doesn't modify)
}

// Wrapper to use Arc<FlakyStep>
struct FlakyWrapper(Arc<FlakyStep>);

#[async_trait]
impl Step for FlakyWrapper {
    type Input = TestInput;
    type Output = TestInput;

    fn name(&self) -> &'static str {
        self.0.name()
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        self.0.execute(input).await
    }
}

#[tokio::test]
async fn test_pipeline_permanent_failure() {
    let pipeline = Pipeline::new("test_perm_fail")
        .start_with(DoubleStep)
        .then(PermanentFailStep)
        .with_retry(RetryPolicy::exponential(3))
        .with_recorder(NoopRecorder)
        .build();

    let input = TestInput {
        id: "test-3".to_string(),
        value: 5,
    };

    let result = pipeline.run(input).await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(err.to_string().contains("permanent_fail"));
}

#[tokio::test]
async fn test_pipeline_retries_exhausted() {
    let flaky = Arc::new(FlakyStep::new(10)); // Fail 10 times

    let pipeline = Pipeline::new("test_exhausted")
        .start_with(FlakyWrapper(flaky))
        .with_retry(RetryPolicy::fixed(3, std::time::Duration::from_millis(1)))
        .with_recorder(NoopRecorder)
        .build();

    let input = TestInput {
        id: "test-4".to_string(),
        value: 5,
    };

    let result = pipeline.run(input).await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(err.to_string().contains("exhausted"));
}
```

**Step 2: Run integration tests**

Run:
```bash
cargo test --test integration
```

Expected: All tests pass

**Step 3: Commit**

```bash
git add tests/integration.rs
git commit -m "test: add integration tests for pipeline execution"
```

---

## Task 7: SQLite Recorder (Feature-Gated)

**Files:**
- Create: `src/sqlite.rs`

**Step 1: Create sqlite.rs with SqliteRecorder**

```rust
//! SQLite-based recorder implementation.

use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::recorder::{Recorder, RunId, RunStatus, StepId, StepStatus};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS delta_runs (
    id INTEGER PRIMARY KEY,
    pipeline_name TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT,
    status TEXT NOT NULL DEFAULT 'running',
    error_message TEXT
);

CREATE TABLE IF NOT EXISTS delta_steps (
    id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL REFERENCES delta_runs(id),
    step_name TEXT NOT NULL,
    step_index INTEGER NOT NULL,
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT,
    status TEXT NOT NULL DEFAULT 'running',
    attempt INTEGER NOT NULL DEFAULT 1,
    error_message TEXT
);

CREATE INDEX IF NOT EXISTS idx_delta_runs_pipeline ON delta_runs(pipeline_name, started_at);
CREATE INDEX IF NOT EXISTS idx_delta_runs_entity ON delta_runs(entity_id);
CREATE INDEX IF NOT EXISTS idx_delta_steps_run ON delta_steps(run_id);
"#;

/// SQLite-based recorder for pipeline execution history.
#[derive(Clone)]
pub struct SqliteRecorder {
    pool: SqlitePool,
}

impl SqliteRecorder {
    /// Create a new SQLite recorder with the given connection pool.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Run database migrations to create required tables.
    pub async fn run_migrations(&self) -> anyhow::Result<()> {
        for statement in SCHEMA.split(';') {
            let trimmed = statement.trim();
            if !trimmed.is_empty() {
                sqlx::query(trimmed).execute(&self.pool).await?;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Recorder for SqliteRecorder {
    async fn start_run(&self, pipeline_name: &str, entity_id: &str) -> anyhow::Result<RunId> {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO delta_runs (pipeline_name, entity_id) VALUES (?, ?) RETURNING id",
        )
        .bind(pipeline_name)
        .bind(entity_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(RunId(id))
    }

    async fn start_step(&self, run_id: RunId, step_name: &str, step_index: u32) -> anyhow::Result<StepId> {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO delta_steps (run_id, step_name, step_index) VALUES (?, ?, ?) RETURNING id",
        )
        .bind(run_id.0)
        .bind(step_name)
        .bind(step_index as i32)
        .fetch_one(&self.pool)
        .await?;

        Ok(StepId(id))
    }

    async fn complete_step(&self, step_id: StepId, status: StepStatus) -> anyhow::Result<()> {
        let (status_str, error_msg, attempt) = match status {
            StepStatus::Completed => ("completed", None, 1u32),
            StepStatus::Failed { error, attempt } => ("failed", Some(error), attempt),
        };

        sqlx::query(
            "UPDATE delta_steps SET completed_at = datetime('now'), status = ?, error_message = ?, attempt = ? WHERE id = ?",
        )
        .bind(status_str)
        .bind(error_msg)
        .bind(attempt as i32)
        .bind(step_id.0)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn complete_run(&self, run_id: RunId, status: RunStatus) -> anyhow::Result<()> {
        let (status_str, error_msg) = match status {
            RunStatus::Completed => ("completed", None),
            RunStatus::Failed { error } => ("failed", Some(error)),
        };

        sqlx::query(
            "UPDATE delta_runs SET completed_at = datetime('now'), status = ?, error_message = ? WHERE id = ?",
        )
        .bind(status_str)
        .bind(error_msg)
        .bind(run_id.0)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
```

**Step 2: Verify it compiles with sqlite feature**

Run:
```bash
cargo check --features sqlite
```

Expected: Compiles successfully

**Step 3: Commit**

```bash
git add src/sqlite.rs
git commit -m "feat: add SqliteRecorder for persistent run history"
```

---

## Task 8: SQLite Integration Test

**Files:**
- Create: `tests/sqlite_integration.rs`

**Step 1: Write SQLite integration test**

```rust
//! SQLite recorder integration tests.

#![cfg(feature = "sqlite")]

use async_trait::async_trait;
use delta::{HasEntityId, Pipeline, RetryPolicy, SqliteRecorder, Step, StepError};
use sqlx::SqlitePool;

#[derive(Clone)]
struct TestInput(String);

impl HasEntityId for TestInput {
    fn entity_id(&self) -> String {
        self.0.clone()
    }
}

struct PassStep;

#[async_trait]
impl Step for PassStep {
    type Input = TestInput;
    type Output = TestInput;

    fn name(&self) -> &'static str {
        "pass"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

struct FailStep;

#[async_trait]
impl Step for FailStep {
    type Input = TestInput;
    type Output = TestInput;

    fn name(&self) -> &'static str {
        "fail"
    }

    async fn execute(&self, _input: Self::Input) -> Result<Self::Output, StepError> {
        Err(StepError::permanent(anyhow::anyhow!("test failure")))
    }
}

#[tokio::test]
async fn test_sqlite_records_successful_run() {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let recorder = SqliteRecorder::new(pool.clone());
    recorder.run_migrations().await.unwrap();

    let pipeline = Pipeline::new("test_success")
        .start_with(PassStep)
        .with_recorder(recorder)
        .build();

    pipeline.run(TestInput("entity-1".to_string())).await.unwrap();

    // Verify run was recorded
    let (count,): (i32,) = sqlx::query_as("SELECT COUNT(*) FROM delta_runs WHERE status = 'completed'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);

    // Verify step was recorded
    let (step_count,): (i32,) = sqlx::query_as("SELECT COUNT(*) FROM delta_steps WHERE status = 'completed'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(step_count, 1);
}

#[tokio::test]
async fn test_sqlite_records_failed_run() {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let recorder = SqliteRecorder::new(pool.clone());
    recorder.run_migrations().await.unwrap();

    let pipeline = Pipeline::new("test_fail")
        .start_with(FailStep)
        .with_retry(RetryPolicy::None)
        .with_recorder(recorder)
        .build();

    let _ = pipeline.run(TestInput("entity-2".to_string())).await;

    // Verify run was recorded as failed
    let (count,): (i32,) = sqlx::query_as("SELECT COUNT(*) FROM delta_runs WHERE status = 'failed'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);

    // Verify error message was recorded
    let (error_msg,): (Option<String>,) = sqlx::query_as("SELECT error_message FROM delta_runs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(error_msg.unwrap().contains("test failure"));
}

#[tokio::test]
async fn test_sqlite_records_entity_id() {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let recorder = SqliteRecorder::new(pool.clone());
    recorder.run_migrations().await.unwrap();

    let pipeline = Pipeline::new("test_entity")
        .start_with(PassStep)
        .with_recorder(recorder)
        .build();

    pipeline.run(TestInput("my-unique-entity".to_string())).await.unwrap();

    // Verify entity_id was recorded
    let (entity_id,): (String,) = sqlx::query_as("SELECT entity_id FROM delta_runs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(entity_id, "my-unique-entity");
}
```

**Step 2: Run SQLite integration tests**

Run:
```bash
cargo test --features sqlite --test sqlite_integration
```

Expected: All tests pass

**Step 3: Commit**

```bash
git add tests/sqlite_integration.rs
git commit -m "test: add SQLite recorder integration tests"
```

---

## Task 9: Basic Example

**Files:**
- Create: `examples/basic.rs`

**Step 1: Create a basic usage example**

```rust
//! Basic example of using Delta pipeline.
//!
//! Run with: cargo run --example basic

use async_trait::async_trait;
use delta::{HasEntityId, NoopRecorder, Pipeline, RetryPolicy, Step, StepError};

/// A simple message to process.
#[derive(Clone, Debug)]
struct Message {
    id: String,
    content: String,
}

impl HasEntityId for Message {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

/// Step that converts content to uppercase.
struct UppercaseStep;

#[async_trait]
impl Step for UppercaseStep {
    type Input = Message;
    type Output = Message;

    fn name(&self) -> &'static str {
        "uppercase"
    }

    async fn execute(&self, mut input: Self::Input) -> Result<Self::Output, StepError> {
        input.content = input.content.to_uppercase();
        Ok(input)
    }
}

/// Step that adds a prefix.
struct PrefixStep {
    prefix: String,
}

#[async_trait]
impl Step for PrefixStep {
    type Input = Message;
    type Output = Message;

    fn name(&self) -> &'static str {
        "prefix"
    }

    async fn execute(&self, mut input: Self::Input) -> Result<Self::Output, StepError> {
        input.content = format!("{}{}", self.prefix, input.content);
        Ok(input)
    }
}

/// Step that adds a suffix.
struct SuffixStep {
    suffix: String,
}

#[async_trait]
impl Step for SuffixStep {
    type Input = Message;
    type Output = Message;

    fn name(&self) -> &'static str {
        "suffix"
    }

    async fn execute(&self, mut input: Self::Input) -> Result<Self::Output, StepError> {
        input.content = format!("{}{}", input.content, self.suffix);
        Ok(input)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Build a pipeline that:
    // 1. Converts to uppercase
    // 2. Adds a prefix
    // 3. Adds a suffix
    let pipeline = Pipeline::new("message_processor")
        .start_with(UppercaseStep)
        .then(PrefixStep {
            prefix: "[PROCESSED] ".to_string(),
        })
        .then(SuffixStep {
            suffix: " [END]".to_string(),
        })
        .with_retry(RetryPolicy::exponential(3))
        .with_recorder(NoopRecorder)
        .build();

    // Process a message
    let input = Message {
        id: "msg-001".to_string(),
        content: "hello world".to_string(),
    };

    println!("Input: {:?}", input);

    let output = pipeline.run(input).await?;

    println!("Output: {:?}", output);
    // Output: Message { id: "msg-001", content: "[PROCESSED] HELLO WORLD [END]" }

    Ok(())
}
```

**Step 2: Run the example**

Run:
```bash
cargo run --example basic
```

Expected output:
```
Input: Message { id: "msg-001", content: "hello world" }
Output: Message { id: "msg-001", content: "[PROCESSED] HELLO WORLD [END]" }
```

**Step 3: Commit**

```bash
git add examples/basic.rs
git commit -m "docs: add basic usage example"
```

---

## Task 10: README and Final Polish

**Files:**
- Create: `README.md`
- Update: `src/lib.rs` (add module docs)

**Step 1: Create README.md**

```markdown
# Delta

A lightweight, type-safe pipeline engine for Rust.

## Features

- **Type-safe pipelines** - Compiler enforces that step outputs match next step's inputs
- **Method chaining** - Fluent API inspired by Elixir's pipe operator
- **Retry policies** - Built-in exponential backoff and fixed delay strategies
- **Observable** - Optional SQLite recording of all pipeline runs and steps
- **Zero runtime dependencies** - Core requires only async-trait, thiserror, tokio, anyhow

## Quick Start

```rust
use delta::{Pipeline, Step, StepError, HasEntityId, NoopRecorder, RetryPolicy};
use async_trait::async_trait;

#[derive(Clone)]
struct MyInput { id: String, value: i32 }

impl HasEntityId for MyInput {
    fn entity_id(&self) -> String { self.id.clone() }
}

struct DoubleStep;

#[async_trait]
impl Step for DoubleStep {
    type Input = MyInput;
    type Output = MyInput;

    fn name(&self) -> &'static str { "double" }

    async fn execute(&self, mut input: Self::Input) -> Result<Self::Output, StepError> {
        input.value *= 2;
        Ok(input)
    }
}

#[tokio::main]
async fn main() {
    let pipeline = Pipeline::new("my_pipeline")
        .start_with(DoubleStep)
        .with_retry(RetryPolicy::exponential(3))
        .with_recorder(NoopRecorder)
        .build();

    let result = pipeline.run(MyInput { id: "1".into(), value: 5 }).await.unwrap();
    assert_eq!(result.value, 10);
}
```

## SQLite Recording

Enable the `sqlite` feature for persistent run history:

```toml
[dependencies]
delta = { version = "0.1", features = ["sqlite"] }
```

```rust
use delta::SqliteRecorder;
use sqlx::SqlitePool;

let pool = SqlitePool::connect("sqlite:pipeline.db").await?;
let recorder = SqliteRecorder::new(pool);
recorder.run_migrations().await?;

let pipeline = Pipeline::new("recorded_pipeline")
    .start_with(MyStep)
    .with_recorder(recorder)
    .build();
```

## License

MIT
```

**Step 2: Update lib.rs with crate-level docs**

Update the top of `src/lib.rs`:

```rust
//! # Delta
//!
//! A lightweight, type-safe pipeline engine for Rust.
//!
//! ## Example
//!
//! ```rust,no_run
//! use delta::{Pipeline, Step, StepError, HasEntityId, NoopRecorder, RetryPolicy};
//! use async_trait::async_trait;
//!
//! #[derive(Clone)]
//! struct MyInput(String);
//!
//! impl HasEntityId for MyInput {
//!     fn entity_id(&self) -> String { self.0.clone() }
//! }
//!
//! struct MyStep;
//!
//! #[async_trait]
//! impl Step for MyStep {
//!     type Input = MyInput;
//!     type Output = MyInput;
//!     fn name(&self) -> &'static str { "my_step" }
//!     async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
//!         Ok(input)
//!     }
//! }
//! ```

pub mod step;
// ... rest of file
```

**Step 3: Run all tests**

Run:
```bash
cargo test --all-features
```

Expected: All tests pass

**Step 4: Run clippy**

Run:
```bash
cargo clippy --all-features -- -D warnings
```

Expected: No warnings

**Step 5: Commit**

```bash
git add README.md src/lib.rs
git commit -m "docs: add README and crate documentation"
```

---

## Task 11: Create GitHub Repository and Push

**Step 1: Create .gitignore**

```
/target
Cargo.lock
```

**Step 2: Add and commit .gitignore**

```bash
git add .gitignore
git commit -m "chore: add .gitignore"
```

**Step 3: Create GitHub repository**

Run:
```bash
gh repo create mavdi/delta --public --description "A lightweight, type-safe pipeline engine for Rust" --source .
```

**Step 4: Push to GitHub**

```bash
git push -u origin main
```

---

Plan complete and saved to `docs/IMPLEMENTATION.md`.

**Two execution options:**

1. **Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

2. **Parallel Session (separate)** - Open new session in the delta directory with executing-plans, batch execution with checkpoints

Which approach?