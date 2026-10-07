// The watch (theseus-hnof): the Ship's insight instruments. Five brass plates, each answering one of the owner's
// questions at a glance, from the daemon's own data: what is working now, what waits for you, what is slow, what today
// cost, and what went wrong. Each plate's "show" (or its key, 1 to 5) lights its vessels and calls on the chart and dims
// the rest; each of its lines flies there; each links to the page that explains it. The numbers and lines are worked
// out in `watch.ts` (pure); this draws them.
//
// The overlay goes with Esc, which the Ship's keys own. The watch reads the moment: the time machine's when the
// operator scrubs back (then every plate is as of then), else now, on a clock that ticks only as often as what it times
// needs. Calm when idle: nothing on a plate moves unless its number changes, and a number that changes glows once (not
// in calm mode, under reduced motion, or in the past).
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import { Link } from 'react-router'
import {
  Anchor, ArrowUpRight, Bell, BellRing, ChevronDown, ChevronRight, CircleCheck, Clock, Coins, Cog, Eye, Hexagon, Hourglass, TriangleAlert,
  type LucideIcon,
} from 'lucide-react'
import { useCalm } from '@/lib/calm'
import { useHistoryRows } from '@/lib/history'
import { cn, stamp } from '@/lib/format'
import type { ShipData } from './useShipData'
import type { ShipModel } from './model'
import {
  dollars, HOUR_MS, keyOf, platesOf, span, watchOf, type Plate, type Spent, type Tone, type WatchKey, type WatchLine, type WatchSet,
  type WatchTarget,
} from './watch.ts'
import './watch.css'

export type { WatchKey, WatchTarget } from './watch.ts'

/** What a plate lights on the chart: the vessels (session ids) and the lights (node ids) it is about. */
export interface WatchFocus {
  key: WatchKey
  vessels: string[]
  lights: string[]
}

export interface WatchProps {
  model: ShipModel | null
  data: ShipData
  /** The overlay that is on, if any. */
  focus: WatchFocus | null
  onFocus: (f: WatchFocus | null) => void
  /** Fly the camera to an item. */
  onFly: (t: WatchTarget) => void
}

/** A plate's icon while its number counts something, and while it is idle. */
const ICONS: Record<WatchKey, [LucideIcon, LucideIcon]> = {
  working: [Cog, Anchor],
  waiting: [BellRing, Bell],
  slow: [Hourglass, Hourglass],
  spent: [Coins, Coins],
  wrong: [TriangleAlert, CircleCheck],
}

/** Under 900 px tall or 1400 px wide, the plates fold to a strip of one line each. */
const COMPACT = '(max-height: 899px), (max-width: 1399px)'
const onCompact = (cb: () => void) => {
  const m = window.matchMedia(COMPACT)
  m.addEventListener('change', cb)
  return () => m.removeEventListener('change', cb)
}
const isCompact = () => window.matchMedia(COMPACT).matches

const sameSet = (a: WatchSet, b: WatchSet) => a.vessels.join() === b.vessels.join() && a.lights.join() === b.lights.join()
const isEmpty = (s: WatchSet) => !s.vessels.length && !s.lights.length

