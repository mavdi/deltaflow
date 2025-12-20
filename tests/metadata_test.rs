//! Tests for Metadata struct and builder pattern.

use async_trait::async_trait;
use deltaflow::{EmitNode, FanOutNode, ForkNode, Metadata, NoopRecorder, Pipeline, Step, StepError, StepNode};

#[test]
fn test_metadata_default_is_empty() {
    let meta = Metadata::default();
    assert!(meta.description.is_none());
    assert!(meta.tags.is_empty());
}

#[test]
fn test_metadata_with_description() {
    let meta = Metadata::new().with_description("test description");
    assert_eq!(meta.description, Some("test description".to_string()));
}

#[test]
fn test_metadata_with_tags() {
    let meta = Metadata::new()
        .with_tag("priority", "high")
        .with_tag("env", "prod");
    assert_eq!(meta.tags.get("priority"), Some(&"high".to_string()));
    assert_eq!(meta.tags.get("env"), Some(&"prod".to_string()));
}

#[test]
fn test_metadata_builder_chain() {
    let meta = Metadata::new()
        .with_description("my description")
        .with_tag("key", "value");
    assert_eq!(meta.description, Some("my description".to_string()));
    assert_eq!(meta.tags.get("key"), Some(&"value".to_string()));
}

#[test]
fn test_step_node_has_metadata() {
    let node = StepNode {
        name: "test".to_string(),
        index: 0,
        metadata: Metadata::new().with_description("test step"),
    };
    assert_eq!(node.metadata.description, Some("test step".to_string()));
}

#[test]
fn test_fork_node_has_metadata() {
    let node = ForkNode {
        target_pipeline: "target".to_string(),
        condition: "predicate".to_string(),
        metadata: Metadata::new().with_description("fork reason"),
    };
    assert_eq!(
        node.metadata.description,
        Some("fork reason".to_string())
    );
}

#[test]
fn test_fan_out_node_has_metadata() {
    let node = FanOutNode {
        targets: vec!["a".to_string(), "b".to_string()],
        metadata: Metadata::new().with_description("fan out reason"),
    };
    assert_eq!(
        node.metadata.description,
        Some("fan out reason".to_string())
    );
}

#[test]
fn test_emit_node_has_metadata() {
    let node = EmitNode {
        target_pipeline: "target".to_string(),
        delay_seconds: None,
        metadata: Metadata::new().with_description("emit reason"),
    };
    assert_eq!(node.metadata.description, Some("emit reason".to_string()));
}

struct DummyStep;

#[async_trait]
impl Step for DummyStep {
    type Input = String;
    type Output = String;
    fn name(&self) -> &'static str {
        "dummy"
    }
    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[test]
fn test_step_builder_desc() {
    let pipeline = Pipeline::new("test")
        .start_with(DummyStep)
        .desc("first step")
        .then(DummyStep)
        .desc("second step")
        .with_recorder(NoopRecorder)
        .build();

    let graph = pipeline.to_graph();
    assert_eq!(
        graph.steps[0].metadata.description,
        Some("first step".to_string())
    );
    assert_eq!(
        graph.steps[1].metadata.description,
        Some("second step".to_string())
    );
}

#[test]
fn test_step_builder_tag() {
    let pipeline = Pipeline::new("test")
        .start_with(DummyStep)
        .tag("priority", "high")
        .with_recorder(NoopRecorder)
        .build();

    let graph = pipeline.to_graph();
    assert_eq!(
        graph.steps[0].metadata.tags.get("priority"),
        Some(&"high".to_string())
    );
}

#[test]
fn test_step_builder_chain_metadata() {
    let pipeline = Pipeline::new("test")
        .start_with(DummyStep)
        .desc("step one")
        .tag("type", "input")
        .then(DummyStep)
        .desc("step two")
        .tag("type", "output")
        .with_recorder(NoopRecorder)
        .build();

    let graph = pipeline.to_graph();
    assert_eq!(
        graph.steps[0].metadata.description,
        Some("step one".to_string())
    );
    assert_eq!(
        graph.steps[0].metadata.tags.get("type"),
        Some(&"input".to_string())
    );
    assert_eq!(
        graph.steps[1].metadata.description,
        Some("step two".to_string())
    );
    assert_eq!(
        graph.steps[1].metadata.tags.get("type"),
        Some(&"output".to_string())
    );
}

#[test]
fn test_step_without_metadata_has_empty_metadata() {
    let pipeline = Pipeline::new("test")
        .start_with(DummyStep)
        .then(DummyStep)
        .with_recorder(NoopRecorder)
        .build();

    let graph = pipeline.to_graph();
    assert!(graph.steps[0].metadata.description.is_none());
    assert!(graph.steps[1].metadata.description.is_none());
}

#[test]
fn test_fork_builder_desc() {
    let pipeline = Pipeline::new("test")
        .start_with(DummyStep)
        .fork_when(|_: &String| true, "target")
        .desc("condition met")
        .with_recorder(NoopRecorder)
        .build();

    let graph = pipeline.to_graph();
    assert_eq!(graph.forks[0].condition, "condition met");
    assert_eq!(
        graph.forks[0].metadata.description,
        Some("condition met".to_string())
    );
}

#[test]
fn test_fork_builder_tag() {
    let pipeline = Pipeline::new("test")
        .start_with(DummyStep)
        .fork_when(|_: &String| true, "target")
        .tag("route", "primary")
        .with_recorder(NoopRecorder)
        .build();

    let graph = pipeline.to_graph();
    assert_eq!(
        graph.forks[0].metadata.tags.get("route"),
        Some(&"primary".to_string())
    );
}

#[test]
fn test_emit_builder_desc() {
    let pipeline = Pipeline::new("test")
        .start_with(DummyStep)
        .emit("target", |s: &String| vec![s.clone()])
        .desc("emit items")
        .with_recorder(NoopRecorder)
        .build();

    let graph = pipeline.to_graph();
    assert_eq!(
        graph.emits[0].metadata.description,
        Some("emit items".to_string())
    );
}

#[test]
fn test_fan_out_builder_desc() {
    let pipeline = Pipeline::new("test")
        .start_with(DummyStep)
        .fan_out(&["a", "b"])
        .desc("broadcast")
        .with_recorder(NoopRecorder)
        .build();

    let graph = pipeline.to_graph();
    assert_eq!(
        graph.fan_outs[0].metadata.description,
        Some("broadcast".to_string())
    );
}

#[test]
fn test_chained_spawn_builders() {
    let pipeline = Pipeline::new("test")
        .start_with(DummyStep)
        .fork_when(|_: &String| true, "t1")
        .desc("fork")
        .fan_out(&["t2"])
        .desc("fanout")
        .emit("t3", |s: &String| vec![s.clone()])
        .desc("emit")
        .with_recorder(NoopRecorder)
        .build();

    let graph = pipeline.to_graph();
    assert_eq!(graph.forks.len(), 1);
    assert_eq!(graph.fan_outs.len(), 1);
    assert_eq!(graph.emits.len(), 1);
}
