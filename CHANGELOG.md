# Changelog

## [0.5.0] - 2025-12-18

### Breaking Changes
- Removed `fork_when_desc()` - use chainable `.desc()` pattern instead: `.fork_when(...).desc(...)`
- Removed `spawn_from()` - use `emit()` instead for generating dynamic tasks
- Removed deprecated `spawns()` method

### Added
- `emit()` - New method for dynamic task generation, replacing `spawn_from()`
- Chainable `.desc()` method for adding descriptions to fork nodes for visualization

### Migration Guide
- Replace `fork_when_desc(pred, target, desc)` with `fork_when(pred, target).desc(desc)`
- Replace `spawn_from(target, fn)` with `emit(target, fn)`
- Replace deprecated `spawns(target, fn)` with `emit(target, fn)`

## [0.4.0] - 2025-12-17

### Added
- `fork_when()` - Conditional branching to target pipeline when predicate matches
- `fork_when_desc()` - Fork with custom description for visualization
- `fan_out()` - Static fan-out to multiple target pipelines
- `spawn_from()` - Renamed from `spawns()` for clarity
- `PipelineGraph` and `to_graph()` - Export pipeline structure for visualization
- `deltaflow-harness` companion crate - Web-based pipeline visualizer

### Changed
- `spawns()` deprecated in favor of `spawn_from()`
- Pipeline output types now require `Serialize` trait bound

### Fixed
- Recursive call in ErasedPipeline::name() method

## [0.3.0] - 2025-12-12

### Fixed
- Per-pipeline concurrency now independent of global limit
- Orphan task recovery on runner startup
- Step indexing in recorder

## [0.2.0] - 2025-12-08

### Added
- `PeriodicScheduler` for time-based task enqueueing
- Per-pipeline concurrency limits via `pipeline_with_concurrency()`

### Changed
- Renamed crate from `delta` to `deltaflow`
