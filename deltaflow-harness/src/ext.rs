//! Extension trait for RunnerBuilder.

use std::sync::Arc;

use deltaflow::{PipelineGraph, RunnerBuilder, TaskStore, TriggerNode};

use crate::graph::GraphResponse;
use crate::server::{run_server, VisualizerState};

/// Extension trait that adds visualization to RunnerBuilder.
pub trait RunnerHarnessExt<S: TaskStore + 'static>: Sized {
    /// Enable the web visualizer on the given port.
    ///
    /// This spawns an HTTP server in the background that serves
    /// a web UI showing the pipeline topology.
    fn with_visualizer(self, port: u16) -> Self;

    /// Enable the web visualizer with trigger nodes from the scheduler.
    ///
    /// Use this when you have triggers configured in your scheduler
    /// that you want to visualize as source nodes in the graph.
    fn with_visualizer_and_triggers(self, port: u16, triggers: Vec<TriggerNode>) -> Self;
}

impl<S: TaskStore + 'static> RunnerHarnessExt<S> for RunnerBuilder<S> {
    fn with_visualizer(self, port: u16) -> Self {
        self.with_visualizer_and_triggers(port, Vec::new())
    }

    fn with_visualizer_and_triggers(self, port: u16, triggers: Vec<TriggerNode>) -> Self {
        // Extract graphs from registered pipelines
        let graphs: Vec<PipelineGraph> = self.get_pipeline_graphs();

        let state = Arc::new(VisualizerState {
            graph: GraphResponse::from_graphs_and_triggers(graphs, triggers),
        });

        // Spawn server in background
        tokio::spawn(async move {
            run_server(state, port).await;
        });

        self
    }
}
