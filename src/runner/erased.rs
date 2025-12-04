//! Type-erased pipeline wrapper for runtime dispatch.

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Arc;

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
    T: Serialize + Send + Sync,
    O: Send + Sync,
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