export function Watch({ model, data, focus, onFocus, onFly }: WatchProps) {
  // The ledger, the one copy: after the Ship's own first reads (the ship's log's pace), so the chart lands first.
  const history = useHistoryRows(true)
  const calm = useCalm((s) => s.calm)
  const compact = useSyncExternalStore(onCompact, isCompact)
  const past = data.past

  // The clock: none in the past (the moment stands still); live, often while something runs, slowly while something
  // waits, and once a minute otherwise (the day's end, the last day's window).
  const running = !!model?.vessels.some((v) => v.state === 'running') || !!data.actions?.some((a) => a.dispatched_at_ms && !a.settled_at_ms && a.tool === 'proc.run')
  const tick = useClock(past ? null : running ? 5000 : data.confirms?.length ? 15_000 : 60_000)
  // A row that landed after the last tick is not in the future: the moment is at least the newest row's time.
  const rows = history.rows
  const now = past ? past.t : Math.max(tick, rows.length ? rows[rows.length - 1].at_unix_ms : 0)
  // The local day: its midnight, and the next (23 or 25 hours on, the day the clocks change).
  const [dayStart, dayEnd] = useMemo(() => {
    const d = new Date(now)
    d.setHours(0, 0, 0, 0)
    return [d.getTime(), new Date(d.getFullYear(), d.getMonth(), d.getDate() + 1).getTime()]
  }, [now])

  const actions = useMemo(() => data.actions ?? (data.synthetic ? [] : undefined), [data.actions, data.synthetic])
  const confirms = useMemo(() => data.confirms ?? (data.synthetic ? [] : undefined), [data.confirms, data.synthetic])
  const watch = useMemo(() => watchOf({ model, actions, confirms, rows, rowsReady: history.ready, now, dayStart, dayEnd }),
    [model, actions, confirms, rows, history.ready, now, dayStart, dayEnd])
  const plates = useMemo(() => platesOf(watch), [watch])

  // The overlay follows its plate: what it lights changes as things start and settle; with nothing left, it goes.
  useEffect(() => {
    if (!focus) return
    const p = plates.find((x) => x.key === focus.key)
    if (!p) return
    if (isEmpty(p.focus)) onFocus(null)
    else if (!sameSet(p.focus, focus)) onFocus({ key: focus.key, ...p.focus })
  }, [plates, focus, onFocus])

  // Esc lets the overlay go: the Ship's keys do it (an overlay first, then the selection; not while an inspector is
  // open), so the key has one owner.
  const show = (p: Plate) => onFocus(focus?.key === p.key ? null : { key: p.key, ...p.focus })

  // The keys 1 to 5 toggle the plates' overlays, down the column; never while typing, nor over a dialog (the tour, the
  // palette).
  const keys = useRef({ plates, focus, show })
  useEffect(() => { keys.current = { plates, focus, show } })
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey || e.repeat) return
      const t = e.target as HTMLElement | null
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable)) return
      if (document.querySelector('[role="dialog"]')) return
      const { plates: ps, focus: f, show: toggle } = keys.current
      const p = ps.find((x) => keyOf(x.key) === e.key)
      if (!p || (isEmpty(p.focus) && f?.key !== p.key)) return
      e.preventDefault()
      toggle(p)
    }
    window.addEventListener('keydown', k)
    return () => window.removeEventListener('keydown', k)
  }, [])

  const glows = !calm && !past

  return (
    <div className="watch" role="region" aria-label="The watch: what is working, waiting, slow, spent, and wrong">
      {past && <AsOf past={past} />}
      {compact ? (
        <Strip plates={plates} focus={focus} onShow={show} onFly={onFly} glows={glows} dayStart={dayStart} />
      ) : (
        plates.map((p) => (
          <PlateCard key={p.key} plate={p} on={focus?.key === p.key} onShow={() => show(p)} onFly={onFly} glows={glows} dayStart={dayStart} />
        ))
      )}
    </div>
  )
}

/** The time machine's moment, above the plates, with what the console's compass and chronometer said of it: the
 *  profile live then, and how long the daemon had been up (or that it was down). */
function AsOf({ past }: { past: NonNullable<ShipData['past']> }) {
  const g = past.gauges
  const up = g.uptimeSecs === null ? 'the daemon was down then' : `up ${span(g.uptimeSecs * 1000)}`
  return (
    <div className="watch-asof brass-tip" title={`The Ship and the watch show the fleet as it was at ${stamp(past.t)}, folded from the ledger${g.profile ? `; the live profile then was ${g.profile}` : ''}; ${up}.`}>
      <Clock size={11} aria-hidden />
      <span>as of {stamp(past.t)}</span>
      {g.profile && <span className="watch-asof-more">· profile {g.profile}</span>}
      <span className={cn('watch-asof-more', g.uptimeSecs === null && 'text-fault')}>· {up}</span>
    </div>
  )
}

