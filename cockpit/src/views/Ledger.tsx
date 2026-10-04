// The ledger explorer: the daemon's append-only record, all of it. A treemap of kinds filters by click (a kind, or
// its family by its header), the family chips filter by a kind's first segment, the histogram's brush filters by
// time, search covers kinds, summaries, and payloads, and the list is virtualized. `?view=nodes` lists the store's
// nodes instead: what was said and done, every session's or one's.
import { useMemo, useRef, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import { Group, Panel as RPanel, Separator } from 'react-resizable-panels'
import { Boxes, Layers, ScrollText, Search, X } from 'lucide-react'
import type { Health, LedgerEntry, NodeInfo, NodeListResult, Tightening } from '@protocol'
import { useLedger } from '@/lib/derive'
import { useRpc } from '@/lib/rpc'
import { nodeSummary, summarize } from '@/lib/summary'
import { clock, cn, short, stamp } from '@/lib/format'
import { ledgerKind, toneHex, type Tone } from '@/lib/taxonomy'
import { axisStyle, type EChartsOption } from '@/lib/chart'
import { Echart } from '@/components/Echart'
import { JsonView } from '@/components/JsonView'
import { Empty, Panel, Pill, Segmented } from '@/components/ui'
import { ShouldHaveAsked } from '@/components/ShouldHaveAsked'
import { Reach } from '@/components/SessionGraph'

const NO_ROWS: LedgerEntry[] = []

export default function Ledger() {
  const nav = useNavigate()
  // The search, the kind and family filters, the session, and the view live in the address
  // (?q=…&kind=a,b&family=tool&session=…&view=nodes), so a view can be bookmarked or shared.
  const [params, setParams] = useSearchParams()
  const view = params.get('view') === 'nodes' ? 'nodes' : 'rows'
  const setView = (v: 'rows' | 'nodes') => setParams((p) => { if (v === 'nodes') p.set('view', v); else p.delete('view'); return p }, { replace: true })
  // Read only what is on screen: the rows only while they are shown.
  const { data } = useLedger(20_000, 5000, undefined, undefined, view === 'rows')
  const rows = data?.rows ?? NO_ROWS
  const q = params.get('q') ?? ''
  const kinds = useMemo(() => new Set((params.get('kind') ?? '').split(',').filter(Boolean)), [params])
  const setQ = (v: string) => setParams((p) => { if (v) p.set('q', v); else p.delete('q'); return p }, { replace: true })
  const setKinds = (f: (s: Set<string>) => Set<string>) => setParams((p) => {
    const next = [...f(new Set((p.get('kind') ?? '').split(',').filter(Boolean)))]
    if (next.length) p.set('kind', next.join(',')); else p.delete('kind')
    return p
  }, { replace: true })
  // A family of kinds (the Observatory's `tool.*` chips, theseus-vm3n.6): every kind whose first segment it is.
  const family = params.get('family')
  const setFamily = (f: string | null) => setParams((p) => { if (f) p.set('family', f); else p.delete('family'); return p }, { replace: true })
  // One session's rows (?session=), as the Observatory's 'this tab's session' did: from a session's own links.
  const session = params.get('session')
  const clearSession = () => setParams((p) => { p.delete('session'); return p }, { replace: true })
  const { data: health } = useRpc<Health>('health', undefined, 5000)
  const tightened = useMemo(() => new Map<string, Tightening>((health?.tightenings ?? []).map((t) => [t.tool, t])), [health])
  const [range, setRange] = useState<[number, number] | null>(null)
  const [pick, setPick] = useState<LedgerEntry | null>(null)

  // The families in the rows read, the busiest first.
  const families = useMemo(() => {
    const m = new Map<string, number>()
    for (const r of rows) { const f = ledgerKind(r.kind).family; m.set(f, (m.get(f) ?? 0) + 1) }
    return [...m.entries()].sort((a, b) => b[1] - a[1])
  }, [rows])
  const inFamily = (r: LedgerEntry) => !family || ledgerKind(r.kind).family === family
  const filtered = useMemo(() => {
    const needle = q.toLowerCase()
    return rows.filter((r) =>
      (!session || r.session_id === session) &&
      (!kinds.size || kinds.has(r.kind)) &&
      (!family || ledgerKind(r.kind).family === family) &&
      (!range || (r.at_unix_ms >= range[0] && r.at_unix_ms <= range[1])) &&
      (!needle || r.kind.includes(needle) || (r.session_id ?? '').includes(needle) || summarize(r).toLowerCase().includes(needle) || JSON.stringify(r.data).toLowerCase().includes(needle)),
    ).reverse()
  }, [rows, kinds, family, q, range, session])

  const toggle = (k: string) => setKinds((s) => { const n = new Set(s); if (n.has(k)) n.delete(k); else n.add(k); return n })

  if (view === 'nodes') return <Nodes session={session} onClearSession={clearSession} onView={setView} />

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-[1fr_1.4fr]">
        <Panel title={<>Kinds · {new Set(rows.map((r) => r.kind)).size}</>} icon={<Layers size={13} />} bodyClassName="h-[220px] p-1">
          <KindMap rows={rows} selected={kinds} family={family} onToggle={toggle} onFamily={(f) => setFamily(family === f ? null : f)} />
        </Panel>
        <Panel title="Over time · drag the brush to filter" icon={<ScrollText size={13} />} bodyClassName="h-[220px] p-2"
          actions={range ? <button onClick={() => setRange(null)} className="flex items-center gap-1 text-[11px] text-live"><X size={11} /> clear range</button> : null}>
          <Histogram rows={rows.filter((r) => (!kinds.size || kinds.has(r.kind)) && inFamily(r))} onRange={setRange} />
        </Panel>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <div className="flex items-center gap-1.5 rounded-md bg-white/5 px-2.5 py-1.5 ring-1 ring-line focus-within:ring-live/40">
          <Search size={13} className="text-ink-faint" />
          <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="search kinds, summaries, payloads, session ids…" className="w-96 bg-transparent text-[12.5px] text-ink outline-none placeholder:text-ink-faint" />
        </div>
        {session && <button onClick={clearSession} title="show every session's rows"><Pill tone="live">session {short(session)} <X size={10} /></Pill></button>}
        {[...kinds].map((k) => <button key={k} onClick={() => toggle(k)}><Pill tone={ledgerKind(k).tone}>{k} <X size={10} /></Pill></button>)}
        <span className="num ml-auto text-[11px] text-ink-faint">{filtered.length.toLocaleString()} of {rows.length.toLocaleString()} rows read · {data?.total?.toLocaleString() ?? '—'} in the ledger</span>
      </div>
      <div className="-mt-1 flex flex-wrap items-center gap-1" title="a family of kinds: every kind whose first segment it is">
        <span className="ship-engraved mr-1 text-[9.5px]">families</span>
        <FamilyChip on={!family} tone="live" onClick={() => setFamily(null)}>all</FamilyChip>
        {families.map(([f, n]) => (
          <FamilyChip key={f} on={family === f} tone={ledgerKind(`${f}.`).tone} onClick={() => setFamily(family === f ? null : f)}>
            {f}.* <span className="text-ink-faint">{n.toLocaleString()}</span>
          </FamilyChip>
        ))}
      </div>

      <Group orientation="horizontal" className="min-h-[360px] flex-1">
        <RPanel defaultSize="60" minSize={420} className="min-h-0">
          <Panel title="Rows" icon={<ScrollText size={13} />} className="h-full" bodyClassName="min-h-0"
            actions={<Segmented value={view} options={['rows', 'nodes'] as const} onChange={setView} />}>
            <RowList rows={filtered} pick={pick} onPick={setPick} />
          </Panel>
        </RPanel>
        <Separator className="mx-1.5 w-1 rounded-full bg-transparent transition-colors hover:bg-live/30" />
        <RPanel defaultSize="40" minSize={300} className="min-h-0">
          <Panel title={pick ? <>row {pick.position} · {pick.kind}</> : 'Row'} className="h-full" bodyClassName="min-h-0 overflow-auto p-3">
            {pick ? (
              <div className="flex flex-col gap-2">
                <div className="text-[12.5px] text-ink">{summarize(pick)}</div>
                <div className="num flex flex-wrap gap-x-3 text-[11px] text-ink-faint">
                  <span>{stamp(pick.at_unix_ms)}</span>
                  {pick.session_id && <button className="text-live hover:underline" onClick={() => nav(`/session/${pick.session_id}`)}>{pick.session_id}</button>}
                  {pick.turn_id && <span>{pick.turn_id}</span>}
                </div>
                {pick.kind === 'tool.notified' && typeof (pick.data as { tool?: unknown })?.tool === 'string' && (
                  <div><ShouldHaveAsked tool={(pick.data as { tool: string }).tool} corr={(pick.data as { correlation_id?: string }).correlation_id} tightened={tightened.get((pick.data as { tool: string }).tool)} /></div>
                )}
                <JsonView value={pick.data} maxHeight="calc(100vh - 360px)" />
              </div>
            ) : <Empty>pick a row</Empty>}
          </Panel>
        </RPanel>
      </Group>
    </div>
  )
}

