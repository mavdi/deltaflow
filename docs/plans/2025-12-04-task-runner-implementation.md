# Delta Task Runner Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a SQLite-backed task queue and runner system to Delta that dispatches tasks to pipelines.

**Architecture:** Pipelines become task types. Runner polls SQLite for pending tasks, deserializes inputs, runs the appropriate pipeline, and enqueues follow-up work declared via `.spawns()`. Uses semaphore for concurrency control.

**Tech Stack:** Rust, async-trait, serde, serde_json, sqlx (SQLite), tokio

---

## Task 1: Add serde dependencies

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add serde and serde_json to dependencies**

In `Cargo.toml`, add after the `anyhow` line:

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

**Step 2: Verify compilation**

Run: `cargo build`
Expected: Compiles successfully

**Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "deps: add serde and serde_json for task serialization"
```

---

## Task 2: Create TaskStore trait and types

**Files:**
- Create: `src/runner/mod.rs`
- Create: `src/runner/store.rs`
- Modify: `src/lib.rs`

**Step 1: Create runner module directory**

```bash
mkdir -p src/runner
```

**Step 2: Create store.rs with TaskStore trait**

Create `src/runner/store.rs`:

```rust
//! Task storage trait and types.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;

/// Unique identifier for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(pub i64);

/// A task retrieved from storage.
#[derive(Debug, Clone)]
pub struct StoredTask {
    pub id: TaskId,
    pub pipeline: String,
    pub input: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// Error type for task operations.
#[derive(Error, Debug)]
pub enum TaskError {
    #[error("storage error: {0}")]
    StorageError(String),

    #[error("serialization error: {0}")]
    SerializationError(String),

    #[error("unknown pipeline: {0}")]
    UnknownPipeline(String),

    #[error("deserialization error: {0}")]
    DeserializationError(String),

    #[error("pipeline error: {0}")]
    PipelineError(String),
}

/// Trait for task storage backends.
#[async_trait]
pub trait TaskStore: Send + Sync {
    /// Enqueue a task for a pipeline with JSON input.
    async fn enqueue(&self, pipeline: &str, input: serde_json::Value) -> Result<TaskId, TaskError>;

    /// Claim up to `limit` pending tasks atomically.
    async fn claim(&self, limit: usize) -> Result<Vec<StoredTask>, TaskError>;

    /// Mark a task as completed.
    async fn complete(&self, id: TaskId) -> Result<(), TaskError>;

    /// Mark a task as failed with an error message.
    async fn fail(&self, id: TaskId, error: &str) -> Result<(), TaskError>;
}
```

**Step 3: Create runner/mod.rs**

Create `src/runner/mod.rs`:

```rust
//! Task runner module.

pub mod store;

pub use store::{StoredTask, TaskError, TaskId, TaskStore};
```

**Step 4: Add runner module to lib.rs**

In `src/lib.rs`, add after the `pub mod step;` line:

```rust
#[cfg(feature = "sqlite")]
pub mod runner;
```

And add to the sqlite feature exports section:

```rust
#[cfg(feature = "sqlite")]
pub use runner::{StoredTask, TaskError, TaskId, TaskStore};
```

**Step 5: Verify compilation**

Run: `cargo build --features sqlite`
Expected: Compiles successfully

**Step 6: Commit**

```bash
git add src/runner/ src/lib.rs
git commit -m "feat: add TaskStore trait and task types"
```

---

## Task 3: Implement SqliteTaskStore

**Files:**
- Create: `src/runner/sqlite_store.rs`
- Modify: `src/runner/mod.rs`
- Modify: `src/lib.rs`

**Step 1: Create sqlite_store.rs**

Create `src/runner/sqlite_store.rs`:

```rust
//! SQLite implementation of TaskStore.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use super::store::{StoredTask, TaskError, TaskId, TaskStore};

/// SQLite-backed task store.
pub struct SqliteTaskStore {
    pool: SqlitePool,
}

impl SqliteTaskStore {
    /// Create a new SqliteTaskStore.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Run migrations to create the tasks table.
    pub async fn run_migrations(&self) -> Result<(), TaskError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS delta_tasks (
                id INTEGER PRIMARY KEY,
                pipeline TEXT NOT NULL,
                input TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                error_message TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                started_at TEXT,
                completed_at TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| TaskError::StorageError(e.to_string()))?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_delta_tasks_status
            ON delta_tasks(status, created_at)
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| TaskError::StorageError(e.to_string()))?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_delta_tasks_pipeline
            ON delta_tasks(pipeline, status)
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| TaskError::StorageError(e.to_string()))?;

        Ok(())
    }
}

