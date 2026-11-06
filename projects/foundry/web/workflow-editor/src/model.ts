import type {
  CreatedEffect,
  EventKind,
  OperationSpec,
  RoleKind,
  RoleSpec,
  TransitionSpec,
  WorkflowBody,
  WorkflowStage,
} from './types';

export interface EditOptions {
  /** Merge consecutive edits of the same field into one undo step. */
  coalesce?: boolean;
  /** Human-readable undo label, e.g. "修改 action". */
  label?: string;
}

export type Selection =
  | { kind: 'stage'; stage: string }
  | { kind: 'operation'; stage: string; index: number }
  | { kind: 'event'; stage: string; index: number }
  | { kind: 'created'; stage: string; index: number }
  | { kind: 'transition'; stage: string; index: number }
  | { kind: 'role'; index: number }
  | { kind: 'taskvar'; name: string };

export const START_NODE = '__start';
export const END_NODE = '__end';

/** The virtual stage that completes a task. It is never a declared stage. */
export const ENDED_STAGE = 'ended';

/** Built-in task-level string variable, written on completion. */
export const RESULT_VARIABLE = 'result';

export const EVENT_KINDS: EventKind[] = [
  'task.created',
  'task.state_changed',
  'child.created',
  'child.state_changed',
];

export const EVENT_KIND_LABELS: Record<EventKind, string> = {
  'task.created': '任务创建',
  'task.state_changed': '任务状态变化',
  'child.created': '子任务创建',
  'child.state_changed': '子任务状态变化',
};

export const TASK_STATE_LABELS: Record<string, string> = {
  pending: 'pending（待处理）',
  in_progress: 'in_progress（进行中）',
};

export const ROLE_KIND_LABELS = {
  human: '人类',
  agent: '智能体',
  any: '不限',
} as const;

export function taskStateLabel(state: string): string {
  return TASK_STATE_LABELS[state] ?? state;
}

export function eventKindLabel(kind: EventKind): string {
  return `${kind}（${EVENT_KIND_LABELS[kind]}）`;
}

export function truncate(value: string, max = 40): string {
  return value.length > max ? `${value.slice(0, max - 1)}…` : value;
}

export function sameSelection(
  left: Selection | null,
  right: Selection | null,
): boolean {
  if (left === null || right === null) return left === right;
  if (left.kind !== right.kind) return false;
  switch (left.kind) {
    case 'role':
      return left.index === (right as { index: number }).index;
    case 'taskvar':
      return left.name === (right as { name: string }).name;
    default: {
      const other = right as { stage: string; index?: number };
      if (left.stage !== other.stage) return false;
      return (
        (left.kind === 'stage' && right.kind === 'stage') ||
        (left.kind !== 'stage' &&
          right.kind !== 'stage' &&
          'index' in left &&
          left.index === other.index)
      );
    }
  }
}

export function isKey(value: string): boolean {
  return /^[a-z][a-z0-9_]*$/.test(value);
}

export function findStage(
  body: WorkflowBody,
  name: string,
): WorkflowStage | undefined {
  return body.stages.find((stage) => stage.name === name);
}

export function updateStage(
  body: WorkflowBody,
  name: string,
  update: (stage: WorkflowStage) => void,
): WorkflowBody {
  return {
    ...body,
    stages: body.stages.map((stage) => {
      if (stage.name !== name) return stage;
      const copy = structuredClone(stage);
      update(copy);
      return copy;
    }),
  };
}

export function updateOperation(
  body: WorkflowBody,
  stage: string,
  index: number,
  update: (operation: OperationSpec) => void,
): WorkflowBody {
  return updateStage(body, stage, (current) => {
    const operation = current.operations[index];
    if (operation) update(operation);
  });
}

export function updateEvent(
  body: WorkflowBody,
  stage: string,
  index: number,
  update: (event: WorkflowStage['events'][number]) => void,
): WorkflowBody {
  return updateStage(body, stage, (current) => {
    const event = current.events[index];
    if (event) update(event);
  });
}

export function updateTransition(
  body: WorkflowBody,
  stage: string,
  index: number,
  update: (transition: TransitionSpec) => void,
): WorkflowBody {
  return updateStage(body, stage, (current) => {
    const transition = current.transitions[index];
    if (transition) update(transition);
  });
}

export function addOperation(
  body: WorkflowBody,
  stage: string,
  operation: OperationSpec,
): [WorkflowBody, number] {
  const index = findStage(body, stage)?.operations.length ?? 0;
  return [
    updateStage(body, stage, (value) => {
      value.operations.push(operation);
    }),
    index,
  ];
}

