//! Pipeline builder and executor.

use async_trait::async_trait;
use serde::Serialize;
use std::sync::Arc;
use thiserror::Error;

use crate::recorder::{NoopRecorder, Recorder, RunId, RunStatus, StepStatus};
use crate::retry::RetryPolicy;
use crate::step::{Step, StepError};

/// Type alias for fork predicate functions.
type ForkPredicate<O> = Arc<dyn Fn(&O) -> bool + Send + Sync>;

/// Type alias for dynamic spawn generator functions.
type SpawnGenerator<O> = Arc<dyn Fn(&O) -> Vec<serde_json::Value> + Send + Sync>;

use std::collections::HashMap;

/// Metadata for steps and spawn operations.
///
/// Used to attach descriptions and tags for visualization and analytics.
#[derive(Default, Clone, Debug, Serialize)]
pub struct Metadata {
    /// Human-readable description for visualization.
    pub description: Option<String>,
    /// Arbitrary key-value tags for filtering and analytics.
    pub tags: HashMap<String, String>,
}

impl Metadata {
    /// Create empty metadata.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the description.
    pub fn with_description(mut self, description: &str) -> Self {
        self.description = Some(description.to_string());
        self
    }

    /// Add a tag.
    pub fn with_tag(mut self, key: &str, value: &str) -> Self {
        self.tags.insert(key.to_string(), value.to_string());
        self
    }
}

/// A rule for spawning work after pipeline completion.
#[derive(Clone)]
pub enum SpawnRule<O> {
    /// Conditional fork: spawn to target if predicate returns true.
    Fork {
        target: &'static str,
        predicate: ForkPredicate<O>,
        metadata: Metadata,
    },
    /// Static fan-out: always spawn to these targets.
    FanOut {
        targets: Vec<&'static str>,
        metadata: Metadata,
    },
    /// Dynamic spawn: generate tasks from output.
    Dynamic {
        target: &'static str,
        generator: SpawnGenerator<O>,
        metadata: Metadata,
    },
}

/// Serializable representation of a pipeline's structure for visualization.
#[derive(Debug, Clone, Serialize)]
pub struct PipelineGraph {
    pub name: String,
    pub steps: Vec<StepNode>,
    pub forks: Vec<ForkNode>,
    pub fan_outs: Vec<FanOutNode>,
    pub emits: Vec<EmitNode>,
}

/// A step in the pipeline graph.
#[derive(Debug, Clone, Serialize)]
pub struct StepNode {
    pub name: String,
    pub index: usize,
    #[serde(flatten)]
    pub metadata: Metadata,
}

/// A conditional fork declaration.
#[derive(Debug, Clone, Serialize)]
pub struct ForkNode {
    pub target_pipeline: String,
    pub condition: String,
    #[serde(flatten)]
    pub metadata: Metadata,
}

/// A static fan-out declaration.
#[derive(Debug, Clone, Serialize)]
pub struct FanOutNode {
    pub targets: Vec<String>,
    #[serde(flatten)]
    pub metadata: Metadata,
}

/// A dynamic spawn (emit) declaration.
#[derive(Debug, Clone, Serialize)]
pub struct EmitNode {
    pub target_pipeline: String,
    #[serde(flatten)]
    pub metadata: Metadata,
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
#[doc(hidden)]
#[async_trait]
pub trait BoxedStep<I, O>: Send + Sync {
    fn name(&self) -> &'static str;
    async fn execute(&self, input: I) -> Result<O, StepError>;
}

/// Wrapper to make any Step into a BoxedStep.
#[doc(hidden)]
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
#[doc(hidden)]
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

    /// Collect step names in order.
    fn collect_step_names(&self, names: &mut Vec<&'static str>);
}

/// Terminal chain - identity transform.
#[doc(hidden)]
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

    fn collect_step_names(&self, _names: &mut Vec<&'static str>) {}
}

/// Chain that runs a step then continues with the rest.
#[doc(hidden)]
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

