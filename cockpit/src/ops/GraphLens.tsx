// The Graph lens (theseus-jid7t.4): every session, task, step and job as one living graph, in lanes across a deck-navy
// canvas, laid out by `opsLayout` (`ops/graph.ts`: sticky by id, ended work folded, the 400-node cap). React Flow draws
// it: minimap, zoom and fit; `?sel=` pans to its node. A click focuses (`?sel=`), ⇧/⌘-click picks, ⇧-drag lassos, a
// double-click opens the deck, a right-click opens the page's verb menu for that node's row, and a task or a
// conversation drags onto a home or a project (`ops/drag.ts`). A running edge flows while the daemon says something;
// Calm and reduced motion still it. Filters and the search fade what they leave out, so nothing moves.
import { useEffect, useMemo, useRef, useState } from 'react'
import { ReactFlow, ReactFlowProvider, Background, Controls, MiniMap, MarkerType, useReactFlow, type Edge, type Node, type NodeChange } from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import { useCalm } from '@/lib/calm'
import { toneHex } from '@/lib/taxonomy'
import { cn } from '@/lib/format'
import { FIT, useFullGraph } from '@/components/FullGraph'

/** The lanes fill the canvas edge to edge: less margin than the other graphs' fit. */
const FIT_LANES = { ...FIT, padding: 0.05 }
/** The least zoom a first view takes: a node's title still reads. */
const READABLE = 0.62
import type { OpsRow } from './board'
import { foldBacks, keeper, layoutRows, NODE_W, type Graph, type GEdge, type LaneMeta } from './graph'
import { NODE_TYPES, typeOf, type LaneData, type NodeData } from './GraphNode'
import type { Selection } from './selection'
import type { DragDrop } from './DropLayer'
import { dragOf, type DragItem } from './rehome'
import { FilterChip, type FilterChipProps } from './FilterChip'
import { OWNER_CALLS, type OwnerCalls } from './state'

const EDGE: Readonly<Record<GEdge['kind'], Partial<Edge>>> = {
  parent: { style: { stroke: '#9c907a', strokeWidth: 1.25 } },
  deps: { style: { stroke: '#c9a96e', strokeWidth: 1.25, strokeDasharray: '6 4' }, markerEnd: { type: MarkerType.ArrowClosed, color: '#c9a96e', width: 14, height: 14 } },
  retry: { style: { stroke: toneHex.think, strokeWidth: 1.25, strokeDasharray: '1 5', strokeLinecap: 'round' } },
}

export interface GraphLensProps {
  /** Unused: the rows are the graph's whole input (theseus-odj0m). The page still passes them, which `ops-parity` pins. */
  views?: readonly unknown[]
  records?: readonly unknown[]
  /** The one work set (`boardRows`): every node of the graph is one of these. */
  rows: readonly OpsRow[]
  lanes: readonly LaneMeta[]
  now: number
  focus: string | null
  picked: Selection
  /** The ids the filters and the search keep: drawn normally. The context around them is outlined, "context" on hover;
   *  the rest are outlined too, or left out when `calls.hideRest`. */
  shownIds: ReadonlySet<string> | null
  /** Each kept card's ancestors up to the thread's head, and the jobs and carried steps of kept cards (`contextOf`). */
  contextIds?: ReadonlySet<string> | null
  /** The counter being filtered by, as a chip on the toolbar. */
  chip?: FilterChipProps | null
  calls?: OwnerCalls
  /** What the filters and the search are now (the address's): the view fits to the kept cards once each time it changes. */
  filterKey?: string
  onFocus: (id: string) => void
  onPick: (id: string) => void
  onLasso: (ids: string[]) => void
  onOpen: (sessionId: string) => void
  /** A right-click on a node that has a row: the page opens its one verb menu there. */
  onContext: (id: string, x: number, y: number) => void
  /** `?redact=1`: a private or sensitive node and lane show a mark for their title. */
  redact: boolean
  /** The lens hands its last layout up, for the page's keys (a ref: no render). */
  layoutRef: { current: Graph | null }
  drag: DragDrop
  titleOf: (sessionId: string) => string
}

export function GraphLens(p: GraphLensProps) {
  return <ReactFlowProvider><Canvas {...p} /></ReactFlowProvider>
}

