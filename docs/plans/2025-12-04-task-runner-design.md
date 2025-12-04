# Delta Task Runner - Design Document

Extends Delta with a task queue and runner system that dispatches tasks to pipelines, enabling Delta to serve as a complete workflow orchestration layer.

## Overview & Goals

### Goals

- **Unified workflow orchestration** - Replace message brokers (NATS) for core workflow coordination
- **Pipeline-centric design** - Pipelines ARE the task types, no separate dispatch boilerplate
- **Durable task queue** - SQLite-backed persistence, survives restarts
- **Observable execution** - Tasks and pipeline runs recorded together for debugging
- **Minimal ceremony** - Stay true to Elixir-inspired simplicity

### Non-goals (for v1)

- Distributed workers (single process)
- Task priorities or scheduling
- Task dependencies/DAGs (tasks are independent, follow-ups are implicit)
- Task cancellation mid-execution

### Example Usage

```rust
// Define pipelines with follow-up spawning
let video_pipeline = Pipeline::new("process_video")
    .start_with(FetchTranscript::new(ytdlp))
    .then(AnalyzeSentiment::new(ollama))
    .then(GenerateSignals::new(strategy))
    .with_retry(RetryPolicy::exponential(5))
    .with_recorder(recorder.clone())
    .spawns("fetch_price", |result: &ProcessedVideo| {
        result.tickers.iter()
            .map(|t| PriceRequest { ticker: t.clone(), date: result.date.clone() })
            .collect()
    })
    .build();

let price_pipeline = Pipeline::new("fetch_price")
    .start_with(FetchPrice::new(tiingo))
    .with_retry(RetryPolicy::exponential(3))
    .with_recorder(recorder.clone())
    .build();

// Build runner with registered pipelines
let runner = Runner::new(SqliteTaskStore::new(pool.clone()))
    .pipeline(video_pipeline)
    .pipeline(price_pipeline)
    .poll_interval(Duration::from_secs(1))
    .max_concurrent(2)
    .build();

// Submit work
runner.submit::<Video>("process_video", video).await?;

// Run the loop
runner.run().await;
```

## Core Architecture

### Pipeline Extensions

Pipelines gain the ability to declare follow-up work:

```rust
impl<I, O> PipelineBuilder<I, O> {
    /// Declare follow-up tasks to spawn on successful completion.
    /// The closure receives the pipeline output and returns inputs for the target pipeline.
    pub fn spawns<T: Serialize>(
        self,
        target_pipeline: &'static str,
        f: impl Fn(&O) -> Vec<T> + Send + Sync + 'static,
    ) -> Self;
}
```

The pipeline stores spawn declarations. The runner executes them after successful completion.

### TaskStore Trait

Pluggable storage backend for task persistence:

```rust
#[async_trait]
pub trait TaskStore: Send + Sync {
    /// Add a task to the queue (status: pending).
    async fn enqueue(&self, pipeline: &str, input: serde_json::Value) -> Result<TaskId, TaskError>;

    /// Claim up to `limit` pending tasks (atomically sets status: running).
    async fn claim(&self, limit: usize) -> Result<Vec<StoredTask>, TaskError>;

    /// Mark a task as completed.
    async fn complete(&self, id: TaskId) -> Result<(), TaskError>;

    /// Mark a task as failed with error message.
    async fn fail(&self, id: TaskId, error: &str) -> Result<(), TaskError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskId(pub i64);

pub struct StoredTask {
    pub id: TaskId,
    pub pipeline: String,
    pub input: serde_json::Value,
    pub created_at: DateTime<Utc>,
}
```

### SQLite Schema

```sql
CREATE TABLE delta_tasks (
    id INTEGER PRIMARY KEY,
    pipeline TEXT NOT NULL,
    input TEXT NOT NULL,              -- JSON
    status TEXT NOT NULL DEFAULT 'pending',  -- pending, running, completed, failed
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    started_at TEXT,
    completed_at TEXT
);

CREATE INDEX idx_delta_tasks_status ON delta_tasks(status, created_at);
CREATE INDEX idx_delta_tasks_pipeline ON delta_tasks(pipeline, status);
```

### SqliteTaskStore

```rust
pub struct SqliteTaskStore {
    pool: SqlitePool,
}

impl SqliteTaskStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn run_migrations(&self) -> Result<(), TaskError> {
        // Creates delta_tasks table if not exists
    }
}
```

The `claim()` method uses SQLite's `UPDATE ... RETURNING` for atomic claim:

```sql
UPDATE delta_tasks
SET status = 'running', started_at = datetime('now')
WHERE id IN (
    SELECT id FROM delta_tasks
    WHERE status = 'pending'
    ORDER BY created_at
    LIMIT ?
)
RETURNING *
```

## Runner

### Structure

```rust
pub struct Runner<S: TaskStore> {
    store: Arc<S>,
    pipelines: HashMap<&'static str, Arc<dyn ErasedPipeline>>,
    poll_interval: Duration,
    max_concurrent: usize,
}
```

### Builder Pattern