/** The live clock: it ticks every `period` ms, or never (null); it reads the time again whenever it starts (back from
 *  the time machine, say). */
function useClock(period: number | null): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (period === null) return
    const read = () => setNow(Date.now())
    const first = setTimeout(read, 0)
    const t = setInterval(read, period)
    return () => { clearTimeout(first); clearInterval(t) }
  }, [period])
  return now
}

// ---------------------------------------------------------------- the plates

/** How many times `pulse` has changed since the plate first had a number: the number's span is keyed by it, so its
 *  glow plays once for each change. A change while glows are off (calm, the past), or as they come back on (from the
 *  time machine to now), plays nothing. */
function useGlow(pulse: string | null, on: boolean): number {
  const [n, setN] = useState(0)
  const last = useRef({ pulse, on })
  useEffect(() => {
    const was = last.current
    last.current = { pulse, on }
    if (was.pulse !== pulse && on && was.on && was.pulse !== null && pulse !== null) setN((x) => x + 1)
  }, [pulse, on])
  return n
}

interface PlateProps { plate: Plate; on: boolean; onShow: () => void; onFly: (t: WatchTarget) => void; glows: boolean; dayStart: number }

function PlateCard({ plate: p, on, onShow, onFly, glows, dayStart }: PlateProps) {
  const glow = useGlow(p.value === '…' ? null : p.pulse, glows)
  const [Busy, Idle] = ICONS[p.key]
  const Icon = p.tone === 'idle' || p.tone === 'ok' ? Idle : Busy
  const id = `watch-${p.key}`
  return (
    <section className={cn('brass-card watch-plate', on && 'watch-plate-on')} aria-labelledby={id}>
      <header className="watch-head">
        <h3 id={id} className="ship-engraved watch-question">{p.question}</h3>
        <ShowToggle plate={p} on={on} onShow={onShow} />
        <PageLink plate={p} />
      </header>
      <div className="watch-figure">
        <span key={glow} className={cn('watch-value', glow > 0 && 'watch-glow')}>{p.value}</span>
        <p className="watch-caption" title={p.caption}>
          <Icon size={11} className={cn('watch-icon', tone(p.tone))} aria-label={p.unit} />{p.caption}
        </p>
      </div>
      {p.key === 'spent' && p.value !== '…' && <Sparkline plate={p as Spent} dayStart={dayStart} />}
      <Lines plate={p} onFly={onFly} />
    </section>
  )
}

function PageLink({ plate: p, size = 13 }: { plate: Plate; size?: number }) {
  return (
    <Link to={p.link.to} className="watch-link" title={`Open ${p.link.label}: the page that explains it`} aria-label={`Open ${p.link.label}`}>
      <ArrowUpRight size={size} aria-hidden />
    </Link>
  )
}

function ShowToggle({ plate: p, on, onShow, compact }: { plate: Plate; on: boolean; onShow: () => void; compact?: boolean }) {
  const empty = isEmpty(p.focus)
  const key = keyOf(p.key)
  const what = `${p.focus.vessels.length} vessel${p.focus.vessels.length === 1 ? '' : 's'}${p.focus.lights.length ? ` and ${p.focus.lights.length} light${p.focus.lights.length === 1 ? '' : 's'}` : ''}`
  return (
    <button type="button" className={cn('watch-show', on && 'watch-show-on')} aria-pressed={on} disabled={!on && empty} onClick={onShow} aria-keyshortcuts={key}
      title={on ? `On the chart: ${what} lit, the rest dimmed. Press again, ${key}, or Esc, to clear it.` : empty ? 'Nothing to show on the chart.' : `Show it on the chart (${key}): light ${what} and dim the rest.`}>
      <Eye size={10} aria-hidden />{compact ? '' : 'show'}
      {compact && <span className="sr-only">show</span>}
      <kbd className="watch-key" aria-hidden>{key}</kbd>
    </button>
  )
}