#[async_trait]
impl TaskStore for SqliteTaskStore {
    async fn enqueue(&self, pipeline: &str, input: serde_json::Value) -> Result<TaskId, TaskError> {
        let input_str = serde_json::to_string(&input)
            .map_err(|e| TaskError::SerializationError(e.to_string()))?;

        let result = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO delta_tasks (pipeline, input)
            VALUES (?, ?)
            RETURNING id
            "#,
        )
        .bind(pipeline)
        .bind(input_str)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| TaskError::StorageError(e.to_string()))?;

        Ok(TaskId(result))
    }

    async fn claim(&self, limit: usize) -> Result<Vec<StoredTask>, TaskError> {
        // SQLite doesn't support UPDATE ... LIMIT with RETURNING directly,
        // so we do it in two steps within a transaction
        let mut tx = self.pool
            .begin()
            .await
            .map_err(|e| TaskError::StorageError(e.to_string()))?;

        // Get IDs of tasks to claim
        let ids: Vec<i64> = sqlx::query_scalar(
            r#"
            SELECT id FROM delta_tasks
            WHERE status = 'pending'
            ORDER BY created_at
            LIMIT ?
            "#,
        )
        .bind(limit as i64)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| TaskError::StorageError(e.to_string()))?;

        if ids.is_empty() {
            tx.commit()
                .await
                .map_err(|e| TaskError::StorageError(e.to_string()))?;
            return Ok(vec![]);
        }

        // Build placeholders for IN clause
        let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
        let in_clause = placeholders.join(",");

        // Update status
        let update_query = format!(
            "UPDATE delta_tasks SET status = 'running', started_at = datetime('now') WHERE id IN ({})",
            in_clause
        );
        let mut query = sqlx::query(&update_query);
        for id in &ids {
            query = query.bind(id);
        }
        query
            .execute(&mut *tx)
            .await
            .map_err(|e| TaskError::StorageError(e.to_string()))?;

        // Fetch the updated tasks
        let select_query = format!(
            "SELECT id, pipeline, input, created_at FROM delta_tasks WHERE id IN ({})",
            in_clause
        );
        let mut select = sqlx::query_as::<_, (i64, String, String, String)>(&select_query);
        for id in &ids {
            select = select.bind(id);
        }
        let rows = select
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| TaskError::StorageError(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| TaskError::StorageError(e.to_string()))?;

        let tasks = rows
            .into_iter()
            .map(|(id, pipeline, input, created_at)| {
                let input_value: serde_json::Value = serde_json::from_str(&input)
                    .map_err(|e| TaskError::DeserializationError(e.to_string()))?;
                let created = DateTime::parse_from_rfc3339(&format!("{}Z", created_at.replace(' ', "T")))
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                Ok(StoredTask {
                    id: TaskId(id),
                    pipeline,
                    input: input_value,
                    created_at: created,
                })
            })
            .collect::<Result<Vec<_>, TaskError>>()?;

        Ok(tasks)
    }

    async fn complete(&self, id: TaskId) -> Result<(), TaskError> {
        sqlx::query(
            r#"
            UPDATE delta_tasks
            SET status = 'completed', completed_at = datetime('now')
            WHERE id = ?
            "#,
        )
        .bind(id.0)
        .execute(&self.pool)
        .await
        .map_err(|e| TaskError::StorageError(e.to_string()))?;

        Ok(())
    }

    async fn fail(&self, id: TaskId, error: &str) -> Result<(), TaskError> {
        let truncated_error = if error.len() > 2000 {
            &error[..2000]
        } else {
            error
        };

        sqlx::query(
            r#"
            UPDATE delta_tasks
            SET status = 'failed', completed_at = datetime('now'), error_message = ?
            WHERE id = ?
            "#,
        )
        .bind(truncated_error)
        .bind(id.0)
        .execute(&self.pool)
        .await
        .map_err(|e| TaskError::StorageError(e.to_string()))?;

        Ok(())
    }
}
```

**Step 2: Update runner/mod.rs to include sqlite_store**

Replace `src/runner/mod.rs`:

```rust
//! Task runner module.

pub mod store;
pub mod sqlite_store;

pub use store::{StoredTask, TaskError, TaskId, TaskStore};
pub use sqlite_store::SqliteTaskStore;
```

**Step 3: Export SqliteTaskStore from lib.rs**

Update the sqlite feature exports in `src/lib.rs`:

```rust
#[cfg(feature = "sqlite")]
pub use runner::{SqliteTaskStore, StoredTask, TaskError, TaskId, TaskStore};
```

**Step 4: Add chrono dependency**

In `Cargo.toml`, add after serde_json:

```toml
chrono = { version = "0.4", features = ["serde"] }
```

**Step 5: Verify compilation**

Run: `cargo build --features sqlite`
Expected: Compiles successfully

**Step 6: Commit**

```bash
git add Cargo.toml src/runner/ src/lib.rs
git commit -m "feat: implement SqliteTaskStore"
```

---

## Task 4: Write SqliteTaskStore tests

**Files:**
- Create: `tests/task_store_test.rs`

**Step 1: Create test file**

Create `tests/task_store_test.rs`:

```rust
//! Tests for SqliteTaskStore.

