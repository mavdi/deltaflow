// Configuration constants for the visualizer

export const COLORS = {
  background: '#000000',
  nodeFill: '#000000',
  nodeStroke: '#ffffff',
  text: '#ffffff',
  label: '#888888',
  edgeSequential: '#666666',
  edgeFork: '#ffffff',
  edgeCycle: '#ff6b6b',
  edgeActive: '#4ecdc4',  // Reserved for future animation
};

export const LAYOUT = {
  rankdir: 'LR',
  nodesep: 50,
  ranksep: 100,
  edgesep: 20,
  marginx: 60,
  marginy: 60,
};

export const NODE = {
  width: 100,
  height: 36,
  rx: 3,  // border radius
  fontSize: 11,
  maxLabelLength: 12,
};

export const EDGE = {
  sequentialWidth: 1,
  forkWidth: 2,
  arrowSize: 6,
};

export const PIPELINE_LABEL = {
  fontSize: 9,
  offsetY: -12,
};
