//! HTTP server for the visualizer.

use axum::{
    extract::State,
    response::{Html, Json},
    routing::get,
    Router,
};
use std::sync::Arc;

use crate::graph::GraphResponse;

/// Shared state for the visualizer server.
pub struct VisualizerState {
    pub graph: GraphResponse,
}

/// Create the router for the visualizer.
pub fn create_router(state: Arc<VisualizerState>) -> Router {
    Router::new()
        .route("/", get(serve_index))
        .route("/api/graph", get(serve_graph))
        .with_state(state)
}

async fn serve_index() -> Html<&'static str> {
    Html(include_str!("../assets/index.html"))
}

async fn serve_graph(State(state): State<Arc<VisualizerState>>) -> Json<GraphResponse> {
    Json(state.graph.clone())
}

/// Start the visualizer server.
pub async fn run_server(state: Arc<VisualizerState>, port: u16) {
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .expect("Failed to bind visualizer port");

    axum::serve(listener, app)
        .await
        .expect("Visualizer server error");
}