/** A plate's lines; past the shown ones, the caption has the count and the link the rest. */
function Lines({ plate: p, onFly }: { plate: Plate; onFly: (t: WatchTarget) => void }) {
  if (!p.lines.length) return null
  return (
    <ul className="watch-lines" aria-label={p.more ? `${p.lines.length} shown, ${p.more} more on ${p.link.label}` : undefined}>
      {p.lines.map((l) => <li key={l.id}><Line line={l} onFly={onFly} /></li>)}
    </ul>
  )
}

function Line({ line: l, onFly }: { line: WatchLine; onFly: (t: WatchTarget) => void }) {
  const body = (
    <>
      <span className="watch-line-main">
        <span className="watch-tag">{l.tag}</span>
        <span className="watch-text">{l.text}</span>
        {l.flag && <Flag flag={l.flag} />}
        {l.figure && <span className="watch-fig">{l.figure}</span>}
      </span>
      {l.detail && <span className="watch-detail">{l.detail}</span>}
    </>
  )
  if (!l.to) return <div className="watch-line" data-tone={l.tone} title={l.title}>{body}</div>
  const to = l.to
  return (
    <button type="button" className="watch-line" data-tone={l.tone} title={`${l.title} Click to fly there.`} onClick={() => onFly(to)}>
      {body}
    </button>
  )
}

/** A flag's words, for its tooltip: the sandbox, an expiry, a failure's kind, or how many times its usual. */
const FLAGS: Record<string, string> = {
  L1: 'in the sandbox (L1)', L0: 'on the host (L0)', expired: 'expired', declined: 'declined', unknown: 'its outcome is unknown',
  disk: 'the disk was under its floor', 'not run': 'never started',
}

function Flag({ flag }: { flag: string }) {
  const word = FLAGS[flag] ?? (flag.startsWith('exit ') ? `exit code ${flag.slice(5)}` : flag.endsWith('×') ? `${flag} its usual` : flag)
  const kind = flag.endsWith('×') ? 'times' : flag.startsWith('exit ') ? 'exit' : flag
  return (
    <span className="watch-flag" data-flag={kind} title={word}>
      {flag === 'L1' && <Hexagon size={9} aria-hidden />}{flag}
    </span>
  )
}

const tone = (t: Tone) => `watch-tone-${t}`

// ---------------------------------------------------------------- the day's spend by hour

const SPARK_W = 258
const SPARK_H = 16
const GAP = 2

/** A bar with a rounded data end, square at the baseline. */
function bar(x: number, w: number, h: number): string {
  const y = SPARK_H - h
  const r = Math.min(2, w / 2, h)
  return `M ${x} ${SPARK_H} V ${y + r} Q ${x} ${y} ${x + r} ${y} H ${x + w - r} Q ${x + w} ${y} ${x + w} ${y + r} V ${SPARK_H} Z`
}

const hh = (h: number) => String(h).padStart(2, '0')

/** A bar an hour of the local day (24, or 23 or 25 the day the clocks change); the hours past in worn brass, the hour
 *  now in gold, the hours to come bare. Each says its local hour: the day the clocks go back has two 01:00s. One series,
 *  so no legend: the plate's title says what it is, and each bar's title its hour and dollars. */
