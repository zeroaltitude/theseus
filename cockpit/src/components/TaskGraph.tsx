// The task graph (M7 39b): the task records as a tree, each with its state, owner, claim, and version, and a waiting
// layer-1 change with its card; and the same records as a graph (React Flow, as SessionGraph draws a session), opened
// from the panel with its state in the address (`?taskgraph=1`, and `&task=<id>` for the one picked). It shows the
// present: under the time machine it says so, and its acts are off. `task.changed` reads `task.list` again (rpc.ts).
import { useEffect, useMemo, useRef } from 'react'
import { Link, useSearchParams } from 'react-router'
import { AnimatePresence } from 'motion/react'
import { ReactFlow, Background, Controls, Handle, Position, type Edge, type Node, type NodeProps } from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import { History, Lock, Network, X } from 'lucide-react'
import type { ConfirmRequest, TaskRecord } from '@protocol'
import { useTick } from '@/lib/hooks'
import { cardOf, changeWords, claimWords, isClosed, layout, taskShort, taskTree, writeTaskGraph } from '@/lib/taskgraph'
import { stateTone, toneHex } from '@/lib/taxonomy'
import { Empty, Panel, Pill, StatePill } from './ui'
import { ConfirmCard } from './ConfirmCard'
import { FIT, useFullGraph } from './FullGraph'
import { opsHref } from '@/ops/address'
import { hiddenIds, recordItem, tally } from '@/ops/retire'
import { RetireBar, useRetire } from '@/ops/RetireBar'

/** The records as a tree, with a button that opens the graph. `past`: the time machine's moment, when it has one. */
export function TaskTree({ records, confirms, past }: { records: TaskRecord[]; confirms: ConfirmRequest[]; past?: number }) {
  const now = useTick(15000)
  const [params, setParams] = useSearchParams()
  const open = params.get('taskgraph') === '1'
  // Soft retire (theseus-99l9z): the tree leaves old records out, never the count in its heading or the records.
  const retire = useRetire('taskgraph')
  const { show, days } = retire.choice
  const at = past ?? now
  // A `?task=` deep link's own record is kept whatever the choice hides, as the Graph lens keeps its focus (F7).
  const linked = params.get('task')
  const out = useMemo(() => hiddenIds(records, (t) => t.id, (t) => t.parent, (t) => recordItem(t, at), show, at, days, [linked].filter((x): x is string => !!x)), [records, show, at, days, linked])
  const counts = useMemo(() => tally(records, (t) => recordItem(t, at), at, days), [records, at, days])
  const rows = useMemo(() => taskTree(records).filter(({ t }) => !out.has(t.id)), [records, out])
  const openN = records.filter((t) => !isClosed(t)).length
  // Opening fills the window, so the graph appears where the click was (it used to open in a panel further down).
  const toggle = () => setParams((p) => writeTaskGraph(p, !open), { replace: open })
  return (
    <Panel help="actions.taskgraph" title={<>Plans and steps · {openN} open{records.length ? ` of ${records.length}` : ''}</>} icon={<Network size={13} />} bodyClassName="max-h-[460px] overflow-auto p-2"
      actions={records.length ? <button onClick={toggle} className="flex items-center gap-1 text-[11px] text-live">{open ? <><X size={11} /> close the graph</> : <><Network size={11} /> open as a graph</>}</button> : null}>
      {past !== undefined && <div className="mb-2 flex items-center gap-1.5 text-[11px] text-wait"><History size={12} /> the plans and steps show the present, not the log&rsquo;s moment; its acts are off</div>}
      {records.length > 0 && <RetireBar choice={retire.choice} set={retire.set} counts={counts} hidden={out.size} className="mb-2 text-[11px]" />}
      {!rows.length && <Empty>{records.length ? 'every record is retired: tick a box above to show them' : 'no plans or steps yet'}</Empty>}
      {rows.map(({ t, depth }) => {
        const claim = claimWords(t, now)
        const change = changeWords(t)
        const card = cardOf(t, confirms)
        return (
          <div key={t.id} className="border-b border-line/50 py-1.5 last:border-0" style={{ paddingLeft: 6 + depth * 16 }}>
            <div className="flex items-center gap-2">
              <StatePill state={t.state} />
              <span className="min-w-0 flex-1 truncate text-[12.5px] text-ink" title={t.objective || t.title}>{t.title}</span>
              {claim && <Pill tone="wait" title={`held by ${t.claim?.by}`}><Lock size={10} /> {claim}</Pill>}
              <span className="num text-[10.5px] text-ink-faint">v{t.version}</span>
            </div>
            <div className="num mt-0.5 flex flex-wrap gap-x-3 text-[10.5px] text-ink-faint">
              <span>{taskShort(t.id)}</span>
              <span>owner {t.owner}</span>
              {t.deps.length > 0 && <span>waits on {t.deps.map(taskShort).join(', ')}</span>}
              {t.evidence.length > 0 && <span>{t.evidence.length} evidence</span>}
              {!t.origin.by_model && <span>the operator&rsquo;s objective</span>}
            </div>
            {change && <div className="mt-1 text-[11.5px] text-wait">{change}{card ? '' : ' (its card is not waiting)'}</div>}
            {card && <div className="mt-1"><AnimatePresence initial={false}><ConfirmCard c={card} past={past} /></AnimatePresence></div>}
          </div>
        )
      })}
    </Panel>
  )
}

