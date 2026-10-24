// The Graph lens (theseus-jid7t.4): every session, task, step and job as one living graph, in lanes across a deck-navy
// canvas, laid out by `opsLayout` (`ops/graph.ts`: sticky by id, ended work folded, the 400-node cap). React Flow draws
// it: minimap, zoom and fit; `?sel=` pans to its node. A click focuses (`?sel=`), ⇧/⌘-click picks, ⇧-drag lassos, a
// double-click opens the deck, a right-click opens the verbs the bulk bar offers for that node, and a task or a
// conversation drags onto a home or a project (`ops/drag.ts`). A running edge flows while the daemon says something;
// Calm and reduced motion still it. Filters and the search fade what they leave out, so nothing moves.
import { useEffect, useMemo, useRef, useState } from 'react'
import { ReactFlow, ReactFlowProvider, Background, Controls, MiniMap, MarkerType, useReactFlow, type Edge, type Node, type NodeChange } from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import { useCalm } from '@/lib/calm'
import type { TaskRecord, WorkView } from '@protocol'
import { toneHex } from '@/lib/taxonomy'
import { cn } from '@/lib/format'
import { FIT } from '@/components/FullGraph'
import type { OpsRow } from './board'
import { keeper, NODE_W, type GEdge, type LaneMeta } from './graph'
import { NODE_TYPES, typeOf, type LaneData, type NodeData } from './GraphNode'
import type { BulkOffer, BulkVerb, Selection } from './selection'
import type { DragDrop } from './DropLayer'
import { dragOf, type DragItem } from './drag'

const EDGE: Readonly<Record<GEdge['kind'], Partial<Edge>>> = {
  parent: { style: { stroke: '#9c907a', strokeWidth: 1.25 } },
  deps: { style: { stroke: '#c9a96e', strokeWidth: 1.25, strokeDasharray: '6 4' }, markerEnd: { type: MarkerType.ArrowClosed, color: '#c9a96e', width: 14, height: 14 } },
  retry: { style: { stroke: toneHex.think, strokeWidth: 1.25, strokeDasharray: '1 5', strokeLinecap: 'round' } },
}

export interface GraphLensProps {
  views: readonly WorkView[]
  records: readonly TaskRecord[]
  rows: readonly OpsRow[]
  lanes: readonly LaneMeta[]
  now: number
  focus: string | null
  picked: Selection
  /** The ids the filters and the search keep: the rest are drawn faint. */
  shownIds: ReadonlySet<string> | null
  onFocus: (id: string) => void
  onPick: (id: string) => void
  onLasso: (ids: string[]) => void
  onOpen: (sessionId: string) => void
  /** The verbs the bulk bar offers for one node, for its right-click menu. */
  offersFor: (id: string) => BulkOffer[]
  onVerb: (verb: BulkVerb, ids: string[]) => void
  drag: DragDrop
  titleOf: (sessionId: string) => string
}

export function GraphLens(p: GraphLensProps) {
  return <ReactFlowProvider><Canvas {...p} /></ReactFlowProvider>
}