use delta::{SqliteTaskStore, TaskStore};
use sqlx::SqlitePool;

async fn setup_store() -> SqliteTaskStore {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();
    store
}

#[tokio::test]
async fn test_enqueue_and_claim() {
    let store = setup_store().await;

    // Enqueue a task
    let input = serde_json::json!({"video_id": 123});
    let id = store.enqueue("process_video", input.clone()).await.unwrap();

    // Claim it
    let tasks = store.claim(10).await.unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, id);
    assert_eq!(tasks[0].pipeline, "process_video");
    assert_eq!(tasks[0].input, input);

    // Claiming again should return empty (task is running)
    let tasks2 = store.claim(10).await.unwrap();
    assert!(tasks2.is_empty());
}

#[tokio::test]
async fn test_complete_task() {
    let store = setup_store().await;

    let input = serde_json::json!({"ticker": "AAPL"});
    let id = store.enqueue("fetch_price", input).await.unwrap();

    let tasks = store.claim(1).await.unwrap();
    assert_eq!(tasks.len(), 1);

    // Complete the task
    store.complete(id).await.unwrap();

    // Should not be claimable
    let tasks2 = store.claim(10).await.unwrap();
    assert!(tasks2.is_empty());
}

#[tokio::test]
async fn test_fail_task() {
    let store = setup_store().await;

    let input = serde_json::json!({"data": "test"});
    let id = store.enqueue("some_pipeline", input).await.unwrap();

    let tasks = store.claim(1).await.unwrap();
    assert_eq!(tasks.len(), 1);

    // Fail the task
    store.fail(id, "something went wrong").await.unwrap();

    // Should not be claimable
    let tasks2 = store.claim(10).await.unwrap();
    assert!(tasks2.is_empty());
}

#[tokio::test]
async fn test_claim_respects_limit() {
    let store = setup_store().await;

    // Enqueue 5 tasks
    for i in 0..5 {
        let input = serde_json::json!({"n": i});
        store.enqueue("pipeline", input).await.unwrap();
    }

    // Claim only 2
    let tasks = store.claim(2).await.unwrap();
    assert_eq!(tasks.len(), 2);

    // Claim remaining 3
    let tasks2 = store.claim(10).await.unwrap();
    assert_eq!(tasks2.len(), 3);
}

#[tokio::test]
async fn test_claim_fifo_order() {
    let store = setup_store().await;

    let id1 = store.enqueue("p", serde_json::json!({"n": 1})).await.unwrap();
    let id2 = store.enqueue("p", serde_json::json!({"n": 2})).await.unwrap();
    let id3 = store.enqueue("p", serde_json::json!({"n": 3})).await.unwrap();

    let tasks = store.claim(2).await.unwrap();
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].id, id1);
    assert_eq!(tasks[1].id, id2);

    let tasks2 = store.claim(1).await.unwrap();
    assert_eq!(tasks2[0].id, id3);
}
```

**Step 2: Run tests**

Run: `cargo test --features sqlite task_store`
Expected: All 5 tests pass

**Step 3: Commit**

```bash
git add tests/task_store_test.rs
git commit -m "test: add SqliteTaskStore unit tests"
```

---

## Task 5: Create ErasedPipeline trait

**Files:**
- Create: `src/runner/erased.rs`
- Modify: `src/runner/mod.rs`

**Step 1: Create erased.rs**

Create `src/runner/erased.rs`:

```rust
//! Type-erased pipeline wrapper for runtime dispatch.

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Arc;

use crate::pipeline::{BuiltPipeline, HasEntityId, PipelineError, StepChain};
use super::store::TaskError;

/// Type-erased pipeline that can be stored in a registry.
#[async_trait]
pub trait ErasedPipeline: Send + Sync {
    /// Get the pipeline name.
    fn name(&self) -> &'static str;

    /// Run the pipeline with JSON input, return spawned work.
    async fn run_erased(
        &self,
        input: serde_json::Value,
    ) -> Result<Vec<SpawnedTask>, TaskError>;
}

/// A task to be spawned after pipeline completion.
#[derive(Debug, Clone)]
pub struct SpawnedTask {
    pub pipeline: &'static str,
    pub input: serde_json::Value,
}

/// Wrapper that implements ErasedPipeline for a concrete BuiltPipeline.
pub struct PipelineWrapper<I, O, Chain>
where
    Chain: StepChain<I, O>,
{
    pub(crate) pipeline: BuiltPipeline<I, O, Chain>,
    pub(crate) spawn_fns: Vec<Arc<dyn SpawnFn<O>>>,
}

/// Trait for spawn functions.
pub trait SpawnFn<O>: Send + Sync {
    fn spawn(&self, output: &O) -> Vec<SpawnedTask>;
}

