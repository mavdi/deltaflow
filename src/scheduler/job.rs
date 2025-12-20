use crate::runner::TaskStore;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, error, info};

/// Type-erased query function that returns JSON values.
pub(crate) type QueryFn =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Vec<serde_json::Value>> + Send>> + Send + Sync>;

/// A registered job configuration.
pub(crate) struct RegisteredJob {
    pub pipeline_name: &'static str,
    pub interval: Duration,
    pub query_fn: QueryFn,
    pub run_on_start: bool,
}

/// A registered trigger configuration.
pub struct RegisteredTrigger {
    pub pipeline_name: &'static str,
    pub interval: Duration,
    pub run_on_start: bool,
}

/// Periodic scheduler that runs registered jobs at intervals.
pub struct PeriodicScheduler<S: TaskStore> {
    task_store: Arc<S>,
    jobs: Vec<RegisteredJob>,
    triggers: Vec<RegisteredTrigger>,
}

impl<S: TaskStore + 'static> PeriodicScheduler<S> {
    pub(crate) fn new(task_store: S, jobs: Vec<RegisteredJob>, triggers: Vec<RegisteredTrigger>) -> Self {
        Self {
            task_store: Arc::new(task_store),
            jobs,
            triggers,
        }
    }

    /// Run all jobs indefinitely.
    pub async fn run(&self) -> ! {
        let mut handles = Vec::new();

        // Spawn job tasks
        for job in &self.jobs {
            let task_store = self.task_store.clone();
            let pipeline_name = job.pipeline_name;
            let interval_duration = job.interval;
            let query_fn = job.query_fn.clone();
            let run_on_start = job.run_on_start;

            handles.push(tokio::spawn(async move {
                Self::run_job(
                    task_store,
                    pipeline_name,
                    interval_duration,
                    query_fn,
                    run_on_start,
                )
                .await
            }));
        }

        // Spawn trigger tasks
        for trigger in &self.triggers {
            let task_store = self.task_store.clone();
            let pipeline_name = trigger.pipeline_name;
            let interval_duration = trigger.interval;
            let run_on_start = trigger.run_on_start;

            handles.push(tokio::spawn(async move {
                Self::run_trigger(task_store, pipeline_name, interval_duration, run_on_start).await
            }));
        }

        // Keep handles in scope to maintain task references
        let _ = handles;

        // Wait forever (jobs run indefinitely)
        futures::future::pending::<()>().await;
        unreachable!()
    }

    async fn run_job(
        task_store: Arc<S>,
        pipeline_name: &'static str,
        interval_duration: Duration,
        query_fn: QueryFn,
        run_on_start: bool,
    ) {
        info!(
            pipeline = pipeline_name,
            interval_secs = interval_duration.as_secs(),
            run_on_start = run_on_start,
            "Starting scheduled job"
        );

        // Run immediately if configured
        if run_on_start {
            Self::execute_job(&task_store, pipeline_name, &query_fn).await;
        }

        let mut ticker = interval(interval_duration);
        ticker.tick().await; // Consume immediate first tick

        loop {
            ticker.tick().await;
            Self::execute_job(&task_store, pipeline_name, &query_fn).await;
        }
    }

    async fn execute_job(task_store: &Arc<S>, pipeline_name: &'static str, query_fn: &QueryFn) {
        debug!(pipeline = pipeline_name, "Executing scheduled job query");

        let items = query_fn().await;

        if items.is_empty() {
            debug!(pipeline = pipeline_name, "No items to enqueue");
            return;
        }

        info!(
            pipeline = pipeline_name,
            count = items.len(),
            "Enqueueing items"
        );

        for item in items {
            if let Err(e) = task_store.enqueue(pipeline_name, item).await {
                error!(
                    pipeline = pipeline_name,
                    error = %e,
                    "Failed to enqueue item"
                );
            }
        }
    }

    async fn run_trigger(
        task_store: Arc<S>,
        pipeline_name: &'static str,
        interval_duration: Duration,
        run_on_start: bool,
    ) {
        info!(
            pipeline = pipeline_name,
            interval_secs = interval_duration.as_secs(),
            "Starting trigger"
        );

        if run_on_start {
            Self::emit_trigger(&task_store, pipeline_name).await;
        }

        let mut ticker = interval(interval_duration);
        ticker.tick().await; // Consume immediate first tick

        loop {
            ticker.tick().await;
            Self::emit_trigger(&task_store, pipeline_name).await;
        }
    }

    async fn emit_trigger(task_store: &Arc<S>, pipeline_name: &'static str) {
        let now = chrono::Utc::now();
        let input = serde_json::to_value(now).expect("DateTime serializes");

        info!(pipeline = pipeline_name, "Emitting trigger");

        if let Err(e) = task_store.enqueue(pipeline_name, input).await {
            error!(
                pipeline = pipeline_name,
                error = %e,
                "Failed to emit trigger"
            );
        }
    }
}
