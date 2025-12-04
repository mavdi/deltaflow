# Delta

A lightweight, type-safe pipeline engine for Rust.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

## Overview

Delta brings declarative, composable pipelines to Rust with compile-time type safety. Inspired by Elixir's pipe operator, it lets you build observable workflows where each step transforms typed inputs to outputs.

**Why Delta?**
- **Explicit pipelines** - Declare workflows via method chaining, not implicit code paths
- **Type safety** - The compiler enforces that each step's output matches the next step's input
- **Observability** - Every run and step can be recorded to SQLite for debugging and analysis
- **Lightweight** - Zero runtime dependencies beyond tokio for async and optional SQLite for persistence
- **Retryable** - Built-in exponential backoff and fixed retry policies with retryable vs. permanent error distinction

## Features

- Type-safe pipeline composition with `.start_with()` and `.then()` method chaining
- Async/await support via the `Step` trait
- Configurable retry policies (exponential backoff, fixed delay, or none)
- Pluggable recording layer for observability
- SQLite recorder with prefixed tables (optional feature)
- Clear distinction between retryable and permanent errors
- Entity ID tracking for correlating pipeline runs with domain entities

## Installation

Add Delta to your `Cargo.toml`:

```toml
[dependencies]
delta = "0.1"
```

To enable SQLite recording, use the `sqlite` feature:

```toml
[dependencies]
delta = { version = "0.1", features = ["sqlite"] }
```

## Quick Start

Here's a simple pipeline that parses a string, doubles it, and formats the result:

```rust
use async_trait::async_trait;
use delta::{HasEntityId, NoopRecorder, Pipeline, RetryPolicy, Step, StepError};

#[derive(Clone)]
struct Input {
    id: String,
    value: String,
}

impl HasEntityId for Input {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

#[derive(Clone)]
struct Number {
    id: String,
    value: i32,
}

impl HasEntityId for Number {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

struct ParseStep;

#[async_trait]
impl Step for ParseStep {
    type Input = Input;
    type Output = Number;

    fn name(&self) -> &'static str {
        "parse"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        match input.value.parse::<i32>() {
            Ok(value) => Ok(Number { id: input.id, value }),
            // Permanent error - retrying won't help
            Err(e) => Err(StepError::permanent(anyhow::anyhow!("Parse failed: {}", e))),
        }
    }
}

struct DoubleStep;

#[async_trait]
impl Step for DoubleStep {
    type Input = Number;
    type Output = Number;

    fn name(&self) -> &'static str {
        "double"
    }

    async fn execute(&self, mut input: Self::Input) -> Result<Self::Output, StepError> {
        input.value *= 2;
        Ok(input)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Build the pipeline
    let pipeline = Pipeline::new("example")
        .start_with(ParseStep)
        .then(DoubleStep)
        .with_retry(RetryPolicy::exponential(3))
        .with_recorder(NoopRecorder)
        .build();

    // Run it
    let input = Input {
        id: "task-123".to_string(),
        value: "42".to_string(),
    };

    let result = pipeline.run(input).await?;
    println!("Result: {}", result.value); // 84

    Ok(())
}
```

## Core Concepts

### Step

The fundamental building block. Each step defines:
- **Input type** - What data it accepts (must be `Clone` for retry support)
- **Output type** - What data it produces
- **Name** - Static string for logging and recording
- **Execute logic** - Async transformation from input to output

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

A type-safe builder for composing steps. The key insight: `.then()` only accepts steps whose `Input` type matches the previous step's `Output` type. This is enforced at compile time.

```rust
let pipeline = Pipeline::new("my_pipeline")
    .start_with(StepA)      // produces TypeX
    .then(StepB)            // accepts TypeX, produces TypeY
    .then(StepC)            // accepts TypeY, produces TypeZ
    .with_retry(RetryPolicy::exponential(5))
    .with_recorder(recorder)
    .build();
```

### Recorder

An optional persistence layer for tracking pipeline execution. Delta provides:
- **NoopRecorder** - No-op implementation for simple scenarios
- **SqliteRecorder** - Persists runs and steps to SQLite (requires `sqlite` feature)

You can also implement the `Recorder` trait for custom backends.

### RetryPolicy

Determines how to handle retryable failures:

```rust
// No retries
RetryPolicy::None

// Fixed delay between attempts
RetryPolicy::fixed(5, Duration::from_secs(2))

// Exponential backoff with default settings (1s initial, 300s max)
RetryPolicy::exponential(5)
```

**Error types:**
- `StepError::Retryable` - Transient failure worth retrying (e.g., network timeout)
- `StepError::Permanent` - Won't succeed on retry (e.g., invalid input)

### HasEntityId

Pipeline inputs and outputs must implement `HasEntityId` to provide an identifier for tracking runs:

```rust
pub trait HasEntityId {
    fn entity_id(&self) -> String;
}
```

This allows you to correlate pipeline runs with your domain entities (e.g., video IDs, user IDs).

## SQLite Recording

Enable the `sqlite` feature to persist pipeline execution history:

```toml
[dependencies]
delta = { version = "0.1", features = ["sqlite"] }
```

Then use `SqliteRecorder`:

```rust
use delta::SqliteRecorder;
use sqlx::SqlitePool;

let pool = SqlitePool::connect("sqlite:pipeline.db").await?;
let recorder = SqliteRecorder::new(pool.clone());

// Run migrations to create tables
recorder.run_migrations().await?;

let pipeline = Pipeline::new("my_pipeline")
    .start_with(MyStep)
    .with_recorder(recorder)
    .build();
```

Delta creates two tables with `delta_` prefix:
- **delta_runs** - Pipeline execution records (pipeline name, entity ID, status, timestamps)
- **delta_steps** - Individual step records (step name, attempt count, status, error messages)

These tables can be joined with your domain tables using the entity ID for rich debugging queries.

## Examples

See the `examples/` directory for complete examples:

```bash
cargo run --example basic
```

## License

MIT