function RowList({ rows, pick, onPick }: { rows: LedgerEntry[]; pick: LedgerEntry | null; onPick: (r: LedgerEntry) => void }) {
  const parent = useRef<HTMLDivElement>(null)
  const v = useVirtualizer({ count: rows.length, getScrollElement: () => parent.current, estimateSize: () => 26, overscan: 20 })
  if (!rows.length) return <Empty>no rows match</Empty>
  return (
    <div ref={parent} className="h-full overflow-auto">
      <div style={{ height: v.getTotalSize(), position: 'relative' }}>
        {v.getVirtualItems().map((it) => {
          const r = rows[it.index]
          return (
            <button key={r.position} onClick={() => onPick(r)}
              className={cn('absolute left-0 flex w-full items-baseline gap-2 border-b border-line/40 px-3 text-left text-[11.5px] hover:bg-white/[0.03]', pick?.position === r.position && 'bg-live/[0.07]')}
              style={{ top: 0, height: it.size, transform: `translateY(${it.start}px)`, lineHeight: `${it.size}px` }}>
              <span className="num w-12 shrink-0 text-right text-ink-faint">{r.position}</span>
              <span className="num w-[118px] shrink-0 text-ink-faint">{stamp(r.at_unix_ms)}</span>
              <span className="num w-44 shrink-0 truncate" style={{ color: toneHex[ledgerKind(r.kind).tone] }}>{r.kind}</span>
              <span className="num w-16 shrink-0 text-[10.5px] text-ink-faint">{r.session_id ? short(r.session_id) : ''}</span>
              <span className="min-w-0 truncate text-ink-dim">{summarize(r)}</span>
            </button>
          )
        })}
      </div>
    </div>
  )
}

