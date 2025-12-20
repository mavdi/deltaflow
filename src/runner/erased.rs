//! Type-erased pipeline wrapper for runtime dispatch.

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::store::TaskError;
use crate::pipeline::{BuiltPipeline, HasEntityId, PipelineGraph, StepChain};

/// Type-erased pipeline that can be stored in a registry.
#[async_trait]
pub trait ErasedPipeline: Send + Sync {
    /// Get the pipeline name.
    fn name(&self) -> &'static str;

    /// Get the pipeline graph for visualization.
    fn to_graph(&self) -> PipelineGraph;

    /// Run the pipeline with JSON input, return spawned work.
    async fn run_erased(&self, input: serde_json::Value) -> Result<Vec<SpawnedTask>, TaskError>;
}

/// A task to be spawned after pipeline completion.
#[derive(Debug, Clone)]
pub struct SpawnedTask {
    pub pipeline: &'static str,
    pub input: serde_json::Value,
    pub delay: Option<std::time::Duration>,
}

#[async_trait]
impl<I, O, Chain> ErasedPipeline for BuiltPipeline<I, O, Chain>
where
    I: Send + Sync + Clone + HasEntityId + DeserializeOwned + 'static,
    O: Send + Sync + Serialize + 'static,
    Chain: StepChain<I, O> + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        BuiltPipeline::name(self)
    }

    fn to_graph(&self) -> PipelineGraph {
        BuiltPipeline::to_graph(self)
    }

    async fn run_erased(&self, input: serde_json::Value) -> Result<Vec<SpawnedTask>, TaskError> {
        let typed_input: I = serde_json::from_value(input)
            .map_err(|e| TaskError::DeserializationError(e.to_string()))?;

        let result = self.run(typed_input).await;

        let spawned: Vec<SpawnedTask> = self
            .get_spawned_from_result(&result)
            .into_iter()
            .map(|(pipeline, input, delay)| SpawnedTask { pipeline, input, delay })
            .collect();

        // Return the result (error if pipeline failed)
        result.map_err(|e| TaskError::PipelineError(e.to_string()))?;

        Ok(spawned)
    }
}
