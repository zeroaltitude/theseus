// A session's content graph: every message, tool call, and result as a node. Edges: a turn's prompt to its first
// reply, a reply to the calls it made, each call to its result, results to the loop that read them, and (dashed)
// the end of one turn to the next. Click a node for its record, and its reach.
import { useEffect, useMemo, useState } from 'react'
import { ReactFlow, Background, Controls, Handle, Position, type Edge, type Node, type NodeProps } from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import { Bot, User, Wrench, FileText } from 'lucide-react'
import { reachWords, type NodeInfo, type NodeReachResult } from '@protocol'
import { call } from '@/lib/rpc'
import { clock, ms, short, tokens, usd } from '@/lib/format'
import { toneHex } from '@/lib/taxonomy'
import { JsonView } from './JsonView'
import { Empty } from './ui'

type D = Record<string, any>
interface GData extends Record<string, unknown> { n: NodeInfo; turn: number }

const KIND: Record<string, { color: string; icon: React.ReactNode; label: string }> = {
  user_message: { color: '#cbd5e1', icon: <User size={12} />, label: 'user' },
  assistant_message: { color: toneHex.model, icon: <Bot size={12} />, label: 'reply' },
  tool_call: { color: toneHex.tool, icon: <Wrench size={12} />, label: 'call' },
  tool_result: { color: toneHex.ok, icon: <FileText size={12} />, label: 'result' },
}

function GNode({ data, selected }: NodeProps<Node<GData>>) {
  const { n } = data
  const k = KIND[n.kind] ?? { color: toneHex.idle, icon: null, label: n.kind }
  const d = (n.detail ?? {}) as D
  const err = n.kind === 'tool_result' && (d.is_error || d.status === 'declined')
  const color = err ? toneHex.fault : k.color
  const stat = n.kind === 'assistant_message' ? `${usd(d.cost_usd)} · ${tokens(d.usage?.output_tokens)} out`
    : n.kind === 'tool_call' ? (d.tool ?? '')
    : n.kind === 'tool_result' ? `${d.status ?? ''}${d.duration_ms != null ? ` · ${ms(d.duration_ms)}` : ''}`
    : (n.author ?? '')
  // A call's own text is empty: show what it does (its plan's summary, else its main argument).
  const callText = n.kind === 'tool_call' ? (d.plan?.summary ?? d.input?.command ?? d.input?.path ?? d.input?.pattern ?? d.input?.query ?? '') : ''
  const text = String(n.text || callText || n.thinking || '').replace(/\s+/g, ' ').slice(0, 90)
  return (
    <div className="w-[230px] rounded-lg bg-hull/95 px-2.5 py-1.5 ring-1 backdrop-blur"
      style={{ boxShadow: `0 0 0 1px ${color}${selected ? 'cc' : '44'}, 0 0 ${selected ? 18 : 10}px -6px ${color}` }}>
      <Handle type="target" position={Position.Top} className="!h-1.5 !w-1.5 !border-0 !bg-line-strong" />
      <div className="flex items-center gap-1.5 text-[10.5px]" style={{ color }}>
        {k.icon}<span className="font-semibold uppercase tracking-wider">{k.label}</span>
        <span className="num ml-auto truncate text-ink-faint">{stat}</span>
      </div>
      <div className="mt-0.5 line-clamp-2 text-[11.5px] leading-snug text-ink-dim">{text || <span className="text-ink-faint">(no text)</span>}</div>
      <Handle type="source" position={Position.Bottom} className="!h-1.5 !w-1.5 !border-0 !bg-line-strong" />
    </div>
  )
}

const nodeTypes = { g: GNode }
type Elk = InstanceType<typeof import('elkjs/lib/elk.bundled.js').default>
let elkReady: Promise<Elk> | null = null
const getElk = () => (elkReady ??= import('elkjs/lib/elk.bundled.js').then((m) => new m.default()))

function build(nodes: NodeInfo[]) {
  const list = nodes.slice(-400)
  const byUse = new Map<string, string>()
  for (const n of list) if (n.kind === 'tool_call') { const u = (n.detail as D | null)?.tool_use_id; if (u) byUse.set(u, n.node_id) }
  const edges: Edge[] = []
  const add = (a: string, b: string, style?: Edge['style'], animated = false) => edges.push({ id: `${a}>${b}`, source: a, target: b, style, animated })
  const turns: string[] = []
  let lastOfTurn: string | null = null
  let pendingResults: string[] = []
  let prevTurn: string | null | undefined
  let turnIdx = 0
  const turnOf = new Map<string, number>()
  for (const n of list) {
    if (n.turn_id !== prevTurn) {
      if (prevTurn !== undefined) { turnIdx++; if (lastOfTurn && n.kind === 'user_message') add(lastOfTurn, n.node_id, { stroke: '#475569', strokeDasharray: '4 4' }) }
      prevTurn = n.turn_id; pendingResults = []
      turns.push(n.turn_id ?? '')
    }
    turnOf.set(n.node_id, turnIdx)
    const d = (n.detail ?? {}) as D
    if (n.kind === 'assistant_message') {
      const prev = list[list.indexOf(n) - 1]
      if (pendingResults.length) { for (const r of pendingResults) add(r, n.node_id, { stroke: toneHex.ok }); pendingResults = [] }
      else if (prev && prev.turn_id === n.turn_id && prev.kind === 'user_message') add(prev.node_id, n.node_id, { stroke: '#64748b' })
      for (const c of (d.tool_calls ?? []) as D[]) { const call = byUse.get(c.id); if (call) add(n.node_id, call, { stroke: toneHex.tool }) }
    } else if (n.kind === 'tool_result') {
      const call = d.tool_use_id ? byUse.get(d.tool_use_id) : undefined
      if (call) add(call, n.node_id, { stroke: toneHex.tool })
      pendingResults.push(n.node_id)
    }
    lastOfTurn = n.node_id
  }
  return { list, edges, turnOf }
}

