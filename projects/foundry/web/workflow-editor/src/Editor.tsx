import type { Connection } from '@xyflow/react';
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { ApiError, api } from './api';
import { DraftSaver, type SaveState } from './autosave';
import { clearBackup, readBackup, type DraftBackup } from './backup';
import { FlowCanvas, type FocusRequest } from './canvas';
import { Inspector, type BodyUpdate } from './Inspector';
import {
  STAGE_HEIGHT,
  STAGE_WIDTH,
  autoLayout,
  mergePositions,
} from './layout';
import {
  ENDED_STAGE,
  END_NODE,
  START_NODE,
  addCreated,
  addOperation,
  addTransition,
  findStage,
  roleReferenceTargets,
  selectionAfterRoleDelete,
  selectionAfterRowDelete,
  uniqueOperationName,
  uniqueStageName,
  updateStage,
  type EditOptions,
  type Selection,
} from './model';
import { RolesPanel, TaskVarsPanel, ValidationPanel } from './panels';
import {
  emptyBody,
  emptyStage,
  type Validation,
  type WorkflowBody,
  type WorkflowDefinition,
  type WorkflowVersionInfo,
} from './types';

export const BASE = import.meta.env.BASE_URL;

interface StagePoint {
  x: number;
  y: number;
}

interface HistoryEntry {
  body: WorkflowBody;
  positions: Record<string, StagePoint>;
  label: string;
}

interface History {
  past: HistoryEntry[];
  future: HistoryEntry[];
  last: { label: string; time: number } | null;
}

interface FieldFocus {
  field: string;
  stage: string | null;
  token: number;
  selectAll: boolean;
}

interface Message {
  kind: 'ok' | 'error';
  text: string;
  link?: { href: string; label: string };
}

type LayoutState = 'saved' | 'saving' | 'failed';

const SAVE_LABELS: Record<SaveState, string> = {
  saved: '已保存',
  dirty: '未保存',
  saving: '保存中',
  retrying: '保存失败，重试中',
  failed: '保存失败',
  conflict: '冲突',
};

function errorText(error: unknown): string {
  if (error instanceof ApiError) {
    return `${error.message}（${error.code}）`;
  }
  return error instanceof Error ? error.message : String(error);
}

function timeText(value: Date): string {
  return value.toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
  });
}

function selectionExists(
  body: WorkflowBody,
  selection: Selection,
): boolean {
  if (selection.kind === 'role') return selection.index < body.roles.length;
  if (selection.kind === 'taskvar') return selection.name in body.vars;
  const stage = body.stages.find((item) => item.name === selection.stage);
  if (!stage) return false;
  switch (selection.kind) {
    case 'stage':
      return true;
    case 'operation':
      return selection.index < stage.operations.length;
    case 'event':
      return selection.index < stage.events.length;
    case 'created':
      return selection.index < stage.created.length;
    case 'transition':
      return selection.index < stage.transitions.length;
  }
}