function KindMap({ rows, selected, family, onToggle, onFamily }: {
  rows: LedgerEntry[]; selected: Set<string>; family: string | null; onToggle: (k: string) => void; onFamily: (f: string) => void
}) {
  const option = useMemo<EChartsOption>(() => {
    const fam = new Map<string, Map<string, number>>()
    for (const r of rows) {
      const f = ledgerKind(r.kind).family
      const m = fam.get(f) ?? new Map<string, number>()
      m.set(r.kind, (m.get(r.kind) ?? 0) + 1)
      fam.set(f, m)
    }
    return {
      tooltip: { formatter: (p: any) => `${p.name}<br/><b>${p.value}</b> rows` },
      series: [{
        type: 'treemap', roam: false, nodeClick: false, breadcrumb: { show: false }, width: '100%', height: '100%',
        itemStyle: { borderColor: '#06101d', borderWidth: 2, gapWidth: 2 },
        label: { color: '#efe3c8', fontSize: 11, fontFamily: 'JetBrains Mono Variable', formatter: (p: any) => `${String(p.name).split('.').slice(1).join('.') || p.name}\n${p.value}` },
        levels: [
          { itemStyle: { borderWidth: 0, gapWidth: 3 } },
          { itemStyle: { gapWidth: 1, borderColor: '#0a1828', borderWidth: 2 }, upperLabel: { show: true, height: 15, color: '#c8bb9b', fontSize: 10, fontWeight: 600 } },
          { itemStyle: { gapWidth: 1 } },
        ],
        data: [...fam.entries()].map(([f, kinds]) => ({
          name: f,
          children: [...kinds.entries()].map(([k, n]) => ({
            name: k, value: n,
            itemStyle: { color: toneHex[ledgerKind(k).tone], opacity: (selected.size && !selected.has(k)) || (family && f !== family) ? 0.18 : 0.75 },
          })),
        })),
      }],
    }
  }, [rows, selected, family])
  if (!rows.length) return <Empty>no rows</Empty>
  // A kind's tile picks that kind; a family's header picks the family.
  return <Echart option={option} onClick={(p: any) => { if (p?.data?.children) onFamily(p.name); else if (p?.data?.value !== undefined) onToggle(p.name) }} />
}

