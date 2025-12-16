use super::job::{PeriodicScheduler, QueryFn, RegisteredJob};
use crate::runner::TaskStore;
use serde::Serialize;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

/// Builder for configuring a PeriodicScheduler.
pub struct SchedulerBuilder<S: TaskStore> {
    task_store: S,
    jobs: Vec<RegisteredJob>,
    pending_run_on_start: bool,
}

impl<S: TaskStore + 'static> SchedulerBuilder<S> {
    /// Create a new scheduler builder with the given task store.
    pub fn new(task_store: S) -> Self {
        Self {
            task_store,
            jobs: Vec::new(),
            pending_run_on_start: false,
        }
    }

    /// Add a job that queries for items and enqueues them to a pipeline.
    ///
    /// The query function is called at each interval and should return
    /// items to enqueue. Each item is serialized to JSON and submitted
    /// to the named pipeline.
    pub fn job<F, Fut, T>(
        mut self,
        pipeline_name: &'static str,
        interval: Duration,
        query_fn: F,
    ) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Vec<T>> + Send + 'static,
        T: Serialize + 'static,
    {
        let query_fn: QueryFn = Arc::new(move || {
            let fut = query_fn();
            Box::pin(async move {
                let items = fut.await;
                items
                    .into_iter()
                    .filter_map(|item| serde_json::to_value(item).ok())
                    .collect()
            })
        });

        self.jobs.push(RegisteredJob {
            pipeline_name,
            interval,
            query_fn,
            run_on_start: self.pending_run_on_start,
        });

        // Reset for next job
        self.pending_run_on_start = false;

        self
    }

    /// Set whether the most recently added job should run immediately on start.
    ///
    /// Must be called after `.job()`. Defaults to false.
    pub fn run_on_start(mut self, run: bool) -> Self {
        if let Some(job) = self.jobs.last_mut() {
            job.run_on_start = run;
        }
        self
    }

    /// Build the scheduler.
    pub fn build(self) -> PeriodicScheduler<S> {
        PeriodicScheduler::new(self.task_store, self.jobs)
    }
}
