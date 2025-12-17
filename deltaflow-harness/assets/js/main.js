// Main entry point for the visualizer

import { buildGraph } from './graph.js';
import { render } from './render.js';
import { setupInteraction } from './interaction.js';

async function init() {
  const loadingEl = document.getElementById('loading');
  const svg = d3.select('#graph');

  try {
    // Fetch graph data
    const response = await fetch('/api/graph');
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }
    const apiData = await response.json();

    // Build Dagre graph
    const g = buildGraph(apiData);

    // Run Dagre layout
    dagre.layout(g);

    // Render to SVG
    const container = render(svg, g, apiData);

    // Setup pan/zoom
    setupInteraction(svg, container);

    // Hide loading indicator
    loadingEl.style.display = 'none';

  } catch (err) {
    loadingEl.textContent = `Error: ${err.message}`;
    console.error('Visualizer error:', err);
  }
}

// Run when DOM is ready
if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', init);
} else {
  init();
}
