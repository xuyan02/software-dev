import { describe, expect, it } from 'vitest';
import { autoLayout } from './layout';
import {
  END_NODE,
  START_NODE,
  deriveEdges,
  moveItem,
  parseTitle,
  renameStage,
  roleReferences,
  serializeTitle,
  stageReferences,
  uniqueOperationName,
  updateStage,
} from './model';
import type { WorkflowBody } from './types';

function body(): WorkflowBody {
  return {
    schema_version: 1,
    roles: [{ name: 'owner', kind: 'human', min_actors: 1, max_actors: null }],
    vars: { attempts: { type: 'int', display_name: '尝试次数', initial: 0 } },
    initial_stage: 'start',
    stages: [
      {
        name: 'start',
        active_roles: ['owner'],
        vars: { done: { type: 'bool', display_name: '完成' } },
        operations: [
          {
            name: 'finish',
            display_name: '完成',
            description: '',
            roles: ['owner'],
            action: 'done = true',
          },
        ],
        events: [{ event: 'child.state_changed', action: 'done = true' }],
        created: [
          {
            create_child: {
              workflow: 'iteration',
              title: "Iteration {count_children('iteration') + 1}",
            },
          },
        ],
        transitions: [
          { on: 'done', action: '', to: 'review' },
          { on: 'false', action: '', to: 'nowhere' },
          { on: 'approved', action: "result = 'approved'", to: 'ended' },
        ],
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

describe('title templates', () => {
  it('round-trips text, fields and counters', () => {
    const raw = "Iteration {count_children('iteration') + 1} · {task.title}";
    const segments = parseTitle(raw);
    expect(segments).not.toBeNull();
    expect(serializeTitle(segments!)).toBe(raw);
  });

  it('escapes literal braces', () => {
    const encoded = serializeTitle([{ kind: 'text', text: 'a{b}c' }]);
    expect(encoded).toBe('a{{b}}c');
    expect(parseTitle(encoded)).toEqual([{ kind: 'text', text: 'a{b}c' }]);
  });

  it('rejects expressions outside the title grammar', () => {
    expect(parseTitle('{task.state}')).toBeNull();
    expect(parseTitle('{trigger.child.workflow}')).toBeNull();
    expect(parseTitle('{}')).toBeNull();
    expect(parseTitle('{count_children(iteration)}')).toBeNull();
    expect(parseTitle('unclosed {')).toBeNull();
  });
});

describe('AST edits', () => {
  it('renaming a stage rewrites initial_stage and transition targets', () => {
    const renamed = renameStage(body(), 'review', 'qa');
    expect(renamed.initial_stage).toBe('start');
    expect(renamed.stages.map((stage) => stage.name)).toEqual(['start', 'qa']);
    expect(renamed.stages[0].transitions[0]).toEqual({
      on: 'done',
      action: '',
      to: 'qa',
    });
  });

  it('reports the references that block deleting a stage', () => {
    expect(stageReferences(body(), 'review')).toEqual(['start 的转移 1']);
    expect(stageReferences(body(), 'start')).toEqual(['初始 stage']);
  });

  it('reports the stages and operations that use a role', () => {
    expect(roleReferences(body(), 'owner')).toEqual([
      'start 的 active_roles',
      'start 的操作 finish',
    ]);
  });

  it('keeps row order when moving transitions', () => {
    const updated = updateStage(body(), 'start', (stage) => {
      stage.transitions.push({ on: 'true', action: '', to: 'review' });
      moveItem(stage.transitions, 3, -1);
    });
    expect(updated.stages[0].transitions.map((item) => item.to)).toEqual([
      'review',
      'nowhere',
      'review',
      'ended',
    ]);
  });
});

describe('derived graph', () => {
  it('derives start and transition edges, mapping ended to the end marker', () => {
    const edges = deriveEdges(body());
    expect(edges.find((edge) => edge.id === 'start')).toMatchObject({
      source: '__start',
      target: 'start',
    });
    const transition = edges.find((edge) => edge.id === 't:start:0');
    expect(transition).toMatchObject({ source: 'start', target: 'review' });
    expect(transition?.label).toBe('done');
    const ended = edges.find((edge) => edge.id === 't:start:2');
    expect(ended).toMatchObject({ source: 'start', target: '__end' });
    expect(ended?.label).toBe('approved');
  });

  it('does not draw edges to stages that do not exist', () => {
    expect(
      deriveEdges(body()).some((edge) => edge.target === 'nowhere'),
    ).toBe(false);
  });

  it('lays out every node, including stages added later', () => {
    const positions = autoLayout(body());
    expect(Object.keys(positions).sort()).toEqual(
      [START_NODE, END_NODE, 'review', 'start'].sort(),
    );
    for (const point of Object.values(positions)) {
      expect(Number.isFinite(point.x)).toBe(true);
      expect(Number.isFinite(point.y)).toBe(true);
    }
    expect(positions.review.x).toBeGreaterThan(positions.start.x);
    expect(positions[START_NODE].x).toBeLessThan(positions.start.x);
    expect(positions[END_NODE].x).toBeGreaterThan(positions.review.x);
  });
});

describe('operation names', () => {
  it('picks the first unused operation name in a stage', () => {
    expect(uniqueOperationName(body().stages[1])).toBe('operation_1');
    expect(uniqueOperationName(body().stages[0])).toBe('operation_1');

    const stage = body().stages[1];
    stage.operations.push({
      name: 'operation_1',
      display_name: '',
      description: '',
      roles: [],
      action: '',
    });
    expect(uniqueOperationName(stage)).toBe('operation_2');
  });
});
