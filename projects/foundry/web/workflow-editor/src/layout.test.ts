import { describe, expect, it } from 'vitest';
import {
  autoLayout,
  markerPositions,
  mergePositions,
} from './layout';
import { END_NODE, START_NODE } from './model';
import type { WorkflowBody } from './types';

function body(): WorkflowBody {
  return {
    schema_version: 1,
    roles: [],
    vars: {},
    initial_stage: 'draft',
    stages: [
      {
        name: 'draft',
        active_roles: [],
        vars: {},
        operations: [],
        events: [],
        created: [],
        transitions: [{ on: 'true', action: '', to: 'review' }],
      },
      {
        name: 'review',
        active_roles: [],
        vars: {},
        operations: [],
        events: [],
        created: [],
        transitions: [],
      },
    ],
  };
}

describe('layout nodes', () => {
  it('auto layout places stages and both markers', () => {
    const positions = autoLayout(body());
    expect(Object.keys(positions).sort()).toEqual(
      ['draft', 'review', START_NODE, END_NODE].sort(),
    );
    expect(positions[START_NODE].x).toBeLessThan(positions.draft.x);
    expect(positions[END_NODE].x).toBeGreaterThan(positions.review.x);
  });

  it('adds default marker positions when the stored layout predates them', () => {
    const positions = mergePositions(body(), {
      positions: {
        draft: { x: 80, y: 120 },
        review: { x: 460, y: 120 },
      },
    });
    expect(positions.draft).toEqual({ x: 80, y: 120 });
    expect(positions[START_NODE]).toEqual({ x: 80 - 220, y: 160 });
    expect(positions[END_NODE]).toEqual({ x: 460 + 300 + 160, y: 160 });
  });

  it('keeps stored marker positions instead of recomputing them', () => {
    const positions = mergePositions(body(), {
      positions: {
        draft: { x: 80, y: 120 },
        review: { x: 460, y: 120 },
        [START_NODE]: { x: 1, y: 2 },
        [END_NODE]: { x: 3, y: 4 },
      },
    });
    expect(positions[START_NODE]).toEqual({ x: 1, y: 2 });
    expect(positions[END_NODE]).toEqual({ x: 3, y: 4 });
  });

  it('moving a stage leaves marker positions unchanged', () => {
    const positions = mergePositions(body(), {
      positions: {
        draft: { x: 80, y: 120 },
        review: { x: 460, y: 120 },
        [START_NODE]: { x: 1, y: 2 },
        [END_NODE]: { x: 3, y: 4 },
      },
    });
    const before = markerPositions(body(), positions);
    positions.draft = { x: 1200, y: 900 };
    const after = markerPositions(body(), positions);
    expect(after).toEqual(before);
  });
});
