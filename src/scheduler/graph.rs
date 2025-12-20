use serde::Serialize;

/// A trigger node for visualization.
#[derive(Debug, Clone, Serialize)]
pub struct TriggerNode {
    pub name: String,
    pub target_pipeline: String,
    pub interval_seconds: u64,
    pub run_on_start: bool,
}