export function uniqueOperationName(stage: WorkflowStage | undefined): string {
  const used = new Set(stage?.operations.map((operation) => operation.name));
  for (let index = 1; ; index += 1) {
    const name = `operation_${index}`;
    if (!used.has(name)) return name;
  }
}

export function addEvent(
  body: WorkflowBody,
  stage: string,
  event: WorkflowStage['events'][number],
): [WorkflowBody, number] {
  const index = findStage(body, stage)?.events.length ?? 0;
  return [
    updateStage(body, stage, (value) => {
      value.events.push(event);
    }),
    index,
  ];
}

export function addCreated(
  body: WorkflowBody,
  stage: string,
  effect: CreatedEffect,
): [WorkflowBody, number] {
  const index = findStage(body, stage)?.created.length ?? 0;
  return [
    updateStage(body, stage, (value) => {
      value.created.push(effect);
    }),
    index,
  ];
}

export function addTransition(
  body: WorkflowBody,
  stage: string,
  transition: TransitionSpec,
): [WorkflowBody, number] {
  const index = findStage(body, stage)?.transitions.length ?? 0;
  return [
    updateStage(body, stage, (value) => {
      value.transitions.push(transition);
    }),
    index,
  ];
}

/**
 * Renames a stage and every reference to it. `initial_stage` and transition
 * targets are the only places a stage name appears in the AST.
 */
export function renameStage(
  body: WorkflowBody,
  from: string,
  to: string,
): WorkflowBody {
  return {
    ...body,
    initial_stage: body.initial_stage === from ? to : body.initial_stage,
    stages: body.stages.map((stage) => {
      const copy = structuredClone(stage);
      if (copy.name === from) copy.name = to;
      for (const transition of copy.transitions) {
        if (transition.to === from) transition.to = to;
      }
      return copy;
    }),
  };
}

export function uniqueStageName(body: WorkflowBody): string {
  for (let index = 1; ; index += 1) {
    const name = `stage_${index}`;
    if (!findStage(body, name)) return name;
  }
}

export function uniqueRoleName(body: WorkflowBody): string {
  for (let index = 1; ; index += 1) {
    const name = `role_${index}`;
    if (!body.roles.some((role) => role.name === name)) return name;
  }
}

/** Places a role is used, for safe deletion. */
export function roleReferences(body: WorkflowBody, name: string): string[] {
  const references: string[] = [];
  for (const stage of body.stages) {
    if (stage.active_roles.includes(name)) {
      references.push(`${stage.name} 的 active_roles`);
    }
    for (const operation of stage.operations) {
      if (operation.roles.includes(name)) {
        references.push(`${stage.name} 的操作 ${operation.name}`);
      }
    }
  }
  return references;
}

/** Places a stage name is referenced from, for safe deletion. */
export function stageReferences(body: WorkflowBody, name: string): string[] {
  const references: string[] = [];
  if (body.initial_stage === name) references.push('初始 stage');
  for (const stage of body.stages) {
    stage.transitions.forEach((transition, index) => {
      if (transition.to === name) {
        references.push(`${stage.name} 的转移 ${index + 1}`);
      }
    });
  }
  return references;
}

export function moveItem<T>(items: T[], index: number, delta: number): void {
  const target = index + delta;
  if (target < 0 || target >= items.length) return;
  const [item] = items.splice(index, 1);
  items.splice(target, 0, item);
}

type RowKind = 'operation' | 'event' | 'created' | 'transition';

/** Selection after deleting a row: next, then previous, then the stage. */
export function selectionAfterRowDelete(
  kind: RowKind,
  stage: string,
  index: number,
  count: number,
): Selection {
  if (count <= 1) return { kind: 'stage', stage };
  const next = index >= count - 1 ? count - 2 : index;
  return { kind, stage, index: next };
}

/** Selection after deleting a role. */
export function selectionAfterRoleDelete(
  index: number,
  count: number,
): Selection | null {
  if (count <= 1) return null;
  return { kind: 'role', index: index >= count - 1 ? count - 2 : index };
}

/** Selection after reordering a row; the object moves, not the selection. */
export function selectionAfterRowMove(
  kind: RowKind,
  stage: string,
  index: number,
  delta: number,
  count: number,
): Selection {
  const target = Math.min(Math.max(index + delta, 0), count - 1);
  return { kind, stage, index: target };
}

