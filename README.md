# Deltaflow

The embeddable workflow engine.

Type-safe, Elixir-inspired pipelines that run in your process. No infrastructure required.

## Why Deltaflow?

- **Type-safe composition** - Compiler enforces step output matches next step's input
- **Elixir-inspired** - Declarative pipelines via method chaining, not scattered callbacks
- **Observable by default** - Every run and step recorded for debugging
- **Embeddable** - A library, not a service. Runs in your process.

## Quick Start

```rust
use deltaflow::{Pipeline, Step, StepError, RetryPolicy, NoopRecorder};

// Define steps implementing the Step trait
struct ParseInput;
struct ProcessData;
struct FormatOutput;

// Build a type-safe pipeline
let pipeline = Pipeline::new("my_workflow")
    .start_with(ParseInput)       // String -> ParsedData
    .then(ProcessData)            // ParsedData -> ProcessedData
    .then(FormatOutput)           // ProcessedData -> Output
    .with_retry(RetryPolicy::exponential(3))
    .with_recorder(NoopRecorder)
    .build();

// Run it
let result = pipeline.run(input).await?;
```

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
deltaflow = "0.2"
```

For SQLite-backed recording and task queue:

```toml
[dependencies]
deltaflow = { version = "0.2", features = ["sqlite"] }
```

## Core Concepts

### Step

The fundamental building block. Each step transforms a typed input to a typed output:

```rust
#[async_trait]
pub trait Step: Send + Sync {
    type Input: Send + Clone;
    type Output: Send;

    fn name(&self) -> &'static str;
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError>;
}
```

### Pipeline

Compose steps with method chaining. The compiler ensures each step's output type matches the next step's input:

```rust
let pipeline = Pipeline::new("process_order")
    .start_with(ValidateOrder)    // Order -> ValidatedOrder
    .then(ChargePayment)          // ValidatedOrder -> PaidOrder
    .then(FulfillOrder)           // PaidOrder -> CompletedOrder
    .with_retry(RetryPolicy::exponential(5))
    .with_recorder(SqliteRecorder::new(pool))
    .build();
```

### Runner

Background task processing with the `sqlite` feature. Register pipelines, submit work, process concurrently:

```rust
let runner = Runner::new(SqliteTaskStore::new(pool))
    .pipeline(order_pipeline)
    .pipeline(notification_pipeline)
    .max_concurrent(4)
    .build();

// Submit work
runner.submit("process_order", order).await?;

// Process tasks
runner.run().await;
```

## Periodic Scheduler

For time-based task enqueueing, use `PeriodicScheduler`:

```rust
use deltaflow::{SchedulerBuilder, SqliteTaskStore};
use std::time::Duration;

let scheduler = SchedulerBuilder::new(task_store)
    .job("process_video", Duration::from_secs(900), {
        let repo = repo.clone();
        move || {
            let repo = repo.clone();
            async move { repo.get_pending_videos().await.unwrap_or_default() }
        }
    })
    .run_on_start(true)

    .job("validate_signals", Duration::from_secs(3600), {
        let repo = repo.clone();
        move || {
            let repo = repo.clone();
            async move { repo.get_pending_signals().await.unwrap_or_default() }
        }
    })
    .run_on_start(false)

    .build();

// Run alongside your pipeline runner
tokio::select! {
    _ = runner.run() => {}
    _ = scheduler.run() => {}
}
```

## Per-Pipeline Concurrency

For pipelines that need rate limiting (e.g., external API calls):

```rust
let runner = RunnerBuilder::new(task_store)
    .pipeline(fast_pipeline)
    .pipeline_with_concurrency(rate_limited_pipeline, 1)  // Only 1 concurrent
    .build();
```

Pipelines with custom concurrency use their own semaphore instead of the global `max_concurrent` limit.

## Status

Deltaflow is **experimental** (0.1.x). The API will evolve based on feedback.

**What works:**
- Pipeline composition with type-safe step chaining
- Retry policies (exponential backoff, fixed delay)
- SQLite recording for observability
- Task runner with concurrent execution
- Follow-up task spawning

**What's coming:**
- Per-step retry policies
- Task priorities
- More storage backends

**Not planned (by design):**
- Distributed execution (single-process by design)
- DAG dependencies (pipelines are linear)

Feedback welcome: [GitHub Issues](https://github.com/mavdi/deltaflow/issues)

## License

MIT