interface TData extends Record<string, unknown> { t: TaskRecord; now: number }

function TNode({ data, selected }: NodeProps<Node<TData>>) {
  const { t, now } = data
  const color = toneHex[stateTone(t.state)] ?? toneHex.idle
  const claim = claimWords(t, now)
  return (
    <div className="w-[230px] rounded-lg bg-hull/95 px-2.5 py-1.5 ring-1 backdrop-blur"
      style={{ boxShadow: `0 0 0 1px ${color}${selected ? 'cc' : '44'}, 0 0 ${selected ? 18 : 10}px -6px ${color}` }}>
      <Handle type="target" position={Position.Left} className="!h-1.5 !w-1.5 !border-0 !bg-line-strong" />
      <div className="flex items-center gap-1.5 text-[10.5px]" style={{ color }}>
        <span className="font-semibold uppercase tracking-wider">{t.state.replace('_', ' ')}</span>
        <span className="num ml-auto text-ink-faint">{taskShort(t.id)} · v{t.version}</span>
      </div>
      <div className="mt-0.5 line-clamp-2 text-[11.5px] leading-snug text-ink">{t.title}</div>
      <div className="num mt-0.5 truncate text-[10px] text-ink-faint">{t.owner}{claim ? ` · 🔒 ${claim}` : ''}{t.proposal ? ' · a change waits' : ''}</div>
      <Handle type="source" position={Position.Right} className="!h-1.5 !w-1.5 !border-0 !bg-line-strong" />
    </div>
  )
}

const nodeTypes = { t: TNode }

/** The graph, when the address opens it: parents to children, and (dashed) each task to what it waits on. The
 *  Operations page's Graph lens shows the same records among the live work, and its link opens it there. */
export function TaskGraphView({ records, past }: { records: TaskRecord[]; past?: number }) {
  const now = useTick(15000)
  const [params, setParams] = useSearchParams()
  const pick = params.get('task')
  const fg = useFullGraph('Plans and steps')
  const { nodes, edges } = useMemo(() => {
    const l = layout(records)
    const nodes: Node<TData>[] = l.nodes.map((n) => ({ id: n.id, type: 't', position: { x: n.x, y: n.y }, data: { t: n.t, now }, selected: n.id === pick }))
    const edges: Edge[] = l.links.map((k) => ({ id: k.id, source: k.source, target: k.target, style: k.dep ? { stroke: '#6e6450', strokeDasharray: '4 4' } : { stroke: '#9c907a' } }))
    return { nodes, edges }
  }, [records, now, pick])
  const box = useRef<HTMLDivElement>(null)
  const opened = params.get('taskgraph') === '1'
  // A deep link opens the panel below the page's others: bring it into view (the full-window graph is already).
  useEffect(() => { if (opened && !fg.full) box.current?.scrollIntoView?.({ block: 'nearest' }) }, [opened, fg.full])
  if (!opened) return null
  const picked = records.find((t) => t.id === pick)
  return (
    <div ref={box}><Panel help="actions.taskgraph" title={<>Plans and steps · {records.length} in all</>} icon={<Network size={13} />} className={fg.panel.className} dialog={fg.panel.dialog}
      actions={<span className="flex items-center gap-3"><Link to={opsHref({ lens: 'graph', sel: pick })} className="text-[11px] text-live hover:underline" data-testid="open-in-ops">open in Operations</Link>{fg.button}</span>}
      bodyClassName={fg.full ? 'flex flex-col overflow-auto p-0' : 'p-0'}>
      {past !== undefined && <div className="flex items-center gap-1.5 px-3 py-1.5 text-[11px] text-wait"><History size={12} /> the present, not the log&rsquo;s moment</div>}
      <div className={fg.full ? 'relative min-h-0 flex-1' : 'relative h-[420px]'}>
        <ReactFlow nodes={nodes} edges={edges} nodeTypes={nodeTypes} fitView fitViewOptions={FIT} onInit={fg.onInit} minZoom={0.1} maxZoom={1.5} colorMode="dark" nodesDraggable={false}
          proOptions={{ hideAttribution: true }} onNodeClick={(_, n) => setParams((p) => { p.set('task', n.id); return p }, { replace: true })}>
          <Background color="rgba(176,141,87,0.10)" gap={20} size={1} />
          <Controls showInteractive={false} className="!bg-hull !shadow-none [&>button]:!border-line [&>button]:!bg-hull [&>button]:!fill-ink-dim" />
        </ReactFlow>
      </div>
      {picked && (
        <div className="num border-t border-line px-3 py-2 text-[11px] text-ink-dim">
          <div className="text-ink">{picked.id} · {picked.title}</div>
          <div>objective: {picked.objective || '(none)'}</div>
          {picked.acceptance.map((a, i) => <div key={i}>accept: {a}</div>)}
          <div>from {picked.origin.session}{picked.session ? ` · its session ${picked.session}` : ''}</div>
        </div>
      )}
    </Panel></div>
  )
}