export function effectKind(
  effect: CreatedEffect,
): 'set_state' | 'create_child' {
  return 'set_state' in effect ? 'set_state' : 'create_child';
}

export function effectLabel(effect: CreatedEffect): string {
  if ('set_state' in effect) return `状态 → ${effect.set_state}`;
  return `创建子 Task ${effect.create_child.workflow}`;
}

export function compactScript(source: string): string {
  const text = source.trim().split('\n')[0]?.trim() ?? '';
  return text === '' ? '（空）' : text;
}

export interface FlowEdge {
  id: string;
  source: string;
  target: string;
  label: string;
  selection: Selection | null;
  /** Index among transitions with the same source and target. */
  parallelIndex: number;
  parallelCount: number;
}

export function deriveEdges(body: WorkflowBody): FlowEdge[] {
  const stages = new Set(body.stages.map((stage) => stage.name));
  const edges: FlowEdge[] = [];
  if (body.initial_stage && stages.has(body.initial_stage)) {
    edges.push({
      id: 'start',
      source: START_NODE,
      target: body.initial_stage,
      label: '开始',
      selection: { kind: 'stage', stage: body.initial_stage },
      parallelIndex: 0,
      parallelCount: 1,
    });
  }
  const groups = new Map<string, number>();
  for (const stage of body.stages) {
    stage.transitions.forEach((transition) => {
      const key = `${stage.name}\u0000${transition.to}`;
      groups.set(key, (groups.get(key) ?? 0) + 1);
    });
  }
  const seen = new Map<string, number>();
  for (const stage of body.stages) {
    stage.transitions.forEach((transition, index) => {
      const target =
        transition.to === ENDED_STAGE ? END_NODE : transition.to;
      if (target !== END_NODE && !stages.has(target)) return;
      const key = `${stage.name}\u0000${transition.to}`;
      const parallelIndex = seen.get(key) ?? 0;
      seen.set(key, parallelIndex + 1);
      edges.push({
        id: `t:${stage.name}:${index}`,
        source: stage.name,
        target,
        label: truncate(compactScript(transition.on), 40),
        selection: { kind: 'transition', stage: stage.name, index },
        parallelIndex,
        parallelCount: groups.get(key) ?? 1,
      });
    });
  }
  return edges;
}

export type TitleSegment =
  | { kind: 'text'; text: string }
  | { kind: 'field'; field: 'task.title' }
  | { kind: 'count'; workflow: string; offset: number };

const TITLE_FIELDS = ['task.title'] as const;

function parseTitleExpression(expression: string): TitleSegment | null {
  const value = expression.trim();
  const field = TITLE_FIELDS.find((candidate) => candidate === value);
  if (field) return { kind: 'field', field };
  if (!value.startsWith('count_children(')) return null;
  const rest = value.slice('count_children('.length);
  const close = rest.indexOf(')');
  if (close < 0) return null;
  const argument = rest.slice(0, close).trim();
  if (!argument.startsWith("'") || !argument.endsWith("'")) return null;
  const workflow = argument.slice(1, -1);
  if (!isKey(workflow)) return null;
  const tail = rest.slice(close + 1).trim();
  let offset = 0;
  if (tail !== '') {
    if (!tail.startsWith('+')) return null;
    const parsed = Number(tail.slice(1).trim());
    if (!Number.isInteger(parsed) || parsed < 0) return null;
    offset = parsed;
  }
  return { kind: 'count', workflow, offset };
}

export function parseTitle(raw: string): TitleSegment[] | null {
  const segments: TitleSegment[] = [];
  let text = '';
  let index = 0;
  while (index < raw.length) {
    const character = raw[index];
    if (character === '{' && raw[index + 1] === '{') {
      text += '{';
      index += 2;
      continue;
    }
    if (character === '}' && raw[index + 1] === '}') {
      text += '}';
      index += 2;
      continue;
    }
    if (character === '{') {
      const close = raw.indexOf('}', index + 1);
      if (close < 0) return null;
      if (text !== '') {
        segments.push({ kind: 'text', text });
        text = '';
      }
      const segment = parseTitleExpression(raw.slice(index + 1, close));
      if (!segment) return null;
      segments.push(segment);
      index = close + 1;
      continue;
    }
    if (character === '}') return null;
    text += character;
    index += 1;
  }
  if (text !== '') segments.push({ kind: 'text', text });
  return segments;
}

