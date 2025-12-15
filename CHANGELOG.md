# Changelog

## [0.4.0] - 2025-12-15

### Added
- `fork_when()` - Conditional branching to target pipeline when predicate matches
- `fork_when_desc()` - Same as fork_when with custom description for visualization
- `fan_out()` - Static fan-out to multiple target pipelines
- `spawn_from()` - Renamed from `spawns()` for clarity
- `PipelineGraph` - Exportable pipeline structure for visualization
- `to_graph()` - Method to export pipeline structure as JSON-serializable graph

### Changed
- `spawns()` is now deprecated in favor of `spawn_from()`
- Pipeline output types now require `Serialize` trait bound
- Internal `SpawnDeclaration` replaced with `SpawnRule` enum

### Fixed
- Recursive call in ErasedPipeline::name() method