function Canvas({ views, records, rows, lanes, now, focus, picked, shownIds, onFocus, onPick, onLasso, onOpen, offersFor, onVerb, drag, titleOf }: GraphLensProps) {
  const rf = useReactFlow()
  const calm = useCalm((s) => s.calm)
  const [future, setFuture] = useState(false)
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set())
  const [tidy, setTidy] = useState(0)
  // The last layout, which the next keeps every placed node of (sticky by id); "tidy" forgets it.
  const [memory] = useState(keeper)
  const g = useMemo(() => memory.lay(views, records, lanes, { now, future, expanded }, tidy), [memory, views, records, lanes, now, future, expanded, tidy])
  const byId = useMemo(() => new Map(rows.map((r) => [r.id, r])), [rows])
  // `?sel=` names a node by its id, or a task by its record's (`tsk_…`, the task graph's link).
  const at = focus ? g.nodes.find((n) => n.id === focus) ?? g.nodes.find((n) => n.record?.id === focus) : undefined
  const fid = at?.id ?? null
  const [moving, setMoving] = useState<{ id: string; x: number; y: number } | null>(null)
  const [hover, setHover] = useState<{ key: string; ok: boolean } | null>(null)
  const [menu, setMenu] = useState<{ id: string; x: number; y: number } | null>(null)
  const boxing = useRef<Set<string> | null>(null)

  const nodes = useMemo(() => {
    const out: Node[] = g.lanes.map((l): Node<LaneData> => ({
      id: `lane:${l.key}`, type: 'lane', position: { x: 0, y: l.y }, draggable: false, selectable: false, zIndex: -1,
      data: { lane: l, width: g.width + 24, hover: hover?.key === `lane:${l.key}` ? (hover.ok ? 'ok' : 'no') : undefined },
    }))
    for (const n of g.nodes) {
      const row = byId.get(n.id)
      const key = n.kind === 'conversation' && row?.sessionId ? `conversation:${row.sessionId}` : n.kind === 'project' ? `project:${n.id}` : null
      const at = moving?.id === n.id ? { x: moving.x, y: moving.y } : { x: n.x, y: n.y }
      out.push({
        id: n.id, type: typeOf(n.kind), position: at, selected: picked.has(n.id), draggable: n.kind === 'task' || n.kind === 'conversation', zIndex: moving?.id === n.id ? 10 : 1,
        data: {
          n, row, now, focused: fid === n.id, picked: picked.has(n.id), dim: shownIds != null && !shownIds.has(n.id) && n.kind !== 'done' && n.kind !== 'folded',
          hover: key && hover?.key === key ? (hover.ok ? 'ok' : 'no') : undefined,
          onFold: n.kind === 'done' ? () => setExpanded((s) => new Set([...s, n.lane])) : undefined,
        } satisfies NodeData,
      })
    }
    return out
  }, [g, byId, now, fid, picked, shownIds, moving, hover])

  const edges = useMemo(() => g.edges.map((e): Edge => ({
    id: e.id, source: e.source, target: e.target, ...EDGE[e.kind],
    ...(e.running && e.kind === 'parent' ? { className: 'ops-flow', style: { stroke: toneHex.live, strokeWidth: 1.5 } } : {}),
  })), [g.edges])

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
    for (const c of changes) {
      if (c.type === 'position' && c.position && c.dragging) setMoving({ id: c.id, x: c.position.x, y: c.position.y })
      if (c.type === 'select' && c.selected && boxing.current && !c.id.startsWith('lane:')) boxing.current.add(c.id)
    }
  }

  return (
    <div className="relative h-full min-h-[420px] overflow-hidden rounded-xl border border-line bg-deck" data-testid="ops-graph" onClick={() => setMenu(null)}>
      <ReactFlow nodes={nodes} edges={edges} nodeTypes={NODE_TYPES} onNodesChange={onNodesChange} fitView fitViewOptions={FIT} minZoom={0.08} maxZoom={1.6} colorMode="dark"
        proOptions={{ hideAttribution: true }} multiSelectionKeyCode={null} selectionKeyCode="Shift" deleteKeyCode={null} onlyRenderVisibleElements
        onNodeClick={(e, n) => {
          if (n.type === 'lane') return
          if (e.shiftKey || e.metaKey || e.ctrlKey) onPick(n.id); else onFocus(n.id)
        }}
        onNodeDoubleClick={(_, n) => { const sid = byId.get(n.id)?.sessionId; if (sid) onOpen(sid) }}
        onNodeContextMenu={(e, n) => { if (n.type === 'lane') return; e.preventDefault(); setMenu({ id: n.id, x: e.clientX, y: e.clientY }) }}
        onSelectionStart={() => { boxing.current = new Set() }}
        onSelectionEnd={() => { const got = boxing.current; boxing.current = null; if (got?.size) onLasso([...got]) }}
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
          nodeColor={(n) => (n.type === 'lane' ? 'rgba(214,165,72,0.08)' : (n.data as NodeData).n?.needs ? toneHex.wait : (n.data as NodeData).picked ? '#d6a548' : '#3b4a5e')} />
      </ReactFlow>
      <div className="pointer-events-auto absolute left-3 top-3 flex items-center gap-2 rounded-lg bg-hull/90 px-2.5 py-1.5 text-[11px] text-ink-dim ring-1 ring-line backdrop-blur">
        <span className="num">{g.nodes.length} nodes{g.folded ? ` · ${g.folded} folded` : ''}</span>
        <button type="button" aria-pressed={future} onClick={() => setFuture((f) => !f)} className={cn('rounded px-1.5 ring-1', future ? 'bg-think/15 text-think ring-think/40' : 'ring-line hover:text-ink')}
          title="wakes, routines, watches and projects (a project takes a dropped task or conversation)">+ future</button>
        <button type="button" onClick={() => { setExpanded(new Set()); setTidy((t) => t + 1) }} className="rounded px-1.5 ring-1 ring-line hover:text-ink"
          title="lay it out afresh: needs-you lanes first, holes closed (live changes never move a node)">tidy</button>
      </div>
      <Legend />
      {menu && <Menu {...menu} offers={offersFor(menu.id)} onVerb={(v) => { setMenu(null); onVerb(v, [menu.id]) }} />}
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
      <span>⇧-drag lasso · ⇧/⌘-click pick · right-click verbs</span>
    </div>
  )
}

/** A node's right-click menu: the verbs the bulk bar offers for it alone, greyed with the reason when it admits none. */
function Menu({ x, y, offers, onVerb }: { x: number; y: number; offers: BulkOffer[]; onVerb: (v: BulkVerb) => void }) {
  return (
    <div role="menu" className="fixed z-40 min-w-[200px] rounded-lg bg-hull/95 py-1 text-[12px] shadow-2xl ring-1 ring-line-strong backdrop-blur" style={{ left: x, top: y }} onClick={(e) => e.stopPropagation()}>
      {offers.map((o) => (
        <button key={o.verb} role="menuitem" type="button" disabled={!o.items.length} onClick={() => onVerb(o.verb)} title={o.reason}
          className="block w-full px-3 py-1 text-left text-ink hover:bg-white/5 disabled:cursor-not-allowed disabled:text-ink-faint">{o.label}</button>
      ))}
    </div>
  )
}
