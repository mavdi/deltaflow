// D3 rendering for the visualizer

import { COLORS, NODE, EDGE, PIPELINE_LABEL } from './config.js';
import { detectBackEdges, getPipelineLabels } from './graph.js';

/**
 * Render the graph to SVG.
 * @param {d3.Selection} svg - D3 selection of the SVG element
 * @param {dagre.graphlib.Graph} g - Graph after dagre.layout()
 * @param {Object} apiData - Original API data for labels
 */
export function render(svg, g, apiData) {
  // Clear previous content but preserve defs
  svg.selectAll(':not(defs)').remove();

  // Ensure arrow markers are defined (once)
  ensureMarkerDefs(svg);

  // Create container group for pan/zoom
  const container = svg.append('g').attr('class', 'container');

  // Detect back-edges for styling
  const backEdges = detectBackEdges(g);

  // Render edges first (behind nodes)
  renderEdges(container, g, backEdges);

  // Render nodes
  renderNodes(container, g);

  // Render pipeline labels
  renderPipelineLabels(container, g, apiData);

  // Set viewBox based on graph dimensions
  const graphData = g.graph();
  svg.attr('viewBox', `0 0 ${graphData.width || 800} ${graphData.height || 600}`);

  return container;
}

function renderNodes(container, g) {
  const nodes = g.nodes().map(id => ({
    id,
    ...g.node(id),
  }));

  const nodeGroup = container.append('g').attr('class', 'nodes');

  const nodeElements = nodeGroup
    .selectAll('g.node')
    .data(nodes)
    .enter()
    .append('g')
    .attr('class', 'node')
    .attr('transform', d => `translate(${d.x - NODE.width / 2}, ${d.y - NODE.height / 2})`);

  // Rectangle
  nodeElements
    .append('rect')
    .attr('width', NODE.width)
    .attr('height', NODE.height)
    .attr('rx', NODE.rx)
    .attr('fill', COLORS.nodeFill)
    .attr('stroke', COLORS.nodeStroke)
    .attr('stroke-width', 2);

  // Label
  nodeElements
    .append('text')
    .attr('x', NODE.width / 2)
    .attr('y', NODE.height / 2)
    .attr('text-anchor', 'middle')
    .attr('dominant-baseline', 'middle')
    .attr('fill', COLORS.text)
    .attr('font-size', NODE.fontSize)
    .attr('font-family', "'JetBrainsMono Nerd Font', monospace")
    .text(d => truncateLabel(d.label));
}

/**
 * Ensure arrow marker definitions exist at SVG level (created once).
 */
function ensureMarkerDefs(svg) {
  if (!svg.select('defs').empty()) return;

  const defs = svg.append('defs');

  // Fork arrow (white)
  defs.append('marker')
    .attr('id', 'arrow-fork')
    .attr('viewBox', '0 0 10 10')
    .attr('refX', 9)
    .attr('refY', 5)
    .attr('markerWidth', EDGE.arrowSize)
    .attr('markerHeight', EDGE.arrowSize)
    .attr('orient', 'auto')
    .append('path')
    .attr('d', 'M 0 0 L 10 5 L 0 10 z')
    .attr('fill', COLORS.edgeFork);

  // Cycle arrow (red)
  defs.append('marker')
    .attr('id', 'arrow-cycle')
    .attr('viewBox', '0 0 10 10')
    .attr('refX', 9)
    .attr('refY', 5)
    .attr('markerWidth', EDGE.arrowSize)
    .attr('markerHeight', EDGE.arrowSize)
    .attr('orient', 'auto')
    .append('path')
    .attr('d', 'M 0 0 L 10 5 L 0 10 z')
    .attr('fill', COLORS.edgeCycle);

  // Sequential arrow (gray)
  defs.append('marker')
    .attr('id', 'arrow-sequential')
    .attr('viewBox', '0 0 10 10')
    .attr('refX', 9)
    .attr('refY', 5)
    .attr('markerWidth', EDGE.arrowSize)
    .attr('markerHeight', EDGE.arrowSize)
    .attr('orient', 'auto')
    .append('path')
    .attr('d', 'M 0 0 L 10 5 L 0 10 z')
    .attr('fill', COLORS.edgeSequential);
}

function renderEdges(container, g, backEdges) {
  const edges = g.edges().map(e => ({
    id: `${e.v}->${e.w}`,
    source: e.v,
    target: e.w,
    points: g.edge(e).points,
    data: g.edge(e),
  }));

  const edgeGroup = container.append('g').attr('class', 'edges');

  // Line generator with smooth curves
  const lineGenerator = d3.line()
    .x(d => d.x)
    .y(d => d.y)
    .curve(d3.curveBasis);

  edges.forEach(edge => {
    const isBackEdge = backEdges.has(edge.id);
    const isSequential = edge.data.type === 'sequential';

    let strokeColor, strokeWidth, strokeDash, marker;

    if (isBackEdge) {
      strokeColor = COLORS.edgeCycle;
      strokeWidth = EDGE.forkWidth;
      strokeDash = '4,3';
      marker = 'url(#arrow-cycle)';
    } else if (isSequential) {
      strokeColor = COLORS.edgeSequential;
      strokeWidth = EDGE.sequentialWidth;
      strokeDash = 'none';
      marker = 'url(#arrow-sequential)';
    } else {
      strokeColor = COLORS.edgeFork;
      strokeWidth = EDGE.forkWidth;
      strokeDash = '4,3';
      marker = 'url(#arrow-fork)';
    }

    edgeGroup
      .append('path')
      .attr('class', `edge ${isSequential ? 'sequential' : 'fork'} ${isBackEdge ? 'back-edge' : ''}`)
      .attr('d', lineGenerator(edge.points))
      .attr('fill', 'none')
      .attr('stroke', strokeColor)
      .attr('stroke-width', strokeWidth)
      .attr('stroke-dasharray', strokeDash)
      .attr('marker-end', marker);

    // Condition label for fork edges
    if (!isSequential && edge.data.condition) {
      const midPoint = edge.points[Math.floor(edge.points.length / 2)];
      edgeGroup
        .append('text')
        .attr('x', midPoint.x)
        .attr('y', midPoint.y - 8)
        .attr('text-anchor', 'middle')
        .attr('fill', COLORS.label)
        .attr('font-size', 9)
        .attr('font-family', "'JetBrainsMono Nerd Font', monospace")
        .text(edge.data.condition);
    }
  });
}

function renderPipelineLabels(container, g, apiData) {
  const labels = getPipelineLabels(g, apiData);

  const labelGroup = container.append('g').attr('class', 'pipeline-labels');

  labelGroup
    .selectAll('text')
    .data(labels)
    .enter()
    .append('text')
    .attr('x', d => d.x)
    .attr('y', d => d.y + PIPELINE_LABEL.offsetY)
    .attr('text-anchor', 'middle')
    .attr('fill', COLORS.label)
    .attr('font-size', PIPELINE_LABEL.fontSize)
    .attr('font-family', "'JetBrainsMono Nerd Font', monospace")
    .text(d => d.name);
}

function truncateLabel(label) {
  if (label.length <= NODE.maxLabelLength) return label;
  return label.slice(0, NODE.maxLabelLength - 2) + '..';
}