```rust
impl Runner<SqliteTaskStore> {
    pub fn new(store: SqliteTaskStore) -> RunnerBuilder<SqliteTaskStore> {
        RunnerBuilder::new(store)
    }
}

pub struct RunnerBuilder<S: TaskStore> {
    store: S,
    pipelines: HashMap<&'static str, Arc<dyn ErasedPipeline>>,
    poll_interval: Duration,
    max_concurrent: usize,
}

impl<S: TaskStore> RunnerBuilder<S> {
    pub fn pipeline<I, O>(mut self, pipeline: BuiltPipeline<I, O>) -> Self
    where
        I: DeserializeOwned + Send + Sync + 'static,
        O: Send + 'static,
    {
        self.pipelines.insert(pipeline.name(), Arc::new(pipeline));
        self
    }

    pub fn poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    pub fn max_concurrent(mut self, n: usize) -> Self {
        self.max_concurrent = n;
        self
    }

    pub fn build(self) -> Runner<S> {
        Runner {
            store: Arc::new(self.store),
            pipelines: self.pipelines,
            poll_interval: self.poll_interval,
            max_concurrent: self.max_concurrent,
        }
    }
}
```

### Typed Submission

```rust
impl<S: TaskStore> Runner<S> {
    /// Submit a task to a named pipeline with type-checked input.
    pub async fn submit<T: Serialize>(&self, pipeline: &str, input: T) -> Result<TaskId, TaskError> {
        let json = serde_json::to_value(input)
            .map_err(|e| TaskError::SerializationError(e.to_string()))?;
        self.store.enqueue(pipeline, json).await
    }
}
```

### Run Loop

```rust
impl<S: TaskStore> Runner<S> {
    pub async fn run(&self) -> ! {
        let semaphore = Arc::new(Semaphore::new(self.max_concurrent));

        loop {
            let available = semaphore.available_permits();
            if available > 0 {
                if let Ok(tasks) = self.store.claim(available).await {
                    for task in tasks {
                        let permit = semaphore.clone().acquire_owned().await.unwrap();
                        let store = self.store.clone();
                        let pipelines = self.pipelines.clone();

                        tokio::spawn(async move {
                            let result = Self::execute_task(&pipelines, &store, &task).await;
                            match result {
                                Ok(()) => { store.complete(task.id).await.ok(); }
                                Err(e) => { store.fail(task.id, &e.to_string()).await.ok(); }
                            }
                            drop(permit);
                        });
                    }
                }
            }

            tokio::time::sleep(self.poll_interval).await;
        }
    }

    async fn execute_task(
        pipelines: &HashMap<&'static str, Arc<dyn ErasedPipeline>>,
        store: &Arc<S>,
        task: &StoredTask,
    ) -> Result<(), TaskError> {
        let pipeline = pipelines.get(task.pipeline.as_str())
            .ok_or_else(|| TaskError::UnknownPipeline(task.pipeline.clone()))?;

        // Run pipeline, collect spawned work
        let spawned = pipeline.run_erased(task.input.clone()).await?;

        // Enqueue follow-up tasks
        for (target, input) in spawned {
            store.enqueue(target, input).await?;
        }

        Ok(())
    }
}
```

### ErasedPipeline Trait

Internal trait for type-erased pipeline execution:

```rust
#[async_trait]
trait ErasedPipeline: Send + Sync {
    fn name(&self) -> &'static str;

    /// Run with JSON input, return spawned work as (pipeline_name, json_input) pairs.
    async fn run_erased(&self, input: serde_json::Value) -> Result<Vec<(&'static str, serde_json::Value)>, PipelineError>;
}

impl<I, O> ErasedPipeline for BuiltPipeline<I, O>
where
    I: DeserializeOwned + Send + Sync + 'static,
    O: Send + 'static,
{
    fn name(&self) -> &'static str {
        self.name
    }

    async fn run_erased(&self, input: serde_json::Value) -> Result<Vec<(&'static str, serde_json::Value)>, PipelineError> {
        let typed_input: I = serde_json::from_value(input)
            .map_err(|e| PipelineError::DeserializationError(e.to_string()))?;

        let output = self.run(typed_input).await?;

        // Execute spawn declarations
        let spawned = self.spawn_declarations.iter()
            .flat_map(|decl| decl.generate(&output))
            .collect();

        Ok(spawned)
    }
}
```

## Error Handling

### TaskError

```rust
#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("Task storage error: {0}")]
    StorageError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Unknown pipeline: {0}")]
    UnknownPipeline(String),

    #[error("Pipeline execution failed: {0}")]
    PipelineError(#[from] PipelineError),
}
```

### Failed Task Handling

- Failed tasks are marked with `status = 'failed'` and `error_message` stored
- Runner continues processing other tasks
- Failed tasks remain in `delta_tasks` for inspection and manual retry
- Pipeline-level retries (via `RetryPolicy`) are exhausted before task is marked failed

## Alpha Integration Example

### Pipeline Definitions

