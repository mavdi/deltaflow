//! Basic usage example for the Delta pipeline engine.
//!
//! This example demonstrates:
//! - Creating custom steps that implement the Step trait
//! - Building a pipeline with method chaining (.start_with, .then)
//! - Configuring retry policy
//! - Using NoopRecorder for simple scenarios
//! - Running the pipeline and handling results

use async_trait::async_trait;
use deltaflow::{HasEntityId, NoopRecorder, Pipeline, RetryPolicy, Step, StepError};

/// Input type that wraps a string and provides an entity ID.
///
/// The entity ID is used for tracking pipeline runs in the recorder.
#[derive(Clone, Debug)]
struct Input {
    id: String,
    value: String,
}

impl HasEntityId for Input {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

/// Intermediate type holding a parsed number.
#[derive(Clone, Debug)]
struct Number {
    id: String,
    value: i32,
}

impl HasEntityId for Number {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

/// Output type with a formatted message.
#[derive(Clone, Debug)]
struct Output {
    id: String,
    message: String,
}

impl HasEntityId for Output {
    fn entity_id(&self) -> String {
        self.id.clone()
    }
}

/// Step 1: Parse a string into a number.
///
/// This step demonstrates error handling:
/// - Invalid input returns a permanent error (not worth retrying)
/// - Valid input is parsed successfully
struct ParseStep;

#[async_trait]
impl Step for ParseStep {
    type Input = Input;
    type Output = Number;

    fn name(&self) -> &'static str {
        "parse"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        println!("[{}] Parsing input: {:?}", self.name(), input.value);

        match input.value.trim().parse::<i32>() {
            Ok(value) => {
                println!("[{}] Successfully parsed: {}", self.name(), value);
                Ok(Number {
                    id: input.id,
                    value,
                })
            }
            Err(e) => {
                // Parsing errors are permanent - retrying won't help
                Err(StepError::permanent(anyhow::anyhow!(
                    "Failed to parse '{}': {}",
                    input.value,
                    e
                )))
            }
        }
    }
}

/// Step 2: Double the number.
///
/// This is a simple transformation step that always succeeds.
struct DoubleStep;

#[async_trait]
impl Step for DoubleStep {
    type Input = Number;
    type Output = Number;

    fn name(&self) -> &'static str {
        "double"
    }

    async fn execute(&self, mut input: Self::Input) -> Result<Self::Output, StepError> {
        println!(
            "[{}] Doubling {} -> {}",
            self.name(),
            input.value,
            input.value * 2
        );
        input.value *= 2;
        Ok(input)
    }
}

/// Step 3: Format the result into a message.
///
/// This step demonstrates working with different output types.
struct FormatStep;

#[async_trait]
impl Step for FormatStep {
    type Input = Number;
    type Output = Output;

    fn name(&self) -> &'static str {
        "format"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, StepError> {
        let message = format!("The final result is: {}", input.value);
        println!("[{}] {}", self.name(), message);

        Ok(Output {
            id: input.id,
            message,
        })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Delta Pipeline Engine - Basic Example ===\n");

    // Build a pipeline that:
    // 1. Parses a string into a number
    // 2. Doubles the number
    // 3. Formats the result into a message
    //
    // The pipeline uses:
    // - Exponential backoff retry policy (3 attempts)
    // - NoopRecorder for simplicity (no persistent storage)
    let pipeline = Pipeline::new("basic_example")
        .start_with(ParseStep)
        .then(DoubleStep)
        .then(FormatStep)
        .with_retry(RetryPolicy::exponential(3))
        .with_recorder(NoopRecorder)
        .build();

    // Example 1: Successful execution
    println!("--- Example 1: Successful execution ---");
    let input = Input {
        id: "example-1".to_string(),
        value: "42".to_string(),
    };

    match pipeline.run(input).await {
        Ok(output) => {
            println!("\nSuccess! {}", output.message);
            println!("Entity ID: {}\n", output.entity_id());
        }
        Err(e) => {
            eprintln!("\nPipeline failed: {}\n", e);
        }
    }

    // Example 2: Invalid input (permanent failure)
    println!("--- Example 2: Invalid input (permanent failure) ---");
    let input = Input {
        id: "example-2".to_string(),
        value: "not a number".to_string(),
    };

    match pipeline.run(input).await {
        Ok(output) => {
            println!("\nSuccess! {}", output.message);
        }
        Err(e) => {
            eprintln!("\nExpected failure: {}\n", e);
        }
    }

    // Example 3: Different retry policy
    println!("--- Example 3: Using fixed retry policy ---");
    let pipeline_fixed = Pipeline::new("fixed_retry_example")
        .start_with(ParseStep)
        .then(DoubleStep)
        .then(FormatStep)
        .with_retry(RetryPolicy::fixed(5, std::time::Duration::from_millis(100)))
        .with_recorder(NoopRecorder)
        .build();

    let input = Input {
        id: "example-3".to_string(),
        value: "100".to_string(),
    };

    match pipeline_fixed.run(input).await {
        Ok(output) => {
            println!("\nSuccess! {}", output.message);
            println!("Pipeline name: {}\n", pipeline_fixed.name());
        }
        Err(e) => {
            eprintln!("\nPipeline failed: {}\n", e);
        }
    }

    println!("=== Examples complete ===");
    Ok(())
}
