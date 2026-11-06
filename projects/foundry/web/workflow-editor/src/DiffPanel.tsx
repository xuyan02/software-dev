import type { DiffEntry, DiffKind } from './diff';
import type { Selection } from './model';

const KIND_LABEL: Record<DiffKind, string> = {
  added: '新增',
  removed: '删除',
  changed: '修改',
};

interface Group {
  key: string;
  label: string;
  entries: DiffEntry[];
}

const GROUP_ORDER: { key: string; label: string; match(scope: string): boolean }[] =
  [
    {
      key: 'stage',
      label: '阶段',
      match: (scope) => scope === 'stage' || scope.startsWith('stage '),
    },
    {
      key: 'role',
      label: '角色',
      match: (scope) => scope === '角色' || scope.startsWith('角色 '),
    },
    { key: 'taskvar', label: '任务变量', match: (scope) => scope === '任务变量' },
    {
      key: 'initial',
      label: '初始 stage',
      match: (scope) => scope === '初始 stage',
    },
    { key: 'schema', label: 'schema', match: (scope) => scope === 'schema' },
  ];

function groupEntries(entries: DiffEntry[]): Group[] {
  const groups: Group[] = [];
  for (const entry of entries) {
    const definition =
      GROUP_ORDER.find((group) => group.match(entry.scope)) ?? {
        key: 'other',
        label: '其他',
      };
    let group = groups.find((item) => item.key === definition.key);
    if (!group) {
      group = { key: definition.key, label: definition.label, entries: [] };
      groups.push(group);
    }
    group.entries.push(entry);
  }
  const order = new Map(GROUP_ORDER.map((group, index) => [group.key, index]));
  return groups.sort(
    (left, right) =>
      (order.get(left.key) ?? GROUP_ORDER.length) -
      (order.get(right.key) ?? GROUP_ORDER.length),
  );
}

export function DiffPanel({
  base,
  target,
  entries,
  error,
  onSelect,
  onClose,
}: {
  base: number;
  target: number;
  entries: DiffEntry[] | null;
  error: string;
  onSelect(selection: Selection): void;
  onClose(): void;
}) {
  const groups = entries ? groupEntries(entries) : [];
  return (
    <section
      className="panel diff-panel"
      aria-label={`版本 v${base} 与 v${target} 的差异`}
    >
      <h2>
        v{base} → v{target}
        {entries && <span className="hint">{entries.length} 处变化</span>}
        <span className="spacer" />
        <button
          type="button"
          className="icon"
          aria-label="关闭版本对比"
          onClick={onClose}
        >
          ×
        </button>
      </h2>
      {entries === null ? (
        <p className="hint">{error || '正在载入对比版本…'}</p>
      ) : entries.length === 0 ? (
        <p className="hint">两个版本的流程定义一致。</p>
      ) : (
        <div className="diff-groups">
          {groups.map((group) => (
            <section className="diff-group" key={group.key}>
              <h3>
                {group.label}
                <span className="hint">{group.entries.length}</span>
              </h3>
              <ul className="diff-list">
                {group.entries.map((entry, index) => (
                  <li key={index} className={`diff-${entry.kind}`}>
                    <button
                      type="button"
                      disabled={entry.selection === null}
                      title={
                        entry.selection
                          ? '定位到流程图'
                          : '已删除，无法定位'
                      }
                      onClick={() => {
                        if (entry.selection) onSelect(entry.selection);
                      }}
                    >
                      <span className="diff-kind">{KIND_LABEL[entry.kind]}</span>
                      <span className="diff-scope">{entry.scope}</span>
                      <span className="diff-text">{entry.text}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </div>
      )}
    </section>
  );
}
