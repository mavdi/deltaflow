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
    pipeline_semaphores: HashMap<&'static str, Arc<tokio::sync::Semaphore>>,
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
                        let store = self.store.clone();
                        let pipelines = self.pipelines.clone();
                        let pipeline_sem = self.pipeline_semaphores.get(task.pipeline.as_str()).cloned();

                        // Acquire either pipeline-specific OR global semaphore, not both
                        // This prevents deadlocks when both semaphores are present
                        let global_sem = semaphore.clone();

                        tokio::spawn(async move {
                            // Use pipeline semaphore if available, otherwise use global
                            let _permit = if let Some(sem) = pipeline_sem {
                                // Pipeline with custom concurrency - use ONLY pipeline semaphore
                                sem.acquire_owned().await.unwrap()
                            } else {
                                // No custom concurrency - use global semaphore
                                global_sem.acquire_owned().await.unwrap()
                            };

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
                        });
                    }
                }
            }

            tokio::time::sleep(self.poll_interval).await;
        }
    }

    async fn execute_task(
        pipelines: &HashMap<&'static str, Arc<dyn ErasedPipeline>>,
        _store: &S,
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
    pipeline_concurrency: HashMap<&'static str, usize>,
}

impl<S: TaskStore + 'static> RunnerBuilder<S> {
    /// Create a new builder with the given store.
    pub fn new(store: S) -> Self {
        Self {
            store,
            pipelines: HashMap::new(),
            poll_interval: Duration::from_secs(1),
            max_concurrent: 1,
            pipeline_concurrency: HashMap::new(),
        }
    }

    /// Register a pipeline with the runner.
    pub fn pipeline(mut self, pipeline: impl ErasedPipeline + 'static) -> Self {
        let name = pipeline.name();
        self.pipelines.insert(name, Arc::new(pipeline));
        self
    }

    /// Register a pipeline with custom concurrency limit.
    ///
    /// This pipeline will use its own semaphore limiting concurrent executions,
    /// independent of and INSTEAD OF the global `max_concurrent` setting.
    /// The pipeline will only be limited by its own concurrency setting.
    ///
    /// # Panics
    ///
    /// Panics if `max_concurrent` is 0.
    pub fn pipeline_with_concurrency(
        mut self,
        pipeline: impl ErasedPipeline + 'static,
        max_concurrent: usize,
    ) -> Self {
        assert!(max_concurrent > 0, "pipeline concurrency must be at least 1");
        let name = pipeline.name();
        self.pipeline_concurrency.insert(name, max_concurrent);
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
        let pipeline_semaphores = self
            .pipeline_concurrency
            .into_iter()
            .map(|(name, limit)| (name, Arc::new(tokio::sync::Semaphore::new(limit))))
            .collect();

        Runner {
            store: Arc::new(self.store),
            pipelines: self.pipelines,
            poll_interval: self.poll_interval,
            max_concurrent: self.max_concurrent,
            pipeline_semaphores,
        }
    }
}
