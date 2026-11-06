import { useRef, useState, type ReactNode } from 'react';
import { TitleEditor } from './TitleEditor';
import {
  ENDED_STAGE,
  EVENT_KINDS,
  ROLE_KIND_LABELS,
  addCreated,
  addEvent,
  applyApprovalTemplate,
  approvalPattern,
  compactScript,
  effectDisplayLabel,
  effectKind,
  eventKindLabel,
  findStage,
  isKey,
  moveItem,
  renameStage,
  roleReferenceTargets,
  selectionAfterRoleDelete,
  selectionAfterRowDelete,
  selectionAfterRowMove,
  stageIncomingReferences,
  stageVarNames,
  taskStateLabel,
  updateEvent,
  updateOperation,
  updateStage,
  updateTransition,
  variableReferences,
  type ApprovalPattern,
  type EditOptions,
  type Selection,
} from './model';
import { KeyInput, VariablesEditor } from './VariablesEditor';
import type {
  CreatedEffect,
  EventSpec,
  OperationSpec,
  RoleKind,
  RoleSpec,
  TaskState,
  TransitionSpec,
  Validation,
  WorkflowBody,
  WorkflowStage,
} from './types';

export type BodyUpdate = ((
  update: (body: WorkflowBody) => WorkflowBody,
  options?: EditOptions,
) => void);

const SET_STATES: TaskState[] = ['pending', 'in_progress'];

function scriptScope(
  body: WorkflowBody,
  stage: WorkflowStage,
  eventFields: string[],
  workflowIds: string[],
): string[] {
  const tokens = [
    ...Object.keys(body.vars),
    ...Object.keys(stage.vars),
    'result',
    'task.state',
    'task.title',
    ...body.roles.map((role) => `active_actors('${role.name}')`),
    ...workflowIds.map((id) => `count_children('${id}')`),
    ...eventFields,
  ];
  return [...new Set(tokens)];
}

function eventFields(event: EventSpec): string[] {
  switch (event.event) {
    case 'task.created':
      return [];
    case 'task.state_changed':
      return ['event.state'];
    case 'child.created':
      return ['event.child.id', 'event.child.workflow', 'event.child.title'];
    case 'child.state_changed':
      return [
        'event.child.id',
        'event.child.workflow',
        'event.child.state',
        'event.child.result',
      ];
  }
}

function operationEventFields(): string[] {
  return ['event.actor'];
}

