//! Extension trait for RunnerBuilder.

use deltaflow::{RunnerBuilder, TaskStore};

/// Extension trait that adds visualization to RunnerBuilder.
pub trait RunnerVisualizerExt<S: TaskStore> {
    /// Enable the web visualizer on the given port.
    fn with_visualizer(self, port: u16) -> Self;
}