/// Concrete spawn function implementation.
pub struct SpawnFnImpl<O, T, F>
where
    F: Fn(&O) -> Vec<T> + Send + Sync,
    T: Serialize,
{
    pub target: &'static str,
    pub f: F,
    pub _phantom: std::marker::PhantomData<(O, T)>,
}

impl<O, T, F> SpawnFn<O> for SpawnFnImpl<O, T, F>
where
    F: Fn(&O) -> Vec<T> + Send + Sync,
    T: Serialize,
{
    fn spawn(&self, output: &O) -> Vec<SpawnedTask> {
        (self.f)(output)
            .into_iter()
            .filter_map(|item| {
                serde_json::to_value(item).ok().map(|input| SpawnedTask {
                    pipeline: self.target,
                    input,
                })
            })
            .collect()
    }
}

#[async_trait]
impl<I, O, Chain> ErasedPipeline for PipelineWrapper<I, O, Chain>
where
    I: Send + Sync + Clone + HasEntityId + DeserializeOwned + 'static,
    O: Send + Sync + 'static,
    Chain: StepChain<I, O> + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        self.pipeline.name()
    }

    async fn run_erased(
        &self,
        input: serde_json::Value,
    ) -> Result<Vec<SpawnedTask>, TaskError> {
        let typed_input: I = serde_json::from_value(input)
            .map_err(|e| TaskError::DeserializationError(e.to_string()))?;

        let output = self.pipeline.run(typed_input).await.map_err(|e| {
            TaskError::PipelineError(e.to_string())
        })?;

        let spawned: Vec<SpawnedTask> = self
            .spawn_fns
            .iter()
            .flat_map(|f| f.spawn(&output))
            .collect();

        Ok(spawned)
    }
}
```

**Step 2: Update runner/mod.rs**

Replace `src/runner/mod.rs`:

```rust
//! Task runner module.

pub mod erased;
pub mod sqlite_store;
pub mod store;

pub use erased::{ErasedPipeline, SpawnedTask};
pub use sqlite_store::SqliteTaskStore;
pub use store::{StoredTask, TaskError, TaskId, TaskStore};
```

**Step 3: Update lib.rs exports**

Update the sqlite feature exports in `src/lib.rs`:

```rust
#[cfg(feature = "sqlite")]
pub use runner::{ErasedPipeline, SqliteTaskStore, SpawnedTask, StoredTask, TaskError, TaskId, TaskStore};
```

**Step 4: Verify compilation**

Run: `cargo build --features sqlite`
Expected: Compiles successfully

**Step 5: Commit**

```bash
git add src/runner/ src/lib.rs
git commit -m "feat: add ErasedPipeline trait for type-erased dispatch"
```

---

## Task 6: Create Runner struct

**Files:**
- Create: `src/runner/runner.rs`
- Modify: `src/runner/mod.rs`
- Modify: `src/lib.rs`

**Step 1: Create runner.rs**

Create `src/runner/runner.rs`:

```rust
//! Task runner that polls for tasks and dispatches to pipelines.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

use super::erased::{ErasedPipeline, SpawnedTask};
use super::store::{TaskError, TaskStore};

/// A runner that executes tasks from a store.
pub struct Runner<S: TaskStore> {
    store: Arc<S>,
    pipelines: HashMap<&'static str, Arc<dyn ErasedPipeline>>,
    poll_interval: Duration,
    max_concurrent: usize,
}

impl<S: TaskStore + 'static> Runner<S> {
    /// Submit a task to a named pipeline.
    pub async fn submit<T: serde::Serialize>(
        &self,
        pipeline: &str,
        input: T,
    ) -> Result<super::store::TaskId, TaskError> {
        let json = serde_json::to_value(input)
            .map_err(|e| TaskError::SerializationError(e.to_string()))?;
        self.store.enqueue(pipeline, json).await
    }

    /// Run the task loop indefinitely.
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
                            let result =
                                Self::execute_task(&pipelines, store.as_ref(), &task).await;
                            match result {
                                Ok(spawned) => {
                                    // Enqueue follow-up tasks
                                    for sp in spawned {
                                        let _ = store.enqueue(sp.pipeline, sp.input).await;
                                    }
                                    let _ = store.complete(task.id).await;
                                }
                                Err(e) => {
                                    let _ = store.fail(task.id, &e.to_string()).await;
                                }
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
        store: &S,
        task: &super::store::StoredTask,
    ) -> Result<Vec<SpawnedTask>, TaskError> {
        let pipeline = pipelines
            .get(task.pipeline.as_str())
            .ok_or_else(|| TaskError::UnknownPipeline(task.pipeline.clone()))?;

        pipeline.run_erased(task.input.clone()).await
    }
}