export function SessionGraph({ nodes }: { nodes: NodeInfo[] }) {
  // One turn at a time reads best (a turn is about ten nodes); "all" shows the session's newest 400.
  const turnIds = useMemo(() => [...new Set(nodes.map((n) => n.turn_id ?? ''))], [nodes])
  const [turn, setTurn] = useState<string | 'all' | null>(null)
  const shown = turn ?? turnIds[turnIds.length - 1] ?? 'all'
  const scoped = useMemo(() => (shown === 'all' ? nodes : nodes.filter((n) => (n.turn_id ?? '') === shown)), [nodes, shown])
  const { list, edges, turnOf } = useMemo(() => build(scoped), [scoped])
  const [laid, setLaid] = useState<Node<GData>[]>([])
  const [pick, setPick] = useState<NodeInfo | null>(null)
  useEffect(() => {
    let cancelled = false
    getElk().then((elk) => elk.layout({
      id: 'root',
      layoutOptions: { 'elk.algorithm': 'layered', 'elk.direction': 'DOWN', 'elk.spacing.nodeNode': '16', 'elk.layered.spacing.nodeNodeBetweenLayers': '28' },
      children: list.map((n) => ({ id: n.node_id, width: 232, height: 52 })),
      edges: edges.map((e) => ({ id: e.id, sources: [e.source], targets: [e.target] })),
    })).then((g) => {
      if (cancelled) return
      const pos = new Map((g.children ?? []).map((c) => [c.id, { x: c.x ?? 0, y: c.y ?? 0 }]))
      setLaid(list.map((n) => ({ id: n.node_id, type: 'g', position: pos.get(n.node_id) ?? { x: 0, y: 0 }, data: { n, turn: turnOf.get(n.node_id) ?? 0 } })))
    }).catch(() => {})
    return () => { cancelled = true }
  }, [list, edges, turnOf])
  if (!nodes.length) return <Empty>no nodes</Empty>
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex gap-1 overflow-x-auto border-b border-line px-2 py-1.5">
        {turnIds.map((t, i) => (
          <button key={t || i} onClick={() => setTurn(t)}
            className={`num shrink-0 rounded-md px-2 py-1 text-[10.5px] ring-1 ring-inset ${t === shown ? 'bg-live/10 text-live ring-live/30' : 'text-ink-faint ring-line hover:text-ink'}`}>
            turn {i + 1}
          </button>
        ))}
        <button onClick={() => setTurn('all')}
          className={`num shrink-0 rounded-md px-2 py-1 text-[10.5px] ring-1 ring-inset ${shown === 'all' ? 'bg-live/10 text-live ring-live/30' : 'text-ink-faint ring-line hover:text-ink'}`}>all</button>
      </div>
      <div className="relative min-h-0 flex-1">
        <ReactFlow key={shown} nodes={laid} edges={edges} nodeTypes={nodeTypes} fitView minZoom={0.1} maxZoom={1.5} colorMode="dark"
          proOptions={{ hideAttribution: true }} onNodeClick={(_, n) => setPick((n.data as GData).n)} nodesDraggable={false}>
          <Background color="rgba(148,163,184,0.10)" gap={20} size={1} />
          <Controls showInteractive={false} className="!bg-hull !shadow-none [&>button]:!border-line [&>button]:!bg-hull [&>button]:!fill-ink-dim" />
        </ReactFlow>
        <div className="num pointer-events-none absolute right-3 top-2 text-[10.5px] text-ink-faint">{list.length} nodes · {edges.length} edges{nodes.length > list.length ? ` · newest ${list.length}` : ''}</div>
      </div>
      <div className="h-48 shrink-0 overflow-auto border-t border-line p-2">
        {pick ? (
          <>
            <div className="num mb-1 text-[11px] text-ink-faint">
              {pick.kind} · {short(pick.node_id)} · position {pick.position} · <Reach key={pick.node_id} id={pick.node_id} />
            </div>
            <JsonView value={{ ...pick, text: pick.text.length > 2000 ? `${pick.text.slice(0, 2000)}…` : pick.text }} maxHeight="140px" />
          </>
        ) : <Empty>click a node for its record</Empty>}
      </div>
    </div>
  )
}

/** A node's reach (theseus-n4m, step 12a), as the Observatory's cell says it: read when clicked, and again on a
 * click; each generation in its tooltip. */
function Reach({ id }: { id: string }) {
  const [r, setR] = useState<NodeReachResult | string | null>(null)
  const ask = () => {
    setR('reading…')
    call<NodeReachResult>('node.reach', { node_id: id }).then(setR, (e: unknown) =>
      setR(`no reach: ${(e as { message?: string }).message ?? String(e)}`),
    )
  }
  if (r === null) {
    return (
      <button onClick={ask} className="rounded px-1.5 ring-1 ring-inset ring-line hover:text-ink" title="where this node went (node.reach)">
        reach
      </button>
    )
  }
  if (typeof r === 'string') return <span>{r}</span>
  const w = reachWords(r)
  return (
    <span onClick={ask} className="cursor-pointer text-live" title={w.generations.join('\n')}>
      {w.seen}
      {w.first != null && w.last != null ? ` · ${clock(w.first)}–${clock(w.last)}` : ''}
    </span>
  )
}