export function serializeTitle(segments: TitleSegment[]): string {
  return segments
    .map((segment) => {
      if (segment.kind === 'text') {
        return segment.text.replaceAll('{', '{{').replaceAll('}', '}}');
      }
      if (segment.kind === 'field') return `{${segment.field}}`;
      const offset = segment.offset === 0 ? '' : ` + ${segment.offset}`;
      return `{count_children('${segment.workflow}')${offset}}`;
    })
    .join('');
}

export const ROLE_KINDS: RoleKind[] = ['human', 'agent', 'any'];

/** Machine summary used by diffs and tests. */
export function roleSummary(role: RoleSpec): string {
  const actors =
    role.max_actors === null || role.max_actors === undefined
      ? `${role.min_actors}+`
      : `${role.min_actors}-${role.max_actors}`;
  return `${role.kind} · ${actors}`;
}

/** Human-readable summary used in panels. */
export function roleSummaryLabel(role: RoleSpec): string {
  const actors =
    role.max_actors === null || role.max_actors === undefined
      ? `至少 ${role.min_actors} 个 actor`
      : `${role.min_actors}–${role.max_actors} 个 actor`;
  return `${ROLE_KIND_LABELS[role.kind]} · ${actors}`;
}

export function effectDisplayLabel(effect: CreatedEffect): string {
  if ('set_state' in effect) return `设置状态 → ${taskStateLabel(effect.set_state)}`;
  return `创建子 Task「${effect.create_child.workflow}」`;
}

