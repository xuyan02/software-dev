import { useEffect, useRef, useState } from 'react';
import { ApiError, api } from './api';
import { FlowCanvas } from './canvas';
import { BASE } from './Editor';
import { DiffPanel } from './DiffPanel';
import { diffBodies } from './diff';
import { Inspector } from './Inspector';
import { mergePositions } from './layout';
import type { Selection } from './model';
import { RolesPanel, TaskVarsPanel } from './panels';
import {
  type WorkflowBody,
  type WorkflowDefinition,
  type WorkflowVersion,
  type WorkflowVersionInfo,
} from './types';

interface StagePoint {
  x: number;
  y: number;
}

function errorText(error: unknown): string {
  if (error instanceof ApiError) return `${error.message}（${error.code}）`;
  return error instanceof Error ? error.message : String(error);
}

function navigate(path: string): void {
  window.location.assign(`${BASE}${path}`);
}

export function VersionView({
  workflow,
  version,
}: {
  workflow: string;
  version: number;
}) {
  const [status, setStatus] = useState<'loading' | 'ready' | 'error' | 'login'>(
    'loading',
  );
  const [error, setError] = useState('');
  const [definition, setDefinition] = useState<WorkflowDefinition | null>(null);
  const [versions, setVersions] = useState<WorkflowVersionInfo[]>([]);
  const [catalog, setCatalog] = useState<WorkflowDefinition[]>([]);
  const [loaded, setLoaded] = useState<WorkflowVersion | null>(null);
  const [body, setBody] = useState<WorkflowBody | null>(null);
  const [positions, setPositions] = useState<Record<string, StagePoint>>({});
  const [selection, setSelection] = useState<Selection | null>(null);
  const [copying, setCopying] = useState(false);
  const [copyTitle, setCopyTitle] = useState('');
  const [message, setMessage] = useState<{
    kind: 'ok' | 'error';
    text: string;
  } | null>(null);
  const [compareVersion, setCompareVersion] = useState<number | null>(null);
  const [compareBody, setCompareBody] = useState<WorkflowBody | null>(null);
  const [compareError, setCompareError] = useState('');
  const compareToken = useRef(0);

  useEffect(() => {
    let cancelled = false;
    setStatus('loading');
    compareToken.current += 1;
    setCompareVersion(null);
    setCompareBody(null);
    setCompareError('');
    async function load() {
      try {
        const [definition, versions, versionBody, layout, catalog] =
          await Promise.all([
            api.definition(workflow),
            api.versions(workflow),
            api.version(workflow, version),
            api.layout(workflow).catch(() => ({ positions: {} })),
            api.list().catch(() => []),
          ]);
        if (cancelled) return;
        setDefinition(definition);
        setVersions(versions);
        setCatalog(catalog);
        setLoaded(versionBody);
        setBody(versionBody.body);
        setPositions(mergePositions(versionBody.body, layout));
        setCopyTitle(`${definition.title}（副本）`);
        setStatus('ready');
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
    };
  }, [workflow, version]);

  useEffect(() => {
    if (message?.kind !== 'ok') return;
    const timer = setTimeout(() => setMessage(null), 6000);
    return () => clearTimeout(timer);
  }, [message]);

  const chooseCompare = async (value: string) => {
    const token = compareToken.current + 1;
    compareToken.current = token;
    setCompareError('');
    setCompareBody(null);
    if (value === '') {
      setCompareVersion(null);
      return;
    }
    const selected = Number(value);
    setCompareVersion(selected);
    try {
      const other = await api.version(workflow, selected);
      if (compareToken.current !== token) return;
      setCompareBody(other.body);
    } catch (failure) {
      if (compareToken.current !== token) return;
      setCompareError(errorText(failure));
    }
  };

  const copy = async () => {
    if (!definition) return;
    const title = copyTitle.trim();
    if (title === '') {
      setMessage({ kind: 'error', text: '标题不能为空。' });
      return;
    }
    setCopying(true);
    setMessage(null);
    try {
      const created = await api.copyFromPlatform(title, workflow, version);
      navigate(`${encodeURIComponent(created.definition.id)}?copied=1`);
    } catch (failure) {
      if (failure instanceof ApiError && failure.status === 401) {
        setStatus('login');
      } else {
        setMessage({ kind: 'error', text: errorText(failure) });
      }
    } finally {
      setCopying(false);
    }
  };

  if (status === 'loading') {
    return <div className="full-screen">正在加载版本…</div>;
  }
  if (status === 'login') {
    return (
      <div className="full-screen">
        <p>会话已失效。</p>
        <a href="/login">重新登录</a>
      </div>
    );
  }
  if (status === 'error' || !definition || !loaded || !body) {
    return (
      <div className="full-screen">
        <p>{error || '版本不存在。'}</p>
        <a href="/workflows">返回流程列表</a>
      </div>
    );
  }

  const workflowIds = Array.from(
    new Set([...catalog.map((item) => item.id), definition.id]),
  );
  const platform = definition.owner.kind === 'platform';
  const canCompare = versions.length > 1;

  return (
    <div className="editor">
      <header className="toolbar">
        <a className="back" href="/workflows">
          ← 流程
        </a>
        <div className="title-block">
          <h1>{definition.title}</h1>
          <span className="hint">历史版本（只读）</span>
        </div>
        <span className="toolbar-divider" aria-hidden="true" />
        <span className="badge readonly">只读 · v{loaded.version}</span>
        <select
          className="version-select"
          aria-label="版本"
          value={loaded.version}
          onChange={(event) =>
            navigate(
              `${encodeURIComponent(workflow)}/versions/${event.target.value}`,
            )
          }
        >
          {versions.map((item) => (
            <option key={item.version} value={item.version}>
              v{item.version}
            </option>
          ))}
        </select>
        <select
          className="version-select"
          value={compareVersion ?? ''}
          aria-label="对比版本"
          disabled={!canCompare}
          title={canCompare ? undefined : '没有其他版本可对比'}
          onChange={(event) => void chooseCompare(event.target.value)}
        >
          <option value="">对比版本…</option>
          {versions
            .filter((item) => item.version !== loaded.version)
            .map((item) => (
              <option key={item.version} value={item.version}>
                对比 v{item.version}
              </option>
            ))}
        </select>
        <span className="hint">
          {loaded.published_by_display_name} ·{' '}
          {new Date(loaded.published_at).toLocaleString()}
        </span>
        <div className="spacer" />
        {definition.editable && (
          <a
            className="button secondary"
            href={`${BASE}${encodeURIComponent(workflow)}`}
          >
            返回草稿
          </a>
        )}
        {platform && (
          <>
            <input
              className="copy-title"
              value={copyTitle}
              aria-label="新流程标题"
              onChange={(event) => setCopyTitle(event.target.value)}
            />
            <button
              type="button"
              className="primary"
              disabled={copying}
              onClick={() => void copy()}
            >
              复制到我的账户
            </button>
          </>
        )}
      </header>

      {message && (
        <div
          className={`banner ${message.kind === 'ok' ? 'ok' : 'error'}`}
          role={message.kind === 'error' ? 'alert' : 'status'}
        >
          <span>{message.text}</span>
        </div>
      )}

      <div className="main">
        <div className="canvas-wrap">
          <FlowCanvas
            body={body}
            positions={positions}
            selection={selection}
            readOnly
            focusRequest={null}
            fitRequest={0}
            onSelect={setSelection}
            onMove={() => {}}
          />
        </div>
        <aside className="sidebar">
          {compareVersion !== null && (
            <DiffPanel
              base={compareVersion}
              target={loaded.version}
              entries={compareBody ? diffBodies(compareBody, body) : null}
              error={compareError}
              onSelect={setSelection}
              onClose={() => {
                compareToken.current += 1;
                setCompareVersion(null);
                setCompareBody(null);
                setCompareError('');
              }}
            />
          )}
          <Inspector
            body={body}
            selection={selection}
            readOnly
            workflowIds={workflowIds}
            validation={null}
            onChange={() => {}}
            onSelect={setSelection}
            onFocusStage={(stage) => setSelection({ kind: 'stage', stage })}
            onFocusField={(_, stage) => {
              if (stage) setSelection({ kind: 'stage', stage });
            }}
          />
          <TaskVarsPanel
            body={body}
            readOnly
            selectedName={selection?.kind === 'taskvar' ? selection.name : null}
            onChange={() => {}}
          />
          <RolesPanel
            body={body}
            readOnly
            selection={selection}
            onSelect={setSelection}
            onChange={() => {}}
          />
        </aside>
      </div>

      <footer className="statusbar">
        <span>版本 v{loaded.version} 不可编辑</span>
        <span>布局为共享元数据，不属于该版本</span>
        <span className="spacer" />
        <span>
          {body.stages.length} 个 stage · {body.roles.length} 个角色
        </span>
      </footer>
    </div>
  );
}
