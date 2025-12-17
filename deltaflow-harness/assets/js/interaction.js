// Pan and zoom interaction handlers

/**
 * Set up pan and zoom on the SVG.
 * @param {d3.Selection} svg - The SVG element
 * @param {d3.Selection} container - The container group to transform
 */
export function setupInteraction(svg, container) {
  const zoom = d3.zoom()
    .scaleExtent([0.1, 4])
    .on('zoom', (event) => {
      container.attr('transform', event.transform);
    });

  svg.call(zoom);

  // Initial centering
  const svgNode = svg.node();
  const bbox = container.node().getBBox();

  const svgWidth = svgNode.clientWidth || 800;
  const svgHeight = svgNode.clientHeight || 600;

  const scale = Math.min(
    svgWidth / (bbox.width + 100),
    svgHeight / (bbox.height + 100),
    1
  );

  const tx = (svgWidth - bbox.width * scale) / 2 - bbox.x * scale;
  const ty = (svgHeight - bbox.height * scale) / 2 - bbox.y * scale;

  svg.call(
    zoom.transform,
    d3.zoomIdentity.translate(tx, ty).scale(scale)
  );
}
