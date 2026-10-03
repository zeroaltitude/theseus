// The fleet: every session and execution, sortable and filterable, beside a live graph of who started whom
// (tasks) and who reports to whom. Rows and nodes open the session deck. The same panel lists the executions (`?view=executions`),
// each with its queue, its budget, and its cancel, as the Observatory's Executions table did.
import { useDeferredValue, useEffect, useMemo, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router'
import { ReactFlow, Background, Controls, Handle, Position, type Edge, type Node, type NodeProps } from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import { Group, Panel as RPanel, Separator } from 'react-resizable-panels'
import { ArrowDownUp, Layers, Network, Plus, Search, Wrench } from 'lucide-react'
import type { ExecutionInfo, SessionInfo } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { useSettled, useTick } from '@/lib/hooks'
import { useWorld } from '@/lib/world'
import { ago, cn, pct, short, stamp, tokens, usd } from '@/lib/format'
import { stateTone, toneHex } from '@/lib/taxonomy'
import { AttentionPill, Empty, LiveDot, Meter, Panel, Pill, Segmented } from '@/components/ui'
import { ExecutionTable } from '@/components/ExecutionTable'

type Key = 'state' | 'title' | 'turns' | 'tools' | 'tokens' | 'cache' | 'cost' | 'active'
type Sort = { k: Key; desc: boolean }

const NO_SESSIONS: SessionInfo[] = []
const NO_EXECUTIONS: ExecutionInfo[] = []

function Th({ k, sort, setSort, children, right }: { k: Key; sort: Sort; setSort: (f: (s: Sort) => Sort) => void; children: React.ReactNode; right?: boolean }) {
  return (
    <th className={cn('cursor-pointer select-none px-2 py-1.5 font-semibold hover:text-ink', right ? 'text-right' : 'text-left')}
      onClick={() => setSort((s) => ({ k, desc: s.k === k ? !s.desc : true }))}>
      <span className={cn('inline-flex items-center gap-1', sort.k === k && 'text-live')}>{children}{sort.k === k && <ArrowDownUp size={10} />}</span>
    </th>
  )
}

const tokIn = (s: SessionInfo) => s.usage.input_tokens + s.usage.cache_read_input_tokens + s.usage.cache_creation_input_tokens

/** session.open from the cockpit: a new conversation with an optional label, opened in its deck, where the
 *  composer sends its first turn. */
function NewSession({ onOpened }: { onOpened: (sessionId: string) => void }) {
  const [busy, setBusy] = useState(false)
  const open = async () => {
    const label = window.prompt('A label for the new session (optional):', '')
    if (label === null) return
    setBusy(true)
    try {
      const s = await call<SessionInfo>('session.open', { kind: 'conversation', label: label.trim() || undefined })
      onOpened(s.session_id)
    } catch (e: any) {
      window.alert(e?.message ?? String(e))
    } finally {
      setBusy(false)
    }
  }
  return (
    <button onClick={open} disabled={busy} title="Open a new conversation session"
      className="flex items-center gap-1 rounded-md px-2 py-0.5 text-[11px] text-live ring-1 ring-live/30 hover:bg-live/10 disabled:opacity-50">
      <Plus size={12} /> {busy ? 'opening…' : 'New session'}
    </button>
  )
}

export default function Fleet() {
  const nav = useNavigate()
  const tick = useTick(5000)
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 2000)
  const { data: el } = useRpc<{ executions: ExecutionInfo[] }>('execution.list', undefined, 2000)
  // The time machine: the fleet as it stood at its moment, folded from the ledger. Deferred, so a scrub's needle
  // never waits on the table and the graph.
  const world = useDeferredValue(useWorld())
  // The graph follows a scrub once the needle rests; the table follows every step.
  const graphWorld = useSettled(world, 180)
  const now = world?.t ?? tick
  const sessions = world?.sessions ?? sl?.sessions ?? NO_SESSIONS
  const executions = world?.executions ?? el?.executions ?? NO_EXECUTIONS
  const execOf = useMemo(() => new Map(executions.map((e) => [e.execution_id, e])), [executions])

  // The search and the state filter live in the address (?q=…&state=…).
  const [params, setParams] = useSearchParams()
  const q = params.get('q') ?? ''
  const state = params.get('state')
  const setQ = (v: string) => setParams((p) => { if (v) p.set('q', v); else p.delete('q'); return p }, { replace: true })
  const setState = (v: string | null) => setParams((p) => { if (v) p.set('state', v); else p.delete('state'); return p }, { replace: true })
  const [sort, setSort] = useState<Sort>({ k: 'active', desc: true })
  const view = params.get('view') === 'executions' ? 'executions' : 'sessions'
  const setView = (v: 'sessions' | 'executions') => setParams((p) => { if (v === 'executions') p.set('view', v); else p.delete('view'); return p }, { replace: true })
  const titleOf = useMemo(() => { const m = new Map(sessions.map((s) => [s.session_id, s.title || s.label || short(s.session_id)])); return (sid: string) => m.get(sid) ?? short(sid) }, [sessions])

  const states = useMemo(() => {
    const m = new Map<string, number>()
    for (const s of sessions) m.set(s.execution_state ?? 'idle', (m.get(s.execution_state ?? 'idle') ?? 0) + 1)
    return [...m.entries()].sort((a, b) => b[1] - a[1])
  }, [sessions])

  const rows = useMemo(() => {
    const needle = q.toLowerCase()
    const val = (s: SessionInfo): number | string => {
      switch (sort.k) {
        case 'state': return s.execution_state ?? ''
        case 'title': return (s.title || s.label || '').toLowerCase()
        case 'turns': return s.turns
        case 'tools': return s.tool_calls ?? 0
        case 'tokens': return tokIn(s)
        case 'cache': return s.usage.cache_read_input_tokens / Math.max(1, tokIn(s))
        case 'cost': return s.cost_usd ?? 0
        case 'active': return s.last_active_ms ?? 0
      }
    }
    return sessions
      .filter((s) => !state || (s.execution_state ?? 'idle') === state)
      .filter((s) => !needle || `${s.title ?? ''} ${s.label ?? ''} ${s.session_id} ${s.model ?? ''}`.toLowerCase().includes(needle))
      .sort((a, b) => {
        const x = val(a), y = val(b)
        const c = x < y ? -1 : x > y ? 1 : 0
        return sort.desc ? -c : c
      })
  }, [sessions, q, state, sort])

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <div className="flex items-center gap-1.5 rounded-md bg-white/5 px-2.5 py-1.5 ring-1 ring-line focus-within:ring-live/40">
          <Search size={13} className="text-ink-faint" />
          <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="search sessions…" className="w-56 bg-transparent text-[12.5px] text-ink outline-none placeholder:text-ink-faint" />
        </div>
        <button onClick={() => setState(null)} className={cn('rounded-md px-2 py-1 text-[11.5px] ring-1 ring-inset', !state ? 'bg-live/10 text-live ring-live/30' : 'text-ink-faint ring-line')}>all {sessions.length}</button>
        {states.map(([s, n]) => (
          <button key={s} onClick={() => setState(state === s ? null : s)} className={cn('rounded-md ring-offset-0', state === s && 'ring-1 ring-live/50')}>
            <Pill tone={stateTone(s)}>{s} <span className="num font-semibold">{n}</span></Pill>
          </button>
        ))}
        {world && <Pill tone="wait">as of {stamp(world.t)} · folded from the ledger</Pill>}
        <span className="num ml-auto text-[11px] text-ink-faint">{executions.length} executions · {usd(sessions.reduce((a, s) => a + (s.cost_usd ?? 0), 0))} across the fleet</span>
      </div>

      <Group orientation="horizontal" className="min-h-0 flex-1">
        <RPanel defaultSize="62" minSize={480} className="min-h-0">
          <Panel title={view === 'executions' ? <>Executions · {executions.length}</> : 'Sessions'} icon={<Layers size={13} />} className="h-full" bodyClassName="min-h-0 overflow-auto"
            actions={<>
              <Segmented value={view} options={['sessions', 'executions'] as const} onChange={setView} />
              {world ? <span className="text-[11px] text-ink-faint">return to LIVE to open a session</span> : <NewSession onOpened={(id) => nav(`/session/${id}`)} />}
            </>}>
            {view === 'executions' ? <ExecutionTable executions={executions} title={titleOf} now={now} past={!!world} /> : <>
            <table className="w-full whitespace-nowrap text-[12px]">
              <thead className="sticky top-0 z-10 bg-hull/95 text-[10px] uppercase tracking-wider text-ink-faint backdrop-blur">
                <tr>
                  <Th k="state" sort={sort} setSort={setSort}>state</Th><Th k="title" sort={sort} setSort={setSort}>session</Th><Th k="turns" sort={sort} setSort={setSort} right>turns</Th><Th k="tools" sort={sort} setSort={setSort} right>tools</Th>
                  <Th k="tokens" sort={sort} setSort={setSort} right>tokens in</Th><Th k="cache" sort={sort} setSort={setSort} right>cache</Th><Th k="cost" sort={sort} setSort={setSort} right>cost</Th>
                  <th className="w-36 px-2 py-1.5 text-left font-semibold">budget</th><Th k="active" sort={sort} setSort={setSort} right>active</Th>
                </tr>
              </thead>
              <tbody>
                {rows.map((s) => {
                  const e = s.execution_id ? execOf.get(s.execution_id) : undefined
                  const b = e?.budget
                  return (
                    <tr key={s.session_id} onClick={() => nav(`/session/${s.session_id}`)} className="cursor-pointer border-t border-line/60 hover:bg-live/[0.04]">
                      <td className="px-2 py-1.5"><AttentionPill a={s.attention} state={s.execution_state} /></td>
                      <td className="max-w-[340px] px-2 py-1.5">
                        <div className="truncate text-ink">{s.title || s.label || 'untitled'}</div>
                        <div className="num truncate text-[10.5px] text-ink-faint">{short(s.session_id)} · {s.kind}{s.label && s.title ? ` · ${s.label}` : ''}{s.model ? ` · ${s.model}` : ''}</div>
                      </td>
                      <td className="num px-2 py-1.5 text-right text-ink">{s.turns}</td>
                      <td className="num px-2 py-1.5 text-right text-tool">{s.tool_calls ?? 0}</td>
                      <td className="num px-2 py-1.5 text-right text-ink-dim">{tokens(tokIn(s))}</td>
                      <td className="num px-2 py-1.5 text-right text-think">{tokIn(s) ? pct(s.usage.cache_read_input_tokens / tokIn(s)) : '—'}</td>
                      <td className="num px-2 py-1.5 text-right text-money">{usd(s.cost_usd)}</td>
                      <td className="px-2 py-1.5">{b ? <><Meter value={b.spent_usd + b.reserved_usd} max={b.limit_usd} tone={b.available_usd > b.limit_usd * 0.2 ? 'ok' : b.available_usd > 0 ? 'wait' : 'fault'} /><div className="num mt-0.5 text-[10px] text-ink-faint">{usd(b.available_usd)} left</div></> : <span className="text-ink-faint">—</span>}</td>
                      <td className="num px-2 py-1.5 text-right text-ink-faint">{ago(s.last_active_ms, now)}{s.pending_confirms ? <span className="ml-1 text-wait">· {s.pending_confirms} waiting</span> : null}</td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
            {!rows.length && <Empty>no sessions match</Empty>}
            </>}
          </Panel>
        </RPanel>
        <Separator className="mx-1.5 w-1 rounded-full bg-transparent transition-colors hover:bg-live/30" />
        <RPanel defaultSize="38" minSize={320} className="min-h-0">
          <Panel title="Graph · tasks and reports" icon={<Network size={13} />} className="h-full" bodyClassName="min-h-0">
            <FleetGraph sessions={graphWorld?.sessions ?? sessions} executions={graphWorld?.executions ?? executions} onOpen={(sid) => nav(`/session/${sid}`)}
              layout={graphWorld ? { sessions: sl?.sessions ?? NO_SESSIONS, executions: el?.executions ?? NO_EXECUTIONS } : undefined} />
          </Panel>
        </RPanel>
      </Group>
    </div>
  )
}

// ---------------------------------------------------------------- the graph

interface SessionNodeData extends Record<string, unknown> { s: SessionInfo; e?: ExecutionInfo }

function SessionNode({ data }: NodeProps<Node<SessionNodeData>>) {
  const { s, e } = data
  const tone = stateTone(s.execution_state)
  return (
    <div className="w-[210px] rounded-xl bg-hull/95 px-3 py-2 shadow-xl ring-1 backdrop-blur" style={{ boxShadow: `0 0 0 1px ${toneHex[tone]}55, 0 0 24px -8px ${toneHex[tone]}88` }}>
      <Handle type="target" position={Position.Left} className="!h-2 !w-2 !border-0 !bg-line-strong" />
      <div className="flex items-center gap-1.5">
        <LiveDot tone={tone} pulse={tone === 'live'} size={6} />
        <span className="truncate text-[12px] font-medium text-ink">{s.title || s.label || short(s.session_id)}</span>
      </div>
      <div className="num mt-0.5 flex items-center gap-2 whitespace-nowrap text-[10.5px] text-ink-faint">
        <span className="min-w-0 truncate" style={{ color: toneHex[tone] }} title={s.attention?.label}>{s.attention?.label ?? s.execution_state ?? 'idle'}</span>
        <span className="ml-auto inline-flex items-center gap-1">{s.turns}t · {s.tool_calls ?? 0}<Wrench size={9} /></span><span className="text-money">{usd(s.cost_usd)}</span>
      </div>
      {e?.budget && <Meter className="mt-1.5" value={e.budget.spent_usd + e.budget.reserved_usd} max={e.budget.limit_usd} tone={tone === 'fault' ? 'fault' : 'ok'} />}
      <Handle type="source" position={Position.Right} className="!h-2 !w-2 !border-0 !bg-line-strong" />
    </div>
  )
}

const nodeTypes = { session: SessionNode }

// The layout engine is large (about 1.4 MB): load it when a graph first draws, so the table shows at once.
type Elk = InstanceType<typeof import('elkjs/lib/elk.bundled.js').default>
let elkReady: Promise<Elk> | null = null
const getElk = () => (elkReady ??= import('elkjs/lib/elk.bundled.js').then((m) => new m.default()))

/** The graph of who started whom. `layout` (the time machine's present lists) lays it out once; the past then only
 *  hides the nodes that did not exist yet, so a scrub never lays it out again and no node moves. */
function FleetGraph({ sessions, executions, onOpen, layout }: {
  sessions: SessionInfo[]; executions: ExecutionInfo[]; onOpen: (sid: string) => void
  layout?: { sessions: SessionInfo[]; executions: ExecutionInfo[] }
}) {
  const [laid, setLaid] = useState<{ nodes: Node<SessionNodeData>[]; edges: Edge[] }>({ nodes: [], edges: [] })
  const lay = layout ?? { sessions, executions }
  const shape = useMemo(() => lay.sessions.map((s) => `${s.session_id}:${s.parent_session_id ?? ''}`).join('|') + lay.executions.map((e) => e.reports_to ?? '').join('|'), [lay.sessions, lay.executions])
  const byExec = useMemo(() => new Map(executions.map((e) => [e.execution_id, e])), [executions])

  useEffect(() => {
    const ids = new Set(lay.sessions.map((s) => s.session_id))
    const sessOfExec = new Map(lay.executions.map((e) => [e.execution_id, e.session_id]))
    const edges: Edge[] = []
    for (const s of lay.sessions) {
      if (s.parent_session_id && ids.has(s.parent_session_id)) {
        edges.push({ id: `t-${s.session_id}`, source: s.parent_session_id, target: s.session_id, style: { stroke: toneHex.tool, strokeWidth: 1.5 } })
      }
    }
    for (const e of lay.executions) {
      const to = e.reports_to ? sessOfExec.get(e.reports_to) ?? e.reports_to : null
      if (to && ids.has(to) && to !== e.session_id && !edges.some((x) => x.source === to && x.target === e.session_id)) {
        edges.push({ id: `r-${e.execution_id}`, source: e.session_id, target: to, style: { stroke: toneHex.model, strokeDasharray: '4 3' } })
      }
    }
    let cancelled = false
    getElk().then((elk) => elk.layout({
      id: 'root',
      layoutOptions: { 'elk.algorithm': 'layered', 'elk.direction': 'RIGHT', 'elk.spacing.nodeNode': '22', 'elk.layered.spacing.nodeNodeBetweenLayers': '60', 'elk.separateConnectedComponents': 'true', 'elk.spacing.componentComponent': '26' },
      children: lay.sessions.map((s) => ({ id: s.session_id, width: 214, height: 74 })),
      edges: edges.map((e) => ({ id: e.id, sources: [e.source], targets: [e.target] })),
    }).then((g) => {
      if (cancelled) return
      const pos = new Map((g.children ?? []).map((c) => [c.id, { x: c.x ?? 0, y: c.y ?? 0 }]))
      setLaid({
        nodes: lay.sessions.map((s) => ({ id: s.session_id, type: 'session', position: pos.get(s.session_id) ?? { x: 0, y: 0 }, data: { s } })),
        edges,
      })
    })).catch(() => {})
    return () => { cancelled = true }
    // Lay out again only when the graph's shape changes; node data refreshes below.
  }, [shape]) // eslint-disable-line react-hooks/exhaustive-deps -- the shape key stands for the lists it reads

  // Fresh data on the laid-out nodes, every poll, without moving them. A node whose shown fields did not change keeps
  // its object, so a scrub of the time machine redraws only the nodes that changed.
  const [kept] = useState(() => new Map<string, { key: string; node: Node<SessionNodeData> }>())
  const cur = useMemo(() => new Map(sessions.map((s) => [s.session_id, s])), [sessions])
  const nodes = useMemo(() => laid.nodes.filter((n) => cur.has(n.id)).map((n) => {
    const s = cur.get(n.id)!
    const e = s.execution_id ? byExec.get(s.execution_id) : undefined
    const key = [n.position.x, n.position.y, s.title, s.label, s.execution_state, s.attention?.label, s.turns, s.tool_calls, s.cost_usd, e?.budget.spent_usd, e?.budget.reserved_usd, e?.budget.limit_usd].join('|')
    const had = kept.get(n.id)
    if (had && had.key === key) return had.node
    const node = { ...n, data: { s, e } }
    kept.set(n.id, { key, node })
    return node
  }), [laid.nodes, cur, byExec, kept])
  // A task's tether flows while the task runs; an edge to a node not yet there is not drawn.
  const edges = useMemo(() => laid.edges
    .filter((e) => cur.has(e.source) && cur.has(e.target))
    .map((e) => (e.id.startsWith('t-') && cur.get(e.target)?.execution_state === 'running' ? { ...e, animated: true } : e)), [laid.edges, cur])

  if (!sessions.length) return <Empty>no sessions</Empty>
  return (
    <div className="relative h-full">
      <ReactFlow nodes={nodes} edges={edges} nodeTypes={nodeTypes} fitView fitViewOptions={{ padding: 0.2 }} minZoom={0.2} maxZoom={1.6}
        proOptions={{ hideAttribution: true }} colorMode="dark" nodesDraggable onNodeClick={(_, n) => onOpen(n.id)}>
        <Background color="rgba(176,141,87,0.12)" gap={22} size={1} />
        <Controls showInteractive={false} className="!bg-hull !shadow-none [&>button]:!border-line [&>button]:!bg-hull [&>button]:!fill-ink-dim" />
      </ReactFlow>
      {!laid.edges.length && <div className="pointer-events-none absolute bottom-3 right-3 text-[11px] text-ink-faint">no tasks or reports yet: edges appear when a session starts a task</div>}
    </div>
  )
}
