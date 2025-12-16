//! Graph transformation for the API response.

use deltaflow::PipelineGraph;
use serde::Serialize;

/// Full graph response for the API.
#[derive(Debug, Clone, Serialize)]
pub struct GraphResponse {
    pub pipelines: Vec<PipelineGraph>,
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
}

impl GraphResponse {
    pub fn from_graphs(graphs: Vec<PipelineGraph>) -> Self {
        let mut connections = Vec::new();

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

            // Dynamic spawns
            for spawn in &graph.dynamic_spawns {
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
            connections,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deltaflow::{ForkNode, StepNode};

    #[test]
    fn test_from_graphs_extracts_connections() {
        let graphs = vec![
            PipelineGraph {
                name: "pipeline_a".to_string(),
                steps: vec![StepNode { name: "Step1".to_string(), index: 0 }],
                forks: vec![ForkNode {
                    target_pipeline: "pipeline_b".to_string(),
                    condition: "always".to_string(),
                }],
                fan_outs: vec![],
                dynamic_spawns: vec![],
            },
            PipelineGraph {
                name: "pipeline_b".to_string(),
                steps: vec![StepNode { name: "Step2".to_string(), index: 0 }],
                forks: vec![],
                fan_outs: vec![],
                dynamic_spawns: vec![],
            },
        ];

        let response = GraphResponse::from_graphs(graphs);

        assert_eq!(response.pipelines.len(), 2);
        assert_eq!(response.connections.len(), 1);
        assert_eq!(response.connections[0].from, "pipeline_a");
        assert_eq!(response.connections[0].to, "pipeline_b");
    }
}
