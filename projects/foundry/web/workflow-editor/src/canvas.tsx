import {
  Background,
  Controls,
  Handle,
  Position,
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
  type Connection,
  type Edge,
  type Node,
  type NodeProps,
  type NodeTypes,
  type OnNodesChange,
} from '@xyflow/react';
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
} from 'react';
import {
  MARKER_HEIGHT,
  MARKER_WIDTH,
  STAGE_HEIGHT,
  STAGE_WIDTH,
  markerPositions,
} from './layout';
import {
  END_NODE,
  START_NODE,
  deriveEdges,
  sameSelection,
  type Selection,
} from './model';
import type { WorkflowBody } from './types';

interface Point {
  x: number;
  y: number;
}

interface StageNodeData extends Record<string, unknown> {
  name: string;
  isInitial: boolean;
  readOnly: boolean;
  /** The selection belongs to an object inside this stage. */
  containsSelection: boolean;
  /** The palette object being dragged over this stage. */
  paletteDrop: 'operation' | 'subtask' | null;
}

type StageNode = Node<StageNodeData, 'stage'>;

interface MarkerNodeData extends Record<string, unknown> {
  label: string;
  side: 'start' | 'end';
  connected: boolean;
  readOnly: boolean;
}

type MarkerNode = Node<MarkerNodeData, 'marker'>;

type FlowEdgeData = { selection: Selection | null };
type FlowEdge = Edge<FlowEdgeData>;

export type PaletteKind = 'stage' | 'operation' | 'subtask';

export type PalettePlacementKind = Exclude<PaletteKind, 'stage'>;

interface PaletteDragState {
  kind: PaletteKind;
  /** Pointer position in viewport coordinates. */
  start: Point;
  current: Point;
  /** Stage under the pointer when dragging an operation or subtask. */
  targetStage: string | null;
  overCanvas: boolean;
  moved: boolean;
}

const OPERATION_LABEL_HEIGHT = 20;
const OPERATION_LABEL_GAP = 4;

/** One operation's visual representation: a compact label above its stage,
 *  connected to the stage top edge by a short stem. Coordinates are relative
 *  to the flow area, so they move with pan and zoom. */
interface OperationView {
  stage: string;
  index: number;
  label: string;
  /** Label center, also the operation's port on the stage top edge. */
  x: number;
  labelTop: number;
  labelWidth: number;
  stageTop: number;
}

function operationKey(stage: string, index: number): string {
  return `operation:${stage}:${index}`;
}

const PALETTE_LABELS: Record<PaletteKind, string> = {
  stage: '阶段',
  operation: '操作',
  subtask: '子任务',
};

const PALETTE_TITLES: Record<PaletteKind, string> = {
  stage: '拖到画布空白处新建 stage；单击在默认位置新建',
  operation: '拖到 stage 新建操作；单击后再点击 stage',
  subtask: '拖到 stage 新建子任务（进入时创建子 Task）；单击后再点击 stage',
};

/** Resolves the stage under a viewport point. Operation labels sit above
 *  their stage and count as the same target. */
function stageAt(
  x: number,
  y: number,
  body: WorkflowBody,
): string | null {
  const element = document.elementFromPoint(x, y);
  if (!(element instanceof Element)) return null;
  const labelStage =
    element
      .closest<HTMLElement>('[data-operation-stage]')
      ?.getAttribute('data-operation-stage') ?? null;
  if (labelStage !== null && body.stages.some((stage) => stage.name === labelStage)) {
    return labelStage;
  }
  const nodeElement = element.closest<HTMLElement>('.react-flow__node');
  const stageName = nodeElement?.dataset.id;
  if (!stageName || stageName === START_NODE || stageName === END_NODE) {
    return null;
  }
  return body.stages.some((stage) => stage.name === stageName)
    ? stageName
    : null;
}

function sameOperationViews(
  current: OperationView[],
  next: OperationView[],
): boolean {
  if (current.length !== next.length) return false;
  return current.every((view, index) => {
    const other = next[index];
    return (
      view.stage === other.stage &&
      view.index === other.index &&
      view.label === other.label &&
      view.x === other.x &&
      view.labelTop === other.labelTop &&
      view.labelWidth === other.labelWidth &&
      view.stageTop === other.stageTop
    );
  });
}

