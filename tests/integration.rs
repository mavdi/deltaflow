//! Integration tests for delta pipeline.

use async_trait::async_trait;
use delta::{HasEntityId, NoopRecorder, Pipeline, RetryPolicy, Step, StepError};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

// Test input type
#[derive(Clone, Debug)]
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

// Step that fails N times then succeeds
struct FlakyStep {
    fail_count: AtomicU32,
    max_failures: u32,
}

impl FlakyStep {
    fn new(max_failures: u32) -> Self {
        Self {
            fail_count: AtomicU32::new(0),
            max_failures,
        }
    }
}

#[async_trait]
impl Step for FlakyStep {
    type Input = TestInput;
    type Output = TestInput;

    fn name(&self) -> &'static str {
        "flaky"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        let count = self.fail_count.fetch_add(1, Ordering::SeqCst);
        if count < self.max_failures {
            Err(StepError::retryable(anyhow::anyhow!("transient failure {}", count + 1)))
        } else {
            Ok(input)
        }
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
async fn test_simple_pipeline() {
    let pipeline = Pipeline::new("test")
        .start_with(DoubleStep)
        .then(AddTenStep)
        .with_recorder(NoopRecorder)
        .build();

    let input = TestInput {
        id: "test-1".to_string(),
        value: 5,
    };

    let result = pipeline.run(input).await.unwrap();
    assert_eq!(result.value, 20); // (5 * 2) + 10 = 20
}

#[tokio::test]
async fn test_pipeline_with_retry_success() {
    let flaky = Arc::new(FlakyStep::new(2)); // Fail twice, then succeed

    let pipeline = Pipeline::new("test_retry")
        .start_with(DoubleStep)
        .then(FlakyWrapper(flaky.clone()))
        .with_retry(RetryPolicy::fixed(3, std::time::Duration::from_millis(10)))
        .with_recorder(NoopRecorder)
        .build();

    let input = TestInput {
        id: "test-2".to_string(),
        value: 5,
    };

    let result = pipeline.run(input).await.unwrap();
    assert_eq!(result.value, 10); // 5 * 2 = 10 (flaky step doesn't modify)
}

// Wrapper to use Arc<FlakyStep>
struct FlakyWrapper(Arc<FlakyStep>);

#[async_trait]
impl Step for FlakyWrapper {
    type Input = TestInput;
    type Output = TestInput;

    fn name(&self) -> &'static str {
        self.0.name()
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        self.0.execute(input).await
    }
}

#[tokio::test]
async fn test_pipeline_permanent_failure() {
    let pipeline = Pipeline::new("test_perm_fail")
        .start_with(DoubleStep)
        .then(PermanentFailStep)
        .with_retry(RetryPolicy::exponential(3))
        .with_recorder(NoopRecorder)
        .build();

    let input = TestInput {
        id: "test-3".to_string(),
        value: 5,
    };

    let result = pipeline.run(input).await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(err.to_string().contains("permanent_fail"));
}

#[tokio::test]
async fn test_pipeline_retries_exhausted() {
    let flaky = Arc::new(FlakyStep::new(10)); // Fail 10 times

    let pipeline = Pipeline::new("test_exhausted")
        .start_with(FlakyWrapper(flaky))
        .with_retry(RetryPolicy::fixed(3, std::time::Duration::from_millis(1)))
        .with_recorder(NoopRecorder)
        .build();

    let input = TestInput {
        id: "test-4".to_string(),
        value: 5,
    };

    let result = pipeline.run(input).await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(err.to_string().contains("exhausted"));
}
