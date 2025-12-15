//! Tests demonstrating failure handling and branch isolation.
//!
//! These tests verify that failures in one branch don't affect siblings,
//! and that various error conditions are handled gracefully.

use async_trait::async_trait;
use deltaflow::{HasEntityId, NoopRecorder, Pipeline, RunnerBuilder, SqliteTaskStore, Step, StepError};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TestItem {
    id: u64,
    label: String,
}

impl HasEntityId for TestItem {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
}

struct PassthroughStep;

#[async_trait]
impl Step for PassthroughStep {
    type Input = TestItem;
    type Output = TestItem;

    fn name(&self) -> &'static str {
        "passthrough"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        Ok(input)
    }
}

struct RecordingStep {
    name: &'static str,
    recorded: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl Step for RecordingStep {
    type Input = TestItem;
    type Output = TestItem;

    fn name(&self) -> &'static str {
        self.name
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        self.recorded.lock().await.push(format!("{}:{}", self.name, input.id));
        Ok(input)
    }
}

struct AlwaysFailsStep {
    name: &'static str,
}

#[async_trait]
impl Step for AlwaysFailsStep {
    type Input = TestItem;
    type Output = TestItem;

    fn name(&self) -> &'static str {
        self.name
    }

    async fn execute(&self, _input: Self::Input) -> Result<Self::Output, StepError> {
        Err(StepError::Permanent(anyhow::anyhow!("always fails")))
    }
}

struct ConditionalFailStep {
    fail_on_label: String,
}

#[async_trait]
impl Step for ConditionalFailStep {
    type Input = TestItem;
    type Output = TestItem;

    fn name(&self) -> &'static str {
        "conditional_fail"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        if input.label == self.fail_on_label {
            return Err(StepError::Permanent(anyhow::anyhow!("conditional failure")));
        }
        Ok(input)
    }
}

#[tokio::test]
async fn test_step_failure_sibling_branches_continue() {
    // Demonstrates: when one fan-out branch fails, siblings still complete
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();

    let recorded = Arc::new(Mutex::new(Vec::new()));

    let main_pipeline = Pipeline::new("main")
        .start_with(PassthroughStep)
        .fan_out(&["branch_a", "branch_b", "branch_c"])
        .with_recorder(NoopRecorder)
        .build();

    let branch_a = Pipeline::new("branch_a")
        .start_with(RecordingStep {
            name: "a",
            recorded: recorded.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    // Branch B always fails
    let branch_b = Pipeline::new("branch_b")
        .start_with(AlwaysFailsStep { name: "b_fail" })
        .with_recorder(NoopRecorder)
        .build();

    let branch_c = Pipeline::new("branch_c")
        .start_with(RecordingStep {
            name: "c",
            recorded: recorded.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    let runner = RunnerBuilder::new(store)
        .pipeline(main_pipeline)
        .pipeline(branch_a)
        .pipeline(branch_b)
        .pipeline(branch_c)
        .poll_interval(Duration::from_millis(50))
        .max_concurrent(4)
        .build();

    runner
        .submit("main", TestItem { id: 1, label: "test".to_string() })
        .await
        .unwrap();

    tokio::select! {
        _ = runner.run() => {}
        _ = tokio::time::sleep(Duration::from_millis(500)) => {}
    }

    let recorded = recorded.lock().await;

    // A and C should have processed despite B failing
    assert!(recorded.contains(&"a:1".to_string()), "Branch A should complete");
    assert!(recorded.contains(&"c:1".to_string()), "Branch C should complete");
    assert_eq!(recorded.len(), 2, "Only A and C should record");
}

#[tokio::test]
async fn test_branch_isolation_downstream_failure() {
    // Demonstrates: failure in B's downstream (D) doesn't affect C's downstream (E)
    // A -> fan_out -> [B, C]
    // B -> D (fails)
    // C -> E (succeeds)
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    let store = SqliteTaskStore::new(pool);
    store.run_migrations().await.unwrap();

    let recorded = Arc::new(Mutex::new(Vec::new()));

    let pipeline_a = Pipeline::new("A")
        .start_with(PassthroughStep)
        .fan_out(&["B", "C"])
        .with_recorder(NoopRecorder)
        .build();

    let pipeline_b = Pipeline::new("B")
        .start_with(RecordingStep {
            name: "B",
            recorded: recorded.clone(),
        })
        .fan_out(&["D"])
        .with_recorder(NoopRecorder)
        .build();

    let pipeline_c = Pipeline::new("C")
        .start_with(RecordingStep {
            name: "C",
            recorded: recorded.clone(),
        })
        .fan_out(&["E"])
        .with_recorder(NoopRecorder)
        .build();

    // D always fails
    let pipeline_d = Pipeline::new("D")
        .start_with(AlwaysFailsStep { name: "D_fail" })
        .with_recorder(NoopRecorder)
        .build();

    // E always succeeds
    let pipeline_e = Pipeline::new("E")
        .start_with(RecordingStep {
            name: "E",
            recorded: recorded.clone(),
        })
        .with_recorder(NoopRecorder)
        .build();

    let runner = RunnerBuilder::new(store)
        .pipeline(pipeline_a)
        .pipeline(pipeline_b)
        .pipeline(pipeline_c)
        .pipeline(pipeline_d)
        .pipeline(pipeline_e)
        .poll_interval(Duration::from_millis(50))
        .max_concurrent(4)
        .build();

    runner
        .submit("A", TestItem { id: 1, label: "cascade".to_string() })
        .await
        .unwrap();

    tokio::select! {
        _ = runner.run() => {}
        _ = tokio::time::sleep(Duration::from_millis(800)) => {}
    }

    let recorded = recorded.lock().await;

    // B and C should both complete
    assert!(recorded.contains(&"B:1".to_string()), "Pipeline B should complete");
    assert!(recorded.contains(&"C:1".to_string()), "Pipeline C should complete");

    // E should complete even though D failed
    assert!(recorded.contains(&"E:1".to_string()), "Pipeline E should complete despite D failing");
}