function Sparkline({ plate: p, dayStart }: { plate: Spent; dayStart: number }) {
  const n = p.hours.length
  const max = Math.max(0, ...p.hours)
  const w = (SPARK_W - GAP * (n - 1)) / n
  const local = p.hours.map((_, i) => new Date(dayStart + i * HOUR_MS).getHours())
  const twice = new Set(local.filter((h, i) => local.indexOf(h) !== i))
  const ticks = [6, 12, 18].map((h) => local.indexOf(h)).filter((i) => i > 0)
  const label = (i: number) => `${hh(local[i])}:00 to ${hh((local[i] + 1) % 24)}:00${twice.has(local[i]) ? (local.indexOf(local[i]) === i ? ' (before the clocks went back)' : ' (after the clocks went back)') : ''}`
  return (
    <svg className="watch-spark" viewBox={`0 0 ${SPARK_W} ${SPARK_H + 3}`} width="100%" height={SPARK_H + 3} role="img"
      aria-label={`Dollars by hour since midnight: ${p.hours.slice(0, p.hour + 1).map((v, i) => `${hh(local[i])}:00 ${dollars(v)}`).join(', ')}`}>
      <line x1={0} x2={SPARK_W} y1={SPARK_H + 0.5} y2={SPARK_H + 0.5} className="watch-spark-base" />
      {ticks.map((i) => <line key={i} x1={i * (w + GAP) - GAP / 2} x2={i * (w + GAP) - GAP / 2} y1={SPARK_H + 0.5} y2={SPARK_H + 3} className="watch-spark-tick" />)}
      {p.hours.map((v, i) => {
        const x = i * (w + GAP)
        const h = max > 0 && v > 0 ? Math.max(1.5, (v / max) * SPARK_H) : 0
        return (
          <g key={i}>
            <title>{`${label(i)} · ${i > p.hour ? 'to come' : dollars(v)}${i === p.hour ? ' (this hour)' : ''}`}</title>
            <rect x={x} y={0} width={w + GAP} height={SPARK_H} className="watch-spark-hit" />
            {h > 0 && <path d={bar(x, w, h)} className={i === p.hour ? 'watch-spark-now' : 'watch-spark-bar'} />}
          </g>
        )
      })}
    </svg>
  )
}

// ---------------------------------------------------------------- compact

/** The plates folded to a strip, one line each; a plate opens under its line, one at a time. */
function Strip({ plates, focus, onShow, onFly, glows, dayStart }: {
  plates: Plate[]; focus: WatchFocus | null; onShow: (p: Plate) => void; onFly: (t: WatchTarget) => void; glows: boolean; dayStart: number
}) {
  const [open, setOpen] = useState<WatchKey | null>(null)
  return (
    <div className="brass-card watch-compact">
      {plates.map((p) => (
        <StripRow key={p.key} plate={p} open={open === p.key} on={focus?.key === p.key} onOpen={() => setOpen(open === p.key ? null : p.key)}
          onShow={() => onShow(p)} onFly={onFly} glows={glows} dayStart={dayStart} />
      ))}
    </div>
  )
}

function StripRow({ plate: p, open, on, onOpen, onShow, onFly, glows, dayStart }: {
  plate: Plate; open: boolean; on: boolean; onOpen: () => void; onShow: () => void; onFly: (t: WatchTarget) => void; glows: boolean; dayStart: number
}) {
  const glow = useGlow(p.value === '…' ? null : p.pulse, glows)
  const [Busy, Idle] = ICONS[p.key]
  const Icon = p.tone === 'idle' || p.tone === 'ok' ? Idle : Busy
  const Chevron = open ? ChevronDown : ChevronRight
  return (
    <>
      <div className={cn('watch-row', on && 'watch-plate-on')}>
        <button type="button" className="watch-row-name" aria-expanded={open} onClick={onOpen} title={`${p.question}: ${p.caption}`}>
          <Chevron size={11} className="text-ink-faint" aria-hidden />
          <Icon size={12} className={tone(p.tone)} aria-hidden />
          <span className="ship-engraved">{p.question}</span>
        </button>
        <span key={glow} className={cn('watch-row-value', glow > 0 && 'watch-glow')}>{p.value}</span>
        <ShowToggle plate={p} on={on} onShow={onShow} compact />
        <PageLink plate={p} size={12} />
      </div>
      {open && (
        <div className="watch-open">
          <p className="watch-caption" title={p.caption}><Icon size={11} className={cn('watch-icon', tone(p.tone))} aria-label={p.unit} />{p.caption}</p>
          {p.key === 'spent' && p.value !== '…' && <Sparkline plate={p as Spent} dayStart={dayStart} />}
          <Lines plate={p} onFly={onFly} />
        </div>
      )}
    </>
  )
}