function PaletteIcon({ kind }: { kind: PaletteKind }) {
  return (
    <svg
      className="palette-icon"
      viewBox="0 0 16 16"
      aria-hidden="true"
      focusable="false"
    >
      {kind === 'stage' && (
        <rect x="1.5" y="3.5" width="13" height="9" rx="2" />
      )}
      {kind === 'operation' && (
        <>
          <circle cx="8" cy="8" r="6" />
          <path className="palette-icon-fill" d="M6.5 5.5 L10.5 8 L6.5 10.5 Z" />
        </>
      )}
      {kind === 'subtask' && (
        <>
          <rect x="1.5" y="2.5" width="13" height="7" rx="1.5" />
          <path d="M8 9.5 V13.5 M5.5 11.5 L8 14 L10.5 11.5" />
        </>
      )}
    </svg>
  );
}

function ObjectPalette({
  armed,
  dragging,
  onStartDrag,
  onActivate,
}: {
  armed: PalettePlacementKind | null;
  dragging: PaletteKind | null;
  onStartDrag(
    kind: PaletteKind,
    event: ReactPointerEvent<HTMLButtonElement>,
  ): void;
  onActivate(kind: PaletteKind, event: ReactMouseEvent<HTMLButtonElement>): void;
}) {
  return (
    <div className="object-palette" role="toolbar" aria-label="添加对象">
      <span className="palette-title">添加</span>
      {(['stage', 'operation', 'subtask'] as PaletteKind[]).map((kind) => (
        <button
          key={kind}
          type="button"
          className={[
            'palette-item',
            dragging === kind ? 'dragging' : '',
            armed === kind ? 'armed' : '',
          ]
            .filter(Boolean)
            .join(' ')}
          title={PALETTE_TITLES[kind]}
          aria-label={PALETTE_TITLES[kind]}
          aria-pressed={kind === 'stage' ? undefined : armed === kind}
          onPointerDown={(event) => onStartDrag(kind, event)}
          onClick={(event) => onActivate(kind, event)}
        >
          <PaletteIcon kind={kind} />
          <span>{PALETTE_LABELS[kind]}</span>
        </button>
      ))}
      <span className="palette-hint">
        拖到画布或 stage；单击后再点 stage 也可以
      </span>
    </div>
  );
}

function StageNodeView({ data, selected }: NodeProps<StageNode>) {
  return (
    <div
      className={[
        'stage-node',
        selected ? 'selected' : '',
        data.isInitial ? 'initial' : '',
        data.readOnly ? 'readonly nopan' : '',
        data.containsSelection ? 'contains-selection' : '',
        data.paletteDrop !== null ? 'palette-drop-stage' : '',
      ]
        .filter(Boolean)
        .join(' ')}
      style={{ width: STAGE_WIDTH }}
    >
      {/* 只读视图也要渲染手柄，否则 React Flow 无法为连线的端点定位。 */}
      <Handle
        type="target"
        position={Position.Left}
        isConnectable={!data.readOnly}
        className={data.readOnly ? 'handle-hidden' : undefined}
      />
      <header className="stage-header">
        {!data.readOnly && (
          <span
            className="stage-grip"
            aria-hidden="true"
            title="拖动调整位置（只改布局，不产生版本）"
          >
            ⠿
          </span>
        )}
        <span className="stage-key">{data.name}</span>
        {data.isInitial && <span className="pill">初始</span>}
      </header>
      <Handle
        type="source"
        position={Position.Right}
        isConnectable={!data.readOnly}
        className={data.readOnly ? 'handle-hidden' : undefined}
      />
    </div>
  );
}