function reducedMotion(): boolean {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

export function Editor({ workflow }: { workflow: string }) {
  const [status, setStatus] = useState<'loading' | 'ready' | 'error' | 'login'>(
    'loading',
  );
  const [error, setError] = useState('');
  const [definition, setDefinition] = useState<WorkflowDefinition | null>(null);
  const [versions, setVersions] = useState<WorkflowVersionInfo[]>([]);
  const [catalog, setCatalog] = useState<WorkflowDefinition[]>([]);
  const [body, setBody] = useState<WorkflowBody>(emptyBody());
  const [positions, setPositions] = useState<Record<string, StagePoint>>({});
  const [selection, setSelection] = useState<Selection | null>(null);
  const [saveState, setSaveState] = useState<SaveState>('saved');
  const [revision, setRevision] = useState(0);
  const [lastSavedAt, setLastSavedAt] = useState<Date | null>(null);
  const [saveError, setSaveError] = useState('');
  const [layoutState, setLayoutState] = useState<LayoutState>('saved');
  const [layoutSavedAt, setLayoutSavedAt] = useState<Date | null>(null);
  const [conflict, setConflict] = useState<{
    currentRevision: number | null;
  } | null>(null);
  const [recovery, setRecovery] = useState<DraftBackup | null>(null);
  const [validation, setValidation] = useState<Validation | null>(null);
  const [validationBody, setValidationBody] = useState<string | null>(null);
  const [validationError, setValidationError] = useState('');
  const [validating, setValidating] = useState(false);
  const [message, setMessage] = useState<Message | null>(null);
  const [publishing, setPublishing] = useState(false);
  const [focusRequest, setFocusRequest] = useState<FocusRequest | null>(null);
  const [fieldFocus, setFieldFocus] = useState<FieldFocus | null>(null);
  const [fitRequest, setFitRequest] = useState(0);
  const [pickerStage, setPickerStage] = useState<string | null>(null);
  const [pickerTarget, setPickerTarget] = useState(ENDED_STAGE);
  const [, bumpHistory] = useState(0);

  const readOnly = definition !== null && !definition.editable;

  const bodyRef = useRef(body);
  const positionsRef = useRef<Record<string, StagePoint>>({});
  const saverRef = useRef<DraftSaver | null>(null);
  const historyRef = useRef<History>({ past: [], future: [], last: null });
  const layoutTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const validationToken = useRef(0);
  const focusToken = useRef(0);
  const fieldToken = useRef(0);
  const nudgeRef = useRef<{ label: string; time: number } | null>(null);
  const sidebarRef = useRef<HTMLElement | null>(null);
  const layoutKeepalive = useRef<() => void>(() => {});

  const runValidation = useCallback(
    (candidate: WorkflowBody) => {
      const token = (validationToken.current += 1);
      setValidating(true);
      setValidationError('');
      api
        .validate(workflow, candidate)
        .then((result) => {
          if (validationToken.current !== token) return;
          setValidation(result);
          setValidationBody(JSON.stringify(candidate));
          setValidating(false);
        })
        .catch((failure: unknown) => {
          if (validationToken.current !== token) return;
          setValidationError(errorText(failure));
          setValidating(false);
        });
    },
    [workflow],
  );

  function record(label: string, coalesce = false): void {
    const history = historyRef.current;
    const now = Date.now();
    if (
      coalesce &&
      history.last !== null &&
      history.last.label === label &&
      now - history.last.time < 800
    ) {
      history.last.time = now;
      return;
    }
    history.past.push({
      body: bodyRef.current,
      positions: positionsRef.current,
      label,
    });
    if (history.past.length > 100) history.past.shift();
    history.future = [];
    history.last = coalesce ? { label, time: now } : null;
    bumpHistory((value) => value + 1);
  }

  function ensurePositions(next: WorkflowBody): void {
    const missing = next.stages.filter(
      (stage) => !positionsRef.current[stage.name],
    );
    if (missing.length === 0) return;
    const computed = autoLayout(next);
    const merged = { ...positionsRef.current };
    for (const stage of missing) {
      const point = computed[stage.name];
      if (point) merged[stage.name] = point;
    }
    positionsRef.current = merged;
    setPositions(merged);
  }

  function showBody(next: WorkflowBody): void {
    bodyRef.current = next;
    setBody(next);
    setSelection((current) =>
      current && !selectionExists(next, current) ? null : current,
    );
    saverRef.current?.markDirty(next);
  }

  function applyBody(
    next: WorkflowBody,
    label: string,
    options?: EditOptions,
  ): void {
    if (next === bodyRef.current) return;
    record(label, options?.coalesce);
    showBody(next);
    ensurePositions(next);
  }

  const commit: BodyUpdate = (update, options) => {
    const current = bodyRef.current;
    const next = update(current);
    if (next === current) return;
    record(options?.label ?? '编辑', options?.coalesce ?? false);
    showBody(next);
    ensurePositions(next);
  };

  function restore(entry: HistoryEntry): void {
    const bodyChanged = entry.body !== bodyRef.current;
    const positionsChanged =
      JSON.stringify(entry.positions) !== JSON.stringify(positionsRef.current);
    bodyRef.current = entry.body;
    setBody(entry.body);
    positionsRef.current = entry.positions;
    setPositions(entry.positions);
    setSelection((current) =>
      current && !selectionExists(entry.body, current) ? null : current,
    );
    if (bodyChanged) saverRef.current?.markDirty(entry.body);
    ensurePositions(entry.body);
    if (positionsChanged) scheduleLayoutSave();
  }

  function undo(): void {
    if (readOnly) return;
    const history = historyRef.current;
    const entry = history.past.pop();
    if (!entry) return;
    history.future.push({
      body: bodyRef.current,
      positions: positionsRef.current,
      label: entry.label,
    });
    history.last = null;
    bumpHistory((value) => value + 1);
    restore(entry);
  }

  function redo(): void {
    if (readOnly) return;
    const history = historyRef.current;
    const entry = history.future.pop();
    if (!entry) return;
    history.past.push({
      body: bodyRef.current,
      positions: positionsRef.current,
      label: entry.label,
    });
    history.last = null;
    bumpHistory((value) => value + 1);
    restore(entry);
  }

  function saveLayoutNow(
    attempt = 0,
    keepalive = false,
  ): Promise<boolean> {
    if (saverRef.current === null) return Promise.resolve(true);
    const kept = new Set([
      ...bodyRef.current.stages.map((stage) => stage.name),
      START_NODE,
      END_NODE,
    ]);
    const filtered = Object.fromEntries(
      Object.entries(positionsRef.current).filter(([key]) => kept.has(key)),
    );
    if (!keepalive) setLayoutState('saving');
    return api
      .saveLayout(workflow, { positions: filtered }, keepalive)
      .then(() => {
        setLayoutState('saved');
        setLayoutSavedAt(new Date());
        return true;
      })
      .catch(() => {
        if (keepalive) return false;
        if (attempt < 3) {
          if (layoutTimer.current !== null) clearTimeout(layoutTimer.current);
          layoutTimer.current = setTimeout(
            () => void saveLayoutNow(attempt + 1),
            2000,
          );
          return false;
        }
        setLayoutState('failed');
        return false;
      });
  }

  function scheduleLayoutSave(): void {
    if (saverRef.current === null) return;
    setLayoutState('saving');
    if (layoutTimer.current !== null) clearTimeout(layoutTimer.current);
    layoutTimer.current = setTimeout(() => void saveLayoutNow(), 1000);
  }

  function moveNode(node: string, position: StagePoint): void {
    if (readOnly) return;
    positionsRef.current = { ...positionsRef.current, [node]: position };
    setPositions(positionsRef.current);
    setLayoutState('saving');
    scheduleLayoutSave();
  }

  function beginDrag(node: string): void {
    if (readOnly) return;
    record(
      node === START_NODE
        ? '移动开始标记'
        : node === END_NODE
          ? '移动结束标记'
          : `移动 stage ${node}`,
    );
    nudgeRef.current = null;
  }

  function nudge(dx: number, dy: number): void {
    if (readOnly || selection?.kind !== 'stage') return;
    const stage = selection.stage;
    const current = positionsRef.current[stage];
    if (!current) return;
    const label = `微调 stage ${stage}`;
    const now = Date.now();
    if (
      nudgeRef.current === null ||
      nudgeRef.current.label !== label ||
      now - nudgeRef.current.time > 500
    ) {
      record(label);
      nudgeRef.current = { label, time: now };
    } else {
      nudgeRef.current.time = now;
    }
    moveNode(stage, { x: current.x + dx, y: current.y + dy });
  }

  function relayout(): void {
    if (readOnly || bodyRef.current.stages.length === 0) return;
    record('自动布局');
    const computed = autoLayout(bodyRef.current);
    positionsRef.current = computed;
    setPositions(computed);
    setFitRequest((value) => value + 1);
    setMessage({ kind: 'ok', text: '已重新布局，可撤销。' });
    void saveLayoutNow();
  }

  function focusField(
    field: string,
    stage: string | null = null,
    selectAll = false,
  ): void {
    if (field.startsWith('var:') && stage !== null) {
      setSelection({ kind: 'stage', stage });
    }
    setFieldFocus({
      field,
      stage,
      token: (fieldToken.current += 1),
      selectAll,
    });
  }

  function centerStage(stage: string): void {
    setFocusRequest({ stage, token: (focusToken.current += 1) });
  }

  function focusStage(stage: string): void {
    setSelection({ kind: 'stage', stage });
    centerStage(stage);
  }

  function nextStagePosition(preferred?: StagePoint): StagePoint {
    if (preferred) return preferred;
    if (bodyRef.current.stages.length === 0) return { x: 120, y: 160 };
    const positions = positionsRef.current;
    const stagePositions = Object.entries(positions)
      .filter(([name]) => name !== START_NODE && name !== END_NODE)
      .map(([, point]) => point);
    const anchorName =
      selection?.kind === 'stage' ? selection.stage : null;
    let anchor = anchorName ? positions[anchorName] : undefined;
    if (!anchor) {
      anchor =
        stagePositions.length === 0
          ? { x: 120, y: 160 }
          : stagePositions.reduce((best, point) =>
              point.x > best.x ? point : best,
            );
    }
    const height = STAGE_HEIGHT;
    const point = { x: anchor.x + STAGE_WIDTH + 160, y: anchor.y };
    let guard = 0;
    while (guard < 50) {
      const occupied = stagePositions.some(
        (other) =>
          Math.abs(other.x - point.x) < STAGE_WIDTH + 40 &&
          Math.abs(other.y - point.y) < height + 20,
      );
      if (!occupied) break;
      point.y += height + 40;
      guard += 1;
    }
    return point;
  }

  function addStageAt(preferred?: StagePoint): void {
    if (readOnly) return;
    const name = uniqueStageName(bodyRef.current);
    const position = nextStagePosition(preferred);
    record('添加 stage');
    positionsRef.current = { ...positionsRef.current, [name]: position };
    setPositions(positionsRef.current);
    const next: WorkflowBody = {
      ...bodyRef.current,
      stages: [...bodyRef.current.stages, emptyStage(name)],
    };
    showBody(next);
    setSelection({ kind: 'stage', stage: name });
    centerStage(name);
    focusField('stage-name', name, true);
    scheduleLayoutSave();
  }

  function addOperationToStage(stageName: string): void {
    if (readOnly) return;
    const stage = findStage(bodyRef.current, stageName);
    if (!stage) return;
    const [next, index] = addOperation(bodyRef.current, stageName, {
      name: uniqueOperationName(stage),
      display_name: '',
      description: '',
      roles: [],
      action: '',
    });
    applyBody(next, '添加操作');
    setSelection({ kind: 'operation', stage: stageName, index });
    centerStage(stageName);
    focusField(
      bodyRef.current.roles.length > 0
        ? 'operation-roles'
        : 'operation-display',
      stageName,
      bodyRef.current.roles.length === 0,
    );
  }

  function addSubtaskToStage(stageName: string): void {
    if (readOnly) return;
    if (!findStage(bodyRef.current, stageName)) return;
    const [next, index] = addCreated(bodyRef.current, stageName, {
      create_child: {
        workflow: catalog[0]?.id ?? definition?.id ?? '',
        title: '子 Task',
      },
    });
    applyBody(next, '添加子任务');
    setSelection({ kind: 'created', stage: stageName, index });
    centerStage(stageName);
    focusField('created-workflow', stageName, true);
  }

  function createTransition(source: string, target: string): void {
    if (readOnly) return;
    const to = target === END_NODE ? ENDED_STAGE : target;
    if (to !== ENDED_STAGE && !findStage(bodyRef.current, to)) return;
    const [next, index] = addTransition(bodyRef.current, source, {
      on: 'false',
      action: '',
      to,
    });
    applyBody(next, '创建转移');
    setSelection({ kind: 'transition', stage: source, index });
    centerStage(source);
    focusField('on', source, true);
  }

  function connect(connection: Connection): void {
    if (readOnly) return;
    const source = connection.source;
    const target = connection.target;
    if (!source || !target) return;
    if (source === START_NODE) {
      if (target === END_NODE) return;
      if (!findStage(bodyRef.current, target)) return;
      const previous = bodyRef.current.initial_stage;
      applyBody(
        { ...bodyRef.current, initial_stage: target },
        previous === null ? '设置初始 stage' : '修改初始 stage',
      );
      setSelection({ kind: 'stage', stage: target });
      centerStage(target);
      if (previous !== null && previous !== target) {
        setMessage({ kind: 'ok', text: `初始 stage 已改为 ${target}。` });
      }
      return;
    }
    if (source === END_NODE || target === START_NODE) return;
    if (!findStage(bodyRef.current, source)) return;
    createTransition(source, target);
  }

  function openPicker(stage: string): void {
    const first =
      bodyRef.current.stages.find((item) => item.name !== stage)?.name ??
      ENDED_STAGE;
    setPickerTarget(first);
    setPickerStage(stage);
  }

  function confirmPicker(): void {
    if (pickerStage === null) return;
    const target = pickerTarget === ENDED_STAGE ? END_NODE : pickerTarget;
    createTransition(pickerStage, target);
    setPickerStage(null);
  }

  function locate(path: string): void {
    const current = bodyRef.current;
    const missing = () =>
      setMessage({
        kind: 'error',
        text: '该位置已不存在，校验结果可能已过期。',
      });

    if (path === 'initial_stage') {
      const stage = current.initial_stage ?? current.stages[0]?.name;
      if (!stage) {
        setSelection(null);
        setMessage({
          kind: 'error',
          text: '还没有 stage 可以设置初始 stage。',
        });
        return;
      }
      setSelection({ kind: 'stage', stage });
      centerStage(stage);
      focusField('initial', stage);
      return;
    }

    const taskVar = /^vars\.([a-z][a-z0-9_]*)/.exec(path);
    if (taskVar) {
      const name = taskVar[1];
      if (!(name in current.vars)) {
        missing();
        return;
      }
      setSelection({ kind: 'taskvar', name });
      focusField(`var:${name}`);
      sidebarRef.current
        ?.querySelector('[data-panel="taskvars"]')
        ?.scrollIntoView({
          block: 'center',
          behavior: reducedMotion() ? 'auto' : 'smooth',
        });
      return;
    }

    const role = /^roles\[(\d+)\]/.exec(path);
    if (role) {
      const index = Number(role[1]);
      if (!current.roles[index]) {
        missing();
        return;
      }
      setSelection({ kind: 'role', index });
      return;
    }

    const stageMatch = /^stages\[(\d+)\]/.exec(path);
    if (!stageMatch) {
      if (path === 'schema_version') {
        setMessage({
          kind: 'error',
          text: 'schema_version 不在画布上，无法定位。',
        });
      } else {
        setMessage({ kind: 'error', text: `无法定位到 ${path}。` });
      }
      return;
    }
    const stage = current.stages[Number(stageMatch[1])];
    if (!stage) {
      missing();
      return;
    }
    const rest = path.slice(stageMatch[0].length);

    const row = /^\.(operations|events|created|transitions)\[(\d+)\](?:\.(.+))?$/.exec(
      rest,
    );
    if (row) {
      const list = row[1];
      const index = Number(row[2]);
      const property = row[3] ?? '';
      const kind =
        list === 'operations'
          ? 'operation'
          : list === 'events'
            ? 'event'
            : list === 'created'
              ? 'created'
              : 'transition';
      const count =
        kind === 'operation'
          ? stage.operations.length
          : kind === 'event'
            ? stage.events.length
            : kind === 'created'
              ? stage.created.length
              : stage.transitions.length;
      if (index >= count) {
        missing();
        return;
      }
      setSelection({ kind, stage: stage.name, index });
      centerStage(stage.name);
      const field =
        kind === 'operation'
          ? property === 'name'
            ? 'operation-name'
            : property === 'display_name'
              ? 'operation-display'
              : property === 'action'
                ? 'action'
                : null
          : kind === 'event'
            ? property === 'event'
              ? 'event-kind'
              : property === 'action'
                ? 'action'
                : null
            : kind === 'created'
              ? property.startsWith('create_child')
                ? 'created-workflow'
                : 'created-state'
              : property === 'on'
                ? 'on'
                : property === 'to'
                  ? 'to'
                  : property === 'action'
                    ? 'transition-action'
                    : null;
      if (field) focusField(field, stage.name);
      return;
    }

    const stageVar = /^\.vars\.([a-z][a-z0-9_]*)/.exec(rest);
    if (stageVar) {
      if (!(stageVar[1] in stage.vars)) {
        missing();
        return;
      }
      setSelection({ kind: 'stage', stage: stage.name });
      centerStage(stage.name);
      focusField(`var:${stageVar[1]}`, stage.name);
      return;
    }

    setSelection({ kind: 'stage', stage: stage.name });
    centerStage(stage.name);
    if (rest === '.name') {
      focusField('stage-name', stage.name, true);
    }
  }

  function deleteSelection(): boolean {
    if (readOnly || selection === null) return false;
    if (selection.kind === 'stage') return false;
    if (selection.kind === 'taskvar') return false;
    if (selection.kind === 'role') {
      const role = bodyRef.current.roles[selection.index];
      if (!role) return false;
      const references = roleReferenceTargets(bodyRef.current, role.name);
      if (references.length > 0) {
        setMessage({
          kind: 'error',
          text: `角色 ${role.name} 仍被引用，无法删除。`,
        });
        return true;
      }
      const roleIndex = selection.index;
      const next = selectionAfterRoleDelete(
        roleIndex,
        bodyRef.current.roles.length,
      );
      commit(
        (current) => ({
          ...current,
          roles: current.roles.filter((_, index) => index !== roleIndex),
        }),
        { label: `删除角色 ${role.name}` },
      );
      setSelection(next);
      return true;
    }
    const stage = findStage(bodyRef.current, selection.stage);
    if (!stage) return false;
    const kind = selection.kind;
    const index = selection.index;
    const count =
      kind === 'operation'
        ? stage.operations.length
        : kind === 'event'
          ? stage.events.length
          : kind === 'created'
            ? stage.created.length
            : stage.transitions.length;
    if (index >= count) return false;
    const next = selectionAfterRowDelete(kind, stage.name, index, count);
    commit(
      (current) =>
        updateStage(current, stage.name, (value) => {
          if (kind === 'operation') value.operations.splice(index, 1);
          else if (kind === 'event') value.events.splice(index, 1);
          else if (kind === 'created') value.created.splice(index, 1);
          else value.transitions.splice(index, 1);
        }),
      { label: `删除${kind === 'operation' ? '操作' : kind === 'event' ? '事件' : kind === 'created' ? '进入效果' : '转移'}` },
    );
    setSelection(next);
    return true;
  }

  useEffect(() => {
    let cancelled = false;
    setStatus('loading');
    setSelection(null);
    setConflict(null);
    setRecovery(null);
    setValidation(null);
    setValidationBody(null);
    setValidationError('');
    setValidating(false);
    setMessage(null);
    setSaveError('');
    setPublishing(false);
    setLayoutState('saved');
    setLayoutSavedAt(null);
    setLastSavedAt(null);
    setPickerStage(null);
    historyRef.current = { past: [], future: [], last: null };
    bumpHistory((value) => value + 1);

    async function load() {
      try {
        const loadedDefinition = await api.definition(workflow);
        const [loadedVersions, layout, loadedCatalog] = await Promise.all([
          api.versions(workflow).catch(() => []),
          api.layout(workflow).catch(() => ({ positions: {} })),
          api.list().catch(() => []),
        ]);
        let draftBody: WorkflowBody;
        let draftRevision = 0;
        if (loadedDefinition.editable) {
          const draft = await api.openDraft(workflow);
          draftBody = draft.body;
          draftRevision = draft.revision;
        } else if (loadedDefinition.latest_version !== null) {
          const version = await api.version(
            workflow,
            loadedDefinition.latest_version,
          );
          draftBody = version.body;
        } else {
          draftBody = emptyBody();
        }
        if (cancelled) return;
        setDefinition(loadedDefinition);
        setVersions(loadedVersions);
        setCatalog(loadedCatalog);
        setBody(draftBody);
        bodyRef.current = draftBody;
        setRevision(draftRevision);
        const merged = mergePositions(draftBody, layout);
        positionsRef.current = merged;
        setPositions(merged);
        if (loadedDefinition.editable) {
          const saver = new DraftSaver(
            workflow,
            draftRevision,
            () => bodyRef.current,
            {
              onState: (state) => {
                setSaveState(state);
                if (state === 'saved') {
                  setSaveError('');
                  setLastSavedAt(new Date());
                }
              },
              onSaved: (savedRevision, savedBody) => {
                setRevision(savedRevision);
                setLastSavedAt(new Date());
                setSaveError('');
                runValidation(savedBody);
              },
              onConflict: (currentRevision) =>
                setConflict({ currentRevision }),
              onFailure: (failure) => {
                if (failure instanceof ApiError && failure.validation) {
                  setValidation(failure.validation);
                  setValidationBody(JSON.stringify(bodyRef.current));
                  setMessage({
                    kind: 'error',
                    text: '草稿包含无法保存的结构错误。',
                  });
                } else if (
                  failure instanceof ApiError &&
                  failure.status < 500
                ) {
                  setSaveError(failure.message);
                }
              },
            },
          );
          saverRef.current = saver;
          layoutKeepalive.current = () => {
            if (saverRef.current?.hasUnsaved()) {
              saverRef.current.saveKeepalive();
            }
            void saveLayoutNow(0, true);
          };
          const backup = readBackup(workflow);
          if (backup) {
            if (JSON.stringify(backup.body) !== JSON.stringify(draftBody)) {
              setRecovery(backup);
            } else {
              clearBackup(workflow);
            }
          }
        }
        if (loadedDefinition.editable) runValidation(draftBody);
        setStatus('ready');
        const params = new URLSearchParams(window.location.search);
        if (params.get('copied') === '1') {
          setMessage({
            kind: 'ok',
            text: '副本中的流程引用仍指向源流程，请按需手动修改。',
          });
          params.delete('copied');
          const query = params.toString();
          window.history.replaceState(
            null,
            '',
            `${window.location.pathname}${query ? `?${query}` : ''}`,
          );
        }
      } catch (failure) {
        if (cancelled) return;
        if (failure instanceof ApiError && failure.status === 401) {
          setStatus('login');
        } else {
          setError(errorText(failure));
          setStatus('error');
        }
      }
    }

    void load();
    return () => {
      cancelled = true;
      saverRef.current?.dispose();
      saverRef.current = null;
      if (layoutTimer.current !== null) clearTimeout(layoutTimer.current);
    };
  }, [workflow, runValidation]);

  useEffect(() => {
    const onPageHide = () => layoutKeepalive.current();
    const onVisibility = () => {
      if (document.visibilityState === 'hidden') layoutKeepalive.current();
    };
    const onBeforeUnload = (event: BeforeUnloadEvent) => {
      if (saverRef.current?.hasUnsaved()) {
        event.preventDefault();
        event.returnValue = '';
      }
    };
    window.addEventListener('pagehide', onPageHide);
    document.addEventListener('visibilitychange', onVisibility);
    window.addEventListener('beforeunload', onBeforeUnload);
    return () => {
      window.removeEventListener('pagehide', onPageHide);
      document.removeEventListener('visibilitychange', onVisibility);
      window.removeEventListener('beforeunload', onBeforeUnload);
    };
  }, []);

  useEffect(() => {
    if (message?.kind !== 'ok') return;
    const timer = setTimeout(() => setMessage(null), 6000);
    return () => clearTimeout(timer);
  }, [message]);

  const selectionKey = selection === null ? 'none' : JSON.stringify(selection);
  useEffect(() => {
    if (selection === null) return;
    sidebarRef.current?.scrollTo({
      top: 0,
      behavior: reducedMotion() ? 'auto' : 'smooth',
    });
  }, [selectionKey]);

  useEffect(() => {
    if (!fieldFocus) return;
    const root = sidebarRef.current;
    if (!root) return;
    const selector = fieldFocus.field.startsWith('var:')
      ? `[data-var="${fieldFocus.field.slice(4)}"]`
      : `[data-field="${fieldFocus.field}"]`;
    const target = root.querySelector<HTMLElement>(selector);
    if (!target) return;
    target.scrollIntoView({
      block: 'center',
      behavior: reducedMotion() ? 'auto' : 'smooth',
    });
    target.classList.add('field-flash');
    const focusable = target.matches('input, textarea, select, button')
      ? target
      : target.querySelector<HTMLElement>('input, textarea, select');
    if (focusable) {
      focusable.focus({ preventScroll: true });
      if (
        fieldFocus.selectAll &&
        (focusable instanceof HTMLInputElement ||
          focusable instanceof HTMLTextAreaElement)
      ) {
        focusable.select();
      }
    }
    const timer = setTimeout(() => target.classList.remove('field-flash'), 1500);
    return () => clearTimeout(timer);
    // A token change is the only trigger; field and target are read from it.
  }, [fieldFocus?.token]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const meta = event.metaKey || event.ctrlKey;
      if (meta && event.key.toLowerCase() === 's') {
        event.preventDefault();
        void saverRef.current?.flush();
        if (!readOnly) void saveLayoutNow();
        return;
      }
      const target = event.target;
      const editing =
        target instanceof HTMLElement &&
        (target.tagName === 'INPUT' ||
          target.tagName === 'TEXTAREA' ||
          target.tagName === 'SELECT' ||
          target.isContentEditable);
      if (editing) return;
      if (pickerStage !== null) {
        if (event.key === 'Escape') {
          event.preventDefault();
          setPickerStage(null);
        }
        return;
      }
      if (meta) {
        const key = event.key.toLowerCase();
        if (key === 'z' && !event.shiftKey) {
          event.preventDefault();
          undo();
        } else if ((key === 'z' && event.shiftKey) || key === 'y') {
          event.preventDefault();
          redo();
        }
        return;
      }
      if (event.key === 'Escape') {
        if (selection !== null) {
          event.preventDefault();
          setSelection(null);
        }
        return;
      }
      if (readOnly) return;
      if (event.key === 'Delete' || event.key === 'Backspace') {
        if (deleteSelection()) event.preventDefault();
        return;
      }
      if (event.key.toLowerCase() === 'c' && !event.altKey) {
        if (selection?.kind === 'stage') {
          event.preventDefault();
          openPicker(selection.stage);
        }
        return;
      }
      if (selection?.kind === 'stage' && event.key.startsWith('Arrow')) {
        const step = event.shiftKey ? 1 : 10;
        const delta =
          event.key === 'ArrowLeft'
            ? { dx: -step, dy: 0 }
            : event.key === 'ArrowRight'
              ? { dx: step, dy: 0 }
              : event.key === 'ArrowUp'
                ? { dx: 0, dy: -step }
                : event.key === 'ArrowDown'
                  ? { dx: 0, dy: step }
                  : null;
        if (delta) {
          event.preventDefault();
          nudge(delta.dx, delta.dy);
        }
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  });

  async function flushDraft(): Promise<boolean> {
    const saver = saverRef.current;
    if (!saver) return true;
    const ok = await saver.flush();
    if (!ok) setMessage({ kind: 'error', text: '保存未完成，请检查冲突或错误。' });
    return ok;
  }

  async function leaveTo(href: string): Promise<void> {
    const ok = await flushDraft();
    if (!ok) {
      const leave = window.confirm('仍有未保存修改，仍要离开？');
      if (!leave) return;
    }
    if (!readOnly) await saveLayoutNow();
    window.location.assign(href);
  }

  async function loadServerVersion(): Promise<void> {
    const label =
      conflict?.currentRevision === null ||
      conflict?.currentRevision === undefined
        ? ''
        : `（revision ${conflict.currentRevision}）`;
    if (
      !window.confirm(
        `将丢弃当前编辑器中的本地修改，载入服务端草稿${label}。`,
      )
    ) {
      return;
    }
    try {
      const draft = await api.openDraft(workflow);
      bodyRef.current = draft.body;
      setBody(draft.body);
      setRevision(draft.revision);
      const merged = mergePositions(draft.body, {
        positions: positionsRef.current,
      });
      positionsRef.current = merged;
      setPositions(merged);
      saverRef.current?.reset(draft.revision);
      setConflict(null);
      setSelection(null);
      historyRef.current = { past: [], future: [], last: null };
      bumpHistory((value) => value + 1);
      runValidation(draft.body);
      setMessage({ kind: 'ok', text: '已载入服务端版本。' });
    } catch (failure) {
      setMessage({ kind: 'error', text: errorText(failure) });
    }
  }

  function overwriteServer(): void {
    const current = conflict?.currentRevision;
    if (current === null || current === undefined) {
      setMessage({
        kind: 'error',
        text: '无法确定服务端 revision，请载入服务端版本。',
      });
      return;
    }
    if (
      !window.confirm(
        `将用当前编辑器内容覆盖服务端草稿（revision ${current}）。服务端较新的修改会丢失，不会自动合并。`,
      )
    ) {
      return;
    }
    saverRef.current?.overwrite(current);
    setRevision(current);
    setConflict(null);
  }

  function restoreBackup(): void {
    if (!recovery) return;
    if (
      !window.confirm(
        '恢复会替换编辑器当前内容，并在随后自动保存到服务端。',
      )
    ) {
      return;
    }
    applyBody(recovery.body, '恢复本地备份');
    setRecovery(null);
    setMessage({ kind: 'ok', text: '已恢复本地备份，随后会自动保存。' });
  }

  function discardBackup(): void {
    clearBackup(workflow);
    setRecovery(null);
  }

  async function publish(): Promise<void> {
    const saver = saverRef.current;
    if (!saver) return;
    setPublishing(true);
    setMessage(null);
    try {
      const saved = await saver.flush();
      if (!saved) {
        setMessage({ kind: 'error', text: '保存未完成，未发布。' });
        return;
      }
      const result = await api.publish(workflow, saver.getRevision());
      saver.reset(result.revision);
      setRevision(result.revision);
      setValidation({ errors: [], warnings: result.warnings });
      setValidationBody(JSON.stringify(bodyRef.current));
      const warningText =
        result.warnings.length > 0
          ? `（${result.warnings.length} 个警告）`
          : '';
      setMessage({
        kind: 'ok',
        text: `已发布 v${result.version} · ${timeText(new Date())}${warningText}`,
        link: {
          href: `${BASE}${encodeURIComponent(workflow)}/versions/${result.version}`,
          label: '查看版本',
        },
      });
      const [nextVersions, nextDefinition] = await Promise.all([
        api.versions(workflow).catch(() => versions),
        api.definition(workflow).catch(() => definition),
      ]);
      setVersions(nextVersions);
      if (nextDefinition) setDefinition(nextDefinition);
    } catch (failure) {
      if (failure instanceof ApiError && failure.status === 409) {
        setConflict({
          currentRevision: failure.details.current_revision ?? null,
        });
      } else if (failure instanceof ApiError && failure.validation) {
        setValidation(failure.validation);
        setValidationBody(JSON.stringify(bodyRef.current));
        setMessage({ kind: 'error', text: '校验失败，未发布。' });
        const first = failure.validation.errors[0];
        if (first) locate(first.path);
      } else {
        setMessage({ kind: 'error', text: errorText(failure) });
      }
    } finally {
      setPublishing(false);
    }
  }

  if (status === 'loading') {
    return <div className="full-screen">正在加载流程…</div>;
  }
  if (status === 'login') {
    return (
      <div className="full-screen">
        <p>会话已失效。</p>
        <a href="/login">重新登录</a>
      </div>
    );
  }
  if (status === 'error' || !definition) {
    return (
      <div className="full-screen">
        <p>{error || '流程不存在或不可见。'}</p>
        <a href="/workflows">返回流程列表</a>
      </div>
    );
  }
  if (readOnly && definition.latest_version === null) {
    return (
      <div className="full-screen">
        <p>该流程尚未发布，暂无可查看内容。</p>
        <a href="/workflows">返回流程列表</a>
      </div>
    );
  }

  const workflowIds = Array.from(
    new Set([...catalog.map((item) => item.id), definition.id]),
  );
  const saveLabel = SAVE_LABELS[saveState];
  const history = historyRef.current;
  const canUndo = !readOnly && history.past.length > 0;
  const canRedo = !readOnly && history.future.length > 0;
  const undoLabel = canUndo
    ? history.past[history.past.length - 1].label
    : '没有可撤销的操作';
  const redoLabel = canRedo
    ? history.future[history.future.length - 1].label
    : '没有可重做的操作';

  const errorCount = validation?.errors.length ?? 0;
  const warningCount = validation?.warnings.length ?? 0;
  const currentBody = JSON.stringify(body);
  const validationStale =
    validation !== null && validationBody !== currentBody;
  const freshErrors =
    validation !== null && !validationStale ? errorCount : 0;
  const freshWarnings =
    validation !== null && !validationStale ? warningCount : 0;

  const publishBlocked =
    publishing ||
    conflict !== null ||
    saveState === 'failed' ||
    freshErrors > 0;

  let publishReason: ReactNode = null;
  if (publishing) {
    publishReason = '发布中…';
  } else if (conflict) {
    publishReason = '存在草稿冲突';
  } else if (saveState === 'failed') {
    publishReason = '保存失败，先重试保存';
  } else if (freshErrors > 0) {
    publishReason = (
      <button
        type="button"
        className="link error-text"
        onClick={() => {
          const first = validation?.errors[0];
          if (first) locate(first.path);
        }}
      >
        {freshErrors} 个校验错误
      </button>
    );
  } else if (freshWarnings > 0) {
    publishReason = (
      <span className="warning-text">{freshWarnings} 个警告</span>
    );
  }

  const validationSummary =
    validation === null
      ? validationError
        ? '校验失败'
        : '等待校验'
      : errorCount > 0
        ? `${errorCount} 个错误`
        : warningCount > 0
          ? `${warningCount} 个警告`
          : '通过';

  const layoutLabel =
    layoutState === 'failed'
      ? '布局未保存 · 重试'
      : layoutState === 'saving'
        ? '布局保存中…'
        : `布局已保存${layoutSavedAt ? ` ${timeText(layoutSavedAt)}` : ''}`;

  const versionValue = readOnly
    ? String(definition.latest_version ?? '')
    : 'draft';

  return (
    <div className="editor">
      <datalist id="workflow-ids">
        {workflowIds.map((id) => (
          <option key={id} value={id} />
        ))}
      </datalist>

      <header className="toolbar">
        <a
          className="back"
          href="/workflows"
          onClick={(event) => {
            event.preventDefault();
            void leaveTo('/workflows');
          }}
        >
          ← 流程
        </a>
        <div className="title-block">
          <h1>{definition.title}</h1>
          <span className="hint">
            {definition.owner.kind === 'platform'
              ? '平台流程'
              : `账户：${definition.owner.display_name}`}
          </span>
        </div>
        <span className="toolbar-divider" aria-hidden="true" />
        <span className={`badge ${readOnly ? 'readonly' : 'draft'}`}>
          {readOnly
            ? definition.latest_version !== null
              ? `只读 · v${definition.latest_version}`
              : '只读 · 未发布'
            : '草稿'}
        </span>
        {!readOnly && (
          <span
            className={`badge save-${saveState}`}
            aria-live="polite"
          >
            {saveLabel}
          </span>
        )}
        <select
          className="version-select"
          aria-label="版本"
          value={versionValue}
          disabled={readOnly && definition.latest_version === null}
          onChange={(event) => {
            if (event.target.value === versionValue) return;
            if (event.target.value === 'draft') return;
            void leaveTo(
              `${BASE}${encodeURIComponent(workflow)}/versions/${event.target.value}`,
            );
          }}
        >
          {!readOnly && <option value="draft">草稿</option>}
          {versions.map((version) => (
            <option key={version.version} value={version.version}>
              v{version.version}
            </option>
          ))}
        </select>
        <div className="spacer" />
        {!readOnly && (
          <>
            <span className="toolbar-divider" aria-hidden="true" />
            <button
              type="button"
              className="secondary"
              disabled={!canUndo}
              title={canUndo ? `撤销：${undoLabel}` : undoLabel}
              onClick={undo}
            >
              撤销
            </button>
            <button
              type="button"
              className="secondary"
              disabled={!canRedo}
              title={canRedo ? `重做：${redoLabel}` : redoLabel}
              onClick={redo}
            >
              重做
            </button>
            <span className="toolbar-divider" aria-hidden="true" />
            <button
              type="button"
              className="secondary"
              disabled={body.stages.length === 0}
              onClick={relayout}
            >
              自动布局
            </button>
            <button
              type="button"
              className="secondary"
              disabled={saveState === 'saved'}
              title="Ctrl/Cmd+S"
              onClick={() => void flushDraft()}
            >
              保存
            </button>
            <span className="publish-group">
              <button
                type="button"
                className="primary"
                disabled={publishBlocked}
                title={typeof publishReason === 'string' ? publishReason : undefined}
                onClick={() => void publish()}
              >
                发布
              </button>
              {publishReason !== null && (
                <span className="publish-reason" aria-live="polite">
                  {publishReason}
                </span>
              )}
            </span>
          </>
        )}
      </header>

      {conflict && (
        <div className="banner error" role="alert">
          <span>
            草稿已被其他编辑或发布推进（服务端 revision{' '}
            {conflict.currentRevision ?? '未知'}）。自动保存已停止；本地修改仍保留在编辑器中，并写入本地备份。
          </span>
          <button
            type="button"
            className="secondary"
            onClick={() => void loadServerVersion()}
          >
            载入服务端版本（丢弃本地修改）
          </button>
          <button type="button" className="danger" onClick={overwriteServer}>
            用我的版本覆盖
          </button>
        </div>
      )}

      {recovery && (
        <div className="banner warning">
          <span>
            检测到未保存的本地备份（
            {new Date(recovery.savedAt).toLocaleString()}）。备份基于 revision{' '}
            {recovery.revision}，服务端当前 revision {revision}。
            {recovery.revision < revision &&
              ' 恢复后会覆盖服务端较新的修改。'}
          </span>
          <button type="button" className="secondary" onClick={restoreBackup}>
            恢复未保存的修改
          </button>
          <button type="button" className="secondary" onClick={discardBackup}>
            丢弃本地备份
          </button>
        </div>
      )}

      {saveError && (
        <div className="banner error" role="alert">
          <span>保存失败：{saveError}</span>
          <button
            type="button"
            className="secondary"
            onClick={() => void saverRef.current?.flush()}
          >
            重试保存
          </button>
        </div>
      )}

      {message && (
        <div
          className={`banner ${message.kind === 'ok' ? 'ok' : 'error'}`}
          role={message.kind === 'error' ? 'alert' : 'status'}
        >
          <span>{message.text}</span>
          {message.link && (
            <a
              className="link"
              href={message.link.href}
              target="_blank"
              rel="noreferrer"
            >
              {message.link.label}
            </a>
          )}
          <button
            type="button"
            className="icon"
            onClick={() => setMessage(null)}
            title="关闭"
            aria-label="关闭消息"
          >
            ×
          </button>
        </div>
      )}

      <a className="skip-link" href="#inspector">
        跳到检查器
      </a>

      <div className="main">
        <div className="canvas-wrap">
          <FlowCanvas
            body={body}
            positions={positions}
            selection={selection}
            readOnly={readOnly}
            focusRequest={focusRequest}
            fitRequest={fitRequest}
            onSelect={setSelection}
            onMove={moveNode}
            onNodeDragStart={readOnly ? undefined : beginDrag}
            onConnect={readOnly ? undefined : connect}
            onAddOperation={readOnly ? undefined : addOperationToStage}
            onAddSubtask={readOnly ? undefined : addSubtaskToStage}
            onAddStageAt={readOnly ? undefined : addStageAt}
          />
        </div>
        <aside className="sidebar" ref={sidebarRef} id="inspector">
          <div id="object-palette-slot" className="object-palette-slot" />
          <Inspector
            body={body}
            selection={selection}
            readOnly={readOnly}
            workflowIds={workflowIds}
            validation={validation}
            onChange={commit}
            onSelect={setSelection}
            onFocusStage={focusStage}
            onFocusField={focusField}
            onStageRenamed={(from, to) => {
              const current = positionsRef.current;
              const point = current[from];
              if (!point) return;
              const next = { ...current, [to]: point };
              delete next[from];
              positionsRef.current = next;
              setPositions(next);
              scheduleLayoutSave();
            }}
          />
          <TaskVarsPanel
            body={body}
            readOnly={readOnly}
            selectedName={selection?.kind === 'taskvar' ? selection.name : null}
            onChange={commit}
          />
          <RolesPanel
            body={body}
            readOnly={readOnly}
            selection={selection}
            onSelect={setSelection}
            onChange={commit}
          />
          <ValidationPanel
            validation={validation}
            stale={validationStale}
            error={validationError || null}
            validating={validating}
            onLocate={locate}
            onRetry={() => runValidation(bodyRef.current)}
          />
        </aside>
      </div>

      <footer className="statusbar" aria-live="polite">
        <span>{readOnly ? '只读' : `${saveLabel}${lastSavedAt ? ` · ${timeText(lastSavedAt)}` : ''}`}</span>
        {!readOnly && (
          <span>
            {layoutState === 'failed' ? (
              <button
                type="button"
                className="link error-text"
                onClick={() => void saveLayoutNow()}
              >
                {layoutLabel}
              </button>
            ) : (
              layoutLabel
            )}
          </span>
        )}
        {readOnly ? (
          <span>只读，拖动空白处平移画布</span>
        ) : (
          <span>Ctrl/Cmd+S 保存 · Ctrl/Cmd+Z 撤销</span>
        )}
        <span>
          {validationSummary}
          {validationStale ? '（基于已保存版本）' : ''}
        </span>
        <span className="spacer" />
        <span>
          {body.stages.length} 个 stage · {body.roles.length} 个角色
        </span>
      </footer>

      {pickerStage !== null && (
        <div
          className="dialog-backdrop"
          onClick={() => setPickerStage(null)}
        >
          <div
            className="dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="connect-title"
            onClick={(event) => event.stopPropagation()}
            onKeyDown={(event) => {
              if (event.key === 'Escape') {
                event.preventDefault();
                setPickerStage(null);
              } else if (event.key === 'Enter') {
                event.preventDefault();
                confirmPicker();
              }
            }}
          >
            <h2 id="connect-title">从 {pickerStage} 创建转移</h2>
            <p className="hint">
              与从右侧手柄拉线相同；新转移的 on 默认为 false，创建后即可编辑。
            </p>
            <label className="field">
              目标
              <select
                value={pickerTarget}
                autoFocus
                onChange={(event) => setPickerTarget(event.target.value)}
              >
                {body.stages.map((stage) => (
                  <option key={stage.name} value={stage.name}>
                    {stage.name}
                  </option>
                ))}
                <option value={ENDED_STAGE}>结束（ended）</option>
              </select>
            </label>
            <div className="row-actions">
              <button
                type="button"
                className="secondary"
                onClick={() => setPickerStage(null)}
              >
                取消
              </button>
              <button
                type="button"
                className="primary"
                onClick={confirmPicker}
              >
                创建转移
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
