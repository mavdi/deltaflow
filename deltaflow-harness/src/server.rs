//! HTTP server for the visualizer.

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{Html, IntoResponse, Json, Response},
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
        .route("/css/main.css", get(serve_css))
        .route("/js/config.js", get(serve_js_config))
        .route("/js/graph.js", get(serve_js_graph))
        .route("/js/render.js", get(serve_js_render))
        .route("/js/interaction.js", get(serve_js_interaction))
        .route("/js/main.js", get(serve_js_main))
        .with_state(state)
}

async fn serve_index() -> Html<&'static str> {
    Html(include_str!("../assets/index.html"))
}

async fn serve_graph(State(state): State<Arc<VisualizerState>>) -> Json<GraphResponse> {
    Json(state.graph.clone())
}

async fn serve_css() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/css")],
        include_str!("../assets/css/main.css"),
    )
        .into_response()
}

async fn serve_js_config() -> Response {
    serve_js(include_str!("../assets/js/config.js"))
}

async fn serve_js_graph() -> Response {
    serve_js(include_str!("../assets/js/graph.js"))
}

async fn serve_js_render() -> Response {
    serve_js(include_str!("../assets/js/render.js"))
}

async fn serve_js_interaction() -> Response {
    serve_js(include_str!("../assets/js/interaction.js"))
}

async fn serve_js_main() -> Response {
    serve_js(include_str!("../assets/js/main.js"))
}

fn serve_js(content: &'static str) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/javascript")],
        content,
    )
        .into_response()
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
