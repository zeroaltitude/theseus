// The command bar (theseus-jid7t.3): the counters, each also a filter (click, or its key) with its last hour as a
// sparkline in its tone and the same numbers as a table (`?table=ops.counters`); the needs-you counter glows while
// anything waits and pulses once when it rises. Then the lens switch, the filter chips, the search with its "n of m",
// and New task.
import { useEffect, useRef, useState, type RefObject } from 'react'
import { Plus, Search, X } from 'lucide-react'
import { useHistory } from '@/lib/hooks'
import { useTableView } from '@/lib/chartview'
import { sparkPoints } from '@/lib/agenda'
import { cn, usd } from '@/lib/format'
import { toneHex, type Tone } from '@/lib/taxonomy'
import { Segmented } from '@/components/ui'
import { HelpButton } from '@/components/Help'
import { LENSES, type Bucket, type Filters, type FilterKey, type Lens } from './address'
import { useOpsAct } from './act'
import { ActBtn } from './ActBtn'

export interface CounterValues { needs: number; working: number; paused: number; still: number; done: number; spent: number }

const COUNTERS: ReadonlyArray<{ b: Bucket; label: string; key?: string; tone: Tone; glyph: string }> = [
  { b: 'needs', label: 'needs you', key: 'b', tone: 'wait', glyph: '●' },
  { b: 'working', label: 'working', key: 'w', tone: 'live', glyph: '◐' },
  { b: 'paused', label: 'paused·queued', key: 'r', tone: 'idle', glyph: '⏸' },
  { b: 'still', label: 'still today', tone: 'think', glyph: '◷' },
  { b: 'done', label: 'done today', key: 'd', tone: 'ok', glyph: '✓' },
  { b: 'spent', label: 'spent today', tone: 'money', glyph: '$' },
]

const LENS_KEYS: Record<Lens, string> = { graph: 'g g', board: 'g b', timeline: 'g t', threads: 'g h' }

/** The last hour, one sample a minute: a timer that reads nothing. */
const SAMPLES = 60
const EVERY_MS = 60_000

export function CommandBar({ counts, f, onFilter, lens, onLens, choices, q, onQ, n, of, onNew, searchRef }: {
  counts: CounterValues; f: Filters; onFilter: (k: FilterKey, v: string | null) => void; lens: Lens; onLens: (l: Lens) => void
  choices: Partial<Record<FilterKey, string[]>>; q: string; onQ: (q: string) => void; n: number; of: number; onNew: () => void
  searchRef: RefObject<HTMLInputElement | null>
}) {
  const { on } = useOpsAct()
  const [table, setTable] = useTableView('ops.counters')
  const hist = {
    needs: useHistory(counts.needs, SAMPLES, EVERY_MS), working: useHistory(counts.working, SAMPLES, EVERY_MS), paused: useHistory(counts.paused, SAMPLES, EVERY_MS),
    still: useHistory(counts.still, SAMPLES, EVERY_MS), done: useHistory(counts.done, SAMPLES, EVERY_MS), spent: useHistory(counts.spent, SAMPLES, EVERY_MS),
  }
  return (
    <div className="flex flex-col gap-2" data-testid="ops-bar">
      <div className="flex flex-wrap items-stretch gap-2">
        {COUNTERS.map((c) => (
          <Counter key={c.b} c={c} value={counts[c.b]} series={hist[c.b]} active={f.bucket === c.b} onClick={() => onFilter('bucket', f.bucket === c.b ? null : c.b)} />
        ))}
        <span className="ml-auto flex items-center gap-2">
          <Segmented value={table ? 'table' : 'chart'} options={['chart', 'table'] as const} onChange={(v) => setTable(v === 'table')} />
          <ActBtn verb="new_task" on={on} onClick={onNew} title="N: a new task, its home the conversation under the cursor"><Plus size={12} /> New task</ActBtn>
        </span>
      </div>
      {table && <CounterTable hist={hist} />}
      <div className="flex flex-wrap items-center gap-2">
        <div role="tablist" aria-label="Lens" className="flex items-center rounded-lg bg-black/30 p-0.5 ring-1 ring-line">
          {LENSES.map((l) => (
            <button key={l} role="tab" aria-selected={l === lens} onClick={() => onLens(l)} title={`${l} (${LENS_KEYS[l]})`}
              className={cn('rounded-md px-2.5 py-1 font-display text-[11px] font-bold uppercase tracking-[0.08em] transition-colors',
                l === lens ? 'bg-gold/15 text-gold shadow-[0_0_12px_-3px_rgb(214_165_72_/_0.6)]' : 'text-ink-faint hover:text-ink')}>
              {l}
            </button>
          ))}
        </div>
        {lens === 'board' && <HelpButton id="ops.board" title="Board" />}
        {(['thread', 'model', 'place', 'kind', 'state', 'project', 'ended'] as const).map((k) => (
          <Chip key={k} k={k} value={f[k]} choices={choices[k] ?? []} onPick={(v) => onFilter(k, v)} />
        ))}
        <label className="ml-auto flex min-w-56 items-center gap-2 rounded-lg px-2.5 py-1 text-[12px] ring-1 ring-line focus-within:ring-live/60">
          <Search size={12} className="text-ink-faint" />
          <input ref={searchRef} value={q} onChange={(e) => onQ(e.target.value)} placeholder="Search titles, briefs, reports… ( / )" aria-label="Search"
            onKeyDown={(e) => { if (e.key === 'Escape') { onQ(''); e.currentTarget.blur() } }}
            className="min-w-0 flex-1 bg-transparent text-ink outline-none placeholder:text-ink-faint" />
          {(q || Object.keys(f).length > 0) && <span className="num text-[11px] text-ink-faint">{n} of {of}</span>}
        </label>
      </div>
    </div>
  )
}

