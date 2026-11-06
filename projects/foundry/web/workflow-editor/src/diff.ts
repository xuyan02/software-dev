import {
  compactScript,
  effectKind,
  effectLabel,
  roleSummary,
  type Selection,
} from './model';
import type {
  EventSpec,
  OperationSpec,
  RoleSpec,
  TransitionSpec,
  VarSpec,
  WorkflowBody,
  WorkflowStage,
} from './types';

export type DiffKind = 'added' | 'removed' | 'changed';

export interface DiffEntry {
  kind: DiffKind;
  /** Where the change happened, e.g. `stage review`. */
  scope: string;
  /** What changed, e.g. `新增操作 finish：完成`. */
  text: string;
  /** Selecting the entry focuses the matching row in the read-only canvas. */
  selection: Selection | null;
}

interface ItemChange<T> {
  index: number;
  item: T;
}

interface PairedChange<T> {
  index: number;
  before: T;
  after: T;
}

interface ListChanges<T> {
  reordered: boolean;
  added: ItemChange<T>[];
  removed: ItemChange<T>[];
  changed: PairedChange<T>[];
}

function emptyChanges<T>(): ListChanges<T> {
  return { reordered: false, added: [], removed: [], changed: [] };
}

function sameSequence(left: string[], right: string[]): boolean {
  return (
    left.length === right.length &&
    left.every((value, index) => value === right[index])
  );
}

/**
 * AST lists have no ids, so a row is identified by its serialized value.
 * Values that only moved are reported once as a reorder; a removed and an
 * added value with the same key are paired as an edit of that row.
 */
function diffList<T>(
  before: T[],
  after: T[],
  keyOf: (item: T) => string | null,
): ListChanges<T> {
  const changes = emptyChanges<T>();
  const beforeValues = before.map((item) => JSON.stringify(item));
  const afterValues = after.map((item) => JSON.stringify(item));
  if (sameSequence(beforeValues, afterValues)) return changes;
  if (sameSequence([...beforeValues].sort(), [...afterValues].sort())) {
    changes.reordered = true;
    return changes;
  }

  const rows = beforeValues.length;
  const columns = afterValues.length;
  const table = Array.from({ length: rows + 1 }, () =>
    new Array<number>(columns + 1).fill(0),
  );
  for (let i = rows - 1; i >= 0; i -= 1) {
    for (let j = columns - 1; j >= 0; j -= 1) {
      table[i][j] =
        beforeValues[i] === afterValues[j]
          ? table[i + 1][j + 1] + 1
          : Math.max(table[i + 1][j], table[i][j + 1]);
    }
  }

  const removed: ItemChange<T>[] = [];
  const added: ItemChange<T>[] = [];
  let i = 0;
  let j = 0;
  while (i < rows && j < columns) {
    if (beforeValues[i] === afterValues[j]) {
      i += 1;
      j += 1;
    } else if (table[i + 1][j] >= table[i][j + 1]) {
      removed.push({ index: i, item: before[i] });
      i += 1;
    } else {
      added.push({ index: j, item: after[j] });
      j += 1;
    }
  }
  while (i < rows) {
    removed.push({ index: i, item: before[i] });
    i += 1;
  }
  while (j < columns) {
    added.push({ index: j, item: after[j] });
    j += 1;
  }

  const pairedRemoved = new Set<ItemChange<T>>();
  const pairedAdded = new Set<ItemChange<T>>();
  for (const addition of added) {
    const key = keyOf(addition.item);
    if (key === null) continue;
    const removal = removed.find(
      (entry) => !pairedRemoved.has(entry) && keyOf(entry.item) === key,
    );
    if (!removal) continue;
    pairedRemoved.add(removal);
    pairedAdded.add(addition);
    changes.changed.push({
      index: addition.index,
      before: removal.item,
      after: addition.item,
    });
  }
  changes.removed = removed.filter((entry) => !pairedRemoved.has(entry));
  changes.added = added.filter((entry) => !pairedAdded.has(entry));
  return changes;
}