function OperationOverlay({
  views,
  selection,
  hovered,
  onHover,
  onSelect,
}: {
  views: OperationView[];
  selection: Selection | null;
  hovered: string | null;
  onHover(key: string | null): void;
  onSelect(selection: Selection): void;
}) {
  return (
    <div className="operation-layer">
      <svg className="operation-line-layer" aria-hidden="true">
        {views.map((view) => {
          const key = operationKey(view.stage, view.index);
          const selected =
            selection?.kind === 'operation' &&
            selection.stage === view.stage &&
            selection.index === view.index;
          return (
            <g
              key={key}
              className={[
                'operation-view',
                selected ? 'selected' : '',
                hovered === key ? 'hovered' : '',
              ]
                .filter(Boolean)
                .join(' ')}
            >
              {view.labelTop + OPERATION_LABEL_HEIGHT < view.stageTop - 1 && (
                <line
                  className="operation-stem"
                  x1={view.x}
                  y1={view.labelTop + OPERATION_LABEL_HEIGHT}
                  x2={view.x}
                  y2={view.stageTop}
                />
              )}
              <circle
                className="operation-port"
                cx={view.x}
                cy={view.stageTop}
                r={2.5}
              />
            </g>
          );
        })}
      </svg>
      {views.map((view) => {
        const key = operationKey(view.stage, view.index);
        const selected =
          selection?.kind === 'operation' &&
          selection.stage === view.stage &&
          selection.index === view.index;
        return (
          <button
            key={key}
            type="button"
            className={[
              'operation-label',
              selected ? 'selected' : '',
              hovered === key ? 'hovered' : '',
            ]
              .filter(Boolean)
              .join(' ')}
            style={{
              left: view.x - view.labelWidth / 2,
              top: view.labelTop,
              width: view.labelWidth,
            }}
            data-operation-stage={view.stage}
            data-operation-index={view.index}
            title="点击编辑操作"
            aria-label={`操作 ${view.label}，stage ${view.stage}`}
            onPointerEnter={() => onHover(key)}
            onPointerLeave={() => onHover(null)}
            onClick={() =>
              onSelect({
                kind: 'operation',
                stage: view.stage,
                index: view.index,
              })
            }
          >
            <PaletteIcon kind="operation" />
            <span className="operation-label-text">{view.label}</span>
          </button>
        );
      })}
    </div>
  );
}

function MarkerNodeView({ data, isConnectable }: NodeProps<MarkerNode>) {
  const connectable = isConnectable && !data.readOnly;
  const description = data.readOnly
    ? data.side === 'start'
      ? '开始标记。只读视图不能拖动。'
      : '结束标记。只读视图不能拖动。'
    : data.side === 'start'
      ? data.connected
        ? '拖动调整开始标记的位置（只改布局）；从右侧手柄拖线可更改初始 stage'
        : '还没有初始 stage。拖动调整位置；从「开始」拖线到 stage，或在 stage 检查器里勾选「初始 stage」。'
      : '拖动调整结束标记的位置（只改布局）';
  return (
    <div
      className={[
        'marker-node nopan',
        `marker-${data.side}`,
        data.readOnly ? '' : 'marker-draggable',
        data.connected ? '' : 'marker-disconnected',
      ]
        .filter(Boolean)
        .join(' ')}
      title={description}
      aria-label={description}
    >
      {data.side === 'end' && (
        <Handle
          type="target"
          position={Position.Left}
          isConnectable={connectable}
          className={data.readOnly ? 'handle-hidden' : undefined}
        />
      )}
      {data.label}
      {data.side === 'start' && (
        <Handle
          type="source"
          position={Position.Right}
          isConnectable={connectable}
          className={data.readOnly ? 'handle-hidden' : undefined}
        />
      )}
    </div>
  );
}

const nodeTypes = {
  stage: StageNodeView,
  marker: MarkerNodeView,
} as NodeTypes;

function reducedMotion(): boolean {
  return (
    typeof window !== 'undefined' &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
  );
}

export interface FocusRequest {
  stage: string | null;
  token: number;
}

