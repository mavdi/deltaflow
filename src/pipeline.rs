//! Pipeline builder and executor.

#![allow(private_bounds)]
#![allow(private_interfaces)]

use async_trait::async_trait;
use serde::Serialize;
use std::sync::Arc;
use thiserror::Error;

use crate::recorder::{NoopRecorder, Recorder, RunId, RunStatus, StepStatus};
use crate::retry::RetryPolicy;
use crate::step::{Step, StepError};

/// Type alias for spawn generator functions.
type SpawnGenerator<O> = Arc<dyn Fn(&O) -> Vec<serde_json::Value> + Send + Sync>;

/// A declaration of work to spawn after pipeline completion.
pub struct SpawnDeclaration<O> {
    pub(crate) target: &'static str,
    pub(crate) generator: SpawnGenerator<O>,
}

impl<O> Clone for SpawnDeclaration<O> {
    fn clone(&self) -> Self {
        Self {
            target: self.target,
            generator: self.generator.clone(),
        }
    }
}

/// Error returned by pipeline execution.
#[derive(Error, Debug)]
pub enum PipelineError {
    /// A step failed permanently.
    #[error("step '{step}' failed: {source}")]
    StepFailed {
        step: &'static str,
        #[source]
        source: anyhow::Error,
    },

    /// A step exhausted all retries.
    #[error("step '{step}' exhausted {attempts} retries: {source}")]
    RetriesExhausted {
        step: &'static str,
        attempts: u32,
        #[source]
        source: anyhow::Error,
    },

    /// Recording failed.
    #[error("recorder error: {0}")]
    RecorderError(#[from] anyhow::Error),
}

/// Trait for types that can provide an entity ID for recording.
pub trait HasEntityId {
    /// Returns the entity identifier for this input.
    fn entity_id(&self) -> String;
}

// Blanket impl for String
impl HasEntityId for String {
    fn entity_id(&self) -> String {
        self.clone()
    }
}

// Blanket impl for &str
impl HasEntityId for &str {
    fn entity_id(&self) -> String {
        self.to_string()
    }
}

/// Internal trait for boxed step execution.
#[async_trait]
pub trait BoxedStep<I, O>: Send + Sync {
    fn name(&self) -> &'static str;
    async fn execute(&self, input: I) -> Result<O, StepError>;
}

/// Wrapper to make any Step into a BoxedStep.
pub struct StepWrapper<S>(pub S);

#[async_trait]
impl<S> BoxedStep<S::Input, S::Output> for StepWrapper<S>
where
    S: Step,
{
    fn name(&self) -> &'static str {
        self.0.name()
    }

    async fn execute(&self, input: S::Input) -> Result<S::Output, StepError> {
        self.0.execute(input).await
    }
}

/// A chain of steps that transforms I -> O.
#[async_trait]
pub trait StepChain<I, O>: Send + Sync {
    async fn run(
        &self,
        input: I,
        run_id: RunId,
        recorder: &dyn Recorder,
        retry_policy: &RetryPolicy,
        start_index: u32,
    ) -> Result<O, PipelineError>;

    /// Returns the number of steps in this chain.
    fn step_count(&self) -> u32;
}

/// Terminal chain - identity transform.
pub struct Identity;

#[async_trait]
impl<T: Send + 'static> StepChain<T, T> for Identity {
    async fn run(
        &self,
        input: T,
        _run_id: RunId,
        _recorder: &dyn Recorder,
        _retry_policy: &RetryPolicy,
        _start_index: u32,
    ) -> Result<T, PipelineError> {
        Ok(input)
    }

    fn step_count(&self) -> u32 {
        0
    }
}

/// Chain that runs a step then continues with the rest.
pub struct ChainedStep<S, Next, I, M, O>
where
    S: BoxedStep<I, M>,
    Next: StepChain<M, O>,
{
    pub step: S,
    pub next: Next,
    pub _phantom: std::marker::PhantomData<(I, M, O)>,
}

