//! Integration tests for SQLite recorder.

#![cfg(feature = "sqlite")]

use async_trait::async_trait;
use deltaflow::{HasEntityId, Pipeline, RetryPolicy, SqliteRecorder, Step, StepError};
use serde::Serialize;
use sqlx::sqlite::SqlitePoolOptions;

// Test input type
#[derive(Clone, Debug, Serialize)]
struct TestInput {
    id: String,
    value: u32,
}

impl HasEntityId for TestInput {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

// Step that doubles the value
struct DoubleStep;

#[async_trait]
impl Step for DoubleStep {
    type Input = TestInput;
    type Output = TestInput;

    fn name(&self) -> &'static str {
        "double"
    }

    async fn execute(&self, mut input: Self::Input) -> Result<Self::Output, StepError> {
        input.value *= 2;
        Ok(input)
    }
}

// Step that adds 10
struct AddTenStep;

#[async_trait]
impl Step for AddTenStep {
    type Input = TestInput;
    type Output = TestInput;

    fn name(&self) -> &'static str {
        "add_ten"
    }

    async fn execute(&self, mut input: Self::Input) -> Result<Self::Output, StepError> {
        input.value += 10;
        Ok(input)
    }
}

// Step that always fails permanently
struct PermanentFailStep;

#[async_trait]
impl Step for PermanentFailStep {
    type Input = TestInput;
    type Output = TestInput;

    fn name(&self) -> &'static str {
        "permanent_fail"
    }

    async fn execute(&self, _input: Self::Input) -> Result<Self::Output, StepError> {
        Err(StepError::permanent(anyhow::anyhow!("permanent failure")))
    }
}

#[tokio::test]
async fn successful_run_recording() {
    // Setup in-memory database
    let pool = SqlitePoolOptions::new()
        .connect(":memory:")
        .await
        .expect("Failed to create in-memory database");

    let recorder = SqliteRecorder::new(pool.clone());
    recorder
        .run_migrations()
        .await
        .expect("Failed to run migrations");

    // Build and run pipeline
    let pipeline = Pipeline::new("test_success")
        .start_with(DoubleStep)
        .then(AddTenStep)
        .with_recorder(recorder)
        .build();

    let input = TestInput {
        id: "entity-123".to_string(),
        value: 5,
    };

    let result = pipeline.run(input).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().value, 20); // (5 * 2) + 10 = 20

    // Verify delta_runs has status='completed'
    let run_status: String =
        sqlx::query_scalar("SELECT status FROM delta_runs WHERE pipeline_name = 'test_success'")
            .fetch_one(&pool)
            .await
            .expect("Failed to fetch run status");

    assert_eq!(run_status, "completed");

    // Verify entity_id is recorded
    let entity_id: String =
        sqlx::query_scalar("SELECT entity_id FROM delta_runs WHERE pipeline_name = 'test_success'")
            .fetch_one(&pool)
            .await
            .expect("Failed to fetch entity_id");

    assert_eq!(entity_id, "entity-123");

    // Verify all steps are marked completed
    let step_statuses: Vec<String> = sqlx::query_scalar(
        "SELECT status FROM delta_steps WHERE run_id = (SELECT id FROM delta_runs WHERE pipeline_name = 'test_success') ORDER BY step_index",
    )
    .fetch_all(&pool)
    .await
    .expect("Failed to fetch step statuses");

    assert_eq!(step_statuses.len(), 2);
    assert_eq!(step_statuses[0], "completed");
    assert_eq!(step_statuses[1], "completed");

    // Verify step names are recorded correctly
    let step_names: Vec<String> = sqlx::query_scalar(
        "SELECT step_name FROM delta_steps WHERE run_id = (SELECT id FROM delta_runs WHERE pipeline_name = 'test_success') ORDER BY step_index",
    )
    .fetch_all(&pool)
    .await
    .expect("Failed to fetch step names");

    assert_eq!(step_names[0], "double");
    assert_eq!(step_names[1], "add_ten");
}

