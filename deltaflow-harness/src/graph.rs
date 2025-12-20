//! Graph transformation for the API response.

use deltaflow::{PipelineGraph, TriggerNode};
use serde::Serialize;

/// Full graph response for the API.
#[derive(Debug, Clone, Serialize)]
pub struct GraphResponse {
    pub pipelines: Vec<PipelineGraph>,
    pub triggers: Vec<TriggerNode>,
    pub connections: Vec<Connection>,
}

/// A connection between pipelines.
#[derive(Debug, Clone, Serialize)]
pub struct Connection {
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub connection_type: ConnectionType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionType {
    Fork,
    FanOut,
    DynamicSpawn,
    Trigger,
}

impl GraphResponse {
    pub fn from_graphs(graphs: Vec<PipelineGraph>) -> Self {
        Self::from_graphs_and_triggers(graphs, Vec::new())
    }

    pub fn from_graphs_and_triggers(graphs: Vec<PipelineGraph>, triggers: Vec<TriggerNode>) -> Self {
        let mut connections = Vec::new();

        // Add trigger connections
        for trigger in &triggers {
            connections.push(Connection {
                from: trigger.name.clone(),
                to: trigger.target_pipeline.clone(),
                connection_type: ConnectionType::Trigger,
                condition: None,
            });
        }

        for graph in &graphs {
            // Forks
            for fork in &graph.forks {
                connections.push(Connection {
                    from: graph.name.clone(),
                    to: fork.target_pipeline.clone(),
                    connection_type: ConnectionType::Fork,
                    condition: Some(fork.condition.clone()),
                });
            }

            // Fan-outs
            for fan_out in &graph.fan_outs {
                for target in &fan_out.targets {
                    connections.push(Connection {
                        from: graph.name.clone(),
                        to: target.clone(),
                        connection_type: ConnectionType::FanOut,
                        condition: None,
                    });
                }
            }

            // Emits (dynamic spawns)
            for spawn in &graph.emits {
                connections.push(Connection {
                    from: graph.name.clone(),
                    to: spawn.target_pipeline.clone(),
                    connection_type: ConnectionType::DynamicSpawn,
                    condition: None,
                });
            }
        }

        GraphResponse {
            pipelines: graphs,
            triggers,
            connections,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deltaflow::{ForkNode, Metadata, StepNode};

    #[test]
    fn test_from_graphs_extracts_connections() {
        let graphs = vec![
            PipelineGraph {
                name: "pipeline_a".to_string(),
                steps: vec![StepNode {
                    name: "Step1".to_string(),
                    index: 0,
                    metadata: Metadata::default(),
                }],
                forks: vec![ForkNode {
                    target_pipeline: "pipeline_b".to_string(),
                    condition: "always".to_string(),
                    metadata: Metadata::default(),
                }],
                fan_outs: vec![],
                emits: vec![],
            },
            PipelineGraph {
                name: "pipeline_b".to_string(),
                steps: vec![StepNode {
                    name: "Step2".to_string(),
                    index: 0,
                    metadata: Metadata::default(),
                }],
                forks: vec![],
                fan_outs: vec![],
                emits: vec![],
            },
        ];

        let response = GraphResponse::from_graphs(graphs);

        assert_eq!(response.pipelines.len(), 2);
        assert_eq!(response.triggers.len(), 0);
        assert_eq!(response.connections.len(), 1);
        assert_eq!(response.connections[0].from, "pipeline_a");
        assert_eq!(response.connections[0].to, "pipeline_b");
    }
}
