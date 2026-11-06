import dagre from '@dagrejs/dagre';
import { END_NODE, START_NODE, deriveEdges } from './model';
import type { WorkflowBody, WorkflowLayout } from './types';

export const STAGE_WIDTH = 300;
export const STAGE_HEIGHT = 44;
export const MARKER_WIDTH = 140;
export const MARKER_HEIGHT = 32;

interface Point {
  x: number;
  y: number;
}

/**
 * Runs dagre left-to-right over the derived graph and returns positions for
 * every canvas node, including the Start and End markers.
 */
export function autoLayout(body: WorkflowBody): Record<string, Point> {
  const graph = new dagre.graphlib.Graph();
  graph.setGraph({ rankdir: 'LR', nodesep: 48, ranksep: 96 });
  graph.setDefaultEdgeLabel(() => ({}));
  graph.setNode(START_NODE, { width: MARKER_WIDTH, height: MARKER_HEIGHT });
  graph.setNode(END_NODE, { width: MARKER_WIDTH, height: MARKER_HEIGHT });
  for (const stage of body.stages) {
    graph.setNode(stage.name, {
      width: STAGE_WIDTH,
      height: STAGE_HEIGHT,
    });
  }
  for (const edge of deriveEdges(body)) {
    if (edge.source !== edge.target) graph.setEdge(edge.source, edge.target);
  }
  dagre.layout(graph);

  const positions: Record<string, Point> = {};
  for (const stage of body.stages) {
    const node = graph.node(stage.name) as { x: number; y: number } | undefined;
    if (!node) continue;
    positions[stage.name] = {
      x: Math.round(node.x - STAGE_WIDTH / 2),
      y: Math.round(node.y - STAGE_HEIGHT / 2),
    };
  }
  // Dagre also ranks the markers, but their spacing is determined by the
  // stage grid so a disconnected End does not land on the origin.
  const markers = derivedMarkerPositions(body, positions);
  positions[START_NODE] = markers.start;
  positions[END_NODE] = markers.end;
  return positions;
}

function derivedMarkerPositions(
  body: WorkflowBody,
  positions: Record<string, Point>,
): { start: Point; end: Point } {
  const stagePoints = body.stages
    .map((stage) => positions[stage.name])
    .filter((point): point is Point => point !== undefined);
  if (stagePoints.length === 0) {
    return { start: { x: 0, y: 120 }, end: { x: 640, y: 120 } };
  }
  const minX = Math.min(...stagePoints.map((point) => point.x));
  const maxX = Math.max(...stagePoints.map((point) => point.x));
  const averageY =
    stagePoints.reduce((sum, point) => sum + point.y, 0) / stagePoints.length;
  const initial = body.stages.find(
    (stage) => stage.name === body.initial_stage,
  );
  const initialPoint = initial ? positions[initial.name] : undefined;
  return {
    start: { x: minX - 220, y: (initialPoint?.y ?? averageY) + 40 },
    end: { x: maxX + STAGE_WIDTH + 160, y: averageY + 40 },
  };
}

/**
 * Resolves the Start/End marker positions from the shared layout. When a
 * layout predates movable markers, both fall back to a deterministic anchor
 * derived from the stage positions.
 */
export function markerPositions(
  body: WorkflowBody,
  positions: Record<string, Point>,
): { start: Point; end: Point } {
  const storedStart = positions[START_NODE];
  const storedEnd = positions[END_NODE];
  if (storedStart && storedEnd) {
    return { start: storedStart, end: storedEnd };
  }
  const derived = derivedMarkerPositions(body, positions);
  return {
    start: storedStart ?? derived.start,
    end: storedEnd ?? derived.end,
  };
}

/**
 * Merges the persisted layout with computed positions for stages the layout
 * does not know about yet (new stages are laid out automatically).
 */
export function mergePositions(
  body: WorkflowBody,
  layout: WorkflowLayout,
): Record<string, Point> {
  const missingStages = body.stages
    .filter((stage) => !layout.positions[stage.name])
    .map((stage) => stage.name);
  const positions: Record<string, Point> = { ...layout.positions };
  if (missingStages.length > 0) {
    const computed = autoLayout(body);
    for (const name of missingStages) {
      const point = computed[name];
      if (point) positions[name] = point;
    }
  }
  // Marker defaults follow the resolved stage layout, which may include
  // positions from an older draft rather than a fresh dagre run.
  const derived = derivedMarkerPositions(body, positions);
  if (!positions[START_NODE]) positions[START_NODE] = derived.start;
  if (!positions[END_NODE]) positions[END_NODE] = derived.end;
  return positions;
}