function roleChanges(before: RoleSpec, after: RoleSpec): string[] {
  const changes: string[] = [];
  if (before.kind !== after.kind) {
    changes.push(`kind ${before.kind} → ${after.kind}`);
  }
  if (before.min_actors !== after.min_actors) {
    changes.push(`min_actors ${before.min_actors} → ${after.min_actors}`);
  }
  const beforeMax = before.max_actors ?? null;
  const afterMax = after.max_actors ?? null;
  if (beforeMax !== afterMax) {
    changes.push(
      `max_actors ${beforeMax ?? '不限'} → ${afterMax ?? '不限'}`,
    );
  }
  return changes;
}

function operationDetail(operation: OperationSpec): string {
  const parts = [operation.display_name].filter((part) => part !== '');
  parts.push(
    operation.roles.length > 0
      ? `角色 ${operation.roles.join('、')}`
      : '无角色',
  );
  parts.push(
    operation.action.trim() === ''
      ? '无 action'
      : `action ${compactScript(operation.action)}`,
  );
  return parts.join(' · ');
}

function eventDetail(event: EventSpec): string {
  return compactScript(event.action);
}

function transitionDetail(transition: TransitionSpec): string {
  const action =
    transition.action.trim() === ''
      ? ''
      : `，action ${compactScript(transition.action)}`;
  return `on ${compactScript(transition.on)}${action}`;
}

function initialText(spec: VarSpec): string {
  return spec.initial === undefined ? '默认' : JSON.stringify(spec.initial);
}

function varDetail(spec: VarSpec): string {
  return `${spec.type} · ${spec.display_name} · 初始 ${initialText(spec)}`;
}

function sameVar(before: VarSpec, after: VarSpec): boolean {
  return (
    before.type === after.type &&
    before.display_name === after.display_name &&
    before.initial === after.initial
  );
}

function diffRoles(
  before: WorkflowBody,
  after: WorkflowBody,
  entries: DiffEntry[],
): void {
  const beforeRoles = new Map(before.roles.map((role) => [role.name, role]));
  const afterRoles = new Set(after.roles.map((role) => role.name));
  after.roles.forEach((role, index) => {
    const previous = beforeRoles.get(role.name);
    if (!previous) {
      entries.push({
        kind: 'added',
        scope: '角色',
        text: `新增 ${role.name}（${roleSummary(role)}）`,
        selection: { kind: 'role', index },
      });
      return;
    }
    const changes = roleChanges(previous, role);
    if (changes.length > 0) {
      entries.push({
        kind: 'changed',
        scope: `角色 ${role.name}`,
        text: changes.join('，'),
        selection: { kind: 'role', index },
      });
    }
  });
  for (const role of before.roles) {
    if (!afterRoles.has(role.name)) {
      entries.push({
        kind: 'removed',
        scope: '角色',
        text: `删除 ${role.name}（${roleSummary(role)}）`,
        selection: null,
      });
    }
  }
}

function diffVars(
  before: WorkflowStage,
  after: WorkflowStage,
  entries: DiffEntry[],
  selectStage: Selection,
): void {
  for (const [name, spec] of Object.entries(after.vars)) {
    const previous = before.vars[name];
    if (!previous) {
      entries.push({
        kind: 'added',
        scope: `stage ${after.name}`,
        text: `新增变量 ${name}（${spec.type} · ${spec.display_name}）`,
        selection: selectStage,
      });
      continue;
    }
    if (!sameVar(previous, spec)) {
      entries.push({
        kind: 'changed',
        scope: `stage ${after.name}`,
        text: `修改变量 ${name}：${varDetail(previous)} → ${varDetail(spec)}`,
        selection: selectStage,
      });
    }
  }
  for (const name of Object.keys(before.vars)) {
    if (!(name in after.vars)) {
      entries.push({
        kind: 'removed',
        scope: `stage ${after.name}`,
        text: `删除变量 ${name}`,
        selection: selectStage,
      });
    }
  }
}

