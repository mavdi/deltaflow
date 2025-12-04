# Deltaflow Documentation & Publication Design

## Overview

Design for creating professional documentation and publishing Deltaflow to crates.io.

## Positioning

- **Tagline:** "The embeddable workflow engine"
- **One-liner:** Type-safe, Elixir-inspired pipelines that run in your process. No infrastructure required.
- **Audience:** Application developers who need background job processing (Sidekiq/Celery crowd)
- **Maturity:** Experimental/Alpha (0.1.x)
- **Crate name:** `deltaflow`
- **License:** MIT

## The Pitch

Deltaflow is to workflow engines what ZeroMQ is to message brokers: the embedded, zero-infrastructure alternative.

**Why Deltaflow?**

1. **Type-safe composition** - Compiler enforces step output matches next step's input
2. **Elixir-inspired** - Declarative pipelines via method chaining, not scattered callbacks
3. **Observable by default** - Every run and step recorded for debugging
4. **Embeddable** - A library, not a service. Runs in your process.

## README Structure

```
# Deltaflow

The embeddable workflow engine.

[One-sentence description]

## Why Deltaflow?

[4 bullet pitch]

## Quick Start

[~30 line minimal example]

## Installation

[Cargo.toml snippet]

## Core Concepts

### Step
[2-3 sentences]

### Pipeline
[2-3 sentences]

### Runner
[2-3 sentences]

## Status

[Alpha disclaimer, what works, what's coming]

## License

MIT
```

## Quick Start Example

Target: ~30 lines, show the shape without boilerplate.

```rust
use deltaflow::{Pipeline, Step, StepError, RetryPolicy, NoopRecorder};

// Define steps as structs implementing the Step trait
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

Note: Step trait implementations shown in "Core Concepts" section, not in quick start.

## Cargo.toml Updates

```toml
[package]
name = "deltaflow"
version = "0.1.0"
edition = "2021"
description = "The embeddable workflow engine. Type-safe, Elixir-inspired pipelines."
license = "MIT"
repository = "https://github.com/mavdi/deltaflow"
documentation = "https://docs.rs/deltaflow"
readme = "README.md"
keywords = ["workflow", "pipeline", "async", "jobs", "tasks"]
categories = ["asynchronous", "concurrency"]

[features]
default = []
sqlite = ["sqlx"]

[dependencies]
# ... existing deps
```

## Publication Checklist

### Pre-publication

1. [ ] Rename crate from `delta` to `deltaflow` in Cargo.toml
2. [ ] Update all internal references to new name
3. [ ] Write new README.md following structure above
4. [ ] Add rustdoc comments to all public items
5. [ ] Ensure `cargo doc` generates clean documentation
6. [ ] Add LICENSE file (MIT)
7. [ ] Update repository URL if needed
8. [ ] Run `cargo publish --dry-run` to validate

### Publication

1. [ ] Create crates.io account (via GitHub)
2. [ ] Run `cargo login` with API token
3. [ ] Run `cargo publish`
4. [ ] Verify crate appears on crates.io
5. [ ] Verify docs.rs documentation generates

### Post-publication

1. [ ] Update GitHub repo description
2. [ ] Add crates.io badge to README
3. [ ] Consider announcement (Reddit r/rust, Twitter, etc.)

## Status Section Content

```markdown
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
```

## Rustdoc Guidelines

For crate-level docs (`lib.rs`):

```rust
//! # Deltaflow
//!
//! The embeddable workflow engine.
//!
//! Deltaflow brings Elixir-style pipeline composition to Rust with compile-time
//! type safety. Build observable workflows where each step transforms typed
//! inputs to outputs.
//!
//! ## Quick Start
//!
//! ```rust
//! // [minimal example]
//! ```
//!
//! ## Feature Flags
//!
//! - `sqlite` - Enable SQLite-backed recording and task storage
```

For public items, brief doc comments explaining purpose and showing usage.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Crate name | deltaflow | `delta` taken; echoes Elixir Flow |
| README length | Minimal | Alpha stage, respect reader time |
| Example length | ~30 lines | Show shape, not boilerplate |
| Comparison table | Omit for now | Add when users ask |
| Anti-use-cases | In Status section | Honest about scope |
| License | MIT | Simple, permissive |
