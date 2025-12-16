use async_trait::async_trait;
use deltaflow::{
    HasEntityId, NoopRecorder, Pipeline, RunnerBuilder, SqliteTaskStore, Step, StepError,
};
use deltaflow_visualizer::RunnerVisualizerExt;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct TestInput {
    id: String,
}

impl HasEntityId for TestInput {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

struct TestStep;

#[async_trait]
impl Step for TestStep {
    type Input = TestInput;
    type Output = TestInput;

    fn name(&self) -> &'static str {
        "TestStep"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

#[tokio::test]
async fn test_visualizer_serves_graph() {
    // Setup
    let pool = sqlx::SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool.clone());
    store.run_migrations().await.unwrap();

    let pipeline = Pipeline::new("test_pipeline")
        .start_with(TestStep)
        .with_recorder(NoopRecorder)
        .build();

    // Build runner with visualizer
    let _runner = RunnerBuilder::new(store)
        .pipeline(pipeline)
        .with_visualizer(13370) // Use unusual port to avoid conflicts
        .build();

    // Give server time to start
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Fetch graph
    let response = reqwest::get("http://localhost:13370/api/graph")
        .await
        .expect("Failed to connect to visualizer");

    assert!(response.status().is_success());

    let body: serde_json::Value = response.json().await.unwrap();
    assert!(body["pipelines"].is_array());
    assert_eq!(body["pipelines"][0]["name"], "test_pipeline");
}