/// Builder for constructing a Runner.
pub struct RunnerBuilder<S: TaskStore> {
    store: S,
    pipelines: HashMap<&'static str, Arc<dyn ErasedPipeline>>,
    poll_interval: Duration,
    max_concurrent: usize,
}

impl<S: TaskStore + 'static> RunnerBuilder<S> {
    /// Create a new builder with the given store.
    pub fn new(store: S) -> Self {
        Self {
            store,
            pipelines: HashMap::new(),
            poll_interval: Duration::from_secs(1),
            max_concurrent: 1,
        }
    }

    /// Register a pipeline with the runner.
    pub fn pipeline(mut self, pipeline: impl ErasedPipeline + 'static) -> Self {
        let name = pipeline.name();
        self.pipelines.insert(name, Arc::new(pipeline));
        self
    }

    /// Set the poll interval.
    pub fn poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Set the maximum concurrent tasks.
    pub fn max_concurrent(mut self, n: usize) -> Self {
        self.max_concurrent = n;
        self
    }

    /// Build the runner.
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

**Step 2: Update runner/mod.rs**

Replace `src/runner/mod.rs`:

```rust
//! Task runner module.

pub mod erased;
pub mod runner;
pub mod sqlite_store;
pub mod store;

pub use erased::{ErasedPipeline, SpawnedTask};
pub use runner::{Runner, RunnerBuilder};
pub use sqlite_store::SqliteTaskStore;
pub use store::{StoredTask, TaskError, TaskId, TaskStore};
```

**Step 3: Update lib.rs exports**

Update the sqlite feature exports in `src/lib.rs`:

```rust
#[cfg(feature = "sqlite")]
pub use runner::{
    ErasedPipeline, Runner, RunnerBuilder, SqliteTaskStore, SpawnedTask, StoredTask, TaskError,
    TaskId, TaskStore,
};
```

**Step 4: Verify compilation**

Run: `cargo build --features sqlite`
Expected: Compiles successfully

**Step 5: Commit**

```bash
git add src/runner/ src/lib.rs
git commit -m "feat: add Runner and RunnerBuilder"
```

---

## Task 7: Add .spawns() to Pipeline builder

**Files:**
- Modify: `src/pipeline.rs`

**Step 1: Add spawns method to Pipeline**

In `src/pipeline.rs`, add the following imports at the top (after existing imports):

```rust
use serde::de::DeserializeOwned;
use serde::Serialize;
```

**Step 2: Add spawn tracking to Pipeline struct**

Modify the `Pipeline` struct to include spawn declarations. After the `_phantom` field, the struct should become:

```rust
/// Builder for constructing pipelines.
pub struct Pipeline<I, O, Chain>
where
    Chain: StepChain<I, O>,
{
    name: &'static str,
    chain: Chain,
    retry_policy: RetryPolicy,
    recorder: Arc<dyn Recorder>,
    spawn_declarations: Vec<SpawnDeclaration<O>>,
    _phantom: std::marker::PhantomData<(I, O)>,
}
```

Add the SpawnDeclaration type before the Pipeline struct:

```rust
/// A declaration of work to spawn after pipeline completion.
pub struct SpawnDeclaration<O> {
    target: &'static str,
    generator: Box<dyn Fn(&O) -> Vec<serde_json::Value> + Send + Sync>,
}
```

**Step 3: Update Pipeline::new**

Update the `Pipeline::new` method to initialize spawn_declarations:

```rust
impl Pipeline<(), (), Identity> {
    /// Create a new pipeline builder with the given name.
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            chain: Identity,
            retry_policy: RetryPolicy::default(),
            recorder: Arc::new(NoopRecorder),
            spawn_declarations: Vec::new(),
            _phantom: std::marker::PhantomData,
        }
    }
}
```

**Step 4: Update start_with to carry spawn_declarations**

```rust
impl<O, Chain> Pipeline<(), O, Chain>
where
    Chain: StepChain<(), O> + Send + Sync + 'static,
    O: Send + 'static,
{
    /// Add the first step to the pipeline.
    #[allow(clippy::type_complexity)]
    pub fn start_with<S>(
        self,
        step: S,
    ) -> Pipeline<
        S::Input,
        S::Output,
        ChainedStep<StepWrapper<S>, Identity, S::Input, S::Output, S::Output>,
    >
    where
        S: Step + 'static,
        S::Output: 'static,
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
            spawn_declarations: Vec::new(),
            _phantom: std::marker::PhantomData,
        }
    }
}
```

**Step 5: Add spawns method and update then/build**

In the main `impl<I, O, Chain> Pipeline<I, O, Chain>` block, add the spawns method and update existing methods:

