// Transform API data to Dagre graph format

import { LAYOUT, NODE } from './config.js';

/**
 * Build a Dagre graph from the API response.
 * @param {Object} apiData - The /api/graph response
 * @returns {dagre.graphlib.Graph} - Configured Dagre graph
 */
export function buildGraph(apiData) {
  const g = new dagre.graphlib.Graph();

  g.setGraph({
    rankdir: LAYOUT.rankdir,
    nodesep: LAYOUT.nodesep,
    ranksep: LAYOUT.ranksep,
    edgesep: LAYOUT.edgesep,
    marginx: LAYOUT.marginx,
    marginy: LAYOUT.marginy,
  });

  g.setDefaultEdgeLabel(() => ({}));

  // Add nodes for each step
  apiData.pipelines.forEach(pipeline => {
    pipeline.steps.forEach(step => {
      const nodeId = `${pipeline.name}:${step.name}`;
      g.setNode(nodeId, {
        label: step.name,
        width: NODE.width,
        height: NODE.height,
        pipeline: pipeline.name,
        stepIndex: step.index,
      });
    });
  });

  // Add sequential edges (within pipelines)
  apiData.pipelines.forEach(pipeline => {
    for (let i = 0; i < pipeline.steps.length - 1; i++) {
      const from = `${pipeline.name}:${pipeline.steps[i].name}`;
      const to = `${pipeline.name}:${pipeline.steps[i + 1].name}`;
      g.setEdge(from, to, {
        type: 'sequential',
        pipeline: pipeline.name,
      });
    }
  });

  // Build lookup: pipeline name -> first step, last step
  const pipelineBounds = new Map();
  apiData.pipelines.forEach(pipeline => {
    if (pipeline.steps.length > 0) {
      pipelineBounds.set(pipeline.name, {
        first: `${pipeline.name}:${pipeline.steps[0].name}`,
        last: `${pipeline.name}:${pipeline.steps[pipeline.steps.length - 1].name}`,
      });
    }
  });

  // Add fork/fan-out edges (between pipelines)
  apiData.connections.forEach(conn => {
    const fromBounds = pipelineBounds.get(conn.from);
    const toBounds = pipelineBounds.get(conn.to);
    if (!fromBounds || !toBounds) return;

    const fromNode = fromBounds.last;
    const toNode = toBounds.first;

    g.setEdge(fromNode, toNode, {
      type: conn.type,
      condition: conn.condition,
      fromPipeline: conn.from,
      toPipeline: conn.to,
    });
  });

  return g;
}

/**
 * Detect back-edges (cycles) by comparing node ranks after layout.
 * @param {dagre.graphlib.Graph} g - Graph after dagre.layout()
 * @returns {Set<string>} - Set of edge IDs that are back-edges ("from->to")
 */
export function detectBackEdges(g) {
  const backEdges = new Set();

  g.edges().forEach(e => {
    const fromNode = g.node(e.v);
    const toNode = g.node(e.w);

    // In LR layout, x increases left-to-right
    // A back-edge goes from higher x to lower x (strictly)
    if (fromNode && toNode && fromNode.x > toNode.x) {
      backEdges.add(`${e.v}->${e.w}`);
    }
  });

  return backEdges;
}

/**
 * Get pipeline labels with positions (above first step of each pipeline).
 * @param {dagre.graphlib.Graph} g - Graph after layout
 * @param {Object} apiData - Original API data
 * @returns {Array} - [{name, x, y}, ...]
 */
export function getPipelineLabels(g, apiData) {
  const labels = [];

  apiData.pipelines.forEach(pipeline => {
    if (pipeline.steps.length === 0) return;

    const firstStepId = `${pipeline.name}:${pipeline.steps[0].name}`;
    const node = g.node(firstStepId);

    if (node) {
      labels.push({
        name: pipeline.name,
        x: node.x,
        y: node.y - NODE.height / 2,
      });
    }
  });

  return labels;
}
