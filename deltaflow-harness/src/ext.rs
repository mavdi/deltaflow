//! Extension trait for RunnerBuilder.

use std::sync::Arc;

use deltaflow::{PipelineGraph, RunnerBuilder, TaskStore};

use crate::graph::GraphResponse;
use crate::server::{run_server, VisualizerState};

/// Extension trait that adds visualization to RunnerBuilder.
pub trait RunnerHarnessExt<S: TaskStore + 'static>: Sized {
    /// Enable the web visualizer on the given port.
    ///
    /// This spawns an HTTP server in the background that serves
    /// a web UI showing the pipeline topology.
    fn with_visualizer(self, port: u16) -> Self;
}

impl<S: TaskStore + 'static> RunnerHarnessExt<S> for RunnerBuilder<S> {
    fn with_visualizer(self, port: u16) -> Self {
        // Extract graphs from registered pipelines
        let graphs: Vec<PipelineGraph> = self.get_pipeline_graphs();

        let state = Arc::new(VisualizerState {
            graph: GraphResponse::from_graphs(graphs),
        });

        // Spawn server in background
        tokio::spawn(async move {
            run_server(state, port).await;
        });

        self
    }
}
