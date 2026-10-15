// The week strip (theseus-p17fd): seven columns by the hour, today marked, a mark for every kind of future and past
// thing (lib/agenda.ts says which and where). Hover names the item; a click selects it (`?sel=`) and its detail shows
// below with its verbs, each one protocol call. Drawn from the layout alone: no read of its own.
import { memo, useMemo } from 'react'
import { ChevronLeft, ChevronRight } from 'lucide-react'
import { cn } from '@/lib/format'
import { toneHex } from '@/lib/taxonomy'
import { glyph, layout, toneOf, type Bar, type Mark, type Placed, type Window } from '@/lib/agenda'
import { markLine } from '@/lib/agendawords'
import { clock } from '@/lib/format'

const HOUR_PX = 20
const HOURS = [0, 3, 6, 9, 12, 15, 18, 21]

const Tick = memo(function Tick({ p, selected, onPick, onHover }: { p: Placed; selected: boolean; onPick: (m: Mark) => void; onHover: (m: Mark | null) => void }) {
  const m = p.mark
  const color = toneHex[toneOf(m)]
  return (
    <button type="button" data-mark={m.kind} data-outcome={m.outcome} title={markLine(m)} onClick={() => onPick(m)}
      onMouseEnter={() => onHover(m)} onMouseLeave={() => onHover(null)}
      className={cn('absolute -translate-y-1/2 text-[11px] leading-none', selected && 'rounded-sm ring-1 ring-gold')}
      style={{ top: `${p.y * 100}%`, left: `${(p.lane / p.lanes) * 100}%`, width: `${100 / p.lanes}%`, color, opacity: m.kind === 'past' && m.outcome === 'skipped' ? 0.6 : 1 }}>
      {glyph(m)}
    </button>
  )
})

const BarMark = memo(function BarMark({ b, selected, onPick }: { b: Bar; selected: boolean; onPick: (m: Mark) => void }) {
  return (
    <button type="button" data-mark="watch" title={markLine(b.mark)} onClick={() => onPick(b.mark)}
      className={cn('absolute left-0 w-1.5 rounded-sm', selected && 'ring-1 ring-gold')}
      style={{ top: `${b.from * 100}%`, height: `max(3px, ${(b.to - b.from) * 100}%)`, background: `${toneHex.think}88` }} />
  )
})

export function WeekStrip({ marks, window: w, now, selected, onPick, onHover, onWeek, weeks, hidden }: {
  marks: readonly Mark[]; window: Window; now: number; selected: string | null
  onPick: (m: Mark) => void; onHover: (m: Mark | null) => void; onWeek: (delta: number) => void; weeks: number; hidden?: boolean
}) {
  const lay = useMemo(() => layout(marks, w), [marks, w])
  const byDay = useMemo(() => {
    const d: { ticks: Placed[]; bars: Bar[] }[] = w.days.map(() => ({ ticks: [], bars: [] }))
    for (const p of lay.placed) d[p.day].ticks.push(p)
    for (const b of lay.bars) d[b.day].bars.push(b)
    return d
  }, [lay, w])
  return (
    <div data-testid="week-strip">
      <div className="mb-2 flex items-center gap-2 text-[11px] text-ink-faint">
        <button type="button" onClick={() => onWeek(-1)} aria-label="Earlier week" className="rounded p-0.5 hover:text-ink"><ChevronLeft size={14} /></button>
        <button type="button" onClick={() => onWeek(-weeks)} disabled={weeks === 0} className="rounded px-1.5 py-0.5 ring-1 ring-line hover:text-ink disabled:opacity-40">Today</button>
        <button type="button" onClick={() => onWeek(1)} aria-label="Later week" className="rounded p-0.5 hover:text-ink"><ChevronRight size={14} /></button>
        <span className="ml-2 flex flex-wrap gap-x-3 gap-y-0.5">
          <span style={{ color: toneHex.live }}>● reminder</span><span style={{ color: toneHex.think }}>○ check-in</span><span style={{ color: toneHex.money }}>│ run due</span>
          <span style={{ color: toneHex.ok }}>✓ ok</span><span style={{ color: toneHex.fault }}>✗ failed</span><span style={{ color: toneHex.idle }}>– skipped</span>
          <span style={{ color: toneHex.wait }}>▷ task starts · ⚑ milestone</span><span style={{ color: toneHex.think }}>▬ watch</span>
        </span>
        {hidden && <span className="ml-auto text-wait">more fell in the week than the strip holds</span>}
      </div>
      <div className="grid grid-cols-[28px_repeat(7,minmax(0,1fr))] gap-px overflow-x-auto rounded-lg bg-line/60 ring-1 ring-line">
        <div className="bg-hull/80" />
        {w.days.map((d) => (
          <div key={d.index} className={cn('bg-hull/80 px-1.5 py-1 text-center text-[11px]', d.today ? 'font-semibold text-gold' : 'text-ink-dim')}>{d.label}{d.hours !== 24 ? <span className="ml-1 text-ink-faint" title="a day with a clock change">{d.hours}h</span> : null}</div>
        ))}
        <div className="relative bg-hull/60" style={{ height: 24 * HOUR_PX }}>
          {HOURS.map((h) => <span key={h} className="num absolute right-1 -translate-y-1/2 text-[9.5px] text-ink-faint" style={{ top: h === 0 ? 6 : h * HOUR_PX }}>{String(h).padStart(2, '0')}</span>)}
        </div>
        {w.days.map((d) => {
          const col = byDay[d.index]
          const nowY = now >= d.start_ms && now < d.end_ms ? (new Date(now).getHours() * 60 + new Date(now).getMinutes()) / 1440 : null
          return (
            <div key={d.index} data-day={d.index} data-today={d.today || undefined} className={cn('relative', d.today ? 'bg-gold/[0.06]' : 'bg-hull/60')} style={{ height: 24 * HOUR_PX }}>
              {HOURS.slice(1).map((h) => <div key={h} className="absolute inset-x-0 border-t border-line/40" style={{ top: h * HOUR_PX }} />)}
              {nowY != null && <div className="absolute inset-x-0 z-10 border-t border-fault/70" style={{ top: `${nowY * 100}%` }} title={`now ${clock(now)}`} />}
              {col.bars.map((b) => <BarMark key={`${b.mark.key}:${b.day}`} b={b} selected={selected === b.mark.key} onPick={onPick} />)}
              <div className="absolute inset-y-0 left-2 right-1">
                {col.ticks.map((p) => <Tick key={p.mark.key} p={p} selected={selected === p.mark.key} onPick={onPick} onHover={onHover} />)}
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}
