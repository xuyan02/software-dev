import { describe, expect, it } from 'vitest';
import { diffBodies } from './diff';
import type { WorkflowBody } from './types';

function body(): WorkflowBody {
  return {
    schema_version: 1,
    roles: [
      { name: 'owner', kind: 'human', min_actors: 1, max_actors: null },
    ],
    vars: {},
    initial_stage: 'start',
    stages: [
      {
        name: 'start',
        active_roles: ['owner'],
        vars: {},
        operations: [
          {
            name: 'finish',
            display_name: '完成',
            description: '',
            roles: ['owner'],
            action: 'done = true',
          },
        ],
        events: [],
        created: [{ set_state: 'in_progress' }],
        transitions: [{ on: 'done', action: '', to: 'review' }],
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

function stage(current: WorkflowBody, name: string) {
  const found = current.stages.find((item) => item.name === name);
  if (!found) throw new Error(`missing stage ${name}`);
  return found;
}

describe('version diff', () => {
  it('reports nothing for identical bodies', () => {
    expect(diffBodies(body(), body())).toEqual([]);
  });

  it('reports role changes, additions and removals', () => {
    const after = body();
    after.roles = [
      { name: 'owner', kind: 'agent', min_actors: 2, max_actors: 3 },
      { name: 'qa', kind: 'human', min_actors: 1, max_actors: null },
    ];
    const entries = diffBodies(body(), after);
    expect(entries).toEqual([
      {
        kind: 'changed',
        scope: '角色 owner',
        text: 'kind human → agent，min_actors 1 → 2，max_actors 不限 → 3',
        selection: { kind: 'role', index: 0 },
      },
      {
        kind: 'added',
        scope: '角色',
        text: '新增 qa（human · 1+）',
        selection: { kind: 'role', index: 1 },
      },
    ]);

    const removed = diffBodies(after, body());
    expect(removed).toContainEqual({
      kind: 'removed',
      scope: '角色',
      text: '删除 qa（human · 1+）',
      selection: null,
    });
  });

  it('reports stage additions, removals and a changed initial stage', () => {
    const after = body();
    after.stages = after.stages.filter((item) => item.name !== 'review');
    after.stages.push({
      name: 'qa',
      active_roles: [],
      vars: {},
      operations: [],
      events: [],
      created: [],
      transitions: [],
    });
    after.initial_stage = 'qa';

    const entries = diffBodies(body(), after);
    expect(entries).toContainEqual({
      kind: 'changed',
      scope: '初始 stage',
      text: 'start → qa',
      selection: { kind: 'stage', stage: 'qa' },
    });
    expect(entries).toContainEqual({
      kind: 'added',
      scope: 'stage',
      text: '新增 qa',
      selection: { kind: 'stage', stage: 'qa' },
    });
    expect(entries).toContainEqual({
      kind: 'removed',
      scope: 'stage',
      text: '删除 review',
      selection: null,
    });
  });

  it('reports active role changes on a stage', () => {
    const after = body();
    stage(after, 'start').active_roles = ['owner', 'qa'];
    expect(diffBodies(body(), after)).toEqual([
      {
        kind: 'changed',
        scope: 'stage start',
        text: '激活角色 +qa',
        selection: { kind: 'stage', stage: 'start' },
      },
    ]);
  });

  it('pairs a changed operation by name', () => {
    const after = body();
    stage(after, 'start').operations[0].display_name = '完成操作';
    const entries = diffBodies(body(), after);
    expect(entries).toEqual([
      {
        kind: 'changed',
        scope: 'stage start',
        text: '修改操作 finish：完成 · 角色 owner · action done = true → 完成操作 · 角色 owner · action done = true',
        selection: { kind: 'operation', stage: 'start', index: 0 },
      },
    ]);
  });

  it('reports events added and removed', () => {
    const after = body();
    stage(after, 'start').events = [{ event: 'task.created', action: '' }];
    const entries = diffBodies(body(), after);
    expect(entries).toContainEqual({
      kind: 'added',
      scope: 'stage start',
      text: '新增事件 task.created：（空）',
      selection: { kind: 'event', stage: 'start', index: 0 },
    });

    const removed = diffBodies(after, body());
    expect(removed).toContainEqual({
      kind: 'removed',
      scope: 'stage start',
      text: '删除事件 task.created',
      selection: { kind: 'stage', stage: 'start' },
    });
  });

  it('detects a pure transition reorder instead of reporting edits', () => {
    const before = body();
    stage(before, 'start').transitions = [
      { on: 'a', action: '', to: 'review' },
      { on: 'b', action: '', to: 'start' },
    ];
    const after = body();
    stage(after, 'start').transitions = [
      { on: 'b', action: '', to: 'start' },
      { on: 'a', action: '', to: 'review' },
    ];

    expect(diffBodies(before, after)).toEqual([
      {
        kind: 'changed',
        scope: 'stage start',
        text: '转移顺序变化：review、start → start、review',
        selection: { kind: 'stage', stage: 'start' },
      },
    ]);
  });

  it('pairs created effects by kind', () => {
    const after = body();
    stage(after, 'start').created = [{ set_state: 'pending' }];
    expect(diffBodies(body(), after)).toEqual([
      {
        kind: 'changed',
        scope: 'stage start',
        text: '修改进入动作：状态 → in_progress → 状态 → pending',
        selection: { kind: 'created', stage: 'start', index: 0 },
      },
    ]);
  });

  it('reports a transition action change', () => {
    const after = body();
    stage(after, 'start').transitions[0].action = "result = 'done'";
    expect(diffBodies(body(), after)).toEqual([
      {
        kind: 'changed',
        scope: 'stage start',
        text: "修改转移 → review：on done → on done，action result = 'done'",
        selection: { kind: 'transition', stage: 'start', index: 0 },
      },
    ]);
  });

  it('reports task variable changes', () => {
    const after = body();
    after.vars = {
      attempts: { type: 'int', display_name: '尝试次数', initial: 0 },
    };
    expect(diffBodies(body(), after)).toEqual([
      {
        kind: 'added',
        scope: '任务变量',
        text: '新增变量 attempts（int · 尝试次数 · 初始 0）',
        selection: { kind: 'taskvar', name: 'attempts' },
      },
    ]);
  });
});