function Canvas({ rows, lanes, now, focus, picked, shownIds, contextIds = null, chip = null, calls = OWNER_CALLS, filterKey = '', onFocus, onPick, onLasso, onOpen, onContext, redact, layoutRef, drag, titleOf }: GraphLensProps) {
  const rf = useReactFlow()
  const fullGraph = useFullGraph('Graph')
  const calm = useCalm((s) => s.calm)
  const [future, setFuture] = useState(false)
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set())
  const [opened, setOpened] = useState<ReadonlySet<string>>(new Set())
  // The owner's plan folds (true open, false folded), kept here as `opened` is, never in the address.
  const [folds, setFolds] = useState<ReadonlyMap<string, boolean>>(new Map())
  const [tidy, setTidy] = useState(0)
  // The last layout, which the next keeps every placed node of (sticky by id); "tidy" forgets it.
  const [memory] = useState(keeper)
  // Q4 A: a counter's click leaves everything but the kept cards and their context out of the drawing, each slot kept as
  // a hole so nothing moves; Q4 B (today's) only outlines the rest.
  const keep = useMemo(() => (calls.hideRest && shownIds ? new Set([...shownIds, ...(contextIds ?? [])]) : undefined), [calls.hideRest, shownIds, contextIds])
  const g = useMemo(() => memory.lay(rows, lanes, { now, future, expanded, opened, keep, folds }, tidy), [memory, rows, lanes, now, future, expanded, opened, keep, folds, tidy])
  // The layout without the openings, which says what each opening can fold back into (`foldBacks`).
  const base = useMemo(() => (expanded.size || opened.size ? layoutRows(rows, lanes, null, { now, future, keep, folds }) : g), [rows, lanes, now, future, keep, folds, expanded, opened, g])
  // The roots' fold-backs read the layout with the lanes' openings: a fold opened inside an opened lane is only there.
  const baseRoots = useMemo(() => (opened.size && expanded.size ? layoutRows(rows, lanes, null, { now, future, expanded, keep, folds }) : base), [rows, lanes, now, future, expanded, opened, keep, folds, base])
  const back = useMemo(() => foldBacks(g, base, expanded, opened, baseRoots), [g, base, expanded, opened, baseRoots])
  useEffect(() => { layoutRef.current = g }, [g, layoutRef])
  const byId = useMemo(() => new Map(rows.map((r) => [r.id, r])), [rows])
  // `?sel=` names a node by its id, or a task by its record's (`tsk_…`, the task graph's link).
  const at = focus ? g.nodes.find((n) => n.id === focus) ?? g.nodes.find((n) => n.record?.id === focus) ?? g.nodes.find((n) => n.head && n.lane === focus) : undefined
  const fid = at?.id ?? null
  const [moving, setMoving] = useState<{ id: string; x: number; y: number } | null>(null)
  const [hover, setHover] = useState<{ key: string; ok: boolean } | null>(null)
  // Each node's size as React Flow measured it: the nodes are the layout's, so the measure is kept here (the minimap
  // draws only measured nodes).
  const [sizes, setSizes] = useState<Readonly<Record<string, { width: number; height: number }>>>({})
  const boxing = useRef<Set<string> | null>(null)

  const nodes = useMemo(() => {
    const out: Node[] = g.lanes.map((l): Node<LaneData> => ({
      id: `lane:${l.key}`, type: 'lane', position: { x: 0, y: l.y }, measured: { width: g.width + 24, height: l.height }, draggable: false, selectable: false, zIndex: -1,
      data: {
        lane: l, width: g.width + 24, redact, hover: hover?.key === `lane:${l.key}` ? (hover.ok ? 'ok' : 'no') : undefined,
        foldBack: back.lanes.find((x) => x.lane === l.key) && { n: back.lanes.find((x) => x.lane === l.key)!.n, onFold: () => setExpanded((s) => { const t = new Set(s); t.delete(l.key); return t }) },
      },
    }))
    for (const n of g.nodes) {
      const row = byId.get(n.id)
      const key = n.kind === 'conversation' && row?.sessionId ? `conversation:${row.sessionId}` : n.kind === 'project' ? `project:${n.id}` : null
      const at = moving?.id === n.id ? { x: moving.x, y: moving.y } : { x: n.x, y: n.y }
      out.push({
        id: n.id, type: typeOf(n), position: at, ...(shownIds != null && !shownIds.has(n.id) && contextIds?.has(n.id) ? { domAttributes: { title: 'context' } } : {}), measured: sizes[n.id], selected: picked.has(n.id), draggable: n.kind === 'task' || n.kind === 'conversation', zIndex: moving?.id === n.id ? 10 : 1,
        data: {
          n, row, now, redact, focused: fid === n.id, laneMeta: n.head ? g.lanes.find((l) => l.key === n.lane) : undefined,
          onPlan: n.open !== undefined ? () => setFolds((m) => new Map(m).set(n.id, !n.open)) : undefined, picked: picked.has(n.id), dim: shownIds != null && !shownIds.has(n.id) && n.kind !== 'done' && n.kind !== 'folded',
          hover: key && hover?.key === key ? (hover.ok ? 'ok' : 'no') : undefined,
          onFoldBack: back.roots.includes(n.id) ? () => setOpened((s) => { const t = new Set(s); t.delete(n.id); return t }) : undefined,
          onFold: n.kind === 'done' ? () => setExpanded((s) => new Set([...s, n.lane])) : n.kind === 'folded' ? () => setOpened((s) => new Set([...s, n.parent ?? n.id.slice(5)])) : undefined,
        } satisfies NodeData,
      })
    }
    return out
  }, [g, back, byId, now, fid, picked, shownIds, contextIds, moving, hover, sizes, redact])

  const edges = useMemo(() => g.edges.map((e): Edge => ({
    id: e.id, source: e.source, target: e.target, ...EDGE[e.kind],
    ...(e.running && e.kind === 'parent' ? { className: 'ops-flow', style: { stroke: toneHex.live, strokeWidth: 1.5 } } : {}),
  })), [g.edges])

  // The first view, once the first read brings nodes (the graph mounts before its data): the whole graph when it fits
  // at a readable zoom, else its top-left at that zoom (the needs-you lanes lead; the minimap has the rest). A focus
  // pans instead.
  const box = useRef<HTMLDivElement>(null)
  const fitted = useRef(false)
  useEffect(() => {
    if (fitted.current || !g.nodes.length || !box.current) return
    fitted.current = true
    if (focus) return
    const last = g.lanes.at(-1)
    const [w, h] = [g.width + 24, last ? last.y + last.height : 1]
    const fit = Math.min(box.current.clientWidth / w, box.current.clientHeight / h) * 0.95
    if (fit >= READABLE) requestAnimationFrame(() => { void rf.fitView(FIT_LANES) })
    else void rf.setViewport({ x: 16, y: 56, zoom: READABLE })
  }, [g, focus, rf])

  // Q4 A: the view fits to the kept cards once per filter change (a live change of the rows does not move it).
  const fittedTo = useRef('')
  useEffect(() => {
    if (!keep || !shownIds || fittedTo.current === filterKey) return
    const kept = g.nodes.filter((n) => shownIds.has(n.id)).map((n) => ({ id: n.id }))
    if (!kept.length) return
    fittedTo.current = filterKey
    requestAnimationFrame(() => { void rf.fitView({ ...FIT_LANES, nodes: kept }) })
  }, [keep, shownIds, filterKey, g, rf])
  useEffect(() => { if (!keep) fittedTo.current = '' }, [keep])

  // `?sel=` pans to its node, once each time it changes.
  const panned = useRef<string | null>(null)
  useEffect(() => {
    if (!at || panned.current === at.id) return
    panned.current = at.id
    void rf.setCenter(at.x + NODE_W / 2, at.y + 60, { zoom: Math.max(rf.getZoom(), 0.7), duration: calm ? 0 : 450 })
  }, [at, rf, calm])

  const sourceOf = (id: string): DragItem | null => {
    const r = byId.get(id)
    return r ? dragOf(r, titleOf) : null
  }
  const targetAt = (x: number, y: number, self: string): Element | null => {
    for (const el of document.elementsFromPoint(x, y)) {
      if ((el.closest('.react-flow__node') as HTMLElement | null)?.dataset.id === self) continue
      const t = el.closest('[data-drop]')
      if (t) return t
    }
    return null
  }

  const onNodesChange = (changes: NodeChange[]) => {
    const measured = changes.flatMap((c) => (c.type === 'dimensions' && c.dimensions ? [[c.id, c.dimensions] as const] : []))
    if (measured.length) setSizes((m) => ({ ...m, ...Object.fromEntries(measured) }))
    for (const c of changes) {
      if (c.type === 'position' && c.position && c.dragging) setMoving({ id: c.id, x: c.position.x, y: c.position.y })
      if (c.type === 'select' && c.selected && boxing.current && !c.id.startsWith('lane:')) boxing.current.add(c.id)
    }
  }

  return (
    <div ref={box} role={fullGraph.panel.dialog ? 'dialog' : undefined} aria-label={fullGraph.panel.dialog} data-testid="ops-graph"
      className={cn('relative h-full min-h-[420px] overflow-hidden rounded-xl border border-line bg-deck', fullGraph.panel.className)}>
      <ReactFlow onInit={fullGraph.onInit} nodes={nodes} edges={edges} nodeTypes={NODE_TYPES} onNodesChange={onNodesChange} minZoom={0.08} maxZoom={1.6} colorMode="dark"
        proOptions={{ hideAttribution: true }} multiSelectionKeyCode={null} selectionKeyCode="Shift" deleteKeyCode={null}
        onNodeClick={(e, n) => {
          if (n.type === 'lane') return
          if (e.shiftKey || e.metaKey || e.ctrlKey) { if (byId.has(n.id)) onPick(n.id) } else onFocus((n.data as NodeData).n?.head ? (n.data as NodeData).n.lane : n.id) // only a node with a row is picked; a thread's head selects the thread
        }}
        onNodeDoubleClick={(_, n) => { const sid = byId.get(n.id)?.sessionId; if (sid) onOpen(sid) }}
        onNodeContextMenu={(e, n) => { if (n.type === 'lane' || !byId.has(n.id)) return; e.preventDefault(); onContext(n.id, e.clientX, e.clientY) }}
        onSelectionStart={() => { boxing.current = new Set() }}
        onSelectionEnd={() => { const got = boxing.current; boxing.current = null; const ids = [...(got ?? [])].filter((id) => byId.has(id)); if (ids.length) onLasso(ids) }}
        onNodeDragStart={(_, n) => { const src = sourceOf(n.id); if (src) drag.begin(src) }}
        onNodeDrag={(e, n) => {
          const { x, y } = pointOf(e)
          const el = targetAt(x, y, n.id)
          const ok = drag.hover(el, x, y)
          const key = el?.getAttribute('data-drop')
          setHover(key ? { key, ok } : null)
        }}
        onNodeDragStop={(e, n) => {
          const { x, y } = pointOf(e)
          const el = targetAt(x, y, n.id)
          setMoving(null)
          setHover(null)
          drag.drop(el)
        }}>
        <Background color="rgba(176,141,87,0.09)" gap={22} size={1} />
        <Controls showInteractive={false} className="!bg-hull !shadow-none [&>button]:!border-line [&>button]:!bg-hull [&>button]:!fill-ink-dim" />
        <MiniMap pannable zoomable className="!bg-hull/90" maskColor="rgba(6,16,29,0.7)" nodeStrokeWidth={2}
          nodeColor={(n) => (n.type === 'lane' ? 'rgba(214,165,72,0.08)' : (n.data as NodeData).n?.needs ? toneHex.wait : (n.data as NodeData).picked ? '#d6a548' : '#6b7d94')} />
      </ReactFlow>
      <div className="pointer-events-auto absolute left-3 top-3 flex items-center gap-2 rounded-lg bg-hull/90 px-2.5 py-1.5 text-[11px] text-ink-dim ring-1 ring-line backdrop-blur">
        {fullGraph.button}
        {chip && <FilterChip {...chip} calls={calls} />}
        <span className="num">{g.nodes.length} nodes{g.folded ? ` · ${g.folded} folded` : ''}{g.hidden ? ` · ${g.hidden} hidden` : ''}</span>
        {g.cantOpen && <span role="status" className="text-wait">could not open: the graph is at its cap; narrow it with the filters or the search</span>}
        <button type="button" aria-pressed={future} onClick={() => setFuture((f) => !f)} className={cn('rounded px-1.5 ring-1', future ? 'bg-think/15 text-think ring-think/40' : 'ring-line hover:text-ink')}
          title="wakes, routines, watches and projects (a project takes a dropped task or conversation)">+ future</button>
        {/* theseus-99l9z: the visibility checkbox group goes here, between "+ future" and "tidy" */}
        <button type="button" onClick={() => { setExpanded(new Set()); setOpened(new Set()); setTidy((t) => t + 1) }} className="rounded px-1.5 ring-1 ring-line hover:text-ink"
          title="lay it out afresh: needs-you lanes first, holes closed (live changes never move a node)">tidy</button>
      </div>
      <Legend />
    </div>
  )
}

