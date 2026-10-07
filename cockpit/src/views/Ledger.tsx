// The ledger explorer: the daemon's append-only record, all of it. A treemap of kinds filters by click (a kind, or
// its family by its header), the family chips filter by a kind's first segment, the histogram's brush filters by
// time, search covers kinds, summaries, and payloads, and the list is virtualized. `?view=nodes` lists the store's
// nodes instead: what was said and done, every session's or one's. The two charts follow the chart method (`lib/viz.ts`,
// theseus-hnof), each with its table view.
//
// The rows are the page's one copy of the ledger (`useHistoryRows`), the whole of it, followed: this view has no loop of
// its own. The filter lives in the address and can be saved under a name in the browser; `follow` keeps the newest rows
// in view as they land, and stops while you scroll away; the rows shown can be downloaded as JSON.
import { useDeferredValue, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import { Group, Panel as RPanel, Separator } from 'react-resizable-panels'
import { Boxes, Download, Layers, ScrollText, Search, Star, X } from 'lucide-react'
import type { Health, LedgerEntry, NodeInfo, NodeListResult, Tightening } from '@protocol'
import { useHistoryRows } from '@/lib/history'
import { useAsOf } from '@/lib/timemachine'
import { SAVED_KEY, applySaved, exportName, exportOf, filterOf, filterRows, newerThan, queryOf, readSaved, withSaved, withoutSaved, writeSaved, type Saved } from '@/lib/ledgerview'
import { useRpc } from '@/lib/rpc'
import { nodeSummary, summarize } from '@/lib/summary'
import { clock, cn, pct, short, stamp } from '@/lib/format'
import { ledgerKind, toneHex, type Tone } from '@/lib/taxonomy'
import type { EChartsOption } from '@/lib/chart'
import { CATEGORICAL, CHROME, FONTS, MARK, TIME_LABELS, TIP_FRAME, TONE_MARK, baseAxis, binByKey, countTick, inkOn, niceScale, squarify, valueAxis, type Bins } from '@/lib/viz'
import { tip, type TipRow } from '@/lib/viztip'
import { textWidth, useWidth } from '@/lib/chartview'
import { ChartPanel, TipArea, TipBody, TipTarget, type LegendItem, type TableSpec } from '@/components/ChartPanel'
import { Echart } from '@/components/Echart'
import { JsonView } from '@/components/JsonView'
import { Empty, Panel, Pill, Segmented } from '@/components/ui'
import { ShouldHaveAsked } from '@/components/ShouldHaveAsked'
import { Reach } from '@/components/SessionGraph'

export default function Ledger() {
  const nav = useNavigate()
  // The search, the kind and family filters, the session, and the view live in the address
  // (?q=…&kind=a,b&family=tool&session=…&view=nodes), so a view can be bookmarked or shared.
  const [params, setParams] = useSearchParams()
  const view = params.get('view') === 'nodes' ? 'nodes' : 'rows'
  const setView = (v: 'rows' | 'nodes') => setParams((p) => { if (v === 'nodes') p.set('view', v); else p.delete('view'); return p }, { replace: true })
  // One copy of the ledger, followed; the whole of it, not a window.
  const history = useHistoryRows()
  const rows = history.rows
  const asOf = useAsOf((s) => s.t)
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
  // The time brush (?from=&to=) and the row picked (?row=) are in the address too.
  const filter = useMemo(() => filterOf(params), [params])
  const range = filter.range
  const setRange = (r: readonly [number, number] | null) => setParams((p) => {
    if (r) { p.set('from', String(Math.round(r[0]))); p.set('to', String(Math.round(r[1]))) } else { p.delete('from'); p.delete('to') }
    return p
  }, { replace: true })
  const follow = params.get('follow') === '1'
  const setFollow = (on: boolean) => setParams((p) => { if (on) p.set('follow', '1'); else p.delete('follow'); return p }, { replace: true })
  const pickPos = Number(params.get('row'))
  const pick = useMemo(() => (Number.isFinite(pickPos) && params.has('row') ? rows.find((r) => r.position === pickPos) ?? null : null), [rows, pickPos, params])
  const setPick = (r: LedgerEntry | null) => setParams((p) => { if (r) p.set('row', String(r.position)); else p.delete('row'); return p }, { replace: true })

  // The families in the rows read, the busiest first.
  const families = useMemo(() => {
    const m = new Map<string, number>()
    for (const r of rows) { const f = ledgerKind(r.kind).family; m.set(f, (m.get(f) ?? 0) + 1) }
    return [...m.entries()].sort((a, b) => b[1] - a[1])
  }, [rows])
  // The search is deferred: a keystroke never waits for the filter to cross the whole ledger.
  const deferred = useDeferredValue(filter)
  const filtered = useMemo(() => filterRows(rows, deferred), [rows, deferred])
  const histRows = useMemo(() => rows.filter((r) => (!kinds.size || kinds.has(r.kind)) && (!family || ledgerKind(r.kind).family === family)), [rows, kinds, family])
  const bins = useMemo(() => overTime(histRows), [histRows])

  const toggle = (k: string) => setKinds((s) => { const n = new Set(s); if (n.has(k)) n.delete(k); else n.add(k); return n })

  if (view === 'nodes') return <Nodes session={session} onClearSession={clearSession} onView={setView} />

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-[1fr_1.4fr]">
        <ChartPanel id="kinds" title={<>Kinds · {new Set(rows.map((r) => r.kind)).size}</>} icon={<Layers size={13} />} height={212}
          empty={rows.length ? undefined : 'no rows'} table={kindsTable(rows)}>
          <KindMap rows={rows} selected={kinds} family={family} onToggle={toggle} onFamily={(f) => setFamily(family === f ? null : f)} />
        </ChartPanel>
        <ChartPanel id="overtime" title="Over time · drag the brush to filter" icon={<ScrollText size={13} />} height={212}
          actions={range ? <button onClick={() => setRange(null)} className="flex items-center gap-1 text-[11px] text-live"><X size={11} /> clear range</button> : null}
          legend={bins.counts.fault.some((n) => n > 0) ? OVER_TIME_LEGEND : undefined}
          empty={histRows.length ? undefined : 'no rows'} table={overTimeTable(bins)}>
          <Histogram bins={bins} onRange={setRange} />
        </ChartPanel>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <div className="flex items-center gap-1.5 rounded-md bg-white/5 px-2.5 py-1.5 ring-1 ring-line focus-within:ring-live/40">
          <Search size={13} className="text-ink-faint" />
          <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="search kinds, summaries, payloads, session ids…" className="w-96 bg-transparent text-[12.5px] text-ink outline-none placeholder:text-ink-faint" />
        </div>
        {session && <button onClick={clearSession} title="show every session's rows"><Pill tone="live">session {short(session)} <X size={10} /></Pill></button>}
        {[...kinds].map((k) => <button key={k} onClick={() => toggle(k)}><Pill tone={ledgerKind(k).tone}>{k} <X size={10} /></Pill></button>)}
        <span className="num ml-auto text-[11px] text-ink-faint">
          {filtered.length.toLocaleString()} of {rows.length.toLocaleString()} rows read · {history.total ? history.total.toLocaleString() : '—'} in the ledger
          {history.partial && ' · this daemon cannot page: only its newest rows'}{!history.ready && ' · reading…'}
        </span>
        <button type="button" onClick={() => setFollow(!follow)} aria-pressed={follow}
          title="Keep the newest rows in view as they land; scrolling away stops it until you scroll back to the top"
          className={cn('num rounded-md px-2 py-1 text-[11px] ring-1 ring-inset', follow ? 'bg-live/10 text-live ring-live/40' : 'text-ink-dim ring-line hover:text-ink')}>follow</button>
        <button type="button" onClick={() => download(exportOf(rows, filter), exportName(filtered.length, Date.now()))} disabled={!filtered.length}
          title={`Download the ${filtered.length.toLocaleString()} rows shown as JSON`}
          className="num flex items-center gap-1 rounded-md px-2 py-1 text-[11px] text-ink-dim ring-1 ring-inset ring-line hover:text-ink disabled:opacity-40"><Download size={12} /> export {filtered.length.toLocaleString()}</button>
        {asOf !== null && <Pill tone="wait" title="the ledger as it is now: the time machine's moment does not move it">shows the present</Pill>}
      </div>
      <SavedFilters query={queryOf(params)} onApply={(sv) => setParams((p) => applySaved(p, sv), { replace: true })} />
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
            <RowList rows={filtered} pick={pick} onPick={setPick} follow={follow} onFollow={setFollow} />
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

function RowList({ rows, pick, onPick, follow, onFollow }: { rows: LedgerEntry[]; pick: LedgerEntry | null; onPick: (r: LedgerEntry) => void; follow: boolean; onFollow: (on: boolean) => void }) {
  const parent = useRef<HTMLDivElement>(null)
  const v = useVirtualizer({ count: rows.length, getScrollElement: () => parent.current, estimateSize: () => 26, overscan: 20 })
  // Follow: the list is newest first, so following is staying at the top. A scroll away pauses it (the rows that land
  // meanwhile are counted); scrolling back to the top goes on. The paused position is the newest row seen.
  const atTop = useRef(true)
  const [seen, setSeen] = useState<number | null>(null)
  const newest = rows.length ? rows[0].position : 0
  const scrolled = (e: React.UIEvent<HTMLDivElement>) => {
    const top = e.currentTarget.scrollTop < 8
    if (top === atTop.current) return
    atTop.current = top
    setSeen(top ? null : newest)
  }
  useEffect(() => {
    if (follow && atTop.current && rows.length) v.scrollToOffset(0)
  }, [follow, newest, rows.length, v])
  const behind = seen !== null ? newerThan(rows, seen) : 0
  if (!rows.length) return <Empty>no rows match</Empty>
  return (
    <div className="relative h-full">
      {follow && seen !== null && behind > 0 && (
        <button type="button" onClick={() => { v.scrollToOffset(0); atTop.current = true; setSeen(null) }}
          className="num absolute left-1/2 top-1 z-10 -translate-x-1/2 rounded-full bg-live/15 px-3 py-0.5 text-[11px] text-live ring-1 ring-live/40">{behind.toLocaleString()} newer · follow paused · jump to the newest</button>
      )}
      <div ref={parent} onScroll={scrolled} className="h-full overflow-auto">
        <div style={{ height: v.getTotalSize(), position: 'relative' }}>
          {v.getVirtualItems().map((it) => {
            const r = rows[it.index]
            return (
              <button key={r.position} onClick={() => { onPick(r); if (follow) onFollow(true) }}
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
    </div>
  )
}

/** The filters kept by name in this browser (`localStorage`, which may be absent or full: then they last as long as the
 *  page does). A saved filter is the address's filter keys as a query; a click puts it back in the address. */
function SavedFilters({ query, onApply }: { query: string; onApply: (s: Saved) => void }) {
  const [list, setList] = useState<Saved[]>(() => { try { return readSaved(localStorage.getItem(SAVED_KEY)) } catch { return [] } })
  const [name, setName] = useState('')
  const keep = (next: Saved[]) => { setList(next); try { localStorage.setItem(SAVED_KEY, writeSaved(next)) } catch { /* the page's copy stands */ } }
  return (
    <div className="-mt-1 flex flex-wrap items-center gap-1" data-saved>
      <span className="ship-engraved mr-1 text-[9.5px]">saved filters</span>
      {!list.length && <span className="text-[11px] text-ink-faint">none yet</span>}
      {list.map((sv) => (
        <span key={sv.name} className={cn('num inline-flex items-center rounded-md ring-1 ring-inset', sv.query === query ? 'text-live ring-live/40' : 'text-ink-dim ring-line')}>
          <button type="button" onClick={() => onApply(sv)} title={sv.query || 'no filter'} className="px-1.5 py-0.5 text-[11px] hover:text-ink">{sv.name}</button>
          <button type="button" onClick={() => { if (window.confirm(`Forget the saved filter “${sv.name}”?`)) keep(withoutSaved(list, sv.name)) }} title="forget it" className="pr-1 text-ink-faint hover:text-fault"><X size={10} /></button>
        </span>
      ))}
      <form className="ml-2 flex items-center gap-1" onSubmit={(e) => { e.preventDefault(); if (name.trim()) { keep(withSaved(list, name, query)); setName('') } }}>
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="name this filter" aria-label="name for this filter"
          className="num w-32 rounded-md bg-white/5 px-1.5 py-0.5 text-[11px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40" />
        <button type="submit" disabled={!name.trim()} title="save the filter in the address under this name" className="text-ink-faint hover:text-live disabled:opacity-40"><Star size={12} /></button>
      </form>
    </div>
  )
}

/** A JSON file made in the browser: nothing is sent anywhere. */
function download(text: string, name: string) {
  const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }))
  const a = document.createElement('a')
  a.href = url
  a.download = name
  document.body.appendChild(a)
  a.click()
  a.remove()
  setTimeout(() => URL.revokeObjectURL(url), 1000)
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

interface KindTile { kind: string; n: number; tone: Tone }
interface FamilyTiles { name: string; n: number; kinds: KindTile[] }

/** The rows read, by family and kind, the most first. */
function familiesOf(rows: LedgerEntry[]): FamilyTiles[] {
  const fam = new Map<string, Map<string, number>>()
  for (const r of rows) {
    const f = ledgerKind(r.kind).family
    const m = fam.get(f) ?? new Map<string, number>()
    m.set(r.kind, (m.get(r.kind) ?? 0) + 1)
    fam.set(f, m)
  }
  return [...fam].map(([name, kinds]) => {
    const ks = [...kinds].map(([kind, n]) => ({ kind, n, tone: ledgerKind(kind).tone })).sort((a, b) => b.n - a.n || a.kind.localeCompare(b.kind))
    return { name, n: ks.reduce((a, k) => a + k.n, 0), kinds: ks }
  }).sort((a, b) => b.n - a.n || a.name.localeCompare(b.name))
}

const KINDS_H = 212
const MONO_11 = `11px ${FONTS.mono}`

/** The kinds as a treemap drawn from the squarified layout: each family's tiles under its header, each kind's tile its
 *  share of the rows read, in its tone's step for marks. A name sits in a tile only where it fits, measured (the old map
 *  cut "started" to "star"); every tile says its kind and rows on hover and on keyboard focus. A tile picks its kind; a
 *  family's header picks the family. */
function KindMap({ rows, selected, family, onToggle, onFamily }: {
  rows: LedgerEntry[]; selected: Set<string>; family: string | null; onToggle: (k: string) => void; onFamily: (f: string) => void
}) {
  const [ref, width] = useWidth<HTMLDivElement>()
  const fams = useMemo(() => familiesOf(rows), [rows])
  const total = rows.length
  const layout = useMemo(() => {
    if (!width) return []
    const famRects = squarify(fams.map((f) => f.n), { x: 0, y: 0, w: width, h: KINDS_H })
    return fams.map((f, i) => {
      const r = famRects[i]
      // A 3 px gap between families, of the surface; a header where the family's name fits.
      const box = { x: r.x + 1.5, y: r.y + 1.5, w: Math.max(0, r.w - 3), h: Math.max(0, r.h - 3) }
      const head = box.h >= 36 && box.w >= textWidth(`${f.name} ${f.n}`, MONO_11) + 14 ? 16 : 0
      const tiles = squarify(f.kinds.map((k) => k.n), { x: box.x, y: box.y + head, w: box.w, h: box.h - head })
      return { f, box, head, tiles: f.kinds.map((k, j) => ({ k, r: tiles[j] })) }
    })
  }, [fams, width])
  const dim = (k: KindTile, f: string) => (selected.size > 0 && !selected.has(k.kind)) || (!!family && f !== family)
  if (!rows.length) return <Empty>no rows</Empty>
  return (
    <TipArea className="h-full w-full">
      <div ref={ref} className="relative w-full" style={{ height: KINDS_H }} role="group" aria-label="the ledger's kinds, by family">
        {layout.map(({ f, box, head, tiles }) => (
          <div key={f.name}>
            {head > 0 && (
              <button type="button" onClick={() => onFamily(f.name)} title={`pick the family: ${f.name}.* (${f.n.toLocaleString()} rows)`}
                className={cn('viz-mark num absolute truncate px-1 text-left text-[10.5px] font-semibold', family === f.name ? 'text-ink' : 'text-ink-dim hover:text-ink')}
                style={{ left: box.x, top: box.y, width: box.w, height: head, lineHeight: `${head}px` }}>
                {f.name} <span className="font-normal text-ink-faint">{f.n.toLocaleString()}</span>
              </button>
            )}
            {tiles.map(({ k, r }) => {
              if (r.w < 1 || r.h < 1) return null
              // A 2 px gap between tiles, of the surface.
              const w = Math.max(0, r.w - 2), h = Math.max(0, r.h - 2)
              const fill = TONE_MARK[k.tone]
              const name = k.kind.split('.').slice(1).join('.') || k.kind
              const nameW = textWidth(name, MONO_11), countW = textWidth(k.n.toLocaleString(), MONO_11)
              const words = w >= Math.max(nameW, countW) + 8 && h >= 30 ? 2 : w >= nameW + 8 && h >= 16 ? 1 : 0
              const on = selected.has(k.kind)
              return (
                <TipTarget key={k.kind} onClick={() => onToggle(k.kind)} label={`${k.kind}: ${k.n} rows`}
                  className="viz-mark num absolute flex flex-col items-start justify-center overflow-hidden rounded-[2px] px-1 text-left text-[11px] leading-[13px]"
                  style={{ left: r.x + 1, top: r.y + 1, width: w, height: h, background: fill, color: inkOn(fill), opacity: dim(k, f.name) ? 0.22 : 1, boxShadow: on ? `0 0 0 1px ${C.text}` : undefined }}
                  tip={<TipBody head={`${f.name}.*`} rows={[{ value: k.n.toLocaleString(), label: k.kind, color: fill, mark: 'rect' }]}
                    foot={`${pct(total ? k.n / total : 0, 1)} of the rows read · a click ${on ? 'drops' : 'picks'} it`} />}>
                  {words > 0 && <span>{name}</span>}
                  {words > 1 && <span className="opacity-80">{k.n.toLocaleString()}</span>}
                </TipTarget>
              )
            })}
          </div>
        ))}
      </div>
    </TipArea>
  )
}

function kindsTable(rows: LedgerEntry[]): TableSpec<KindTile & { family: string }> {
  type R = KindTile & { family: string }
  const out: R[] = familiesOf(rows).flatMap((f) => f.kinds.map((k) => ({ ...k, family: f.name })))
  return {
    caption: 'every kind in the rows read, by family, the most first', rows: out, rowKey: (r: R) => r.kind,
    columns: [
      { key: 'family', label: 'family', cell: (r: R) => r.family },
      { key: 'kind', label: 'kind', cell: (r: R) => r.kind, className: 'num' },
      { key: 'rows', label: 'rows', num: true, cell: (r: R) => r.n.toLocaleString() },
      { key: 'share', label: 'share', num: true, cell: (r: R) => pct(rows.length ? r.n / rows.length : 0, 1) },
    ],
  }
}

/** The tones a row's kind takes, as the over-time chart's tip and table name them. */
const TONES: { tone: Tone; word: string }[] = [
  { tone: 'live', word: 'turns & executions' }, { tone: 'model', word: 'model' }, { tone: 'tool', word: 'tools' },
  { tone: 'think', word: 'context' }, { tone: 'wait', word: 'policy & sessions' }, { tone: 'ok', word: 'discord' },
  { tone: 'money', word: 'budget' }, { tone: 'fault', word: 'faults' }, { tone: 'idle', word: 'other' },
]
const BINS = 90

/** The rows over time, a bin each 90th of the record: the rows in the accent and the faults among them on top in the fault
 *  tone's step (the one series the eye should find); every family's count in the tip and the table. The brush under it
 *  filters the rows by time. */
function overTime(rows: LedgerEntry[]): Bins {
  // A loop, not a spread: the whole ledger is more rows than a call's arguments can hold.
  let t0 = Infinity, last = -Infinity
  for (const r of rows) { if (r.at_unix_ms < t0) t0 = r.at_unix_ms; if (r.at_unix_ms > last) last = r.at_unix_ms }
  if (!rows.length) { t0 = 0; last = 0 }
  return binByKey(rows, (r) => r.at_unix_ms, (r) => ledgerKind(r.kind).tone, TONES.map((t) => t.tone), t0, Math.max(last, t0 + 60_000), BINS)
}

function Histogram({ bins, onRange }: { bins: Bins; onRange: (r: readonly [number, number] | null) => void }) {
  const option = useMemo<EChartsOption>(() => {
    const mids = bins.starts.map((s, i) => (s + bins.ends[i]) / 2)
    const faults = bins.counts.fault
    const rest = bins.totals.map((t, i) => t - faults[i])
    const sc = niceScale(Math.max(1, ...bins.totals), 4, 1)
    const vaxis = valueAxis(), axis = baseAxis()
    const t1 = bins.ends[bins.ends.length - 1]
    return {
      grid: { left: 12, right: 16, top: 10, bottom: 40, containLabel: true },
      tooltip: {
        ...TIP_FRAME, trigger: 'axis', axisPointer: { type: 'shadow', shadowStyle: { color: 'rgba(176,141,87,0.08)' } },
        formatter: (ps: any) => {
          const i = (Array.isArray(ps) ? ps[0] : ps)?.dataIndex
          if (i === undefined || bins.starts[i] === undefined) return ''
          const rows: TipRow[] = TONES.filter((t) => bins.counts[t.tone][i] > 0)
            .map((t) => ({ value: countTick(bins.counts[t.tone][i]), label: t.word, color: t.tone === 'fault' ? TONE_MARK.fault : undefined, mark: 'rect', strong: t.tone === 'fault' }))
          rows.push({ value: countTick(bins.totals[i]), label: bins.totals[i] === 1 ? 'row in all' : 'rows in all', color: ACCENT, mark: 'rect' })
          return tip(`${stamp(bins.starts[i])} – ${clock(bins.ends[i])}`, rows)
        },
      },
      xAxis: { type: 'time', min: bins.starts[0], max: t1, ...axis, axisLabel: { ...axis.axisLabel, formatter: TIME_LABELS } },
      yAxis: { ...vaxis, min: 0, max: sc.max, interval: sc.interval, axisLabel: { ...vaxis.axisLabel, formatter: countTick } },
      dataZoom: [{ type: 'slider', height: 16, bottom: 4, borderColor: 'transparent', backgroundColor: 'rgba(176,141,87,0.05)', fillerColor: 'rgba(34,211,238,0.14)', showDetail: false, realtime: false, labelFormatter: '' }],
      series: [
        { name: 'rows', type: 'bar', stack: 'rows', barWidth: '80%', barMaxWidth: MARK.bar, data: mids.map((m, i) => [m, rest[i]]),
          itemStyle: { color: ACCENT, borderRadius: 0 } },
        { name: 'faults', type: 'bar', stack: 'rows', barWidth: '80%', barMaxWidth: MARK.bar, data: mids.map((m, i) => [m, faults[i]]),
          itemStyle: { color: TONE_MARK.fault, borderColor: C.surface, borderWidth: 0.5 } },
      ],
    }
  }, [bins])
  const t0 = bins.starts[0], t1 = bins.ends[bins.ends.length - 1]
  return (
    <Echart option={option} onDataZoom={(z) => {
      if (z.start <= 0.5 && z.end >= 99.5) return onRange(null)
      onRange([t0 + (z.start / 100) * (t1 - t0), t0 + (z.end / 100) * (t1 - t0)])
    }} />
  )
}

function overTimeTable(b: Bins): TableSpec<number> {
  const present = TONES.filter((t) => b.counts[t.tone].some((n) => n > 0))
  return {
    caption: 'the rows read over time, a bin each 90th of the record, by family, the newest first', rows: b.starts.map((_, i) => i).filter((i) => b.totals[i] > 0).reverse(),
    rowKey: (i: number) => String(b.starts[i]),
    columns: [
      { key: 'when', label: 'from', cell: (i: number) => stamp(b.starts[i]) },
      { key: 'to', label: 'to', cell: (i: number) => clock(b.ends[i]) },
      ...present.map((t) => ({ key: t.tone, label: t.word, num: true, cell: (i: number) => countTick(b.counts[t.tone][i]) })),
      { key: 'all', label: 'in all', num: true, cell: (i: number) => countTick(b.totals[i]) },
    ],
  }
}

const C = CHROME.dark
const ACCENT = CATEGORICAL.dark[0]
const OVER_TIME_LEGEND: LegendItem[] = [
  { key: 'rows', label: 'rows', color: ACCENT, mark: 'rect' },
  { key: 'faults', label: 'faults among them', color: TONE_MARK.fault, mark: 'rect' },
]
