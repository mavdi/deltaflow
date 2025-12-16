//! Web-based pipeline visualization for Deltaflow.
//!
//! Add `.with_visualizer(port)` to your RunnerBuilder to enable
//! a web UI showing pipeline topology.

pub mod server;
pub mod graph;

mod ext;
pub use ext::RunnerVisualizerExt;
