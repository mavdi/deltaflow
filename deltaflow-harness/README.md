# deltaflow-harness

Web-based pipeline visualization for [Deltaflow](https://crates.io/crates/deltaflow).

![Pipeline visualization](https://raw.githubusercontent.com/mavdi/deltaflow/master/docs/assets/example.png)

## Installation

```toml
[dependencies]
deltaflow = { version = "0.4", features = ["sqlite"] }
deltaflow-harness = "0.1"
```

## Usage

```rust
use deltaflow::{Pipeline, RunnerBuilder, SqliteTaskStore};
use deltaflow_harness::RunnerHarnessExt;

let runner = RunnerBuilder::new(store)
    .pipeline(my_pipeline)
    .with_visualizer(3000)  // Start web UI at http://localhost:3000
    .build();
```

Open `http://localhost:3000` in your browser to see your pipeline topology.

## Features

- Pipeline steps rendered as connected boxes
- Fork, fan-out, and dynamic spawn connections
- Interactive canvas with pan support
- Automatic layout for complex topologies

## Documentation

For full documentation, see [Deltaflow on docs.rs](https://docs.rs/deltaflow).