#[tokio::test]
async fn failed_run_recording() {
    // Setup in-memory database
    let pool = SqlitePoolOptions::new()
        .connect(":memory:")
        .await
        .expect("Failed to create in-memory database");

    let recorder = SqliteRecorder::new(pool.clone());
    recorder
        .run_migrations()
        .await
        .expect("Failed to run migrations");

    // Build and run pipeline that will fail
    let pipeline = Pipeline::new("test_failure")
        .start_with(DoubleStep)
        .then(PermanentFailStep)
        .with_retry(RetryPolicy::fixed(2, std::time::Duration::from_millis(1)))
        .with_recorder(recorder)
        .build();

    let input = TestInput {
        id: "entity-456".to_string(),
        value: 5,
    };

    let result = pipeline.run(input).await;
    assert!(result.is_err());

    // Verify delta_runs has status='failed'
    let run_status: String =
        sqlx::query_scalar("SELECT status FROM delta_runs WHERE pipeline_name = 'test_failure'")
            .fetch_one(&pool)
            .await
            .expect("Failed to fetch run status");

    assert_eq!(run_status, "failed");

    // Verify error message is recorded
    let error_message: Option<String> = sqlx::query_scalar(
        "SELECT error_message FROM delta_runs WHERE pipeline_name = 'test_failure'",
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to fetch error message");

    assert!(error_message.is_some());
    assert!(error_message.unwrap().contains("permanent_fail"));

    // Verify the first step completed successfully
    let first_step_status: String = sqlx::query_scalar(
        "SELECT status FROM delta_steps WHERE run_id = (SELECT id FROM delta_runs WHERE pipeline_name = 'test_failure') AND step_index = 0",
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to fetch first step status");

    assert_eq!(first_step_status, "completed");

    // Verify the failing step is recorded as failed
    let failing_step_status: String = sqlx::query_scalar(
        "SELECT status FROM delta_steps WHERE run_id = (SELECT id FROM delta_runs WHERE pipeline_name = 'test_failure') AND step_index = 1",
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to fetch failing step status");

    assert_eq!(failing_step_status, "failed");

    // Verify the failing step has an error message
    let step_error: Option<String> = sqlx::query_scalar(
        "SELECT error_message FROM delta_steps WHERE run_id = (SELECT id FROM delta_runs WHERE pipeline_name = 'test_failure') AND step_index = 1",
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to fetch step error message");

    assert!(step_error.is_some());
    assert!(step_error.unwrap().contains("permanent failure"));
}

#[tokio::test]
async fn entity_id_recording() {
    // Setup in-memory database
    let pool = SqlitePoolOptions::new()
        .connect(":memory:")
        .await
        .expect("Failed to create in-memory database");

    let recorder = SqliteRecorder::new(pool.clone());
    recorder
        .run_migrations()
        .await
        .expect("Failed to run migrations");

    // Build and run pipeline
    let pipeline = Pipeline::new("test_entity_id")
        .start_with(DoubleStep)
        .with_recorder(recorder)
        .build();

    let input = TestInput {
        id: "unique-entity-789".to_string(),
        value: 42,
    };

    let result = pipeline.run(input).await;
    assert!(result.is_ok());

    // Verify entity_id from HasEntityId is correctly stored
    let stored_entity_id: String = sqlx::query_scalar(
        "SELECT entity_id FROM delta_runs WHERE pipeline_name = 'test_entity_id'",
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to fetch entity_id");

    assert_eq!(stored_entity_id, "unique-entity-789");

    // Verify the entity_id index works by querying with it
    let pipeline_name: String = sqlx::query_scalar(
        "SELECT pipeline_name FROM delta_runs WHERE entity_id = 'unique-entity-789'",
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to query by entity_id");

    assert_eq!(pipeline_name, "test_entity_id");
}