function diffBodyVars(
  before: WorkflowBody,
  after: WorkflowBody,
  entries: DiffEntry[],
): void {
  for (const [name, spec] of Object.entries(after.vars)) {
    const previous = before.vars[name];
    if (!previous) {
      entries.push({
        kind: 'added',
        scope: '任务变量',
        text: `新增变量 ${name}（${varDetail(spec)}）`,
        selection: { kind: 'taskvar', name },
      });
      continue;
    }
    if (!sameVar(previous, spec)) {
      entries.push({
        kind: 'changed',
        scope: '任务变量',
        text: `修改变量 ${name}：${varDetail(previous)} → ${varDetail(spec)}`,
        selection: { kind: 'taskvar', name },
      });
    }
  }
  for (const name of Object.keys(before.vars)) {
    if (!(name in after.vars)) {
      entries.push({
        kind: 'removed',
        scope: '任务变量',
        text: `删除变量 ${name}`,
        selection: null,
      });
    }
  }
}

function diffOperations(
  before: WorkflowStage,
  after: WorkflowStage,
  entries: DiffEntry[],
  selectStage: Selection,
): void {
  const scope = `stage ${after.name}`;
  const operations = diffList(before.operations, after.operations, (item) => item.name);
  if (operations.reordered) {
    entries.push({
      kind: 'changed',
      scope,
      text: `操作顺序变化：${before.operations
        .map((item) => item.name)
        .join('、')} → ${after.operations.map((item) => item.name).join('、')}`,
      selection: selectStage,
    });
  }
  for (const change of operations.changed) {
    entries.push({
      kind: 'changed',
      scope,
      text: `修改操作 ${change.after.name}：${operationDetail(change.before)} → ${operationDetail(change.after)}`,
      selection: { kind: 'operation', stage: after.name, index: change.index },
    });
  }
  for (const removal of operations.removed) {
    entries.push({
      kind: 'removed',
      scope,
      text: `删除操作 ${removal.item.name}：${operationDetail(removal.item)}`,
      selection: selectStage,
    });
  }
  for (const addition of operations.added) {
    entries.push({
      kind: 'added',
      scope,
      text: `新增操作 ${addition.item.name}：${operationDetail(addition.item)}`,
      selection: { kind: 'operation', stage: after.name, index: addition.index },
    });
  }
}

function diffEvents(
  before: WorkflowStage,
  after: WorkflowStage,
  entries: DiffEntry[],
  selectStage: Selection,
): void {
  const scope = `stage ${after.name}`;
  const events = diffList(before.events, after.events, (item) => item.event);
  for (const change of events.changed) {
    entries.push({
      kind: 'changed',
      scope,
      text: `修改事件 ${change.after.event}：${eventDetail(change.before)} → ${eventDetail(change.after)}`,
      selection: { kind: 'event', stage: after.name, index: change.index },
    });
  }
  for (const removal of events.removed) {
    entries.push({
      kind: 'removed',
      scope,
      text: `删除事件 ${removal.item.event}`,
      selection: selectStage,
    });
  }
  for (const addition of events.added) {
    entries.push({
      kind: 'added',
      scope,
      text: `新增事件 ${addition.item.event}：${eventDetail(addition.item)}`,
      selection: { kind: 'event', stage: after.name, index: addition.index },
    });
  }
}

function diffCreated(
  before: WorkflowStage,
  after: WorkflowStage,
  entries: DiffEntry[],
  selectStage: Selection,
): void {
  const scope = `stage ${after.name}`;
  const created = diffList(before.created, after.created, effectKind);
  for (const change of created.changed) {
    entries.push({
      kind: 'changed',
      scope,
      text: `修改进入动作：${effectLabel(change.before)} → ${effectLabel(change.after)}`,
      selection: { kind: 'created', stage: after.name, index: change.index },
    });
  }
  for (const removal of created.removed) {
    entries.push({
      kind: 'removed',
      scope,
      text: `删除进入动作：${effectLabel(removal.item)}`,
      selection: selectStage,
    });
  }
  for (const addition of created.added) {
    entries.push({
      kind: 'added',
      scope,
      text: `新增进入动作：${effectLabel(addition.item)}`,
      selection: { kind: 'created', stage: after.name, index: addition.index },
    });
  }
}