function FamilyChip({ on, tone, onClick, children }: { on: boolean; tone: Tone; onClick: () => void; children: React.ReactNode }) {
  return (
    <button onClick={onClick} className={cn('num rounded-md px-1.5 py-0.5 text-[11px] ring-1 ring-inset transition-colors', on ? 'bg-white/[0.06]' : 'ring-line text-ink-dim hover:text-ink')}
      style={on ? { color: toneHex[tone], boxShadow: `inset 0 0 0 1px ${toneHex[tone]}66` } : undefined}>
      {children}
    </button>
  )
}

const NODE_KINDS = ['all', 'user_message', 'assistant_message', 'tool_call', 'tool_result'] as const

function nodeTone(n: NodeInfo): Tone {
  switch (n.kind) {
    case 'user_message': return 'idle'
    case 'assistant_message': return 'model'
    case 'tool_call': return 'tool'
    case 'tool_result': return (n.detail as { is_error?: boolean } | null)?.is_error ? 'fault' : 'ok'
    default: return 'idle'
  }
}

/** The store's nodes, every session's or one's (the Observatory's Nodes list, theseus-vm3n.6): what was said and done,
 *  the newest 120, by kind. Each says itself in a line; a pick shows its record, its session, and on a click its reach.
 *  Read every 5 s, and only while shown. */
function Nodes({ session, onClearSession, onView }: { session: string | null; onClearSession: () => void; onView: (v: 'rows' | 'nodes') => void }) {
  const nav = useNavigate()
  const [params, setParams] = useSearchParams()
  const kind = (NODE_KINDS as readonly string[]).includes(params.get('nkind') ?? '') ? params.get('nkind')! : 'all'
  const setKind = (k: string) => setParams((p) => { if (k !== 'all') p.set('nkind', k); else p.delete('nkind'); return p }, { replace: true })
  const { data } = useRpc<NodeListResult>('node.list', { session_id: session, kind: kind === 'all' ? null : kind, n: 120 }, 5000)
  const nodes = useMemo(() => [...(data?.nodes ?? [])].sort((a, b) => b.position - a.position), [data])
  const { data: sl } = useRpc<{ sessions: { session_id: string; title?: string | null; label?: string | null }[] }>('session.list', undefined, 5000)
  const titleOf = useMemo(() => new Map((sl?.sessions ?? []).map((s) => [s.session_id, s.title || s.label || short(s.session_id)])), [sl])
  const [pickId, setPickId] = useState<string | null>(null)
  const pick = nodes.find((n) => n.node_id === pickId) ?? null
  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <div className="flex items-center gap-1" title="the kind of node">
          {NODE_KINDS.map((k) => (
            <FamilyChip key={k} on={kind === k} tone={k === 'assistant_message' ? 'model' : k === 'tool_call' ? 'tool' : k === 'tool_result' ? 'ok' : 'live'} onClick={() => setKind(k)}>{k}</FamilyChip>
          ))}
        </div>
        {session && <button onClick={onClearSession} title="show every session's nodes"><Pill tone="live">session {titleOf.get(session) ?? short(session)} <X size={10} /></Pill></button>}
        <span className="num ml-auto text-[11px] text-ink-faint">{nodes.length} shown · {data?.total?.toLocaleString() ?? '—'} in the store{session ? ' · this session' : ' · every session'}</span>
      </div>
      <Group orientation="horizontal" className="min-h-[360px] flex-1">
        <RPanel defaultSize="60" minSize={420} className="min-h-0">
          <Panel title="Nodes · the newest first" icon={<Boxes size={13} />} className="h-full" bodyClassName="min-h-0 overflow-auto"
            actions={<Segmented value="nodes" options={['rows', 'nodes'] as const} onChange={onView} />}>
            {!nodes.length && <Empty>{data ? 'no nodes match' : 'reading the nodes…'}</Empty>}
            {nodes.map((n) => (
              <button key={n.node_id} data-node={n.node_id} onClick={() => setPickId(n.node_id)}
                className={cn('flex w-full items-baseline gap-2 border-b border-line/40 px-3 py-1 text-left text-[11.5px] hover:bg-white/[0.03]', pickId === n.node_id && 'bg-live/[0.07]')}>
                <span className="num w-12 shrink-0 text-right text-ink-faint">{n.position}</span>
                <span className="num w-16 shrink-0 text-ink-faint">{clock(n.at_unix_ms)}</span>
                <span className="num w-[118px] shrink-0 truncate" style={{ color: toneHex[nodeTone(n)] }}>{n.kind}</span>
                <span className="num w-24 shrink-0 truncate text-[10.5px] text-ink-faint" title={n.node_id}>{short(n.node_id)}</span>
                <span className="num w-12 shrink-0 text-[10.5px] text-ink-faint">{n.loop_index != null ? `loop ${n.loop_index}` : ''}</span>
                {!session && <span className="num w-24 shrink-0 truncate text-[10.5px] text-ink-faint" title={n.session_id}>{titleOf.get(n.session_id) ?? short(n.session_id)}</span>}
                <span className="min-w-0 truncate text-ink-dim">{nodeSummary(n)}</span>
              </button>
            ))}
          </Panel>
        </RPanel>
        <Separator className="mx-1.5 w-1 rounded-full bg-transparent transition-colors hover:bg-live/30" />
        <RPanel defaultSize="40" minSize={300} className="min-h-0">
          <Panel title={pick ? <>node {pick.position} · {pick.kind}</> : 'Node'} className="h-full" bodyClassName="min-h-0 overflow-auto p-3">
            {pick ? (
              <div className="flex flex-col gap-2">
                <div className="text-[12.5px] text-ink">{nodeSummary(pick)}</div>
                <div className="num flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-ink-faint">
                  <span>{stamp(pick.at_unix_ms)}</span>
                  <button className="text-live hover:underline" onClick={() => nav(`/session/${pick.session_id}`)} title={pick.session_id}>{titleOf.get(pick.session_id) ?? short(pick.session_id)}</button>
                  {pick.turn_id && <span>{short(pick.turn_id)}</span>}
                  <Reach key={pick.node_id} id={pick.node_id} />
                </div>
                <JsonView value={{ ...pick, text: pick.text.length > 4000 ? `${pick.text.slice(0, 4000)}…` : pick.text }} maxHeight="calc(100vh - 300px)" />
              </div>
            ) : <Empty>pick a node</Empty>}
          </Panel>
        </RPanel>
      </Group>
    </div>
  )
}

