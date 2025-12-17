//! Tests for Metadata struct and builder pattern.

use deltaflow::Metadata;

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
