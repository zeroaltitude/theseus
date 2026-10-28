// The week strip v2 (theseus-jid7t.5): seven local days (or one, in Day zoom) by the wall-clock hour, three layers a
// day. The calendar's events as background blocks in the calendar's own material (its `color`: brass, verdigris and
// the rest of the hull's, never a signal tone), a sensitive calendar's hatched with a lock, free/busy blocks hatched
// grey; the "ran" bars of each execution in a 3 px left gutter, coloured by outcome; and Theseus's marks in the right
// gutter, the Agenda's glyphs and tones. All-day events ride a band above the hours, the night folds to a band with
// its count, a ◆ marks a clash, and the thin cyan line is now. Drawn from the layout alone (`ops/timeline.ts`): no
// read of its own. A drag says on its cursor what a drop would do, or why it is refused (`ops/drag.ts`).
import { memo, useLayoutEffect, useMemo, useRef, useState, type DragEvent } from 'react'
import { ChevronLeft, ChevronRight, Lock, MoonStar, Repeat } from 'lucide-react'
import { cn } from '@/lib/format'
import { toneHex } from '@/lib/taxonomy'
import { glyph, layout, toneOf, type Mark, type Placed, type Window } from '@/lib/agenda'
import { markLine } from '@/lib/agendawords'
import { eventFacts, hatch, hm, stateOf } from './calendar'
import { centerHour, eventLayout, hourAt, heightOf, nightFold, openHours, ranBars, scrollFor, yOf, type AllDay, type Block, type CalEvent, type Clashes, type Ran } from './timeline'
import { taskLayout, taskLine, type TaskBlock, type TaskPlaced } from './taskblocks'
import { dropAt, dropOf, type Drop } from './drag'
import type { CalendarWindow, RunRow, WorkView } from '@protocol'

const FOLD_PX = 18
const GUTTER = 22
/** The share of a day's width the task lane takes when a day has task blocks (the events keep the rest). */
const TASK_SHARE = 34

export interface Hover { line: string }

const EventBlock = memo(function EventBlock({ b, y, h, selected, clash, why, dim, redact, onPick, onHover, onDrag }: {
  b: Block; y: number; h: number; selected: boolean; clash: boolean; why?: string; dim: boolean; redact: boolean
  onPick: (key: string) => void; onHover: (line: string | null) => void; onDrag: (e: DragEvent, what: { event: true }) => void
}) {
  const f = eventFacts(b.ev, b.cal, redact)
  const declined = b.ev.self_response === 'declined'
  const busy = b.ev.busy || b.cal.status === 'busy_only'
  return (
    <button type="button" data-event={b.cal.sensitive ? 'sensitive' : 'open'} data-busy={busy || undefined} draggable onDragStart={(e) => onDrag(e, { event: true })}
      title={`${f.tip}${clash && why ? `\n◆ ${why}` : ''}`} onClick={() => onPick(`ev:${b.key}`)} onMouseEnter={() => onHover(f.header)} onMouseLeave={() => onHover(null)}
      className={cn('absolute overflow-hidden rounded-[3px] px-1 text-left text-[10.5px] leading-tight text-ink', selected && 'z-10 ring-1 ring-gold', declined && 'opacity-50')}
      style={{
        top: y, height: Math.max(h - 1, 6), left: `calc(${(b.lane / b.lanes) * 100}% + 1px)`, width: `calc(${100 / b.lanes}% - 2px)`,
        background: hatch(b.cal.color, b.cal.sensitive, busy), boxShadow: `inset 2px 0 0 ${b.cal.color}${declined ? '66' : 'dd'}`,
        outline: declined ? `1px dashed ${b.cal.color}88` : undefined, opacity: dim ? 0.25 : undefined,
      }}>
      <span className="flex items-start gap-0.5">
        {clash && <span className="shrink-0 text-wait" aria-label="clash">◆</span>}
        {b.cal.sensitive && <Lock size={8} className="mt-[2px] shrink-0 text-ink-dim" aria-label="sensitive" />}
        <span className={cn('min-w-0', f.block === 'busy' ? 'italic text-ink-dim' : '', h > 26 ? 'line-clamp-2' : 'truncate')}>{f.block}</span>
      </span>
      {h > 30 && <span className="num block truncate text-[9.5px] text-ink-faint">{b.ev.all_day ? 'all day' : hm(b.ev.start_ms)}</span>}
    </button>
  )
})