#[async_trait]
impl<S, Next, I, M, O> StepChain<I, O> for ChainedStep<S, Next, I, M, O>
where
    I: Send + Sync + Clone + 'static,
    M: Send + Sync + 'static,
    O: Send + Sync + 'static,
    S: BoxedStep<I, M> + Send + Sync,
    Next: StepChain<M, O> + Send + Sync,
{
    async fn run(
        &self,
        input: I,
        run_id: RunId,
        recorder: &dyn Recorder,
        retry_policy: &RetryPolicy,
        start_index: u32,
    ) -> Result<O, PipelineError> {
        let step_name = self.step.name();
        let step_id = recorder.start_step(run_id, step_name, start_index).await?;

        // Execute with retry
        let mut attempt = 0u32;
        let output = loop {
            attempt += 1;
            match self.step.execute(input.clone()).await {
                Ok(output) => break output,
                Err(StepError::Permanent(e)) => {
                    recorder
                        .complete_step(
                            step_id,
                            StepStatus::Failed {
                                error: e.to_string(),
                                attempt,
                            },
                        )
                        .await?;
                    return Err(PipelineError::StepFailed {
                        step: step_name,
                        source: e,
                    });
                }
                Err(StepError::Retryable(e)) => {
                    if let Some(delay) = retry_policy.delay_for_attempt(attempt) {
                        tokio::time::sleep(delay).await;
                    } else {
                        recorder
                            .complete_step(
                                step_id,
                                StepStatus::Failed {
                                    error: e.to_string(),
                                    attempt,
                                },
                            )
                            .await?;
                        return Err(PipelineError::RetriesExhausted {
                            step: step_name,
                            attempts: attempt,
                            source: e,
                        });
                    }
                }
            }
        };

        recorder
            .complete_step(step_id, StepStatus::Completed)
            .await?;

        // Continue with next steps
        self.next
            .run(output, run_id, recorder, retry_policy, start_index + 1)
            .await
    }

    fn step_count(&self) -> u32 {
        1 + self.next.step_count()
    }
}

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
            spawn_declarations: Vec::new(),
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
            generator: Arc::new(move |output| {
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

/// Chain that runs first chain then a step.
pub struct ThenChain<First, S, I, M, O>
where
    First: StepChain<I, M>,
    S: BoxedStep<M, O>,
{
    pub first: First,
    pub step: S,
    pub _phantom: std::marker::PhantomData<(I, M, O)>,
}

#[async_trait]
impl<First, S, I, M, O> StepChain<I, O> for ThenChain<First, S, I, M, O>
where
    I: Send + Sync + Clone + 'static,
    M: Send + Sync + Clone + 'static,
    O: Send + Sync + 'static,
    First: StepChain<I, M> + Send + Sync,
    S: BoxedStep<M, O> + Send + Sync,
{
    async fn run(
        &self,
        input: I,
        run_id: RunId,
        recorder: &dyn Recorder,
        retry_policy: &RetryPolicy,
        start_index: u32,
    ) -> Result<O, PipelineError> {
        // Run first chain
        let mid = self
            .first
            .run(input, run_id, recorder, retry_policy, start_index)
            .await?;

        let next_index = start_index + self.first.step_count();

        let step_name = self.step.name();
        let step_id = recorder.start_step(run_id, step_name, next_index).await?;

        // Execute with retry
        let mut attempt = 0u32;
        let output = loop {
            attempt += 1;
            match self.step.execute(mid.clone()).await {
                Ok(output) => break output,
                Err(StepError::Permanent(e)) => {
                    recorder
                        .complete_step(
                            step_id,
                            StepStatus::Failed {
                                error: e.to_string(),
                                attempt,
                            },
                        )
                        .await?;
                    return Err(PipelineError::StepFailed {
                        step: step_name,
                        source: e,
                    });
                }
                Err(StepError::Retryable(e)) => {
                    if let Some(delay) = retry_policy.delay_for_attempt(attempt) {
                        tokio::time::sleep(delay).await;
                    } else {
                        recorder
                            .complete_step(
                                step_id,
                                StepStatus::Failed {
                                    error: e.to_string(),
                                    attempt,
                                },
                            )
                            .await?;
                        return Err(PipelineError::RetriesExhausted {
                            step: step_name,
                            attempts: attempt,
                            source: e,
                        });
                    }
                }
            }
        };

        recorder
            .complete_step(step_id, StepStatus::Completed)
            .await?;
        Ok(output)
    }

    fn step_count(&self) -> u32 {
        self.first.step_count() + 1
    }
}

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

impl<I, O, Chain> BuiltPipeline<I, O, Chain>
where
    I: Send + Clone + HasEntityId + 'static,
    O: Send + 'static,
    Chain: StepChain<I, O> + Send + Sync,
{
    /// Execute the pipeline with the given input.
    pub async fn run(&self, input: I) -> Result<O, PipelineError> {
        let entity_id = input.entity_id();
        let run_id = self.recorder.start_run(self.name, &entity_id).await?;

        match self
            .chain
            .run(input, run_id, self.recorder.as_ref(), &self.retry_policy, 0)
            .await
        {
            Ok(output) => {
                self.recorder
                    .complete_run(run_id, RunStatus::Completed)
                    .await?;
                Ok(output)
            }
            Err(e) => {
                self.recorder
                    .complete_run(
                        run_id,
                        RunStatus::Failed {
                            error: e.to_string(),
                        },
                    )
                    .await?;
                Err(e)
            }
        }
    }

    /// Get the pipeline name.
    pub fn name(&self) -> &'static str {
        self.name
    }

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
}