    fn collect_step_names(&self, names: &mut Vec<&'static str>) {
        names.push(self.step.name());
        self.next.collect_step_names(names);
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
    spawn_rules: Vec<SpawnRule<O>>,
    step_metadata: Vec<Metadata>,
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
            spawn_rules: Vec::new(),
            step_metadata: Vec::new(),
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
    ) -> StepBuilder<
        S::Input,
        S::Output,
        ChainedStep<StepWrapper<S>, Identity, S::Input, S::Output, S::Output>,
    >
    where
        S: Step + 'static,
    {
        let pipeline = Pipeline {
            name: self.name,
            chain: ChainedStep {
                step: StepWrapper(step),
                next: Identity,
                _phantom: std::marker::PhantomData,
            },
            retry_policy: self.retry_policy,
            recorder: self.recorder,
            spawn_rules: Vec::new(),
            step_metadata: vec![Metadata::default()],
            _phantom: std::marker::PhantomData,
        };
        StepBuilder {
            pipeline,
            step_index: 0,
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
            spawn_rules: Vec::new(),
            step_metadata: self.step_metadata,
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
    ///
    /// The generator function receives the pipeline output and returns
    /// a list of inputs to enqueue to the target pipeline.
    pub fn spawn_from<T, F>(mut self, target: &'static str, f: F) -> Self
    where
        T: Serialize + 'static,
        F: Fn(&O) -> Vec<T> + Send + Sync + 'static,
    {
        self.spawn_rules.push(SpawnRule::Dynamic {
            target,
            generator: Arc::new(move |output| {
                f(output)
                    .into_iter()
                    .filter_map(|item| serde_json::to_value(item).ok())
                    .collect()
            }),
            metadata: Metadata::default(),
        });
        self
    }

    /// Keep old name as alias for backward compatibility.
    #[deprecated(since = "0.4.0", note = "Use spawn_from instead")]
    pub fn spawns<T, F>(self, target: &'static str, f: F) -> Self
    where
        T: Serialize + 'static,
        F: Fn(&O) -> Vec<T> + Send + Sync + 'static,
    {
        self.spawn_from(target, f)
    }

    /// Conditionally fork to a target pipeline when predicate returns true.
    ///
    /// The output is serialized and sent to the target pipeline.
    /// Multiple forks can match - they are not mutually exclusive.
    pub fn fork_when<F>(mut self, predicate: F, target: &'static str) -> Self
    where
        F: Fn(&O) -> bool + Send + Sync + 'static,
    {
        self.spawn_rules.push(SpawnRule::Fork {
            target,
            predicate: Arc::new(predicate),
            metadata: Metadata::default(),
        });
        self
    }

    /// Conditionally fork with a custom description for visualization.
    pub fn fork_when_desc<F>(
        mut self,
        predicate: F,
        target: &'static str,
        description: &str,
    ) -> Self
    where
        F: Fn(&O) -> bool + Send + Sync + 'static,
    {
        self.spawn_rules.push(SpawnRule::Fork {
            target,
            predicate: Arc::new(predicate),
            metadata: Metadata::new().with_description(description),
        });
        self
    }

    /// Fan out to multiple target pipelines unconditionally.
    ///
    /// The output is serialized and sent to ALL specified targets.
    pub fn fan_out(mut self, targets: &[&'static str]) -> Self {
        self.spawn_rules.push(SpawnRule::FanOut {
            targets: targets.to_vec(),
            metadata: Metadata::default(),
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
            spawn_rules: self.spawn_rules,
            step_metadata: self.step_metadata,
            _phantom: std::marker::PhantomData,
        }
    }
}

/// Builder returned after adding a step, allowing metadata configuration.
pub struct StepBuilder<I, O, Chain>
where
    Chain: StepChain<I, O>,
{
    pipeline: Pipeline<I, O, Chain>,
    step_index: usize,
}

impl<I, O, Chain> StepBuilder<I, O, Chain>
where
    I: Send + Sync + Clone + 'static,
    O: Send + Sync + Clone + 'static,
    Chain: StepChain<I, O> + Send + Sync + 'static,
{
    /// Add a description to the last added step.
    pub fn desc(mut self, description: &str) -> Self {
        if let Some(meta) = self.pipeline.step_metadata.get_mut(self.step_index) {
            meta.description = Some(description.to_string());
        }
        self
    }

    /// Add a tag to the last added step.
    pub fn tag(mut self, key: &str, value: &str) -> Self {
        if let Some(meta) = self.pipeline.step_metadata.get_mut(self.step_index) {
            meta.tags.insert(key.to_string(), value.to_string());
        }
        self
    }

    /// Add another step to the pipeline.
    pub fn then<S>(self, step: S) -> StepBuilder<I, S::Output, impl StepChain<I, S::Output>>
    where
        S: Step<Input = O> + 'static,
    {
        let mut pipeline = Pipeline {
            name: self.pipeline.name,
            chain: ThenChain {
                first: self.pipeline.chain,
                step: StepWrapper(step),
                _phantom: std::marker::PhantomData,
            },
            retry_policy: self.pipeline.retry_policy,
            recorder: self.pipeline.recorder,
            spawn_rules: Vec::new(),
            step_metadata: self.pipeline.step_metadata,
            _phantom: std::marker::PhantomData,
        };
        pipeline.step_metadata.push(Metadata::default());
        let step_index = pipeline.step_metadata.len() - 1;
        StepBuilder { pipeline, step_index }
    }

    /// Set the retry policy.
    pub fn with_retry(mut self, policy: RetryPolicy) -> Self {
        self.pipeline.retry_policy = policy;
        self
    }

    /// Set the recorder.
    pub fn with_recorder<R: Recorder + 'static>(mut self, recorder: R) -> Self {
        self.pipeline.recorder = Arc::new(recorder);
        self
    }

    /// Conditionally fork to a target pipeline.
    // TODO: Task 5 - Return dedicated ForkBuilder instead of Self
    pub fn fork_when<F>(mut self, predicate: F, target: &'static str) -> Self
    where
        F: Fn(&O) -> bool + Send + Sync + 'static,
    {
        self.pipeline.spawn_rules.push(SpawnRule::Fork {
            target,
            predicate: Arc::new(predicate),
            metadata: Metadata::default(),
        });
        self
    }

    /// Conditionally fork with a custom description for visualization.
    pub fn fork_when_desc<F>(
        mut self,
        predicate: F,
        target: &'static str,
        description: &str,
    ) -> Self
    where
        F: Fn(&O) -> bool + Send + Sync + 'static,
    {
        self.pipeline.spawn_rules.push(SpawnRule::Fork {
            target,
            predicate: Arc::new(predicate),
            metadata: Metadata::new().with_description(description),
        });
        self
    }

    /// Fan out to multiple targets.
    // TODO: Task 5 - Return dedicated FanOutBuilder instead of Self
    pub fn fan_out(mut self, targets: &[&'static str]) -> Self {
        self.pipeline.spawn_rules.push(SpawnRule::FanOut {
            targets: targets.to_vec(),
            metadata: Metadata::default(),
        });
        self
    }

    /// Declare follow-up tasks to spawn on successful completion.
    pub fn spawn_from<T, F>(mut self, target: &'static str, f: F) -> Self
    where
        T: Serialize + 'static,
        F: Fn(&O) -> Vec<T> + Send + Sync + 'static,
    {
        self.pipeline.spawn_rules.push(SpawnRule::Dynamic {
            target,
            generator: Arc::new(move |output| {
                f(output)
                    .into_iter()
                    .filter_map(|item| serde_json::to_value(item).ok())
                    .collect()
            }),
            metadata: Metadata::default(),
        });
        self
    }

    /// Alias for spawn_from (emit).
    // TODO: Task 5 - Return dedicated EmitBuilder instead of Self
    pub fn emit<T, F>(self, target: &'static str, generator: F) -> Self
    where
        T: Serialize + 'static,
        F: Fn(&O) -> Vec<T> + Send + Sync + 'static,
    {
        self.spawn_from(target, generator)
    }

    /// Build the pipeline.
    pub fn build(self) -> BuiltPipeline<I, O, Chain> {
        BuiltPipeline {
            name: self.pipeline.name,
            chain: self.pipeline.chain,
            retry_policy: self.pipeline.retry_policy,
            recorder: self.pipeline.recorder,
            spawn_rules: self.pipeline.spawn_rules,
            step_metadata: self.pipeline.step_metadata,
            _phantom: std::marker::PhantomData,
        }
    }
}

/// Chain that runs first chain then a step.
#[doc(hidden)]
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

    fn collect_step_names(&self, names: &mut Vec<&'static str>) {
        self.first.collect_step_names(names);
        names.push(self.step.name());
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
    pub(crate) spawn_rules: Vec<SpawnRule<O>>,
    step_metadata: Vec<Metadata>,
    _phantom: std::marker::PhantomData<(I, O)>,
}

impl<I, O, Chain> BuiltPipeline<I, O, Chain>
where
    I: Send + Clone + HasEntityId + 'static,
    O: Send + Serialize + 'static,
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
        let mut spawned = Vec::new();

        for rule in &self.spawn_rules {
            match rule {
                SpawnRule::Fork {
                    target, predicate, ..
                } => {
                    if predicate(output) {
                        if let Ok(value) = serde_json::to_value(output) {
                            spawned.push((*target, value));
                        }
                    }
                }
                SpawnRule::FanOut { targets, .. } => {
                    if let Ok(value) = serde_json::to_value(output) {
                        for target in targets {
                            spawned.push((*target, value.clone()));
                        }
                    }
                }
                SpawnRule::Dynamic { target, generator, .. } => {
                    for input in generator(output) {
                        spawned.push((*target, input));
                    }
                }
            }
        }

        spawned
    }

    /// Export the pipeline structure as a graph for visualization.
    pub fn to_graph(&self) -> PipelineGraph {
        let mut step_names = Vec::new();
        self.chain.collect_step_names(&mut step_names);

        let steps: Vec<StepNode> = step_names
            .into_iter()
            .enumerate()
            .map(|(index, name)| StepNode {
                name: name.to_string(),
                index,
                metadata: self
                    .step_metadata
                    .get(index)
                    .cloned()
                    .unwrap_or_default(),
            })
            .collect();

        let mut forks = Vec::new();
        let mut fan_outs = Vec::new();
        let mut emits = Vec::new();

        for rule in &self.spawn_rules {
            match rule {
                SpawnRule::Fork {
                    target,
                    metadata,
                    ..
                } => {
                    forks.push(ForkNode {
                        target_pipeline: target.to_string(),
                        condition: metadata
                            .description
                            .clone()
                            .unwrap_or_else(|| format!("fork to {}", target)),
                        metadata: metadata.clone(),
                    });
                }
                SpawnRule::FanOut { targets, metadata } => {
                    fan_outs.push(FanOutNode {
                        targets: targets.iter().map(|s| s.to_string()).collect(),
                        metadata: metadata.clone(),
                    });
                }
                SpawnRule::Dynamic { target, metadata, .. } => {
                    emits.push(EmitNode {
                        target_pipeline: target.to_string(),
                        metadata: metadata.clone(),
                    });
                }
            }
        }

        PipelineGraph {
            name: self.name.to_string(),
            steps,
            forks,
            fan_outs,
            emits,
        }
    }
}