/** A task beside the meetings, as a block of its own: a live one is solid in the live tone with a glowing, growing edge, a
 *  finished one holds its span in its outcome's tone, a planned one (a deferred start, a routine's occurrence) is a dashed
 *  outline in a faint fill, and a recurring one carries ↻. Dashed against the events' solid hatching, and in a signal
 *  tone where an event is in a calendar's own material, so the two are never confused. */
const TaskBlockView = memo(function TaskBlockView({ t, y, h, now, selected, clash, dim, onPick, onHover }: {
  t: TaskPlaced; y: number; h: number; now: number; selected: boolean; clash: boolean; dim: boolean
  onPick: (key: string) => void; onHover: (line: string | null) => void
}) {
  const b = t.block
  const hex = toneHex[b.tone]
  const line = taskLine(b, now)
  return (
    <button type="button" data-task-block={b.kind} data-live={b.live || undefined} data-recurring={b.recurring || undefined} data-planned={b.planned || undefined}
      title={`${line}${clash ? '\n◆ falls inside a meeting' : ''}`} onClick={() => onPick(b.key)} onMouseEnter={() => onHover(line)} onMouseLeave={() => onHover(null)}
      className={cn('absolute overflow-hidden rounded-[3px] px-1 text-left text-[10.5px] leading-tight text-ink', selected && 'z-10 ring-1 ring-gold')}
      style={{
        top: y, height: Math.max(h - 1, 6), left: `calc(${(t.lane / t.lanes) * 100}% + 1px)`, width: `calc(${100 / t.lanes}% - 2px)`,
        background: b.planned ? `${hex}14` : `${hex}${b.live ? '33' : '22'}`,
        outline: b.planned ? `1px dashed ${hex}aa` : undefined, boxShadow: `inset 2px 0 0 ${hex}${b.planned ? '88' : 'dd'}${b.live && !t.continues ? `, 0 2px 6px -1px ${hex}` : ''}`,
        borderBottom: b.live && !t.continues ? `2px solid ${hex}` : undefined, opacity: dim ? 0.25 : undefined,
      }}>
      <span className="flex items-start gap-0.5">
        {clash && <span className="shrink-0 text-wait" aria-label="clash">◆</span>}
        {b.recurring && <Repeat size={8} className="mt-[2px] shrink-0" style={{ color: hex }} aria-label="recurring" />}
        <span className={cn('min-w-0', b.planned && 'italic text-ink-dim', h > 26 ? 'line-clamp-2' : 'truncate')}>{b.title}</span>
      </span>
      {h > 30 && <span className="num block truncate text-[9.5px] text-ink-faint">{b.live ? (b.paused ? 'paused' : 'running') : b.kind === 'finished' ? b.state : b.planned ? hm(b.from_ms) : ''}{b.cost_usd != null ? ` · $${b.cost_usd.toFixed(2)}` : ''}</span>}
    </button>
  )
})

const Tick = memo(function Tick({ p, y, selected, clash, dim, onPick, onHover, onDrag }: {
  p: Placed; y: number; selected: boolean; clash: boolean; dim: boolean
  onPick: (key: string) => void; onHover: (line: string | null) => void; onDrag: (e: DragEvent, what: Mark) => void
}) {
  const m = p.mark
  return (
    <button type="button" data-mark={m.kind} data-outcome={m.outcome} title={`${markLine(m)}${clash ? '\n◆ falls inside a meeting' : ''}`} onClick={() => onPick(m.key)}
      draggable onDragStart={(e) => onDrag(e, m)} onMouseEnter={() => onHover(markLine(m).split('\n')[0])} onMouseLeave={() => onHover(null)}
      className={cn('absolute z-[5] -translate-y-1/2 text-center text-[11px] leading-none', selected && 'rounded-sm ring-1 ring-gold')}
      style={{ top: y, left: `${(p.lane / p.lanes) * 100}%`, width: `${Math.max(100 / p.lanes, 40)}%`, color: toneHex[toneOf(m)], opacity: dim ? 0.25 : m.kind === 'past' && m.outcome === 'skipped' ? 0.6 : 1 }}>
      {clash && <span className="absolute -left-2 top-1/2 -translate-y-1/2 text-[8px] text-wait">◆</span>}{glyph(m)}
    </button>
  )
})

