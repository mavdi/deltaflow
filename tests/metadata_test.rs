//! Tests for Metadata struct and builder pattern.

use deltaflow::{EmitNode, FanOutNode, ForkNode, Metadata, StepNode};

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
        metadata: Metadata::new().with_description("emit reason"),
    };
    assert_eq!(node.metadata.description, Some("emit reason".to_string()));
}