/** Names used by every stage, for checking task-variable collisions. */
export function stageVarNames(
  body: WorkflowBody,
  exceptStage?: string,
): string[] {
  const names = new Set<string>();
  for (const stage of body.stages) {
    if (stage.name === exceptStage) continue;
    for (const name of Object.keys(stage.vars)) names.add(name);
  }
  return [...names];
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function mentions(source: string, name: string): boolean {
  return new RegExp(`\\b${escapeRegExp(name)}\\b`).test(source);
}

/**
 * Script references to a variable. Stage variables are only visible inside
 * their own stage; task variables are visible everywhere.
 */
export function variableReferences(
  body: WorkflowBody,
  name: string,
  stageName?: string,
): string[] {
  const references: string[] = [];
  for (const stage of body.stages) {
    if (stageName !== undefined && stage.name !== stageName) continue;
    stage.operations.forEach((operation, index) => {
      if (mentions(operation.action, name)) {
        references.push(
          `${stage.name} 的操作 ${operation.name || index + 1} 的 action`,
        );
      }
    });
    stage.events.forEach((event, index) => {
      if (mentions(event.action, name)) {
        references.push(
          `${stage.name} 的事件 ${event.event}（第 ${index + 1} 个）的 action`,
        );
      }
    });
    stage.transitions.forEach((transition, index) => {
      if (mentions(transition.on, name)) {
        references.push(`${stage.name} 的转移 ${index + 1} 的 on`);
      }
      if (mentions(transition.action, name)) {
        references.push(`${stage.name} 的转移 ${index + 1} 的 action`);
      }
    });
  }
  return references;
}

export interface ReferenceTarget {
  label: string;
  selection: Selection;
}

/** Clickable places a stage is referenced from, for the delete guard. */
export function stageIncomingReferences(
  body: WorkflowBody,
  name: string,
): ReferenceTarget[] {
  const references: ReferenceTarget[] = [];
  for (const stage of body.stages) {
    stage.transitions.forEach((transition, index) => {
      if (transition.to === name) {
        references.push({
          label: `${stage.name} 的转移 ${index + 1}`,
          selection: { kind: 'transition', stage: stage.name, index },
        });
      }
    });
  }
  return references;
}

/** Clickable places a role is used. Scripts count as references too. */
export function roleReferenceTargets(
  body: WorkflowBody,
  name: string,
): ReferenceTarget[] {
  const references: ReferenceTarget[] = [];
  const script = `active_actors('${name}')`;
  for (const stage of body.stages) {
    if (stage.active_roles.includes(name)) {
      references.push({
        label: `${stage.name} 的 active_roles`,
        selection: { kind: 'stage', stage: stage.name },
      });
    }
    stage.operations.forEach((operation, index) => {
      if (operation.roles.includes(name)) {
        references.push({
          label: `${stage.name} 的操作 ${operation.name}`,
          selection: { kind: 'operation', stage: stage.name, index },
        });
      }
      if (operation.action.includes(script)) {
        references.push({
          label: `${stage.name} 的操作 ${operation.name} 的 action`,
          selection: { kind: 'operation', stage: stage.name, index },
        });
      }
    });
    stage.events.forEach((event, index) => {
      if (event.action.includes(script)) {
        references.push({
          label: `${stage.name} 的事件 ${event.event} 的 action`,
          selection: { kind: 'event', stage: stage.name, index },
        });
      }
    });
    stage.transitions.forEach((transition, index) => {
      if (
        transition.on.includes(script) ||
        transition.action.includes(script)
      ) {
        references.push({
          label: `${stage.name} 的转移 ${index + 1}`,
          selection: { kind: 'transition', stage: stage.name, index },
        });
      }
    });
  }
  return references;
}

export interface ApprovalPattern {
  variable: string;
  role: string;
  operation: string;
  target: string;
}

/**
 * Recognizes the RFC 0006 "all actors of a role approve" template. The label
 * is derived; editing any generated object can stop the match and the row
 * falls back to its raw expression.
 */
export function approvalPattern(
  stage: WorkflowStage,
  transition: TransitionSpec,
): ApprovalPattern | null {
  const match =
    /^\s*([a-z][a-z0-9_]*)\s*⊇\s*active_actors\('([a-z][a-z0-9_]*)'\)\s*$/.exec(
      transition.on,
    );
  if (!match) return null;
  const [, variable, role] = match;
  const spec = stage.vars[variable];
  if (!spec || spec.type !== 'actor_set') return null;
  const operation = stage.operations.find(
    (candidate) =>
      candidate.roles.length === 1 &&
      candidate.roles[0] === role &&
      candidate.action.trim() === `add event.actor to ${variable}`,
  );
  if (!operation) return null;
  return {
    variable,
    role,
    operation: operation.name,
    target: transition.to,
  };
}

function uniqueName(used: Set<string>, base: string): string {
  if (!used.has(base)) return base;
  for (let index = 2; ; index += 1) {
    const candidate = `${base}_${index}`;
    if (!used.has(candidate)) return candidate;
  }
}

function uniqueDisplayName(used: Set<string>, base: string): string {
  if (!used.has(base)) return base;
  for (let index = 2; ; index += 1) {
    const candidate = `${base} ${index}`;
    if (!used.has(candidate)) return candidate;
  }
}

export interface ApprovalTemplateInput {
  role: string;
  operation: string;
  target: string;
}

export interface ApprovalTemplateResult {
  body: WorkflowBody;
  variable: string;
  variableDisplayName: string;
  operation: string;
  transition: number;
}

/**
 * Generates the three ordinary AST objects described by RFC 0006 §7.5. There
 * is no template object in the definition; all three are editable afterwards.
 */
export function applyApprovalTemplate(
  body: WorkflowBody,
  stageName: string,
  input: ApprovalTemplateInput,
): ApprovalTemplateResult | null {
  const stage = findStage(body, stageName);
  if (!stage) return null;
  if (!stage.active_roles.includes(input.role)) {
    throw new Error('所选角色不在该 stage 的 active roles 中。');
  }
  const varUsed = new Set(Object.keys(body.vars));
  for (const other of body.stages) {
    for (const name of Object.keys(other.vars)) varUsed.add(name);
  }
  const variable = uniqueName(varUsed, 'approved');
  const displayUsed = new Set(
    Object.values(stage.vars).map((spec) => spec.display_name),
  );
  const variableDisplayName = uniqueDisplayName(displayUsed, '已批准');
  const operationUsed = new Set(stage.operations.map((item) => item.name));
  const operation = uniqueName(
    operationUsed,
    input.operation.trim() === '' ? 'review_approved' : input.operation.trim(),
  );
  const transition = stage.transitions.length;

  const next = updateStage(body, stageName, (value) => {
    value.vars[variable] = {
      type: 'actor_set',
      display_name: variableDisplayName,
    };
    value.operations.push({
      name: operation,
      display_name: '批准',
      description: `审核并批准；所有 ${input.role} 确认后进入 ${
        input.target === ENDED_STAGE ? '结束' : input.target
      }。`,
      roles: [input.role],
      action: `add event.actor to ${variable}`,
    });
    value.transitions.push({
      on: `${variable} ⊇ active_actors('${input.role}')`,
      action: '',
      to: input.target,
    });
  });
  return {
    body: next,
    variable,
    variableDisplayName,
    operation,
    transition,
  };
}