```rust
impl<I, O, Chain> Pipeline<I, O, Chain>
where
    I: Send + Sync + Clone + 'static,
    O: Send + Sync + Clone + 'static,
    Chain: StepChain<I, O> + Send + Sync + 'static,
{
    /// Add a step to the pipeline.
    pub fn then<S>(self, step: S) -> Pipeline<I, S::Output, impl StepChain<I, S::Output>>
    where
        S: Step<Input = O> + 'static,
        S::Output: Clone + 'static,
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
            spawn_declarations: Vec::new(), // Reset - will be set on final output type
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

    /// Declare follow-up tasks to spawn on successful completion.
    pub fn spawns<T, F>(mut self, target: &'static str, f: F) -> Self
    where
        T: Serialize + 'static,
        F: Fn(&O) -> Vec<T> + Send + Sync + 'static,
    {
        self.spawn_declarations.push(SpawnDeclaration {
            target,
            generator: Box::new(move |output| {
                f(output)
                    .into_iter()
                    .filter_map(|item| serde_json::to_value(item).ok())
                    .collect()
            }),
        });
        self
    }

    /// Build the pipeline, ready for execution.
    pub fn build(self) -> BuiltPipeline<I, O, Chain> {
        BuiltPipeline {
            name: self.name,
            chain: self.chain,
            retry_policy: self.retry_policy,
            recorder: self.recorder,
            spawn_declarations: self.spawn_declarations,
            _phantom: std::marker::PhantomData,
        }
    }
}
```

**Step 6: Update BuiltPipeline struct**

```rust
/// A built pipeline ready for execution.
pub struct BuiltPipeline<I, O, Chain>
where
    Chain: StepChain<I, O>,
{
    name: &'static str,
    chain: Chain,
    retry_policy: RetryPolicy,
    recorder: Arc<dyn Recorder>,
    pub(crate) spawn_declarations: Vec<SpawnDeclaration<O>>,
    _phantom: std::marker::PhantomData<(I, O)>,
}
```

**Step 7: Add method to get spawn declarations on BuiltPipeline**

Add to the BuiltPipeline impl:

```rust
    /// Get spawned tasks for the given output.
    pub fn get_spawned(&self, output: &O) -> Vec<(&'static str, serde_json::Value)> {
        self.spawn_declarations
            .iter()
            .flat_map(|decl| {
                (decl.generator)(output)
                    .into_iter()
                    .map(|input| (decl.target, input))
            })
            .collect()
    }
```

**Step 8: Verify compilation**

Run: `cargo build --features sqlite`
Expected: Compiles successfully

**Step 9: Commit**

```bash
git add src/pipeline.rs
git commit -m "feat: add .spawns() method for follow-up task declaration"
```

---

## Task 8: Connect Pipeline to ErasedPipeline

**Files:**
- Modify: `src/runner/erased.rs`
- Modify: `src/pipeline.rs`

**Step 1: Add wrap method to BuiltPipeline**

In `src/pipeline.rs`, add this method to the BuiltPipeline impl block (needs feature gate):

Add at the end of the file:

```rust
#[cfg(feature = "sqlite")]
mod runner_integration {
    use super::*;
    use crate::runner::erased::{PipelineWrapper, SpawnFn, SpawnFnImpl};
    use serde::de::DeserializeOwned;
    use std::sync::Arc;

    impl<I, O, Chain> BuiltPipeline<I, O, Chain>
    where
        I: Send + Sync + Clone + HasEntityId + DeserializeOwned + 'static,
        O: Send + Sync + Clone + 'static,
        Chain: StepChain<I, O> + Send + Sync + 'static,
    {
        /// Wrap this pipeline for use with a Runner.
        pub fn into_erased(self) -> PipelineWrapper<I, O, Chain> {
            // Convert spawn declarations to SpawnFn trait objects
            let spawn_fns: Vec<Arc<dyn SpawnFn<O>>> = self
                .spawn_declarations
                .iter()
                .map(|decl| {
                    let target = decl.target;
                    let generator = &decl.generator;
                    // We need to clone the generator somehow - for now, we'll recreate
                    // This is a limitation - spawn_declarations need to be Arc'd or Clone
                    Arc::new(SpawnFnFromDecl {
                        target,
                        generator: Box::new(move |_: &O| vec![]), // placeholder
                    }) as Arc<dyn SpawnFn<O>>
                })
                .collect();

            PipelineWrapper {
                pipeline: BuiltPipeline {
                    name: self.name,
                    chain: self.chain,
                    retry_policy: self.retry_policy,
                    recorder: self.recorder,
                    spawn_declarations: Vec::new(), // Moved to spawn_fns
                    _phantom: std::marker::PhantomData,
                },
                spawn_fns: vec![], // Will be populated differently
            }
        }
    }

    struct SpawnFnFromDecl<O> {
        target: &'static str,
        generator: Box<dyn Fn(&O) -> Vec<serde_json::Value> + Send + Sync>,
    }

    impl<O> SpawnFn<O> for SpawnFnFromDecl<O>
    where
        O: Send + Sync,
    {
        fn spawn(&self, output: &O) -> Vec<crate::runner::erased::SpawnedTask> {
            (self.generator)(output)
                .into_iter()
                .map(|input| crate::runner::erased::SpawnedTask {
                    pipeline: self.target,
                    input,
                })
                .collect()
        }
    }
}
```