function ScriptInput({
  value,
  onChange,
  readOnly,
  label,
  rows = 4,
  field,
  tokens = [],
  hint,
  autoFocus,
}: {
  value: string;
  onChange(value: string): void;
  readOnly: boolean;
  label: string;
  rows?: number;
  field?: string;
  tokens?: string[];
  hint?: string;
  autoFocus?: boolean;
}) {
  const ref = useRef<HTMLTextAreaElement | null>(null);

  if (readOnly) {
    return (
      <div className="doc-field">
        <span className="doc-label">{label}</span>
        <pre className="code-block">{value.trim() === '' ? '（空）' : value}</pre>
      </div>
    );
  }

  const insert = (token: string) => {
    const element = ref.current;
    if (!element) {
      onChange(value + token);
      return;
    }
    const start = element.selectionStart ?? value.length;
    const end = element.selectionEnd ?? value.length;
    const next = value.slice(0, start) + token + value.slice(end);
    onChange(next);
    requestAnimationFrame(() => {
      element.focus();
      const position = start + token.length;
      element.setSelectionRange(position, position);
    });
  };

  return (
    <div className="field script-field">
      <label>
        {label}
        <textarea
          ref={ref}
          className="script-input"
          rows={rows}
          value={value}
          data-field={field}
          spellCheck={false}
          autoFocus={autoFocus}
          onChange={(event) => onChange(event.target.value)}
        />
      </label>
      {hint && <span className="hint">{hint}</span>}
      {tokens.length > 0 && (
        <div className="script-tokens" aria-label="可用变量和函数">
          <span className="hint">可用：</span>
          {tokens.map((token) => (
            <button
              type="button"
              key={token}
              className="token"
              onClick={() => insert(token)}
              title={`插入 ${token}`}
            >
              {token}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function RowTools({
  index,
  count,
  onMove,
  onDelete,
  deleteLabel,
}: {
  index: number;
  count: number;
  onMove(delta: number): void;
  onDelete(): void;
  deleteLabel: string;
}) {
  return (
    <div className="row-actions">
      <button
        type="button"
        className="secondary"
        disabled={index === 0}
        onClick={() => onMove(-1)}
      >
        上移
      </button>
      <button
        type="button"
        className="secondary"
        disabled={index === count - 1}
        onClick={() => onMove(1)}
      >
        下移
      </button>
      <button type="button" className="danger" onClick={onDelete}>
        {deleteLabel}
      </button>
    </div>
  );
}

function DocField({
  label,
  children,
  code,
}: {
  label: string;
  children: ReactNode;
  code?: boolean;
}) {
  return (
    <div className="doc-field">
      <span className="doc-label">{label}</span>
      {code ? <pre className="code-block">{children}</pre> : <div>{children}</div>}
    </div>
  );
}

function DocChips({ values, empty }: { values: string[]; empty: string }) {
  if (values.length === 0) return <span className="hint">{empty}</span>;
  return (
    <span className="chip-row">
      {values.map((value) => (
        <span className="chip" key={value}>
          {value}
        </span>
      ))}
    </span>
  );
}

function OperationInspector({
  body,
  stage,
  index,
  operation,
  update,
  readOnly,
  workflowIds,
  onSelect,
}: {
  body: WorkflowBody;
  stage: WorkflowStage;
  index: number;
  operation: OperationSpec;
  update: BodyUpdate;
  readOnly: boolean;
  workflowIds: string[];
  onSelect(selection: Selection | null): void;
}) {
  const mutate = (
    change: (value: OperationSpec) => void,
    options?: EditOptions,
  ) =>
    update(
      (current) => updateOperation(current, stage.name, index, change),
      options,
    );
  const label = `操作 ${stage.name}/${operation.name}`;
  const invalidRoles = operation.roles.filter(
    (role) => !stage.active_roles.includes(role),
  );
  const tokens = scriptScope(
    body,
    stage,
    operationEventFields(),
    workflowIds,
  );
  const [nameError, setNameError] = useState<string | null>(null);
  const rename = (next: string): boolean => {
    if (next === operation.name) {
      setNameError(null);
      return true;
    }
    if (!isKey(next)) {
      setNameError(
        '操作 name 必须以小写字母开头，只能包含小写字母、数字和下划线。',
      );
      return false;
    }
    if (
      stage.operations.some(
        (item, itemIndex) => itemIndex !== index && item.name === next,
      )
    ) {
      setNameError('同一 stage 内操作 name 不能重复。');
      return false;
    }
    setNameError(null);
    mutate((value) => (value.name = next), { label: `${label} 名称` });
    return true;
  };

  if (readOnly) {
    return (
      <section className="inspector-section">
        <h3>操作</h3>
        <DocField label="name">{operation.name}</DocField>
        <DocField label="显示名">
          {operation.display_name || '（未填写）'}
        </DocField>
        <DocField label="说明">{operation.description || '（空）'}</DocField>
        <DocField label="允许的角色">
          <DocChips values={operation.roles} empty="无角色" />
        </DocField>
        <DocField label="action" code>
          {operation.action.trim() === '' ? '（空）' : operation.action}
        </DocField>
      </section>
    );
  }

  return (
    <section className="inspector-section">
      <h3>操作</h3>
      <KeyInput
        key={`${stage.name}/${index}`}
        value={operation.name}
        label="名称（同一 stage 内唯一）"
        readOnly={false}
        field="operation-name"
        onCommit={rename}
      />
      {nameError && <p className="error-text">{nameError}</p>}
      <label className="field">
        显示名
        <input
          value={operation.display_name}
          data-field="operation-display"
          onChange={(event) =>
            mutate((value) => (value.display_name = event.target.value), {
              coalesce: true,
              label: `${label} 显示名`,
            })
          }
        />
        {operation.display_name.trim() === '' && (
          <span className="hint">建议填写显示名；卡片目前回退显示 name。</span>
        )}
      </label>
      <label className="field">
        说明
        <textarea
          rows={3}
          value={operation.description}
          onChange={(event) =>
            mutate((value) => (value.description = event.target.value), {
              coalesce: true,
              label: `${label} 说明`,
            })
          }
        />
      </label>
      <div className="field-label">允许的角色（至少一个）</div>
      <div className="role-checks" data-field="operation-roles">
        {body.roles.length === 0 && (
          <span className="error-text">
            尚未声明角色。先在右侧「角色」面板添加角色，再勾选允许发送该操作的角色。
          </span>
        )}
        {body.roles.map((role) => {
          const checked = operation.roles.includes(role.name);
          const last = checked && operation.roles.length === 1;
          return (
            <label className="checkbox" key={role.name}>
              <input
                type="checkbox"
                checked={checked}
                disabled={last}
                title={
                  last
                    ? '至少保留一个角色；如需停用此操作，请删除操作。'
                    : checked
                      ? undefined
                      : '勾选后会同时加入该 stage 的 active roles。'
                }
                onChange={(event) =>
                  update(
                    (current) => {
                      let next = updateOperation(
                        current,
                        stage.name,
                        index,
                        (value) => {
                          if (event.target.checked) {
                            if (!value.roles.includes(role.name)) {
                              value.roles.push(role.name);
                            }
                          } else {
                            value.roles = value.roles.filter(
                              (item) => item !== role.name,
                            );
                          }
                        },
                      );
                      if (event.target.checked) {
                        next = updateStage(next, stage.name, (value) => {
                          if (!value.active_roles.includes(role.name)) {
                            value.active_roles.push(role.name);
                          }
                        });
                      }
                      return next;
                    },
                    { label: `${label} 允许的角色` },
                  )
                }
              />
              {role.name}
            </label>
          );
        })}
      </div>
      {body.roles.length > 0 && (
        <p className="hint">
          勾选角色即可授权；角色会同时加入该 stage 的 active roles。
        </p>
      )}
      {invalidRoles.length > 0 && (
        <div className="invalid-roles">
          <span className="hint">以下角色已不在 active roles 中：</span>
          {invalidRoles.map((role) => (
            <span className="invalid-role" key={role}>
              {role}
              <button
                type="button"
                className="icon"
                aria-label={`移除无效角色 ${role}`}
                onClick={() =>
                  mutate((value) => {
                    value.roles = value.roles.filter((item) => item !== role);
                  })
                }
              >
                ×
              </button>
            </span>
          ))}
        </div>
      )}
      {operation.roles.length === 0 && (
        <p className="error-text">操作至少需要一个角色，当前没有角色。</p>
      )}
      <ScriptInput
        label="action 脚本"
        rows={7}
        field="action"
        value={operation.action}
        tokens={tokens}
        hint="操作被发送时执行。可用 event.actor 获得发送者。"
        autoFocus={operation.name.startsWith('operation_') && operation.action === ''}
        onChange={(action) =>
          mutate((value) => (value.action = action), {
            coalesce: true,
            label: `${label} action`,
          })
        }
        readOnly={false}
      />
      <RowTools
        index={index}
        count={stage.operations.length}
        onMove={(delta) => {
          update(
            (current) =>
              updateStage(current, stage.name, (value) =>
                moveItem(value.operations, index, delta),
              ),
            { label: `移动${label}` },
          );
          onSelect(
            selectionAfterRowMove(
              'operation',
              stage.name,
              index,
              delta,
              stage.operations.length,
            ),
          );
        }}
        onDelete={() => {
          const next = selectionAfterRowDelete(
            'operation',
            stage.name,
            index,
            stage.operations.length,
          );
          update((current) =>
            updateStage(current, stage.name, (value) => {
              value.operations.splice(index, 1);
            }),
          );
          onSelect(next);
        }}
        deleteLabel="删除操作"
      />
    </section>
  );
}

function EventInspector({
  body,
  stage,
  index,
  event,
  update,
  readOnly,
  workflowIds,
  onSelect,
}: {
  body: WorkflowBody;
  stage: WorkflowStage;
  index: number;
  event: EventSpec;
  update: BodyUpdate;
  readOnly: boolean;
  workflowIds: string[];
  onSelect(selection: Selection | null): void;
}) {
  const mutate = (change: (value: EventSpec) => void, options?: EditOptions) =>
    update(
      (current) => updateEvent(current, stage.name, index, change),
      options,
    );
  const tokens = scriptScope(body, stage, eventFields(event), workflowIds);

  if (readOnly) {
    return (
      <section className="inspector-section">
        <h3>事件</h3>
        <DocField label="event">{eventKindLabel(event.event)}</DocField>
        <DocField label="action" code>
          {event.action.trim() === '' ? '（空）' : event.action}
        </DocField>
      </section>
    );
  }

  return (
    <section className="inspector-section">
      <h3>事件</h3>
      <label className="field">
        event
        <select
          value={event.event}
          data-field="event-kind"
          onChange={(change) =>
            mutate(
              (value) =>
                (value.event = change.target.value as EventSpec['event']),
              { label: `事件 ${stage.name}/${index + 1} 类型` },
            )
          }
        >
          {EVENT_KINDS.map((kind) => {
            const used = stage.events.some(
              (item, itemIndex) => itemIndex !== index && item.event === kind,
            );
            return (
              <option key={kind} value={kind} disabled={used}>
                {eventKindLabel(kind)}
                {used ? '（已被使用）' : ''}
              </option>
            );
          })}
        </select>
      </label>
      <ScriptInput
        label="action 脚本"
        rows={7}
        field="action"
        value={event.action}
        tokens={tokens}
        hint={eventFields(event).length > 0 ? `本事件可用：${eventFields(event).join('、')}` : '本事件没有额外字段。'}
        autoFocus={event.action === ''}
        onChange={(action) =>
          mutate((value) => (value.action = action), {
            coalesce: true,
            label: `事件 ${stage.name}/${index + 1} action`,
          })
        }
        readOnly={false}
      />
      <RowTools
        index={index}
        count={stage.events.length}
        onMove={(delta) => {
          update(
            (current) =>
              updateStage(current, stage.name, (value) =>
                moveItem(value.events, index, delta),
              ),
          );
          onSelect(
            selectionAfterRowMove(
              'event',
              stage.name,
              index,
              delta,
              stage.events.length,
            ),
          );
        }}
        onDelete={() => {
          const next = selectionAfterRowDelete(
            'event',
            stage.name,
            index,
            stage.events.length,
          );
          update((current) =>
            updateStage(current, stage.name, (value) => {
              value.events.splice(index, 1);
            }),
          );
          onSelect(next);
        }}
        deleteLabel="删除事件"
      />
      <p className="hint">
        每个 stage 每种事件最多一个。事件没有角色限制；在 action 中判断
        event 字段决定是否处理。
      </p>
    </section>
  );
}

function CreatedInspector({
  stage,
  index,
  effect,
  update,
  readOnly,
  workflowIds,
  onSelect,
}: {
  stage: WorkflowStage;
  index: number;
  effect: CreatedEffect;
  update: BodyUpdate;
  readOnly: boolean;
  workflowIds: string[];
  onSelect(selection: Selection | null): void;
}) {
  const kind = effectKind(effect);
  const set = (next: CreatedEffect, options?: EditOptions) =>
    update(
      (current) =>
        updateStage(current, stage.name, (value) => {
          value.created[index] = next;
        }),
      options,
    );

  if (readOnly) {
    return (
      <section className="inspector-section">
        <h3>进入效果</h3>
        <DocField label="类型">
          {kind === 'set_state' ? 'set_state' : 'create_child'}
        </DocField>
        {kind === 'set_state' && 'set_state' in effect && (
          <DocField label="状态">{taskStateLabel(effect.set_state)}</DocField>
        )}
        {kind === 'create_child' && 'create_child' in effect && (
          <>
            <DocField label="子流程">{effect.create_child.workflow}</DocField>
            <DocField label="标题模板" code>
              {effect.create_child.title}
            </DocField>
          </>
        )}
      </section>
    );
  }

  return (
    <section className="inspector-section">
      <h3>进入效果</h3>
      <label className="field">
        类型
        <select
          value={kind}
          onChange={(event) => {
            if (event.target.value === 'set_state') {
              set({ set_state: 'in_progress' }, { label: '切换进入效果类型' });
            } else {
              set(
                {
                  create_child: {
                    workflow: workflowIds[0] ?? '',
                    title: '子 Task',
                  },
                },
                { label: '切换进入效果类型' },
              );
            }
          }}
        >
          <option value="set_state">设置状态（set_state）</option>
          <option value="create_child">创建子 Task（create_child）</option>
        </select>
      </label>
      {kind === 'set_state' && 'set_state' in effect && (
        <label className="field">
          状态
          <select
            value={effect.set_state}
            data-field="created-state"
            onChange={(event) =>
              set(
                { set_state: event.target.value as TaskState },
                { label: '修改进入效果状态' },
              )
            }
          >
            {SET_STATES.map((state) => (
              <option key={state} value={state}>
                {taskStateLabel(state)}
              </option>
            ))}
          </select>
        </label>
      )}
      {kind === 'create_child' && 'create_child' in effect && (
        <div className="stack">
          <label className="field">
            子流程
            <input
              value={effect.create_child.workflow}
              list="workflow-ids"
              data-field="created-workflow"
              onChange={(event) =>
                set(
                  {
                    create_child: {
                      ...effect.create_child,
                      workflow: event.target.value,
                    },
                  },
                  {
                    coalesce: true,
                    label: '修改进入效果子流程',
                  },
                )
              }
            />
          </label>
          <div className="field">
            <span className="field-label">标题模板</span>
            <TitleEditor
              value={effect.create_child.title}
              workflowIds={workflowIds}
              onChange={(title) =>
                set(
                  { create_child: { ...effect.create_child, title } },
                  { coalesce: true, label: '修改标题模板' },
                )
              }
            />
          </div>
        </div>
      )}
      <RowTools
        index={index}
        count={stage.created.length}
        onMove={(delta) => {
          update((current) =>
            updateStage(current, stage.name, (value) =>
              moveItem(value.created, index, delta),
            ),
          );
          onSelect(
            selectionAfterRowMove(
              'created',
              stage.name,
              index,
              delta,
              stage.created.length,
            ),
          );
        }}
        onDelete={() => {
          const next = selectionAfterRowDelete(
            'created',
            stage.name,
            index,
            stage.created.length,
          );
          update((current) =>
            updateStage(current, stage.name, (value) => {
              value.created.splice(index, 1);
            }),
          );
          onSelect(next);
        }}
        deleteLabel="删除进入效果"
      />
      <p className="hint">每次进入 stage 按顺序执行；自转移也会重新执行。</p>
    </section>
  );
}

function ApprovalBanner({
  pattern,
  stage,
  onSelect,
  onFocusField,
}: {
  pattern: ApprovalPattern;
  stage: WorkflowStage;
  onSelect(selection: Selection | null): void;
  onFocusField?(field: string, stage?: string): void;
}) {
  return (
    <div className="template-banner">
      <strong>全员确认模板</strong>
      <span>
        所有 {pattern.role} 确认后 →{' '}
        {pattern.target === ENDED_STAGE ? '结束' : pattern.target}
      </span>
      <div className="template-links">
        <button
          type="button"
          className="link"
          onClick={() => {
            const index = stage.operations.findIndex(
              (item) => item.name === pattern.operation,
            );
            if (index >= 0) {
              onSelect({ kind: 'operation', stage: stage.name, index });
            }
          }}
        >
          查看 operation
        </button>
        <button
          type="button"
          className="link"
          onClick={() => onFocusField?.(`var:${pattern.variable}`, stage.name)}
        >
          查看变量 {pattern.variable}
        </button>
      </div>
    </div>
  );
}

function TransitionInspector({
  body,
  stage,
  index,
  transition,
  update,
  readOnly,
  workflowIds,
  onSelect,
  onFocusField,
}: {
  body: WorkflowBody;
  stage: WorkflowStage;
  index: number;
  transition: TransitionSpec;
  update: BodyUpdate;
  readOnly: boolean;
  workflowIds: string[];
  onSelect(selection: Selection | null): void;
  onFocusField?(field: string, stage?: string): void;
}) {
  const stages = body.stages.map((item) => item.name);
  const targets = [...stages, ENDED_STAGE];
  const mutate = (
    change: (value: TransitionSpec) => void,
    options?: EditOptions,
  ) =>
    update(
      (current) => updateTransition(current, stage.name, index, change),
      options,
    );
  const pattern = approvalPattern(stage, transition);
  const tokens = scriptScope(body, stage, [], workflowIds);
  const parallel = stage.transitions.filter(
    (item) => item.to === transition.to,
  ).length;
  const label = `转移 ${stage.name}/${index + 1}`;

  if (readOnly) {
    return (
      <section className="inspector-section">
        <h3>转移</h3>
        {pattern && (
          <ApprovalBanner
            pattern={pattern}
            stage={stage}
            onSelect={onSelect}
            onFocusField={onFocusField}
          />
        )}
        <DocField label="on" code>
          {transition.on}
        </DocField>
        <DocField label="action" code>
          {transition.action.trim() === '' ? '（空）' : transition.action}
        </DocField>
        <DocField label="to">
          {transition.to === ENDED_STAGE ? '结束（ended）' : transition.to}
        </DocField>
      </section>
    );
  }

  return (
    <section className="inspector-section">
      <h3>转移</h3>
      {pattern && (
        <ApprovalBanner
          pattern={pattern}
          stage={stage}
          onSelect={onSelect}
          onFocusField={onFocusField}
        />
      )}
      <ScriptInput
        label="on 表达式（为 true 时转移）"
        rows={4}
        field="on"
        value={transition.on}
        tokens={tokens}
        hint="必须是返回 boolean 的表达式；on 中不能使用任何 event.* 字段。"
        autoFocus={transition.on === 'false'}
        onChange={(on) =>
          mutate((value) => (value.on = on), {
            coalesce: true,
            label: `${label} on`,
          })
        }
        readOnly={false}
      />
      <ScriptInput
        label="action 脚本（可选，on 匹配后、进入 to 前执行）"
        rows={4}
        field="transition-action"
        value={transition.action}
        tokens={tokens}
        hint="同样不能使用 event.*。目标是 ended 时可在这里写入 result。"
        onChange={(action) =>
          mutate((value) => (value.action = action), {
            coalesce: true,
            label: `${label} action`,
          })
        }
        readOnly={false}
      />
      <label className="field">
        to
        <select
          value={transition.to}
          data-field="to"
          onChange={(event) =>
            mutate((value) => (value.to = event.target.value), {
              label: `${label} 目标`,
            })
          }
        >
          {!targets.includes(transition.to) && (
            <option value={transition.to}>{transition.to}（不存在）</option>
          )}
          {stages.map((name) => (
            <option key={name} value={name}>
              {name}
            </option>
          ))}
          <option value={ENDED_STAGE}>结束（ended）</option>
        </select>
      </label>
      {transition.to === ENDED_STAGE && (
        <p className="hint">
          如需返回结果，在 action 中写入 result；非空 result 会复制到 task.result。
        </p>
      )}
      {transition.to === stage.name && (
        <p className="hint">
          自转移会重新进入当前 stage：stage 变量重置，进入效果重新执行。
        </p>
      )}
      {parallel > 1 && (
        <p className="hint">
          本 stage 到 {transition.to === ENDED_STAGE ? '结束' : transition.to}{' '}
          共 {parallel} 条转移，按顺序求值，第一条 on 为 true 的执行。
        </p>
      )}
      <RowTools
        index={index}
        count={stage.transitions.length}
        onMove={(delta) => {
          update(
            (current) =>
              updateStage(current, stage.name, (value) =>
                moveItem(value.transitions, index, delta),
              ),
            { label: `移动${label}` },
          );
          onSelect(
            selectionAfterRowMove(
              'transition',
              stage.name,
              index,
              delta,
              stage.transitions.length,
            ),
          );
        }}
        onDelete={() => {
          const next = selectionAfterRowDelete(
            'transition',
            stage.name,
            index,
            stage.transitions.length,
          );
          update((current) =>
            updateStage(current, stage.name, (value) => {
              value.transitions.splice(index, 1);
            }),
          );
          onSelect(next);
        }}
        deleteLabel="删除转移"
      />
    </section>
  );
}

function StageInspector({
  body,
  stage,
  readOnly,
  workflowIds,
  onChange,
  onSelect,
  onFocusField,
  onStageRenamed,
}: {
  body: WorkflowBody;
  stage: WorkflowStage;
  readOnly: boolean;
  workflowIds: string[];
  onChange: BodyUpdate;
  onSelect(selection: Selection | null): void;
  onFocusField?(field: string, stage?: string): void;
  onStageRenamed?(from: string, to: string): void;
}) {
  const [error, setError] = useState<string | null>(null);
  const [approvalOpen, setApprovalOpen] = useState(false);
  const [approvalRole, setApprovalRole] = useState(
    stage.active_roles[0] ?? '',
  );
  const [approvalOperation, setApprovalOperation] = useState('review_approved');
  const [approvalTarget, setApprovalTarget] = useState(ENDED_STAGE);
  const [approvalError, setApprovalError] = useState<string | null>(null);

  const isInitial = body.initial_stage === stage.name;
  const incoming = stageIncomingReferences(body, stage.name);
  const rename = (next: string): boolean => {
    if (next === stage.name) return true;
    if (!isKey(next)) {
      setError('stage name 必须以小写字母开头，只能包含小写字母、数字和下划线。');
      return false;
    }
    if (next === ENDED_STAGE) {
      setError(`${ENDED_STAGE} 是保留的虚拟 stage，不能用作 stage name。`);
      return false;
    }
    if (findStage(body, next)) {
      setError(`stage ${next} 已存在。`);
      return false;
    }
    setError(null);
    onChange((current) => renameStage(current, stage.name, next), {
      label: `重命名 stage ${stage.name}`,
    });
    onStageRenamed?.(stage.name, next);
    onSelect({ kind: 'stage', stage: next });
    return true;
  };

  const addEffect = (effect: CreatedEffect) => {
    const index = stage.created.length;
    onChange(
      (current) => addCreated(current, stage.name, effect)[0],
      { label: '添加进入效果' },
    );
    onSelect({ kind: 'created', stage: stage.name, index });
    onFocusField?.(
      'set_state' in effect ? 'created-state' : 'created-workflow',
      stage.name,
    );
  };

  const unusedEvent = EVENT_KINDS.find(
    (kind) => !stage.events.some((event) => event.event === kind),
  );
  const addEventHandler = () => {
    if (unusedEvent === undefined) return;
    const index = stage.events.length;
    onChange(
      (current) => addEvent(current, stage.name, { event: unusedEvent, action: '' })[0],
      { label: '添加事件' },
    );
    onSelect({ kind: 'event', stage: stage.name, index });
    onFocusField?.('action', stage.name);
  };

  const applyApproval = () => {
    try {
      const result = applyApprovalTemplate(body, stage.name, {
        role: approvalRole,
        operation: approvalOperation,
        target: approvalTarget,
      });
      if (!result) throw new Error('stage 已不存在。');
      onChange(() => result.body, { label: '添加全员确认模板' });
      setApprovalOpen(false);
      setApprovalError(null);
      onSelect({
        kind: 'transition',
        stage: stage.name,
        index: result.transition,
      });
      onFocusField?.('on', stage.name);
    } catch (failure) {
      setApprovalError(
        failure instanceof Error ? failure.message : String(failure),
      );
    }
  };

  if (readOnly) {
    return (
      <section className="inspector-section">
        <h3>Stage</h3>
        <DocField label="name">{stage.name}</DocField>
        <DocField label="初始 stage">{isInitial ? '是' : '否'}</DocField>
        <DocField label="active roles">
          <DocChips values={stage.active_roles} empty="无角色" />
        </DocField>
        <DocField label="stage 变量">
          <VariablesEditor
            vars={stage.vars}
            readOnly
            emptyHint="尚未声明变量"
            onChange={() => {}}
          />
        </DocField>
        <DocField label="事件">
          {stage.events.length === 0 ? (
            <span className="hint">无</span>
          ) : (
            <ul className="plain-list">
              {stage.events.map((event, index) => (
                <li key={index}>
                  {eventKindLabel(event.event)}：{compactScript(event.action)}
                </li>
              ))}
            </ul>
          )}
        </DocField>
        <DocField label="进入效果">
          {stage.created.length === 0 ? (
            <span className="hint">无</span>
          ) : (
            <ul className="plain-list">
              {stage.created.map((effect, index) => (
                <li key={index}>{effectDisplayLabel(effect)}</li>
              ))}
            </ul>
          )}
        </DocField>
        <DocField label="转移">
          {stage.transitions.length === 0 ? (
            <span className="hint">无</span>
          ) : (
            <ul className="plain-list">
              {stage.transitions.map((transition, index) => (
                <li key={index}>
                  → {transition.to === ENDED_STAGE ? '结束' : transition.to}：
                  on {transition.on}
                </li>
              ))}
            </ul>
          )}
        </DocField>
      </section>
    );
  }

  const stageVarsExisting = [
    ...Object.keys(body.vars),
    ...stageVarNames(body, stage.name),
  ];

  return (
    <section className="inspector-section">
      <h3>Stage</h3>
      <KeyInput
        key={stage.name}
        value={stage.name}
        label="name"
        readOnly={false}
        field="stage-name"
        onCommit={rename}
      />
      {error && <p className="error-text">{error}</p>}
      <label className="checkbox">
        <input
          type="checkbox"
          checked={isInitial}
          data-field="initial"
          onChange={(event) =>
            onChange(
              (current) => ({
                ...current,
                initial_stage: event.target.checked ? stage.name : null,
              }),
              { label: '设置初始 stage' },
            )
          }
        />
        初始 stage
      </label>
      <div className="field-label">active roles</div>
      <div className="role-checks">
        {body.roles.length === 0 && (
          <span className="hint">
            尚未声明角色。{' '}
            <button
              type="button"
              className="link"
              onClick={() =>
                document
                  .querySelector<HTMLInputElement>('#new-role-name')
                  ?.focus()
              }
            >
              去添加角色
            </button>
          </span>
        )}
        {body.roles.map((role) => (
          <label className="checkbox" key={role.name}>
            <input
              type="checkbox"
              checked={stage.active_roles.includes(role.name)}
              onChange={(event) => {
                const referenced = stage.operations.filter((operation) =>
                  operation.roles.includes(role.name),
                ).length;
                if (
                  !event.target.checked &&
                  referenced > 0 &&
                  !window.confirm(
                    `「${role.name}」仍被 ${referenced} 个操作引用。移除后这些操作会保留该角色并显示为无效角色，校验会报错。确定移除？`,
                  )
                ) {
                  return;
                }
                onChange(
                  (current) =>
                    updateStage(current, stage.name, (value) => {
                      if (event.target.checked) {
                        if (!value.active_roles.includes(role.name)) {
                          value.active_roles.push(role.name);
                        }
                      } else {
                        value.active_roles = value.active_roles.filter(
                          (name) => name !== role.name,
                        );
                      }
                    }),
                  { label: `修改 ${stage.name} active roles` },
                );
              }}
            />
            {role.name}
          </label>
        ))}
      </div>
      <div className="field-label">stage 变量</div>
      <p className="hint">每次进入该 stage 重新初始化；自转移也会重置。</p>
      <VariablesEditor
        vars={stage.vars}
        readOnly={false}
        existing={stageVarsExisting}
        emptyHint="尚未声明变量"
        references={(name) => variableReferences(body, name, stage.name)}
        onChange={(vars, options) =>
          onChange(
            (current) =>
              updateStage(current, stage.name, (value) => {
                value.vars = vars;
              }),
            options,
          )
        }
      />
      <div className="field-label">事件</div>
      {stage.events.length === 0 && (
        <span className="hint">尚未声明事件；每个 stage 每种事件最多一个。</span>
      )}
      <ul className="selection-list">
        {stage.events.map((event, index) => (
          <li key={index}>
            <button
              type="button"
              className="selection-item"
              onClick={() =>
                onSelect({ kind: 'event', stage: stage.name, index })
              }
            >
              <span className="row-label">{eventKindLabel(event.event)}</span>
              <span className="row-detail">{compactScript(event.action)}</span>
            </button>
          </li>
        ))}
      </ul>
      <div className="action-add">
        <button
          type="button"
          className="link"
          disabled={unusedEvent === undefined}
          title={
            unusedEvent === undefined
              ? '四种事件都已声明'
              : '添加事件'
          }
          onClick={addEventHandler}
        >
          + 事件
        </button>
      </div>
      <div className="field-label">进入效果</div>
      {stage.created.length === 0 && (
        <span className="hint">每次进入该 stage 按顺序执行</span>
      )}
      <ul className="selection-list">
        {stage.created.map((effect, index) => (
          <li key={index}>
            <button
              type="button"
              className="selection-item"
              onClick={() =>
                onSelect({ kind: 'created', stage: stage.name, index })
              }
            >
              {effectDisplayLabel(effect)}
            </button>
          </li>
        ))}
      </ul>
      <div className="action-add">
        <button
          type="button"
          className="link"
          onClick={() => addEffect({ set_state: 'in_progress' })}
        >
          + set_state
        </button>
        <button
          type="button"
          className="link"
          onClick={() =>
            addEffect({
              create_child: {
                workflow: workflowIds[0] ?? '',
                title: '子 Task',
              },
            })
          }
        >
          + create_child
        </button>
      </div>

      <div className="field-label">转移</div>
      {stage.transitions.length === 0 ? (
        <span className="hint">
          从 stage 右侧手柄拖线到目标 stage 或「结束」创建转移。
        </span>
      ) : (
        <ul className="selection-list">
          {stage.transitions.map((transition, index) => {
            const pattern = approvalPattern(stage, transition);
            return (
              <li key={index}>
                <button
                  type="button"
                  className="selection-item"
                  onClick={() =>
                    onSelect({ kind: 'transition', stage: stage.name, index })
                  }
                >
                  <span className="row-label">
                    {pattern && (
                      <span className="template-mark" title="全员确认模板">
                        模板
                      </span>
                    )}
                    → {transition.to === ENDED_STAGE ? '结束' : transition.to}
                  </span>
                  <span className="row-detail">
                    on {compactScript(transition.on)}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      )}

      <div className="template-block">
        <button
          type="button"
          className="link template-toggle"
          onClick={() => {
            setApprovalOpen((open) => !open);
            setApprovalRole(stage.active_roles[0] ?? '');
            setApprovalTarget(ENDED_STAGE);
          }}
          disabled={stage.active_roles.length === 0}
          title={
            stage.active_roles.length === 0
              ? '先为 stage 选择 active roles'
              : undefined
          }
        >
          添加全员确认
        </button>
        {approvalOpen && (
          <div className="template-form">
            <p className="hint">
              生成一个 stage 变量、一个 operation 和一条转移；三项都可继续编辑。
            </p>
            <label className="field">
              角色
              <select
                value={approvalRole}
                onChange={(event) => setApprovalRole(event.target.value)}
              >
                {stage.active_roles.map((role) => (
                  <option key={role} value={role}>
                    {role}
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              operation 名称
              <input
                value={approvalOperation}
                onChange={(event) => setApprovalOperation(event.target.value)}
              />
            </label>
            <label className="field">
              目标
              <select
                value={approvalTarget}
                onChange={(event) => setApprovalTarget(event.target.value)}
              >
                {body.stages.map((item) => (
                  <option key={item.name} value={item.name}>
                    {item.name}
                  </option>
                ))}
                <option value={ENDED_STAGE}>结束（ended）</option>
              </select>
            </label>
            <p className="hint">
              变量 approved（actor_set），operation 的 roles 只包含所选角色，
              转移追加到末尾。名称冲突时会自动追加后缀。
            </p>
            <button type="button" className="primary" onClick={applyApproval}>
              生成
            </button>
            {approvalError && (
              <p className="error-text">{approvalError}</p>
            )}
          </div>
        )}
      </div>

      <div className="row-actions">
        <button
          type="button"
          className="danger"
          disabled={incoming.length > 0}
          title={
            incoming.length > 0
              ? `仍被引用：${incoming.map((item) => item.label).join('；')}`
              : '删除 stage'
          }
          onClick={() => {
            const counts = [
              stage.operations.length > 0
                ? `${stage.operations.length} 个操作`
                : '',
              stage.events.length > 0 ? `${stage.events.length} 个事件` : '',
              stage.created.length > 0
                ? `${stage.created.length} 个进入效果`
                : '',
              stage.transitions.length > 0
                ? `${stage.transitions.length} 条转移`
                : '',
            ].filter(Boolean);
            const detail =
              counts.length > 0 ? `将一起删除：${counts.join('、')}。` : '';
            const initialNote = isInitial
              ? '删除后流程没有初始 stage，需要重新指定。'
              : '';
            if (
              (detail !== '' || initialNote !== '') &&
              !window.confirm(`删除 stage ${stage.name}？${detail}${initialNote}`)
            ) {
              return;
            }
            onChange((current) => ({
              ...current,
              initial_stage:
                current.initial_stage === stage.name
                  ? null
                  : current.initial_stage,
              stages: current.stages.filter(
                (item) => item.name !== stage.name,
              ),
            }));
            onSelect(null);
          }}
        >
          删除 stage
        </button>
      </div>
      {incoming.length > 0 && (
        <div className="reference-list">
          <span className="hint">删除前需要先移除引用：</span>
          {incoming.map((reference) => (
            <button
              type="button"
              className="link"
              key={reference.label}
              onClick={() => onSelect(reference.selection)}
            >
              {reference.label}
            </button>
          ))}
        </div>
      )}
    </section>
  );
}

function RoleInspector({
  body,
  index,
  role,
  readOnly,
  onChange,
  onSelect,
}: {
  body: WorkflowBody;
  index: number;
  role: RoleSpec;
  readOnly: boolean;
  onChange: BodyUpdate;
  onSelect(selection: Selection | null): void;
}) {
  const references = roleReferenceTargets(body, role.name);
  const set = (mutate: (value: RoleSpec) => void, options?: EditOptions) =>
    onChange((current) => {
      const roles = current.roles.map((item) => ({ ...item }));
      mutate(roles[index]);
      return { ...current, roles };
    }, options);
  const maxInvalid =
    role.max_actors != null && role.max_actors < role.min_actors;

  if (readOnly) {
    return (
      <section className="inspector-section">
        <h3>角色</h3>
        <DocField label="name">{role.name}</DocField>
        <DocField label="类型">{ROLE_KIND_LABELS[role.kind]}</DocField>
        <DocField label="min_actors">{role.min_actors}</DocField>
        <DocField label="max_actors">
          {role.max_actors ?? '不限'}
        </DocField>
      </section>
    );
  }

  return (
    <section className="inspector-section">
      <h3>角色</h3>
      <p className="stage-key" data-field="role-name">
        {role.name}
      </p>
      <p className="hint">
        角色 name 创建后不可修改：脚本以字符串引用角色，改名无法安全重写。
      </p>
      <label className="field">
        类型
        <select
          value={role.kind}
          onChange={(event) =>
            set((value) => (value.kind = event.target.value as RoleKind), {
              label: `修改角色 ${role.name} 类型`,
            })
          }
        >
          {(['human', 'agent', 'any'] as RoleKind[]).map((kind) => (
            <option key={kind} value={kind}>
              {ROLE_KIND_LABELS[kind]}
            </option>
          ))}
        </select>
      </label>
      <label className="field">
        min_actors
        <input
          type="number"
          min={0}
          value={role.min_actors}
          onChange={(event) =>
            set(
              (value) =>
                (value.min_actors = Math.max(
                  0,
                  Number(event.target.value) || 0,
                )),
              { coalesce: true, label: `修改角色 ${role.name} min_actors` },
            )
          }
        />
      </label>
      <label className="field">
        max_actors（留空为不限）
        <input
          type="number"
          min={0}
          aria-invalid={maxInvalid || undefined}
          value={role.max_actors ?? ''}
          onChange={(event) =>
            set(
              (value) =>
                (value.max_actors =
                  event.target.value === ''
                    ? null
                    : Number(event.target.value)),
              { coalesce: true, label: `修改角色 ${role.name} max_actors` },
            )
          }
        />
        {maxInvalid && (
          <span className="error-text">max_actors 不能小于 min_actors。</span>
        )}
      </label>
      <div className="row-actions">
        <button
          type="button"
          className="danger"
          disabled={references.length > 0}
          title={
            references.length > 0
              ? `仍被使用：${references.map((item) => item.label).join('；')}`
              : '删除角色'
          }
          onClick={() => {
            const next = selectionAfterRoleDelete(index, body.roles.length);
            onChange((current) => ({
              ...current,
              roles: current.roles.filter((_, i) => i !== index),
            }));
            onSelect(next);
          }}
        >
          删除角色
        </button>
      </div>
      {references.length > 0 && (
        <div className="reference-list">
          <span className="hint">仍在使用：</span>
          {references.map((reference) => (
            <button
              type="button"
              className="link"
              key={`${reference.label}-${JSON.stringify(reference.selection)}`}
              onClick={() => onSelect(reference.selection)}
            >
              {reference.label}
            </button>
          ))}
        </div>
      )}
    </section>
  );
}

function Breadcrumb({
  body,
  selection,
  onSelect,
  onFocusStage,
}: {
  body: WorkflowBody;
  selection: Selection;
  onSelect(selection: Selection | null): void;
  onFocusStage?(stage: string): void;
}) {
  const stage =
    selection.kind === 'role' || selection.kind === 'taskvar'
      ? undefined
      : findStage(body, selection.stage);
  let leaf: string | null = null;
  if (selection.kind === 'role') {
    leaf = `角色 ${body.roles[selection.index]?.name ?? ''}`;
  } else if (selection.kind === 'taskvar') {
    leaf = `任务变量 ${selection.name}`;
  } else if (selection.kind === 'operation' && stage) {
    const operation = stage.operations[selection.index];
    leaf = `操作 ${operation?.display_name || operation?.name || ''}`;
  } else if (selection.kind === 'event' && stage) {
    leaf = `事件 ${stage.events[selection.index]?.event ?? ''}`;
  } else if (selection.kind === 'created') {
    leaf = `进入效果 ${selection.index + 1}`;
  } else if (selection.kind === 'transition' && stage) {
    const to = stage.transitions[selection.index]?.to;
    leaf = `转移 → ${to === ENDED_STAGE ? '结束' : to ?? ''}`;
  }
  return (
    <nav className="breadcrumb" aria-label="当前位置">
      <button type="button" className="link" onClick={() => onSelect(null)}>
        所有 stage
      </button>
      {stage && (
        <>
          <span className="crumb-sep">/</span>
          <button
            type="button"
            className="link"
            onClick={() =>
              onFocusStage
                ? onFocusStage(stage.name)
                : onSelect({ kind: 'stage', stage: stage.name })
            }
          >
            {stage.name}
          </button>
        </>
      )}
      {leaf && (
        <>
          <span className="crumb-sep">/</span>
          <span className="crumb-leaf">{leaf}</span>
        </>
      )}
    </nav>
  );
}

function StageNav({
  body,
  validation,
  readOnly,
  onFocusStage,
}: {
  body: WorkflowBody;
  validation: Validation | null;
  readOnly: boolean;
  onFocusStage(stage: string): void;
}) {
  const [filter, setFilter] = useState('');
  const needle = filter.trim().toLowerCase();
  const stages = body.stages.filter((stage) =>
    stage.name.toLowerCase().includes(needle),
  );
  const counts = new Map<string, { errors: number; warnings: number }>();
  const add = (path: string, key: 'errors' | 'warnings') => {
    const match = /^stages\[(\d+)\]/.exec(path);
    if (!match) return;
    const stage = body.stages[Number(match[1])];
    if (!stage) return;
    counts.set(stage.name, {
      errors: counts.get(stage.name)?.errors ?? 0,
      warnings: counts.get(stage.name)?.warnings ?? 0,
      [key]: (counts.get(stage.name)?.[key] ?? 0) + 1,
    });
  };
  validation?.errors.forEach((issue) => add(issue.path, 'errors'));
  validation?.warnings.forEach((issue) => add(issue.path, 'warnings'));
  return (
    <div className="stage-nav">
      <p className="sidebar-empty">
        {readOnly
          ? '选择一个 stage 或对象查看详情。'
          : '选择一个对象开始编辑。拖动卡片调整布局，从右侧手柄拉出转移。'}
      </p>
      {body.stages.length > 12 && (
        <input
          value={filter}
          placeholder="筛选 stage"
          aria-label="筛选 stage"
          onChange={(event) => setFilter(event.target.value)}
        />
      )}
      <ul className="stage-nav-list">
        {stages.map((stage) => {
          const issue = counts.get(stage.name);
          return (
            <li key={stage.name}>
              <button
                type="button"
                className="stage-nav-item"
                onClick={() => onFocusStage(stage.name)}
              >
                <span>{stage.name}</span>
                {body.initial_stage === stage.name && (
                  <span className="pill">初始</span>
                )}
                <span className="spacer" />
                {issue?.errors ? (
                  <span className="issue-count error">{issue.errors}</span>
                ) : null}
                {issue?.warnings ? (
                  <span className="issue-count warning">
                    {issue.warnings}
                  </span>
                ) : null}
                <span className="hint">
                  {stage.operations.length} 操作 · {stage.transitions.length}{' '}
                  转移
                </span>
              </button>
            </li>
          );
        })}
      </ul>
      {stages.length === 0 && <p className="hint">还没有 stage。</p>}
    </div>
  );
}

export function Inspector({
  body,
  selection,
  readOnly,
  workflowIds,
  validation,
  onChange,
  onSelect,
  onFocusStage,
  onFocusField,
  onStageRenamed,
}: {
  body: WorkflowBody;
  selection: Selection | null;
  readOnly: boolean;
  workflowIds: string[];
  validation: Validation | null;
  onChange: BodyUpdate;
  onSelect(selection: Selection | null): void;
  onFocusStage?(stage: string): void;
  onFocusField?(field: string, stage?: string): void;
  onStageRenamed?(from: string, to: string): void;
}) {
  if (selection === null) {
    return (
      <div className="inspector">
        <StageNav
          body={body}
          validation={validation}
          readOnly={readOnly}
          onFocusStage={(stage) => {
            if (onFocusStage) onFocusStage(stage);
            else onSelect({ kind: 'stage', stage });
          }}
        />
      </div>
    );
  }
  if (selection.kind === 'taskvar') {
    return (
      <div className="inspector">
        <Breadcrumb body={body} selection={selection} onSelect={onSelect} />
        <p className="sidebar-empty">
          任务变量在下方“任务变量”面板中编辑；当前选中 {selection.name}。
        </p>
      </div>
    );
  }
  if (selection.kind === 'role') {
    const role = body.roles[selection.index];
    if (!role) return <p className="sidebar-empty">角色已不存在。</p>;
    return (
      <div className="inspector">
        <Breadcrumb body={body} selection={selection} onSelect={onSelect} />
        <RoleInspector
          body={body}
          index={selection.index}
          role={role}
          readOnly={readOnly}
          onChange={onChange}
          onSelect={onSelect}
        />
      </div>
    );
  }
  const stage = findStage(body, selection.stage);
  if (!stage) return <p className="sidebar-empty">stage 已不存在。</p>;

  let content: ReactNode;
  if (selection.kind === 'stage') {
    content = (
      <StageInspector
        body={body}
        stage={stage}
        readOnly={readOnly}
        workflowIds={workflowIds}
        onChange={onChange}
        onSelect={onSelect}
        onFocusField={onFocusField}
        onStageRenamed={onStageRenamed}
      />
    );
  } else if (selection.kind === 'operation') {
    const operation = stage.operations[selection.index];
    content = operation ? (
      <OperationInspector
        body={body}
        stage={stage}
        index={selection.index}
        operation={operation}
        update={onChange}
        readOnly={readOnly}
        workflowIds={workflowIds}
        onSelect={onSelect}
      />
    ) : (
      <p className="sidebar-empty">操作已不存在。</p>
    );
  } else if (selection.kind === 'event') {
    const event = stage.events[selection.index];
    content = event ? (
      <EventInspector
        body={body}
        stage={stage}
        index={selection.index}
        event={event}
        update={onChange}
        readOnly={readOnly}
        workflowIds={workflowIds}
        onSelect={onSelect}
      />
    ) : (
      <p className="sidebar-empty">事件已不存在。</p>
    );
  } else if (selection.kind === 'created') {
    const effect = stage.created[selection.index];
    content = effect ? (
      <CreatedInspector
        stage={stage}
        index={selection.index}
        effect={effect}
        update={onChange}
        readOnly={readOnly}
        workflowIds={workflowIds}
        onSelect={onSelect}
      />
    ) : (
      <p className="sidebar-empty">进入效果已不存在。</p>
    );
  } else {
    const transition = stage.transitions[selection.index];
    content = transition ? (
      <TransitionInspector
        body={body}
        stage={stage}
        index={selection.index}
        transition={transition}
        update={onChange}
        readOnly={readOnly}
        workflowIds={workflowIds}
        onSelect={onSelect}
        onFocusField={onFocusField}
      />
    ) : (
      <p className="sidebar-empty">转移已不存在。</p>
    );
  }

  return (
    <div className="inspector">
      <Breadcrumb
        body={body}
        selection={selection}
        onSelect={onSelect}
        onFocusStage={onFocusStage}
      />
      {content}
    </div>
  );
}