function diffTransitions(
  before: WorkflowStage,
  after: WorkflowStage,
  entries: DiffEntry[],
  selectStage: Selection,
): void {
  const scope = `stage ${after.name}`;
  const transitions = diffList(
    before.transitions,
    after.transitions,
    (item) => item.to,
  );
  if (transitions.reordered) {
    entries.push({
      kind: 'changed',
      scope,
      text: `转移顺序变化：${before.transitions
        .map((item) => item.to)
        .join('、')} → ${after.transitions.map((item) => item.to).join('、')}`,
      selection: selectStage,
    });
  }
  for (const change of transitions.changed) {
    entries.push({
      kind: 'changed',
      scope,
      text: `修改转移 → ${change.after.to}：${transitionDetail(change.before)} → ${transitionDetail(change.after)}`,
      selection: { kind: 'transition', stage: after.name, index: change.index },
    });
  }
  for (const removal of transitions.removed) {
    entries.push({
      kind: 'removed',
      scope,
      text: `删除转移 → ${removal.item.to}：${transitionDetail(removal.item)}`,
      selection: selectStage,
    });
  }
  for (const addition of transitions.added) {
    entries.push({
      kind: 'added',
      scope,
      text: `新增转移 → ${addition.item.to}：${transitionDetail(addition.item)}`,
      selection: { kind: 'transition', stage: after.name, index: addition.index },
    });
  }
}

function diffStage(
  before: WorkflowStage,
  after: WorkflowStage,
  entries: DiffEntry[],
): void {
  const scope = `stage ${after.name}`;
  const selectStage: Selection = { kind: 'stage', stage: after.name };

  const addedRoles = after.active_roles.filter(
    (role) => !before.active_roles.includes(role),
  );
  const removedRoles = before.active_roles.filter(
    (role) => !after.active_roles.includes(role),
  );
  if (addedRoles.length > 0 || removedRoles.length > 0) {
    const parts = [
      ...addedRoles.map((role) => `+${role}`),
      ...removedRoles.map((role) => `-${role}`),
    ];
    entries.push({
      kind: 'changed',
      scope,
      text: `激活角色 ${parts.join('、')}`,
      selection: selectStage,
    });
  }

  diffVars(before, after, entries, selectStage);
  diffOperations(before, after, entries, selectStage);
  diffEvents(before, after, entries, selectStage);
  diffCreated(before, after, entries, selectStage);
  diffTransitions(before, after, entries, selectStage);
}

/** Semantic changes from `before` to `after`; layout and titles are not part of a version. */
export function diffBodies(
  before: WorkflowBody,
  after: WorkflowBody,
): DiffEntry[] {
  const entries: DiffEntry[] = [];
  if (before.schema_version !== after.schema_version) {
    entries.push({
      kind: 'changed',
      scope: 'schema',
      text: `${before.schema_version} → ${after.schema_version}`,
      selection: null,
    });
  }
  diffRoles(before, after, entries);
  diffBodyVars(before, after, entries);
  if (before.initial_stage !== after.initial_stage) {
    entries.push({
      kind: 'changed',
      scope: '初始 stage',
      text: `${before.initial_stage ?? '未设置'} → ${after.initial_stage ?? '未设置'}`,
      selection: after.initial_stage
        ? { kind: 'stage', stage: after.initial_stage }
        : null,
    });
  }

  const beforeStages = new Map(before.stages.map((stage) => [stage.name, stage]));
  const afterStages = new Set(after.stages.map((stage) => stage.name));
  for (const stage of after.stages) {
    const previous = beforeStages.get(stage.name);
    if (!previous) {
      entries.push({
        kind: 'added',
        scope: 'stage',
        text: `新增 ${stage.name}`,
        selection: { kind: 'stage', stage: stage.name },
      });
      continue;
    }
    diffStage(previous, stage, entries);
  }
  for (const stage of before.stages) {
    if (!afterStages.has(stage.name)) {
      entries.push({
        kind: 'removed',
        scope: 'stage',
        text: `删除 ${stage.name}`,
        selection: null,
      });
    }
  }
  return entries;
}