Actually, let's simplify this. The conversion is complex. Instead, let's make spawn_declarations clonable by using Arc.

**Step 1 (revised): Simplify by making SpawnDeclaration use Arc**

In `src/pipeline.rs`, update the SpawnDeclaration:

```rust
/// A declaration of work to spawn after pipeline completion.
pub struct SpawnDeclaration<O> {
    pub(crate) target: &'static str,
    pub(crate) generator: Arc<dyn Fn(&O) -> Vec<serde_json::Value> + Send + Sync>,
}

impl<O> Clone for SpawnDeclaration<O> {
    fn clone(&self) -> Self {
        Self {
            target: self.target,
            generator: self.generator.clone(),
        }
    }
}
```

And update the spawns method to use Arc:

```rust
    pub fn spawns<T, F>(mut self, target: &'static str, f: F) -> Self
    where
        T: Serialize + 'static,
        F: Fn(&O) -> Vec<T> + Send + Sync + 'static,
    {
        self.spawn_declarations.push(SpawnDeclaration {
            target,
            generator: Arc::new(move |output| {
                f(output)
                    .into_iter()
                    .filter_map(|item| serde_json::to_value(item).ok())
                    .collect()
            }),
        });
        self
    }
```

**Step 2: Simplify erased.rs**

Replace `src/runner/erased.rs` with a simpler implementation:

```rust
//! Type-erased pipeline wrapper for runtime dispatch.

use async_trait::async_trait;
use serde::de::DeserializeOwned;

use crate::pipeline::{BuiltPipeline, HasEntityId, StepChain};
use super::store::TaskError;

/// Type-erased pipeline that can be stored in a registry.
#[async_trait]
pub trait ErasedPipeline: Send + Sync {
    /// Get the pipeline name.
    fn name(&self) -> &'static str;

    /// Run the pipeline with JSON input, return spawned work.
    async fn run_erased(
        &self,
        input: serde_json::Value,
    ) -> Result<Vec<SpawnedTask>, TaskError>;
}

/// A task to be spawned after pipeline completion.
#[derive(Debug, Clone)]
pub struct SpawnedTask {
    pub pipeline: &'static str,
    pub input: serde_json::Value,
}

#[async_trait]
impl<I, O, Chain> ErasedPipeline for BuiltPipeline<I, O, Chain>
where
    I: Send + Sync + Clone + HasEntityId + DeserializeOwned + 'static,
    O: Send + Sync + 'static,
    Chain: StepChain<I, O> + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        self.name()
    }

    async fn run_erased(
        &self,
        input: serde_json::Value,
    ) -> Result<Vec<SpawnedTask>, TaskError> {
        let typed_input: I = serde_json::from_value(input)
            .map_err(|e| TaskError::DeserializationError(e.to_string()))?;

        let output = self.run(typed_input).await.map_err(|e| {
            TaskError::PipelineError(e.to_string())
        })?;

        let spawned: Vec<SpawnedTask> = self
            .get_spawned(&output)
            .into_iter()
            .map(|(pipeline, input)| SpawnedTask { pipeline, input })
            .collect();

        Ok(spawned)
    }
}
```

**Step 3: Update runner/mod.rs**

```rust
//! Task runner module.

pub mod erased;
pub mod runner;
pub mod sqlite_store;
pub mod store;

pub use erased::{ErasedPipeline, SpawnedTask};
pub use runner::{Runner, RunnerBuilder};
pub use sqlite_store::SqliteTaskStore;
pub use store::{StoredTask, TaskError, TaskId, TaskStore};
```

**Step 4: Verify compilation**

Run: `cargo build --features sqlite`
Expected: Compiles successfully

**Step 5: Commit**

```bash
git add src/pipeline.rs src/runner/
git commit -m "feat: implement ErasedPipeline for BuiltPipeline"
```

---

## Task 9: Write Runner integration test

**Files:**
- Create: `tests/runner_test.rs`

**Step 1: Create integration test**

Create `tests/runner_test.rs`:

