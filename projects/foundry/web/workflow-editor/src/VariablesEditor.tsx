import { useEffect, useState } from 'react';
import { isKey, RESULT_VARIABLE, type EditOptions } from './model';
import type { InitialValue, VarSpec, VarType } from './types';

export const VAR_TYPES: VarType[] = ['bool', 'string', 'int', 'actor_set'];

export const VAR_TYPE_LABELS: Record<VarType, string> = {
  bool: 'bool（布尔）',
  string: 'string（字符串）',
  int: 'int（整数）',
  actor_set: 'actor_set（actor 集合）',
};

function displayValue(spec: VarSpec): string {
  if (spec.initial === undefined) return '默认';
  return String(spec.initial);
}

export function KeyInput({
  value,
  onCommit,
  label,
  readOnly,
  field,
}: {
  value: string;
  onCommit(value: string): boolean;
  label: string;
  readOnly: boolean;
  field?: string;
}) {
  const [draft, setDraft] = useState(value);
  const [invalid, setInvalid] = useState(false);
  useEffect(() => {
    setDraft(value);
    setInvalid(false);
  }, [value]);
  return (
    <label className="field">
      {label}
      <input
        value={draft}
        readOnly={readOnly}
        data-field={field}
        aria-invalid={invalid || undefined}
        onChange={(event) => {
          setDraft(event.target.value);
          setInvalid(false);
        }}
        onBlur={() => {
          if (draft.trim() === value) return;
          const ok = onCommit(draft.trim());
          setInvalid(!ok);
        }}
        onKeyDown={(event) => {
          if (event.key === 'Enter') event.currentTarget.blur();
          if (event.key === 'Escape') {
            setDraft(value);
            setInvalid(false);
            event.currentTarget.blur();
          }
        }}
      />
    </label>
  );
}

function InitialField({
  spec,
  readOnly,
  onChange,
}: {
  spec: VarSpec;
  readOnly: boolean;
  onChange(initial: InitialValue | undefined): void;
}) {
  if (readOnly) return null;
  if (spec.type === 'actor_set') return null;
  if (spec.type === 'bool') {
    return (
      <label className="checkbox">
        <input
          type="checkbox"
          checked={spec.initial === true}
          disabled={readOnly}
          onChange={(event) =>
            onChange(event.target.checked ? true : undefined)
          }
        />
        初始 true
      </label>
    );
  }
  return (
    <input
      className="var-initial"
      type={spec.type === 'int' ? 'number' : 'text'}
      value={spec.initial === undefined ? '' : String(spec.initial)}
      readOnly={readOnly}
      placeholder="初始值"
      aria-label={`${spec.display_name} 初始值`}
      onChange={(event) => {
        const value = event.target.value;
        if (value === '') {
          onChange(undefined);
        } else if (spec.type === 'int') {
          const parsed = Number(value);
          onChange(Number.isInteger(parsed) ? parsed : undefined);
        } else {
          onChange(value);
        }
      }}
    />
  );
}