export interface FlowCanvasProps {
  body: WorkflowBody;
  positions: Record<string, Point>;
  selection: Selection | null;
  readOnly: boolean;
  focusRequest: FocusRequest | null;
  fitRequest: number;
  onSelect(selection: Selection | null): void;
  onMove(stage: string, position: Point): void;
  onNodeDragStart?(stage: string): void;
  onConnect?(connection: Connection): void;
  /** An operation palette item was dropped on a stage. */
  onAddOperation?(stage: string): void;
  /** A subtask palette item was dropped on a stage. */
  onAddSubtask?(stage: string): void;
  onAddStageAt?(position?: Point): void;
}

function CanvasInner({
  body,
  positions,
  selection,
  readOnly,
  focusRequest,
  fitRequest,
  onSelect,
  onMove,
  onNodeDragStart,
  onConnect,
  onAddOperation,
  onAddSubtask,
  onAddStageAt,
}: FlowCanvasProps) {
  const instance = useReactFlow();
  const wrapper = useRef<HTMLDivElement | null>(null);
  const stack = useRef<HTMLDivElement | null>(null);
  // React Flow writes measured sizes through onNodesChange. Keep them on the
  // controlled nodes so it never falls back to an unmeasured node while dragging.
  const [measured, setMeasured] = useState<
    Record<string, { width: number; height: number }>
  >({});
  const [paletteDrag, setPaletteDrag] = useState<PaletteDragState | null>(null);
  const paletteDragRef = useRef<PaletteDragState | null>(null);
  paletteDragRef.current = paletteDrag;
  const [armedPalette, setArmedPalette] =
    useState<PalettePlacementKind | null>(null);
  const armedPaletteRef = useRef<PalettePlacementKind | null>(null);
  armedPaletteRef.current = armedPalette;
  const dragMoved = useRef(false);
  const bodyRef = useRef(body);
  bodyRef.current = body;
  // Panning and zooming move the stage cards without changing AST positions,
  // so operation labels must be re-measured from the DOM.
  const [geometryTick, setGeometryTick] = useState(0);
  const [operationViews, setOperationViews] = useState<OperationView[]>([]);
  const [hoveredOperation, setHoveredOperation] = useState<string | null>(null);

  const markers = useMemo(
    () => markerPositions(body, positions),
    [body, positions],
  );

  // Only replace this value when the drop target actually changes, so pointer
  // moves across the same stage do not rebuild every node.
  const paletteDropStage =
    paletteDrag !== null && paletteDrag.kind !== 'stage'
      ? paletteDrag.targetStage
      : null;
  const paletteDropKind =
    paletteDrag !== null && paletteDrag.kind !== 'stage'
      ? paletteDrag.kind
      : null;

  const nodes = useMemo<(StageNode | MarkerNode)[]>(() => {
    const stageNodes: StageNode[] = body.stages.map((stage) => {
      return {
        id: stage.name,
        type: 'stage',
        position: positions[stage.name] ?? { x: 0, y: 0 },
        data: {
          name: stage.name,
          isInitial: body.initial_stage === stage.name,
          readOnly,
          containsSelection:
            selection !== null &&
            selection.kind !== 'stage' &&
            selection.kind !== 'role' &&
            selection.kind !== 'taskvar' &&
            selection.stage === stage.name,
          paletteDrop:
            paletteDropKind !== null && paletteDropStage === stage.name
              ? paletteDropKind
              : null,
        },
        selected:
          selection?.kind === 'stage' && selection.stage === stage.name,
        draggable: !readOnly,
        connectable: !readOnly,
        width: STAGE_WIDTH,
        height: STAGE_HEIGHT,
        measured: measured[stage.name],
        ariaLabel: `stage ${stage.name}${
          body.initial_stage === stage.name ? '，初始' : ''
        }`,
      };
    });
    return [
      {
        id: START_NODE,
        type: 'marker',
        position: markers.start,
        data: {
          label:
            body.initial_stage === null ? '开始（未连接）' : '开始',
          side: 'start',
          connected: body.initial_stage !== null,
          readOnly,
        },
        draggable: !readOnly,
        selectable: false,
        width: MARKER_WIDTH,
        height: MARKER_HEIGHT,
        measured: measured[START_NODE],
      },
      ...stageNodes,
      {
        id: END_NODE,
        type: 'marker',
        position: markers.end,
        data: { label: '结束', side: 'end', connected: true, readOnly },
        draggable: !readOnly,
        selectable: false,
        width: MARKER_WIDTH,
        height: MARKER_HEIGHT,
        measured: measured[END_NODE],
      },
    ];
  }, [
    body,
    positions,
    selection,
    readOnly,
    measured,
    markers,
    paletteDropStage,
    paletteDropKind,
  ]);

  const edges = useMemo<FlowEdge[]>(
    () =>
      deriveEdges(body).map((edge) => {
        const selected = sameSelection(selection, edge.selection);
        const curvature =
          0.25 * (edge.parallelIndex + 1) * (edge.parallelIndex % 2 === 0 ? 1 : -1);
        return {
          id: edge.id,
          source: edge.source,
          target: edge.target,
          label: edge.label,
          type: edge.parallelCount > 1 ? 'default' : 'smoothstep',
          pathOptions:
            edge.parallelCount > 1 ? { curvature } : undefined,
          data: { selection: edge.selection },
          selectable: edge.selection !== null,
          labelBgPadding: [6, 4],
          labelBgBorderRadius: 4,
          animated: false,
          className: selected ? 'edge-selected' : undefined,
          ariaLabel: `转移：${edge.label}`,
        };
      }),
    [body, selection],
  );

  const handleNodesChange = useCallback<OnNodesChange<StageNode | MarkerNode>>(
    (changes) => {
      for (const change of changes) {
        if (change.type === 'dimensions' && change.dimensions) {
          const { width, height } = change.dimensions;
          setMeasured((current) =>
            current[change.id]?.width === width &&
            current[change.id]?.height === height
              ? current
              : { ...current, [change.id]: { width, height } },
          );
        }
        if (
          !readOnly &&
          change.type === 'position' &&
          change.position
        ) {
          onMove(change.id, change.position);
        }
      }
    },
    [onMove, readOnly],
  );

  useEffect(() => {
    if (!focusRequest?.stage) return;
    const point = positions[focusRequest.stage];
    if (!point) return;
    void instance.setCenter(
      point.x + STAGE_WIDTH / 2,
      point.y + STAGE_HEIGHT / 2,
      {
        zoom: instance.getZoom(),
        duration: reducedMotion() ? 0 : 300,
      },
    );
  }, [focusRequest?.token, focusRequest?.stage, positions, instance]);

  useEffect(() => {
    if (fitRequest === 0) return;
    void instance.fitView({
      padding: 0.2,
      duration: reducedMotion() ? 0 : 200,
    });
  }, [fitRequest, instance]);

  const paletteCallbacks = useRef({
    onAddOperation,
    onAddSubtask,
    onAddStageAt,
  });
  paletteCallbacks.current = {
    onAddOperation,
    onAddSubtask,
    onAddStageAt,
  };

  const applyPaletteToStage = (
    kind: PalettePlacementKind,
    stage: string,
  ): void => {
    if (kind === 'operation') {
      paletteCallbacks.current.onAddOperation?.(stage);
    } else {
      paletteCallbacks.current.onAddSubtask?.(stage);
    }
  };

  const beginPaletteDrag = useCallback(
    (kind: PaletteKind, event: ReactPointerEvent<HTMLButtonElement>) => {
      if (readOnly) return;
      event.stopPropagation();
      dragMoved.current = false;
      setArmedPalette(null);
      setPaletteDrag({
        kind,
        start: { x: event.clientX, y: event.clientY },
        current: { x: event.clientX, y: event.clientY },
        targetStage: null,
        overCanvas: false,
        moved: false,
      });
    },
    [readOnly],
  );

  const activatePalette = useCallback(
    (kind: PaletteKind, event: ReactMouseEvent<HTMLButtonElement>) => {
      if (readOnly) return;
      // A mouse click that ends a real drag must not enter placement mode.
      if (event.detail > 0 && dragMoved.current) return;
      dragMoved.current = false;
      if (kind === 'stage') {
        paletteCallbacks.current.onAddStageAt?.();
        return;
      }
      setArmedPalette((current) => (current === kind ? null : kind));
    },
    [readOnly],
  );

  const draggingPalette = paletteDrag?.kind ?? null;

  useEffect(() => {
    if (paletteDrag === null) return;
    const pointerOverFlow = (x: number, y: number): boolean => {
      const rect = wrapper.current?.getBoundingClientRect();
      if (!rect) return false;
      return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
    };
    const move = (event: PointerEvent) => {
      const point = { x: event.clientX, y: event.clientY };
      const start = paletteDragRef.current?.start;
      const moved =
        dragMoved.current ||
        (start !== undefined &&
          Math.hypot(point.x - start.x, point.y - start.y) > 4);
      if (moved) dragMoved.current = true;
      const overCanvas = pointerOverFlow(point.x, point.y);
      const targetStage =
        overCanvas && paletteDragRef.current?.kind !== 'stage'
          ? stageAt(point.x, point.y, bodyRef.current)
          : null;
      setPaletteDrag((current) => {
        if (!current) return current;
        if (
          current.current.x === point.x &&
          current.current.y === point.y &&
          current.targetStage === targetStage &&
          current.overCanvas === overCanvas &&
          current.moved === moved
        ) {
          return current;
        }
        return { ...current, current: point, targetStage, overCanvas, moved };
      });
    };
    const finish = (event: PointerEvent) => {
      const state = paletteDragRef.current;
      setPaletteDrag(null);
      if (state === null || !state.moved) return;
      const overCanvas = pointerOverFlow(event.clientX, event.clientY);
      if (state.kind === 'stage') {
        if (!overCanvas) return;
        const position = instance.screenToFlowPosition({
          x: event.clientX,
          y: event.clientY,
        });
        paletteCallbacks.current.onAddStageAt?.(position);
        return;
      }
      if (!overCanvas) return;
      const stage = stageAt(event.clientX, event.clientY, bodyRef.current);
      if (stage === null) return;
      applyPaletteToStage(state.kind, stage);
    };
    const cancel = () => setPaletteDrag(null);
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setPaletteDrag(null);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', finish);
    window.addEventListener('pointercancel', cancel);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', cancel);
      window.removeEventListener('keydown', onKey);
    };
  }, [draggingPalette, instance]);

  useEffect(() => {
    if (armedPalette === null) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setArmedPalette(null);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [armedPalette]);

  // Keyboard completion of placement mode: with an operation or subtask
  // armed, Enter/Space on a focused stage applies it.
  useEffect(() => {
    if (armedPalette === null) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Enter' && event.key !== ' ') return;
      const element = event.target;
      if (!(element instanceof Element)) return;
      if (
        element.closest(
          '.object-palette, button, input, textarea, select, a',
        )
      ) {
        return;
      }
      const nodeElement = element.closest<HTMLElement>('.react-flow__node');
      const stageName = nodeElement?.dataset.id;
      if (
        !stageName ||
        stageName === START_NODE ||
        stageName === END_NODE ||
        !bodyRef.current.stages.some((stage) => stage.name === stageName)
      ) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      setArmedPalette(null);
      applyPaletteToStage(armedPalette, stageName);
    };
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [armedPalette]);

  const handleClickCapture = (event: ReactMouseEvent<HTMLDivElement>) => {
    const kind = armedPaletteRef.current;
    if (kind === null) return;
    const element = event.target as Element;
    if (element.closest('.object-palette')) return;
    if (!element.closest('.flow-wrap')) {
      setArmedPalette(null);
      return;
    }
    const stageName = stageAt(event.clientX, event.clientY, bodyRef.current);
    if (stageName === null) {
      setArmedPalette(null);
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    setArmedPalette(null);
    applyPaletteToStage(kind, stageName);
  };

  useEffect(() => {
    const container = stack.current;
    const flow = wrapper.current;
    if (!container || !flow) return;

    const measure = () => {
      const flowRect = flow.getBoundingClientRect();
      if (flowRect.width === 0 || flowRect.height === 0) {
        setOperationViews((current) => (current.length === 0 ? current : []));
        return;
      }
      const views: OperationView[] = [];
      const placed: {
        left: number;
        right: number;
        top: number;
        bottom: number;
      }[] = [];
      for (const stage of body.stages) {
        if (stage.operations.length === 0) continue;
        const node = container.querySelector<HTMLElement>(
          `.react-flow__node[data-id="${CSS.escape(stage.name)}"]`,
        );
        if (!node) continue;
        const rect = node.getBoundingClientRect();
        if (
          rect.right < flowRect.left ||
          rect.left > flowRect.right ||
          rect.bottom < flowRect.top ||
          rect.top > flowRect.bottom
        ) {
          continue;
        }
        const stageLeft = rect.left - flowRect.left;
        const stageTop = Math.round(rect.top - flowRect.top);
        const width = rect.width;
        if (width <= 0) continue;
        const count = stage.operations.length;
        for (let index = 0; index < count; index += 1) {
          const operation = stage.operations[index];
          const label = operation.display_name.trim() || operation.name;
          const labelWidth = Math.min(
            148,
            Math.max(52, 34 + Array.from(label).length * 12),
          );
          const half = labelWidth / 2;
          const desired = stageLeft + (width * (index + 1)) / (count + 1);
          const x = Math.round(
            Math.min(
              stageLeft + width - 10 - half,
              Math.max(stageLeft + 10 + half, desired),
            ),
          );
          const left = x - half;
          let labelTop = Math.round(stageTop - 16 - OPERATION_LABEL_HEIGHT);
          for (let attempt = 0; attempt < 8; attempt += 1) {
            const overlaps = placed.some(
              (box) =>
                left < box.right + OPERATION_LABEL_GAP &&
                left + labelWidth > box.left - OPERATION_LABEL_GAP &&
                labelTop < box.bottom + OPERATION_LABEL_GAP &&
                labelTop + OPERATION_LABEL_HEIGHT >
                  box.top - OPERATION_LABEL_GAP,
            );
            if (!overlaps) break;
            labelTop -= OPERATION_LABEL_HEIGHT + OPERATION_LABEL_GAP;
          }
          labelTop = Math.max(4, labelTop);
          placed.push({
            left,
            right: left + labelWidth,
            top: labelTop,
            bottom: labelTop + OPERATION_LABEL_HEIGHT,
          });
          views.push({
            stage: stage.name,
            index,
            label,
            x,
            labelTop,
            labelWidth,
            stageTop,
          });
        }
      }
      setOperationViews((current) =>
        sameOperationViews(current, views) ? current : views,
      );
    };

    const frame = requestAnimationFrame(measure);
    return () => cancelAnimationFrame(frame);
  }, [body, positions, geometryTick]);

  useEffect(() => {
    const flow = wrapper.current;
    if (!flow || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(() =>
      setGeometryTick((value) => value + 1),
    );
    observer.observe(flow);
    return () => observer.disconnect();
  }, []);

  const handleDoubleClick = (event: ReactMouseEvent<HTMLDivElement>) => {
    if (readOnly || !onAddStageAt) return;
    const target = event.target as HTMLElement;
    if (!target.closest('.react-flow__pane')) return;
    if (
      target.closest(
        '.react-flow__node, .react-flow__edge, .react-flow__controls, .react-flow__minimap',
      )
    ) {
      return;
    }
    const point = instance.screenToFlowPosition({
      x: event.clientX,
      y: event.clientY,
    });
    onAddStageAt(point);
  };

  return (
    <div
      ref={stack}
      className="canvas-stack"
      onClickCapture={handleClickCapture}
    >
      {!readOnly && (
        <ObjectPalette
          armed={armedPalette}
          dragging={draggingPalette}
          onStartDrag={beginPaletteDrag}
          onActivate={activatePalette}
        />
      )}
      <div
        ref={wrapper}
        className="flow-wrap"
        onDoubleClick={handleDoubleClick}
      >
        <ReactFlow<StageNode | MarkerNode, FlowEdge>
          nodes={nodes}
          edges={edges}
          nodeTypes={nodeTypes}
          onNodesChange={handleNodesChange}
          onNodeDragStart={(_, node) => onNodeDragStart?.(node.id)}
          onNodeClick={(_, node) => {
            if (node.id === START_NODE || node.id === END_NODE) {
              onSelect(null);
            } else {
              onSelect({ kind: 'stage', stage: node.id });
            }
          }}
          onEdgeClick={(_, edge) => {
            if (edge.data?.selection) onSelect(edge.data.selection);
          }}
          onPaneClick={() => onSelect(null)}
          onMove={() => setGeometryTick((value) => value + 1)}
          onConnect={onConnect}
          nodesDraggable={!readOnly}
          nodesConnectable={!readOnly}
          edgesFocusable
          elementsSelectable
          selectionOnDrag={false}
          multiSelectionKeyCode={null}
          deleteKeyCode={null}
          nodeDragThreshold={4}
          zoomOnDoubleClick={false}
          fitView
          fitViewOptions={{ padding: 0.2 }}
          proOptions={{ hideAttribution: true }}
        >
          <Background gap={20} />
          <Controls showInteractive={false} />
        </ReactFlow>
        {body.stages.length === 0 && (
          <div className="canvas-empty">
            {readOnly ? (
              <p>该流程没有 stage。</p>
            ) : (
              <>
                <p>
                  还没有 stage。把上方工具栏的「阶段」拖到画布，或双击空白处。
                </p>
                <button
                  type="button"
                  className="primary"
                  onClick={() => onAddStageAt?.({ x: 120, y: 160 })}
                >
                  + 创建第一个 stage
                </button>
              </>
            )}
          </div>
        )}
        {body.stages.length > 0 &&
          body.initial_stage === null &&
          armedPalette === null && (
            <div className="canvas-hint">
              还没有初始 stage。从「开始」拖线到 stage，或在 stage
              检查器里勾选「初始 stage」。
            </div>
          )}
        {armedPalette !== null && (
          <div className="canvas-hint palette-armed-hint">
            已选择{armedPalette === 'operation' ? '操作' : '子任务'}：点击
            stage {armedPalette === 'operation' ? '新建操作' : '添加子任务'}。按
            Esc 取消。
          </div>
        )}
        <div className="canvas-mode-hint">
          {readOnly
            ? '只读：拖动空白处、按住中键或按住空格平移画布。'
            : '拖动卡片调整位置（只改布局）；从右侧手柄拖出转移；从上方工具栏拖入阶段、操作或子任务。'}
        </div>
        <OperationOverlay
          views={operationViews}
          selection={selection}
          hovered={hoveredOperation}
          onHover={setHoveredOperation}
          onSelect={onSelect}
        />
      </div>
      {paletteDrag !== null && (
        <div
          className={[
            'palette-drag-ghost',
            paletteDrag.overCanvas &&
            (paletteDrag.kind === 'stage' || paletteDrag.targetStage !== null)
              ? 'droppable'
              : '',
          ]
            .filter(Boolean)
            .join(' ')}
          style={{
            left: paletteDrag.current.x + 14,
            top: paletteDrag.current.y + 18,
          }}
        >
          <PaletteIcon kind={paletteDrag.kind} />
          <span className="palette-drag-name">
            {PALETTE_LABELS[paletteDrag.kind]}
          </span>
          <span className="palette-drag-hint">
            {paletteDragHint(paletteDrag)}
          </span>
        </div>
      )}
    </div>
  );
}

function paletteDragHint(drag: PaletteDragState): string {
  if (drag.kind === 'stage') {
    return drag.overCanvas ? '松开：在画布创建阶段' : '拖到画布空白处';
  }
  if (drag.targetStage !== null) {
    return drag.kind === 'operation'
      ? `松开：在 ${drag.targetStage} 新建操作`
      : `松开：在 ${drag.targetStage} 添加子任务`;
  }
  return '拖到 stage';
}

export function FlowCanvas(props: FlowCanvasProps) {
  return (
    <ReactFlowProvider>
      <CanvasInner {...props} />
    </ReactFlowProvider>
  );
}
