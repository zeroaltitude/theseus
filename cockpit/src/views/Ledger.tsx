// The ledger explorer: the daemon's append-only record, all of it. A treemap of kinds filters by click, the
// histogram's brush filters by time, search covers kinds, summaries, and payloads, and the list is virtualized.
import { useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import { Group, Panel as RPanel, Separator } from 'react-resizable-panels'
import { Layers, ScrollText, Search, X } from 'lucide-react'
import type { LedgerEntry } from '@protocol'
import { useLedger } from '@/lib/derive'
import { summarize } from '@/lib/summary'
import { cn, short, stamp } from '@/lib/format'
import { ledgerKind, toneHex } from '@/lib/taxonomy'
import { axisStyle, type EChartsOption } from '@/lib/chart'
import { Echart } from '@/components/Echart'
import { JsonView } from '@/components/JsonView'
import { Empty, Panel, Pill } from '@/components/ui'

export default function Ledger() {
  const nav = useNavigate()
  const { data } = useLedger(20_000, 5000)
  const rows = data?.rows ?? []
  const [kinds, setKinds] = useState<Set<string>>(new Set())
  const [q, setQ] = useState('')
  const [range, setRange] = useState<[number, number] | null>(null)
  const [pick, setPick] = useState<LedgerEntry | null>(null)

  const filtered = useMemo(() => {
    const needle = q.toLowerCase()
    return rows.filter((r) =>
      (!kinds.size || kinds.has(r.kind)) &&
      (!range || (r.at_unix_ms >= range[0] && r.at_unix_ms <= range[1])) &&
      (!needle || r.kind.includes(needle) || (r.session_id ?? '').includes(needle) || summarize(r).toLowerCase().includes(needle) || JSON.stringify(r.data).toLowerCase().includes(needle)),
    ).reverse()
  }, [rows, kinds, q, range])

  const toggle = (k: string) => setKinds((s) => { const n = new Set(s); if (n.has(k)) n.delete(k); else n.add(k); return n })

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-[1fr_1.4fr]">
        <Panel title={<>Kinds · {new Set(rows.map((r) => r.kind)).size}</>} icon={<Layers size={13} />} bodyClassName="h-[220px] p-1">
          <KindMap rows={rows} selected={kinds} onToggle={toggle} />
        </Panel>
        <Panel title="Over time · drag the brush to filter" icon={<ScrollText size={13} />} bodyClassName="h-[220px] p-2"
          actions={range ? <button onClick={() => setRange(null)} className="flex items-center gap-1 text-[11px] text-live"><X size={11} /> clear range</button> : null}>
          <Histogram rows={kinds.size ? rows.filter((r) => kinds.has(r.kind)) : rows} onRange={setRange} />
        </Panel>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <div className="flex items-center gap-1.5 rounded-md bg-white/5 px-2.5 py-1.5 ring-1 ring-line focus-within:ring-live/40">
          <Search size={13} className="text-ink-faint" />
          <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="search kinds, summaries, payloads, session ids…" className="w-96 bg-transparent text-[12.5px] text-ink outline-none placeholder:text-ink-faint" />
        </div>
        {[...kinds].map((k) => <button key={k} onClick={() => toggle(k)}><Pill tone={ledgerKind(k).tone}>{k} <X size={10} /></Pill></button>)}
        <span className="num ml-auto text-[11px] text-ink-faint">{filtered.length.toLocaleString()} of {rows.length.toLocaleString()} rows read · {data?.total?.toLocaleString() ?? '—'} in the ledger</span>
      </div>

      <Group orientation="horizontal" className="min-h-[360px] flex-1">
        <RPanel defaultSize="60" minSize={420} className="min-h-0">
          <Panel title="Rows" icon={<ScrollText size={13} />} className="h-full" bodyClassName="min-h-0">
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

function KindMap({ rows, selected, onToggle }: { rows: LedgerEntry[]; selected: Set<string>; onToggle: (k: string) => void }) {
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
        itemStyle: { borderColor: '#070a10', borderWidth: 2, gapWidth: 2 },
        label: { color: '#e2e8f0', fontSize: 11, fontFamily: 'JetBrains Mono Variable', formatter: (p: any) => `${String(p.name).split('.').slice(1).join('.') || p.name}\n${p.value}` },
        levels: [
          { itemStyle: { borderWidth: 0, gapWidth: 3 } },
          { itemStyle: { gapWidth: 1, borderColor: '#0b1018', borderWidth: 2 }, upperLabel: { show: true, height: 15, color: '#94a3b8', fontSize: 10, fontWeight: 600 } },
          { itemStyle: { gapWidth: 1 } },
        ],
        data: [...fam.entries()].map(([f, kinds]) => ({
          name: f,
          children: [...kinds.entries()].map(([k, n]) => ({
            name: k, value: n,
            itemStyle: { color: toneHex[ledgerKind(k).tone], opacity: selected.size && !selected.has(k) ? 0.18 : 0.75 },
          })),
        })),
      }],
    }
  }, [rows, selected])
  if (!rows.length) return <Empty>no rows</Empty>
  return <Echart option={option} onClick={(p: any) => { if (p?.data?.value !== undefined && !p.data.children) onToggle(p.name) }} />
}

function Histogram({ rows, onRange }: { rows: LedgerEntry[]; onRange: (r: [number, number] | null) => void }) {
  const buckets = 90
  const { option, edges } = useMemo(() => {
    const t0 = Math.min(...rows.map((r) => r.at_unix_ms), Date.now())
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
      dataZoom: [{ type: 'slider', height: 16, bottom: 4, borderColor: 'transparent', backgroundColor: 'rgba(148,163,184,0.05)', fillerColor: 'rgba(34,211,238,0.14)', showDetail: false, realtime: false }],
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