/** Where a drag's pointer is, a mouse's or a finger's. */
const pointOf = (e: MouseEvent | TouchEvent): { x: number; y: number } => {
  const t = 'changedTouches' in e ? e.changedTouches[0] : e
  return { x: t?.clientX ?? 0, y: t?.clientY ?? 0 }
}

function Legend() {
  const line = (dash: string | undefined, color: string, cls?: string) => <svg width="28" height="6" aria-hidden className={cls}><line x1="0" y1="3" x2="28" y2="3" stroke={color} strokeWidth="1.5" strokeDasharray={dash} strokeLinecap="round" /></svg>
  return (
    <div className="pointer-events-none absolute bottom-3 left-14 flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg bg-hull/85 px-2.5 py-1 text-[10px] text-ink-faint ring-1 ring-line">
      <span className="flex items-center gap-1">{line(undefined, '#9c907a')} under</span>
      <span className="flex items-center gap-1">{line('6 4', '#c9a96e')} waits on ▸</span>
      <span className="flex items-center gap-1">{line('1 5', toneHex.think)} retries</span>
      <span className="flex items-center gap-1">{line(undefined, toneHex.live)} running</span>
      <span className="text-wait">● needs you</span>
      <span title="a step: to do, doing, blocked, done">☐ ◐ ■ ☑ step</span>
      <span title="a plan folds into its card; ▸ opens it">▸ plan n/m</span>
      <span>⇧-drag lasso · ⇧/⌘-click pick · right-click verbs</span>
    </div>
  )
}