```rust
fn build_video_pipeline(config: &Config, recorder: SqliteRecorder) -> BuiltPipeline<Video, ProcessedVideo> {
    Pipeline::new("process_video")
        .start_with(FetchTranscript::new(config.ytdlp.clone()))
        .then(AnalyzeSentiment::new(config.ollama.clone()))
        .then(GenerateSignals::new(config.strategy.clone()))
        .with_retry(RetryPolicy::exponential(5))
        .with_recorder(recorder)
        .spawns("fetch_price", |result: &ProcessedVideo| {
            result.tickers.iter()
                .map(|t| PriceRequest {
                    ticker: t.clone(),
                    date: result.date.clone()
                })
                .collect()
        })
        .build()
}

fn build_price_pipeline(config: &Config, recorder: SqliteRecorder) -> BuiltPipeline<PriceRequest, ()> {
    Pipeline::new("fetch_price")
        .start_with(FetchPrice::new(config.tiingo.clone()))
        .then(SavePrice::new(config.repo.clone()))
        .with_retry(RetryPolicy::exponential(3))
        .with_recorder(recorder)
        .build()
}
```

### Channel Poller

```rust
impl ChannelPoller {
    pub async fn run<S: TaskStore>(&self, runner: &Runner<S>) {
        let mut interval = tokio::time::interval(self.poll_interval);
        loop {
            interval.tick().await;
            if let Err(e) = self.poll_once(runner).await {
                tracing::warn!("Poll cycle failed: {e}");
            }
        }
    }

    async fn poll_once<S: TaskStore>(&self, runner: &Runner<S>) -> Result<()> {
        let channels = self.repo.get_seed_channels().await?;

        let new_videos: Vec<_> = futures::stream::iter(channels)
            .then(|ch| self.ytdlp.list_channel_videos(&ch))
            .filter_map(|res| async { res.ok() })
            .flatten()
            .filter(|v| async {
                !self.repo.video_exists(&v.youtube_id).await.unwrap_or(true)
            })
            .collect()
            .await;

        for video in new_videos {
            self.repo.insert_video(&video).await?;
            runner.submit("process_video", video).await?;
        }

        Ok(())
    }
}
```

### Main Entry Point

```rust
async fn main() -> Result<()> {
    let config = Config::load()?;
    let pool = SqlitePool::connect(&config.database_url).await?;

    let recorder = SqliteRecorder::new(pool.clone());
    recorder.run_migrations().await?;

    let task_store = SqliteTaskStore::new(pool.clone());
    task_store.run_migrations().await?;

    let video_pipeline = build_video_pipeline(&config, recorder.clone());
    let price_pipeline = build_price_pipeline(&config, recorder.clone());

    let runner = Runner::new(task_store)
        .pipeline(video_pipeline)
        .pipeline(price_pipeline)
        .poll_interval(Duration::from_secs(1))
        .max_concurrent(2)
        .build();

    let poller = ChannelPoller::new(
        config.repo.clone(),
        config.ytdlp.clone(),
        config.poll_interval,
    );

    tokio::select! {
        _ = poller.run(&runner) => {}
        _ = runner.run() => {}
    }

    Ok(())
}
```

## File Structure

### New Delta Files

```
delta/src/
├── lib.rs              # existing + new exports
├── step.rs             # existing (unchanged)
├── pipeline.rs         # extended with .spawns()
├── executor.rs         # existing (unchanged)
├── retry.rs            # existing (unchanged)
├── recorder.rs         # existing (unchanged)
├── sqlite.rs           # existing (unchanged)
├── runner/
│   ├── mod.rs          # Runner struct, builder, ErasedPipeline
│   ├── store.rs        # TaskStore trait, TaskId, StoredTask
│   └── sqlite_store.rs # SqliteTaskStore implementation
└── task.rs             # TaskError
```

### New Dependencies

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[dependencies.sqlx]
version = "0.8"
features = ["runtime-tokio", "sqlite", "json"]
optional = true
```

### Public API Additions

```rust
// lib.rs additions
pub use runner::{Runner, RunnerBuilder, TaskStore, SqliteTaskStore, TaskId, StoredTask};
pub use task::TaskError;
```

## Design Decisions Summary

| Aspect | Decision | Rationale |
|--------|----------|-----------|
| Task types | Pipelines are task types | No separate enum/dispatch boilerplate |
| Storage | SQLite with pluggable trait | Consistent with existing Delta patterns |
| Serialization | serde + JSON | Human-readable, debuggable, universal |
| Concurrency | Semaphore-limited | Simple, predictable resource usage |
| Follow-ups | `.spawns()` declaration | Keeps steps pure, orchestration at pipeline level |
| Submission | Typed `runner.submit::<T>()` | Type safety at call site |
| Polling | Configurable interval | Simple, no complex notification system |
| Failed tasks | Mark and continue | Pipeline retries suffice, manual inspection for failures |

## Migration from NATS

For Alpha, migrating from NATS to Delta runner:

1. Remove `alpha-nats` crate dependency
2. Replace `VideoProcessor` NATS consumer with Delta pipeline
3. Replace `PriceFetcher` NATS consumer with Delta pipeline
4. Replace NATS publisher calls with `runner.submit()`
5. Remove NATS stream/consumer configuration
6. Update `ChannelPoller` to submit to runner instead of publishing

The `delta_tasks` and `delta_runs`/`delta_steps` tables together provide full observability previously split between NATS and application logs.
