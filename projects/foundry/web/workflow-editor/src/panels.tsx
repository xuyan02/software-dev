import { useState } from 'react';
import {
  isKey,
  roleSummaryLabel,
  stageVarNames,
  variableReferences,
  type EditOptions,
  type Selection,
} from './model';
import type {
  RoleKind,
  Validation,
  WorkflowBody,
} from './types';
import { VariablesEditor } from './VariablesEditor';

interface BodyChange {
  (update: (body: WorkflowBody) => WorkflowBody, options?: EditOptions): void;
}

export function TaskVarsPanel({
  body,
  readOnly,
  selectedName,
  onChange,
}: {
  body: WorkflowBody;
  readOnly: boolean;
  selectedName: string | null;
  onChange: BodyChange;
}) {
  const existing = stageVarNames(body);
  return (
    <section className="panel" data-panel="taskvars">
      <h2>任务变量</h2>
      <p className="hint">
        跨 stage 保留。result 是内置任务级 string 变量，不在这里声明。
      </p>
      <VariablesEditor
        vars={body.vars}
        readOnly={readOnly}
        existing={existing}
        emptyHint="尚未声明任务变量"
        highlight={selectedName}
        references={(name) => variableReferences(body, name)}
        onChange={(vars, options) =>
          onChange((current) => ({ ...current, vars }), options)
        }
      />
    </section>
  );
}

export function RolesPanel({
  body,
  readOnly,
  selection,
  onSelect,
  onChange,
}: {
  body: WorkflowBody;
  readOnly: boolean;
  selection: Selection | null;
  onSelect(selection: Selection | null): void;
  onChange: BodyChange;
}) {
  const [name, setName] = useState('');
  const [kind, setKind] = useState<RoleKind>('human');
  const [error, setError] = useState<string | null>(null);

  const add = () => {
    const value = name.trim();
    if (!isKey(value)) {
      setError('角色 name 必须以小写字母开头，只能包含小写字母、数字和下划线。');
      return;
    }
    if (body.roles.some((role) => role.name === value)) {
      setError(`角色 ${value} 已存在。`);
      return;
    }
    setError(null);
    setName('');
    const index = body.roles.length;
    onChange((current) => ({
      ...current,
      roles: [
        ...current.roles,
        { name: value, kind, min_actors: 1, max_actors: null },
      ],
    }));
    onSelect({ kind: 'role', index });
  };

  return (
    <section className="panel">
      <h2>角色</h2>
      {body.roles.length === 0 && <p className="hint">尚未声明角色。</p>}
      <ul className="role-list">
        {body.roles.map((role, index) => (
          <li key={role.name}>
            <button
              type="button"
              className={
                selection?.kind === 'role' && selection.index === index
                  ? 'role active'
                  : 'role'
              }
              aria-pressed={
                selection?.kind === 'role' && selection.index === index
              }
              onClick={() => onSelect({ kind: 'role', index })}
            >
              <span>{role.name}</span>
              <span className="hint">{roleSummaryLabel(role)}</span>
            </button>
          </li>
        ))}
      </ul>
      {!readOnly && (
        <div className="role-add">
          <input
            id="new-role-name"
            value={name}
            placeholder="role name"
            aria-label="新角色 name"
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') add();
            }}
          />
          <select
            value={kind}
            aria-label="新角色类型"
            onChange={(event) => setKind(event.target.value as RoleKind)}
          >
            <option value="human">人类</option>
            <option value="agent">智能体</option>
            <option value="any">不限</option>
          </select>
          <button type="button" className="secondary" onClick={add}>
            添加
          </button>
        </div>
      )}
      {error && <p className="error-text">{error}</p>}
    </section>
  );
}

export function ValidationPanel({
  validation,
  stale,
  error,
  validating,
  onLocate,
  onRetry,
}: {
  validation: Validation | null;
  stale: boolean;
  error: string | null;
  validating: boolean;
  onLocate(path: string): void;
  onRetry(): void;
}) {
  const errors = validation?.errors ?? [];
  const warnings = validation?.warnings ?? [];

  let summary = '保存后自动校验。';
  if (validation) {
    if (errors.length > 0) {
      summary = `${errors.length} 个错误${
        warnings.length > 0 ? ` · ${warnings.length} 个警告` : ''
      }`;
    } else if (warnings.length > 0) {
      summary = `通过 · ${warnings.length} 个警告`;
    } else {
      summary = '通过，没有错误或警告。';
    }
  }

  return (
    <section className="panel" data-panel="validation">
      <h2>
        校验
        {validation && errors.length === 0 && (
          <span className="ok-text">通过</span>
        )}
        {validation && errors.length > 0 && (
          <span className="error-text">{errors.length} 个错误</span>
        )}
        {validation && warnings.length > 0 && (
          <span className="warning-text">{warnings.length} 个警告</span>
        )}
      </h2>
      {validating && <p className="hint">正在校验…</p>}
      {stale && validation && (
        <p className="hint">
          基于已保存版本；当前修改保存后重新校验。
        </p>
      )}
      {error && (
        <div className="validation-error">
          <p className="error-text">校验请求失败，结果可能不是最新。</p>
          <p className="hint">{error}</p>
          <button type="button" className="secondary" onClick={onRetry}>
            重试
          </button>
        </div>
      )}
      {validation === null && <p className="hint">{summary}</p>}
      <div
        className={error || validating ? 'validation-stale' : undefined}
        aria-busy={validating || undefined}
      >
        {validation !== null &&
          errors.length === 0 &&
          warnings.length === 0 && <p className="hint">{summary}</p>}
        {errors.map((issue, index) => (
          <button
            type="button"
            key={`error-${index}`}
            className="issue error"
            onClick={() => onLocate(issue.path)}
          >
            <code>{issue.path}</code>
            <span>{issue.message}</span>
          </button>
        ))}
        {warnings.map((issue, index) => (
          <button
            type="button"
            key={`warning-${index}`}
            className="issue warning"
            onClick={() => onLocate(issue.path)}
          >
            <code>{issue.path}</code>
            <span>{issue.message}</span>
          </button>
        ))}
      </div>
    </section>
  );
}