export interface StripProps {
  window: Window; marks: readonly Mark[]; events: readonly CalEvent[]; cals: readonly CalendarWindow[]
  views: readonly WorkView[]; runs: Readonly<Record<string, readonly RunRow[]>>
  /** The task blocks (`ops/taskblocks.ts`): tasks going, gone and planned, and a routine's occurrences. */
  tasks?: readonly TaskBlock[]
  now: number; past: boolean; selected: string | null; clash: Clashes; redact: boolean
  /** A key that matches the search, or every key when there is none. */
  matches: (key: string) => boolean
  zoom: boolean; weeks: number
  /** The panel fills the window: the hours take its height instead of two thirds of the page's. */
  full?: boolean
  onPick: (key: string) => void; onHover: (line: string | null) => void; onWeek: (delta: number) => void; onToday: () => void
  onZoom: (day: number | null) => void; onDrop: (d: Drop & { method: string }) => void
}

export function WeekStripV2(p: StripProps) {
  const { window: w, marks, events, now } = p
  const hourPx = p.zoom ? 44 : 22
  const lay = useMemo(() => layout(marks, w), [marks, w])
  const ev = useMemo(() => eventLayout(events, w), [events, w])
  const ran = useMemo(() => ranBars(p.views, p.runs, w, now), [p.views, p.runs, w, now])
  const placedTasks = useMemo(() => taskLayout(p.tasks ?? [], w), [p.tasks, w])
  const nf = useMemo(() => nightFold(w, marks, ev.blocks), [w, marks, ev.blocks])
  // All 24 hours show by default; folding the night is the owner's choice.
  const [foldNight, setFoldNight] = useState(false)
  const fold = !foldNight || (nf.early === 0 && nf.late === 24) ? null : nf
  const H = heightOf(fold, hourPx, FOLD_PX)
  const byDay = useMemo(() => {
    const d = w.days.map(() => ({ ticks: [] as Placed[], blocks: [] as Block[], ran: [] as Ran[], allDay: [] as AllDay[], tasks: [] as TaskPlaced[] }))
    for (const x of lay.placed) d[x.day].ticks.push(x)
    for (const b of ev.blocks) d[b.day].blocks.push(b)
    for (const a of ev.allDay) d[a.day].allDay.push(a)
    for (const r of ran) d[r.day].ran.push(r)
    for (const t of placedTasks) d[t.day].tasks.push(t)
    return d
  }, [lay, ev, ran, placedTasks, w])
  const bandRows = Math.max(0, ...byDay.map((d) => d.allDay.length))
  const lanes = p.cals.filter((c) => stateOf(c).draws === 'lane')
  const [cursor, setCursor] = useState<{ x: number; y: number; text: string; ok: boolean } | null>(null)
  const dragged = useRef<Mark | { event: true } | null>(null)
  const yHour = (h: number) => yOf(h, fold, hourPx, FOLD_PX)
  // The first view of a week, a day, a zoom, or Today scrolls the hours so that now (when today is shown), else the
  // first meeting or one-off mark, else 08:00, is in the middle. Once the owner scrolls it stays where they put it.
  const box = useRef<HTMLDivElement>(null)
  const [again, setAgain] = useState(0)
  const seen = useRef('')
  const set = useRef(0)
  const moved = useRef(false)
  const nowDate = new Date(now)
  const centre = Math.floor(centerHour(w.days.some((d) => d.today) ? nowDate.getHours() + nowDate.getMinutes() / 60 : null, openHours(w, marks, ev.blocks)))
  const viewKey = `${w.from_ms}:${p.zoom}:${again}:${foldNight}`
  useLayoutEffect(() => {
    const el = box.current
    if (!el) return
    if (seen.current !== viewKey) { seen.current = viewKey; moved.current = false }
    if (moved.current) return
    el.scrollTop = scrollFor(yOf(centre, fold, hourPx, FOLD_PX), el.clientHeight, el.scrollHeight)
    set.current = el.scrollTop
  }, [viewKey, centre, fold, hourPx])
  const atY = (frac: number) => yHour(frac * 24)
  const onDrag = (e: DragEvent, what: Mark | { event: true }) => { dragged.current = what; e.dataTransfer.effectAllowed = 'move'; e.dataTransfer.setData('text/plain', 'theseus-mark') }
  const dropFor = (e: DragEvent, day: Window['days'][number]) => {
    const r = e.currentTarget.getBoundingClientRect()
    const at = dropAt(day.start_ms, hourAt(e.clientY - r.top, fold, hourPx, FOLD_PX))
    return dragged.current ? dropOf(dragged.current, at, p.past, now) : null
  }
  const hours = Array.from({ length: 25 }, (_, h) => h).filter((h) => !fold || (h >= fold.early && h <= fold.late))
  return (
    <div data-testid="week-strip" className="relative" onDragEnd={() => { dragged.current = null; setCursor(null) }}>
      <div className="mb-2 flex flex-wrap items-center gap-2 text-[11px] text-ink-faint">
        <button type="button" onClick={() => p.onWeek(-1)} aria-label={p.zoom ? 'Earlier day' : 'Earlier week'} title=", (comma)" className="rounded p-0.5 hover:text-ink"><ChevronLeft size={14} /></button>
        <button type="button" onClick={() => { moved.current = false; setAgain((n) => n + 1); p.onToday() }} disabled={p.weeks === 0 && !p.zoom} className="rounded px-1.5 py-0.5 ring-1 ring-line hover:text-ink disabled:opacity-40">Today</button>
        <button type="button" onClick={() => p.onWeek(1)} aria-label={p.zoom ? 'Later day' : 'Later week'} title=". (period)" className="rounded p-0.5 hover:text-ink"><ChevronRight size={14} /></button>
        <span role="tablist" aria-label="Zoom" className="ml-1 flex items-center rounded-md bg-black/30 p-0.5 ring-1 ring-line">
          {(['day', 'week'] as const).map((z) => (
            <button key={z} role="tab" aria-selected={(z === 'day') === p.zoom} onClick={() => (z === 'week' ? p.onZoom(null) : !p.zoom && p.onZoom(-1))} title={z === 'day' ? 'Day zoom (d)' : 'Week zoom (w)'}
              className={cn('rounded px-2 py-0.5 font-display text-[10px] font-bold uppercase tracking-[0.08em]', (z === 'day') === p.zoom ? 'bg-gold/15 text-gold' : 'hover:text-ink')}>{z}</button>
          ))}
        </span>
        {nf.early > 0 || nf.late < 24 ? (
          <button type="button" onClick={() => setFoldNight((v) => !v)} className="flex items-center gap-1 rounded px-1.5 py-0.5 ring-1 ring-line hover:text-ink" title="the night hours with no meeting or one-off mark fold to a band">
            <MoonStar size={11} /> {foldNight ? `night folded${nf.count ? ` · ${nf.count} mark${nf.count === 1 ? '' : 's'} inside` : ''}` : 'fold the night'}
          </button>
        ) : null}
      </div>
      <div ref={box} className="overflow-auto rounded-lg bg-line/60 ring-1 ring-line" data-testid="strip-hours" style={{ maxHeight: p.full ? 'calc(100vh - 9rem)' : '66vh' }}
        onScroll={(e) => { if (Math.abs(e.currentTarget.scrollTop - set.current) > 2) moved.current = true }}>
        <div className="grid gap-px" style={{ gridTemplateColumns: `30px repeat(${w.days.length}, minmax(0, 1fr))`, minWidth: p.zoom ? 0 : 640 }}>
          <div className="sticky top-0 z-30 h-6 bg-hull/80" />
          {w.days.map((d) => (
            <button type="button" key={d.start_ms} onClick={() => p.onZoom(p.zoom ? null : d.index)} title={p.zoom ? 'back to the week (w)' : 'this day (d)'}
              className={cn('sticky top-0 z-30 h-6 bg-hull/80 px-1.5 py-1 text-center text-[11px] hover:text-ink', d.today ? 'font-semibold text-gold' : 'text-ink-dim')}>
              {d.label}{d.hours !== 24 ? <span className="ml-1 text-ink-faint" title="a day with a clock change">{d.hours}h</span> : null}
            </button>
          ))}
          {bandRows > 0 && <>
            <div className="sticky top-6 z-30 bg-hull/70 pr-1 pt-0.5 text-right text-[9px] text-ink-faint">all day</div>
            {w.days.map((d) => (
              <div key={d.start_ms} className="sticky top-6 z-30 flex flex-col gap-px bg-hull/70 p-0.5" style={{ minHeight: bandRows * 17 + 4 }}>
                {byDay[d.index].allDay.map((a) => {
                  const f = eventFacts(a.ev, a.cal, p.redact)
                  return (
                    <button type="button" key={a.key} title={f.tip} onClick={() => p.onPick(`ev:${a.key}`)} onMouseEnter={() => p.onHover(f.header)} onMouseLeave={() => p.onHover(null)}
                      className={cn('flex h-4 items-center gap-0.5 truncate rounded-[3px] px-1 text-left text-[10px] text-ink', p.selected === `ev:${a.key}` && 'ring-1 ring-gold')}
                      style={{ background: hatch(a.cal.color, a.cal.sensitive, a.ev.busy), boxShadow: `inset 2px 0 0 ${a.cal.color}`, opacity: p.matches(`ev:${a.key}`) ? 1 : 0.25 }}>
                      {a.cal.sensitive && <Lock size={8} className="shrink-0" />}<span className="truncate">{f.block}</span>
                    </button>
                  )
                })}
              </div>
            ))}
          </>}
          <div className="relative bg-hull/60" style={{ height: H }}>
            {hours.filter((h) => h % (p.zoom ? 1 : 3) === 0 && h < 24).map((h) => <span key={h} className="num absolute right-1 -translate-y-1/2 text-[9.5px] text-ink-faint" style={{ top: Math.max(6, yHour(h)) }}>{String(h).padStart(2, '0')}</span>)}
          </div>
          {w.days.map((d) => {
            const col = byDay[d.index]
            const nowY = now >= d.start_ms && now < d.end_ms ? yHour(new Date(now).getHours() + new Date(now).getMinutes() / 60) : null
            return (
              <div key={d.start_ms} data-day={d.index} data-today={d.today || undefined} className={cn('relative', d.today ? 'bg-gold/[0.05]' : 'bg-hull/60')} style={{ height: H }}
                onDragOver={(e) => {
                  const x = dropFor(e, d)
                  if (!x) return
                  e.preventDefault()
                  e.dataTransfer.dropEffect = 'refused' in x ? 'none' : 'move'
                  const host = (e.currentTarget.closest('[data-testid="week-strip"]') as HTMLElement).getBoundingClientRect()
                  setCursor({ x: e.clientX - host.left + 14, y: e.clientY - host.top + 10, text: 'refused' in x ? x.refused : x.words, ok: !('refused' in x) })
                }}
                onDragLeave={() => setCursor(null)}
                onDrop={(e) => { e.preventDefault(); const x = dropFor(e, d); setCursor(null); dragged.current = null; if (x && !('refused' in x)) p.onDrop(x) }}>
                {hours.filter((h) => h > 0 && h < 24).map((h) => <div key={h} className={cn('absolute inset-x-0 border-t', h % 6 === 0 ? 'border-line/60' : 'border-line/25')} style={{ top: yHour(h) }} />)}
                {fold && fold.early > 0 && <div className="absolute inset-x-0 top-0 bg-black/25" style={{ height: FOLD_PX, backgroundImage: 'repeating-linear-gradient(90deg, transparent 0 5px, rgb(255 255 255 / 0.04) 5px 6px)' }} title={`00:00–${String(fold.early).padStart(2, '0')}:00 folded`} />}
                {fold && fold.late < 24 && <div className="absolute inset-x-0 bottom-0 bg-black/25" style={{ height: FOLD_PX, backgroundImage: 'repeating-linear-gradient(90deg, transparent 0 5px, rgb(255 255 255 / 0.04) 5px 6px)' }} title={`${fold.late}:00–24:00 folded`} />}
                {lanes.map((c, i) => <div key={c.id} className="absolute inset-y-1 w-1.5 rounded-sm border border-dashed" style={{ left: 5 + i * 8, borderColor: `${toneHex.wait}88` }} title={`${c.label}: ${stateOf(c).words}`} />)}
                {/* layer 2: the ran bars */}
                {col.ran.map((r) => <div key={r.key} className="absolute left-0 w-[3px] rounded-r-sm" title={r.line} style={{ top: atY(r.top), height: Math.max(2, atY(r.bottom) - atY(r.top)), background: toneHex[r.tone] }} />)}
                {/* layer 1: the events */}
                <div className="absolute inset-y-0 left-[4px]" style={{ right: col.tasks.length ? `calc(${GUTTER}px + ${TASK_SHARE}%)` : GUTTER }}>
                  {col.blocks.map((b) => (
                    <EventBlock key={b.key} b={b} y={atY(b.top)} h={atY(b.bottom) - atY(b.top)} selected={p.selected === `ev:${b.key}`} clash={p.clash.keys.has(b.key)} why={p.clash.why.get(b.key)}
                      dim={!p.matches(`ev:${b.key}`)} redact={p.redact} onPick={p.onPick} onHover={p.onHover} onDrag={onDrag} />
                  ))}
                </div>
                {/* the task blocks, beside the events: a lane of their own at the column's right, left of the marks */}
                {col.tasks.length > 0 && (
                  <div className="absolute inset-y-0" style={{ right: GUTTER, width: `${TASK_SHARE}%` }} data-testid="task-lane">
                    {col.tasks.map((t) => (
                      <TaskBlockView key={`${t.key}:${t.day}`} t={t} y={atY(t.top)} h={atY(t.bottom) - atY(t.top)} now={now} selected={p.selected === t.key} clash={false}
                        dim={!p.matches(t.key)} onPick={p.onPick} onHover={p.onHover} />
                    ))}
                  </div>
                )}
                {/* layer 3: the marks, with the watch bars at the column's edge */}
                {lay.bars.filter((b) => b.day === d.index).map((b) => (
                  <button type="button" key={`${b.mark.key}:${b.day}`} data-mark="watch" title={markLine(b.mark)} onClick={() => p.onPick(b.mark.key)} draggable onDragStart={(e) => onDrag(e, b.mark)}
                    className={cn('absolute right-0 w-1 rounded-sm', p.selected === b.mark.key && 'ring-1 ring-gold')}
                    style={{ top: atY(b.from), height: Math.max(3, atY(b.to) - atY(b.from)), background: `${toneHex.think}88` }} />
                ))}
                <div className="absolute inset-y-0 right-1" style={{ width: GUTTER - 6 }}>
                  {col.ticks.map((t) => <Tick key={t.mark.key} p={t} y={atY(t.y)} selected={p.selected === t.mark.key} clash={p.clash.keys.has(t.mark.key)} dim={!p.matches(t.mark.key)} onPick={p.onPick} onHover={p.onHover} onDrag={onDrag} />)}
                </div>
                {nowY != null && <div className="pointer-events-none absolute inset-x-0 z-20 border-t border-live" style={{ top: nowY, boxShadow: `0 0 6px ${toneHex.live}` }} title={`now ${hm(now)}`} />}
              </div>
            )
          })}
        </div>
      </div>
      {cursor && <div role="status" className={cn('pointer-events-none absolute z-30 max-w-64 rounded-md bg-hull/95 px-2 py-1 text-[11px] shadow-xl ring-1', cursor.ok ? 'text-ink ring-live/50' : 'text-wait ring-wait/50')} style={{ left: cursor.x, top: cursor.y }}>{cursor.ok ? '' : '⊘ '}{cursor.text}</div>}
    </div>
  )
}