function Counter({ c, value, series, active, onClick }: { c: (typeof COUNTERS)[number]; value: number; series: number[]; active: boolean; onClick: () => void }) {
  // The needs-you counter pulses once when it rises; Calm stills it (index.css).
  const was = useRef(value)
  const [pulse, setPulse] = useState(0)
  useEffect(() => { if (c.b === 'needs' && value > was.current) setPulse((p) => p + 1); was.current = value }, [value, c.b])
  const glow = c.b === 'needs' && value > 0
  const pts = sparkPoints(series.length > 1 ? series : [value, value], 64, 16)
  return (
    <button key={pulse} onClick={onClick} aria-pressed={active} title={`${c.label}${c.key ? ` (${c.key})` : ''}: a click shows only these`}
      className={cn('flex min-w-[118px] flex-col gap-0.5 rounded-lg bg-hull/70 px-2.5 py-1.5 text-left ring-1 transition-shadow',
        active ? 'ring-gold shadow-[0_0_14px_-3px_rgb(214_165_72_/_0.6)]' : 'ring-line hover:ring-line-strong',
        glow && !active && 'ring-wait/60 shadow-[0_0_16px_-4px_var(--color-wait)]', pulse > 0 && 'ops-pulse-once')}>
      <span className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-[0.06em] text-ink-faint">
        <span style={{ color: toneHex[c.tone] }} aria-hidden>{c.glyph}</span>{c.label}{c.key && <span className="kbd ml-auto !text-[9.5px]">{c.key}</span>}
      </span>
      <span className="flex items-end gap-2">
        <span className="num text-[18px] font-semibold leading-none" style={{ color: c.b === 'needs' && value === 0 ? undefined : toneHex[c.tone] }}>{c.b === 'spent' ? usd(value) : value}</span>
        <svg width={64} height={16} className="ml-auto" aria-hidden><polyline points={pts} fill="none" stroke={toneHex[c.tone]} strokeWidth={1.25} strokeLinejoin="round" /></svg>
      </span>
    </button>
  )
}

/** The sparklines' numbers: each counter every ten minutes over the last hour, as sampled. */
function CounterTable({ hist }: { hist: Record<Bucket, number[]> }) {
  const at = [-60, -50, -40, -30, -20, -10, 0]
  const pick = (s: number[], m: number) => s.length ? s[Math.max(0, s.length - 1 + m)] : undefined
  return (
    <table className="num w-full text-[11px]">
      <thead className="text-[10px] uppercase tracking-wider text-ink-faint"><tr><th className="px-2 text-left">counter</th>{at.map((m) => <th key={m} className="px-2 text-right">{m ? `${m} min` : 'now'}</th>)}</tr></thead>
      <tbody>
        {COUNTERS.map((c) => (
          <tr key={c.b} className="border-t border-line/50">
            <td className="px-2 text-ink-dim">{c.glyph} {c.label}</td>
            {at.map((m) => { const v = pick(hist[c.b], m); return <td key={m} className="px-2 text-right text-ink">{v === undefined || hist[c.b].length - 1 + m < 0 ? '—' : c.b === 'spent' ? usd(v) : v}</td> })}
          </tr>
        ))}
      </tbody>
    </table>
  )
}

const CHIP_WORDS: Record<string, string> = { thread: 'thread', model: 'model', place: 'place', kind: 'kind', state: 'state', project: 'project', ended: 'ended within' }
const ENDED = ['1h', '3h', '1d', '7d']

function Chip({ k, value, choices, onPick }: { k: FilterKey; value?: string; choices: string[]; onPick: (v: string | null) => void }) {
  const opts = k === 'ended' ? ENDED : choices
  if (!opts.length && !value) return null
  return (
    <span className={cn('flex items-center rounded-full text-[11px] ring-1', value ? 'bg-gold/10 text-gold ring-gold/50' : 'text-ink-faint ring-line')}>
      <select value={value ?? ''} onChange={(e) => onPick(e.target.value || null)} aria-label={`filter by ${CHIP_WORDS[k]}`}
        className="cursor-pointer appearance-none bg-transparent py-0.5 pl-2.5 pr-2 outline-none [&>option]:bg-hull [&>option]:text-ink">
        <option value="">{CHIP_WORDS[k]}</option>
        {opts.map((o) => <option key={o} value={o}>{CHIP_WORDS[k]}: {o}</option>)}
      </select>
      {value && <button onClick={() => onPick(null)} aria-label={`clear ${CHIP_WORDS[k]}`} className="pr-1.5 hover:text-ink"><X size={10} /></button>}
    </span>
  )
}