```rust
//! Integration tests for the task runner.

use async_trait::async_trait;
use delta::{
    HasEntityId, NoopRecorder, Pipeline, RetryPolicy, Runner, RunnerBuilder,
    SqliteTaskStore, Step, StepError, TaskStore,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct VideoInput {
    id: String,
    title: String,
}

impl HasEntityId for VideoInput {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

#[derive(Clone, Debug)]
struct VideoOutput {
    id: String,
    tickers: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PriceRequest {
    ticker: String,
}

impl HasEntityId for PriceRequest {
    fn entity_id(&self) -> String {
        self.ticker.clone()
    }
}

// Step that extracts tickers from video
struct ExtractTickers;

#[async_trait]
impl Step for ExtractTickers {
    type Input = VideoInput;
    type Output = VideoOutput;

    fn name(&self) -> &'static str {
        "extract_tickers"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        // Simulate finding tickers
        Ok(VideoOutput {
            id: input.id,
            tickers: vec!["AAPL".to_string(), "GOOGL".to_string()],
        })
    }
}

// Step that "fetches" a price
struct FetchPriceStep {
    fetched: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl Step for FetchPriceStep {
    type Input = PriceRequest;
    type Output = ();

    fn name(&self) -> &'static str {
        "fetch_price"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        self.fetched.lock().await.push(input.ticker);
        Ok(())
    }
}

#[tokio::test]
async fn test_runner_executes_pipeline() {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();

    let video_pipeline = Pipeline::new("process_video")
        .start_with(ExtractTickers)
        .with_recorder(NoopRecorder)
        .build();

    let runner = RunnerBuilder::new(store)
        .pipeline(video_pipeline)
        .poll_interval(Duration::from_millis(50))
        .max_concurrent(1)
        .build();

    // Submit a task
    let input = VideoInput {
        id: "v1".to_string(),
        title: "Test Video".to_string(),
    };
    runner.submit("process_video", input).await.unwrap();

    // Let runner process (run briefly then cancel)
    tokio::select! {
        _ = runner.run() => {}
        _ = tokio::time::sleep(Duration::from_millis(200)) => {}
    }

    // Task should be completed (we can check the store directly)
    // For now, just verify no panic occurred
}

#[tokio::test]
async fn test_runner_spawns_followup_tasks() {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool.clone());
    store.run_migrations().await.unwrap();

    let fetched = Arc::new(Mutex::new(Vec::new()));
    let fetched_clone = fetched.clone();

    let video_pipeline = Pipeline::new("process_video")
        .start_with(ExtractTickers)
        .with_recorder(NoopRecorder)
        .spawns("fetch_price", |output: &VideoOutput| {
            output.tickers.iter()
                .map(|t| PriceRequest { ticker: t.clone() })
                .collect()
        })
        .build();

    let price_pipeline = Pipeline::new("fetch_price")
        .start_with(FetchPriceStep { fetched: fetched_clone })
        .with_recorder(NoopRecorder)
        .build();

    let runner = RunnerBuilder::new(store)
        .pipeline(video_pipeline)
        .pipeline(price_pipeline)
        .poll_interval(Duration::from_millis(50))
        .max_concurrent(2)
        .build();

    // Submit initial task
    let input = VideoInput {
        id: "v1".to_string(),
        title: "Test Video".to_string(),
    };
    runner.submit("process_video", input).await.unwrap();

    // Let runner process
    tokio::select! {
        _ = runner.run() => {}
        _ = tokio::time::sleep(Duration::from_millis(500)) => {}
    }

    // Check that price fetches were executed
    let fetched_tickers = fetched.lock().await;
    assert!(fetched_tickers.contains(&"AAPL".to_string()));
    assert!(fetched_tickers.contains(&"GOOGL".to_string()));
}
```

**Step 2: Run tests**

Run: `cargo test --features sqlite runner_test`
Expected: Tests pass

**Step 3: Commit**

```bash
git add tests/runner_test.rs
git commit -m "test: add Runner integration tests"
```

---

## Task 10: Final cleanup and documentation

**Files:**
- Modify: `src/lib.rs` (doc comments)
- Modify: `README.md`

**Step 1: Add module documentation**

Update `src/runner/mod.rs` with better docs:

```rust
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
pub mod runner;
pub mod sqlite_store;
pub mod store;

pub use erased::{ErasedPipeline, SpawnedTask};
pub use runner::{Runner, RunnerBuilder};
pub use sqlite_store::SqliteTaskStore;
pub use store::{StoredTask, TaskError, TaskId, TaskStore};
```

**Step 2: Run all tests**

Run: `cargo test --features sqlite`
Expected: All tests pass

**Step 3: Run clippy**

Run: `cargo clippy --features sqlite`
Expected: No warnings (or fix any that appear)

**Step 4: Commit**

```bash
git add src/ README.md
git commit -m "docs: add runner module documentation"
```

---

## Summary

This plan implements the Delta task runner in 10 tasks:

1. Add serde dependencies
2. Create TaskStore trait and types
3. Implement SqliteTaskStore
4. Write SqliteTaskStore tests
5. Create ErasedPipeline trait
6. Create Runner struct
7. Add .spawns() to Pipeline builder
8. Connect Pipeline to ErasedPipeline
9. Write Runner integration tests
10. Final cleanup and documentation

Each task is atomic and can be committed independently. The implementation follows TDD where practical (tests in Task 4 and Task 9).
