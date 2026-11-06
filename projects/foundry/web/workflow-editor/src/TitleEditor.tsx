import { parseTitle, serializeTitle, type TitleSegment } from './model';

const FIELD_OPTIONS = ['task.title'] as const;

export function TitleEditor({
  value,
  onChange,
  workflowIds,
}: {
  value: string;
  onChange(value: string): void;
  workflowIds: string[];
}) {
  const segments = parseTitle(value);
  if (!segments) {
    return (
      <div className="title-editor">
        <p className="error-text">
          无法解析标题模板：<code>{value}</code>
        </p>
        <input
          value={value}
          onChange={(event) => onChange(event.target.value)}
          aria-label="标题模板"
        />
      </div>
    );
  }
  const update = (next: TitleSegment[]) => onChange(serializeTitle(next));
  return (
    <div className="title-editor">
      {segments.map((segment, index) => (
        <div className="title-segment" key={index}>
          {segment.kind === 'text' && (
            <input
              value={segment.text}
              placeholder="文本"
              onChange={(event) => {
                const copy = [...segments];
                copy[index] = { kind: 'text', text: event.target.value };
                update(copy);
              }}
            />
          )}
          {segment.kind === 'field' && (
            <select
              value={segment.field}
              onChange={(event) => {
                const copy = [...segments];
                copy[index] = {
                  kind: 'field',
                  field: event.target.value as (typeof FIELD_OPTIONS)[number],
                };
                update(copy);
              }}
            >
              {FIELD_OPTIONS.map((field) => (
                <option key={field} value={field}>
                  {field}
                </option>
              ))}
            </select>
          )}
          {segment.kind === 'count' && (
            <>
              <span className="hint">count_children(</span>
              <input
                value={segment.workflow}
                list="workflow-ids"
                aria-label="子流程 id"
                onChange={(event) => {
                  const copy = [...segments];
                  copy[index] = { ...segment, workflow: event.target.value };
                  update(copy);
                }}
              />
              <span className="hint">) +</span>
              {segment.workflow === '' && (
                <span className="error-text">请填写流程 id</span>
              )}
              <input
                type="number"
                min={0}
                value={segment.offset}
                aria-label="偏移"
                onChange={(event) => {
                  const copy = [...segments];
                  copy[index] = {
                    ...segment,
                    offset: Math.max(0, Number(event.target.value) || 0),
                  };
                  update(copy);
                }}
              />
            </>
          )}
          <button
            type="button"
            className="icon"
            title="删除片段"
            onClick={() => update(segments.filter((_, i) => i !== index))}
          >
            ×
          </button>
        </div>
      ))}
      <div className="title-add">
        <button
          type="button"
          className="link"
          onClick={() => update([...segments, { kind: 'text', text: '' }])}
        >
          + 文本
        </button>
        <button
          type="button"
          className="link"
          onClick={() =>
            update([...segments, { kind: 'field', field: 'task.title' }])
          }
        >
          + 字段
        </button>
        <button
          type="button"
          className="link"
          disabled={workflowIds.length === 0}
          title={
            workflowIds.length === 0
              ? '没有可引用的流程 id，暂时无法添加计数'
              : undefined
          }
          onClick={() =>
            update([
              ...segments,
              {
                kind: 'count',
                workflow: workflowIds[0] ?? '',
                offset: 1,
              },
            ])
          }
        >
          + 计数
        </button>
      </div>
      {workflowIds.length === 0 && (
        <p className="hint">没有可引用的流程 id，暂时无法添加计数。</p>
      )}
      <pre className="title-preview">{value}</pre>
    </div>
  );
}