export function VariablesEditor({
  vars,
  readOnly,
  existing = [],
  emptyHint,
  highlight,
  references,
  onChange,
}: {
  vars: Record<string, VarSpec>;
  readOnly: boolean;
  /** Names that are already taken elsewhere and cannot be used here. */
  existing?: string[];
  emptyHint: string;
  highlight?: string | null;
  /** Places a variable is referenced; used for the delete confirmation. */
  references?: (name: string) => string[];
  onChange(next: Record<string, VarSpec>, options?: EditOptions): void;
}) {
  const [name, setName] = useState('');
  const [type, setType] = useState<VarType>('bool');
  const [displayName, setDisplayName] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const entries = Object.entries(vars);

  const validName = (candidate: string, from?: string): string | null => {
    if (!isKey(candidate)) {
      return '变量名必须以小写字母开头，只能包含小写字母、数字和下划线。';
    }
    if (candidate === RESULT_VARIABLE) {
      return 'result 是内置任务级 string 变量，不能声明为变量。';
    }
    if (candidate !== from && (candidate in vars || existing.includes(candidate))) {
      return `变量 ${candidate} 已存在。`;
    }
    return null;
  };

  const rename = (from: string, to: string): boolean => {
    if (from === to) return true;
    const problem = validName(to, from);
    if (problem) {
      setError(problem);
      return false;
    }
    setError(null);
    const remaining = references?.(from).length ?? 0;
    setNotice(
      remaining > 0
        ? `${remaining} 处脚本仍使用旧名称 ${from}，校验会定位；请按需手动修改。`
        : null,
    );
    onChange(
      Object.fromEntries(
        Object.entries(vars).map(([key, spec]) =>
          key === from ? [to, spec] : [key, spec],
        ),
      ),
      { coalesce: true, label: `重命名变量 ${from}` },
    );
    return true;
  };

  const add = () => {
    const variableName = name.trim();
    const problem = validName(variableName);
    if (problem) {
      setError(problem);
      return;
    }
    setError(null);
    setNotice(null);
    setName('');
    setDisplayName('');
    onChange({
      ...vars,
      [variableName]: { type, display_name: displayName || variableName },
    });
  };

  const remove = (key: string) => {
    const used = references?.(key) ?? [];
    const message =
      used.length > 0
        ? `删除变量 ${key}？以下位置仍在使用它，删除后会校验失败：\n${used.join('\n')}`
        : `删除变量 ${key}？`;
    if (!window.confirm(message)) return;
    const next = { ...vars };
    delete next[key];
    onChange(next);
  };

  if (readOnly) {
    if (entries.length === 0) {
      return <span className="hint">{emptyHint}</span>;
    }
    return (
      <dl className="doc-list">
        {entries.map(([key, spec]) => (
          <div
            className={[
              'doc-var',
              key === highlight ? 'field-flash' : '',
            ]
              .filter(Boolean)
              .join(' ')}
            key={key}
            data-var={key}
          >
            <dt>
              {spec.display_name}
              <span className="doc-key">{key}</span>
            </dt>
            <dd>
              {VAR_TYPE_LABELS[spec.type]}
              {spec.initial !== undefined && ` · 初始 ${displayValue(spec)}`}
            </dd>
          </div>
        ))}
      </dl>
    );
  }

  return (
    <div className="stack">
      {entries.length === 0 && <span className="hint">{emptyHint}</span>}
      {entries.map(([key, spec]) => (
        <div
          className={[
            'field',
            'var-field',
            key === highlight ? 'field-flash' : '',
          ]
            .filter(Boolean)
            .join(' ')}
          key={key}
          data-var={key}
        >
          <div className="var-row">
            <KeyInput
              key={key}
              value={key}
              label=""
              readOnly={false}
              field={`var-name:${key}`}
              onCommit={(next) => rename(key, next)}
            />
            <select
              value={spec.type}
              aria-label={`${key} 类型`}
              onChange={(event) =>
                onChange(
                  {
                    ...vars,
                    [key]: {
                      ...spec,
                      type: event.target.value as VarType,
                      initial: undefined,
                    },
                  },
                  { label: `修改变量 ${key} 类型` },
                )
              }
            >
              {VAR_TYPES.map((item) => (
                <option key={item} value={item}>
                  {item}
                </option>
              ))}
            </select>
            <input
              value={spec.display_name}
              placeholder="显示名"
              aria-label={`${key} display_name`}
              onChange={(event) =>
                onChange(
                  {
                    ...vars,
                    [key]: { ...spec, display_name: event.target.value },
                  },
                  { coalesce: true, label: `修改变量 ${key} 显示名` },
                )
              }
            />
            <InitialField
              spec={spec}
              readOnly={false}
              onChange={(initial) =>
                onChange(
                  { ...vars, [key]: { ...spec, initial } },
                  { label: `修改变量 ${key} 初始值` },
                )
              }
            />
            <button
              type="button"
              className="icon"
              title="删除变量"
              aria-label={`删除变量 ${key}`}
              onClick={() => remove(key)}
            >
              ×
            </button>
          </div>
        </div>
      ))}
      <div className="role-add">
        <input
          value={name}
          placeholder="变量名"
          aria-label="新变量名"
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') add();
          }}
        />
        <select
          value={type}
          aria-label="新变量类型"
          onChange={(event) => setType(event.target.value as VarType)}
        >
          {VAR_TYPES.map((item) => (
            <option key={item} value={item}>
              {item}
            </option>
          ))}
        </select>
        <input
          value={displayName}
          placeholder="显示名"
          aria-label="新变量显示名"
          onChange={(event) => setDisplayName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') add();
          }}
        />
        <button type="button" className="secondary" onClick={add}>
          添加
        </button>
      </div>
      {error && <p className="error-text">{error}</p>}
      {notice && (
        <p className="hint" role="status">
          {notice}
        </p>
      )}
    </div>
  );
}