function Histogram({ rows, onRange }: { rows: LedgerEntry[]; onRange: (r: [number, number] | null) => void }) {
  const buckets = 90
  const { option, edges } = useMemo(() => {
    const t0 = Math.min(...rows.map((r) => r.at_unix_ms))
    const t1 = Math.max(...rows.map((r) => r.at_unix_ms), t0 + 60_000)
    const size = (t1 - t0) / buckets
    const tones = ['live', 'model', 'tool', 'think', 'wait', 'ok', 'money', 'fault', 'idle'] as const
    const series = Object.fromEntries(tones.map((t) => [t, new Array(buckets).fill(0)])) as Record<string, number[]>
    for (const r of rows) series[ledgerKind(r.kind).tone][Math.min(buckets - 1, Math.floor((r.at_unix_ms - t0) / size))]++
    const edges = Array.from({ length: buckets + 1 }, (_, i) => t0 + i * size)
    const option: EChartsOption = {
      grid: { left: 36, right: 12, top: 8, bottom: 44 },
      tooltip: { trigger: 'axis', axisPointer: { type: 'shadow' } },
      xAxis: { type: 'category', data: edges.slice(0, -1).map((t) => stamp(t)), ...axisStyle, splitLine: { show: false }, axisLabel: { ...axisStyle.axisLabel, hideOverlap: true } },
      yAxis: { type: 'value', minInterval: 1, ...axisStyle },
      dataZoom: [{ type: 'slider', height: 16, bottom: 4, borderColor: 'transparent', backgroundColor: 'rgba(176,141,87,0.05)', fillerColor: 'rgba(34,211,238,0.14)', showDetail: false, realtime: false }],
      series: tones.filter((t) => series[t].some((v) => v > 0)).map((t) => ({
        name: t, type: 'bar', stack: 'k', data: series[t], barWidth: '85%', itemStyle: { color: toneHex[t], opacity: 0.85 },
      })),
    }
    return { option, edges }
  }, [rows])
  if (!rows.length) return <Empty>no rows</Empty>
  return (
    <Echart option={option} onDataZoom={(z) => {
      if (z.start <= 0.5 && z.end >= 99.5) return onRange(null)
      const a = edges[Math.floor((z.start / 100) * buckets)]
      const b = edges[Math.min(buckets, Math.ceil((z.end / 100) * buckets))]
      onRange([a, b])
    }} />
  )
}
