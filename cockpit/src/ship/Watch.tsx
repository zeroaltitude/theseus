// The watch (theseus-hnof): the Ship's insight instruments. Five brass plates, each answering one of the owner's
// questions at a glance, from the daemon's own data: what is working now, what waits for you, what is slow, what today
// cost, and what went wrong. Each plate's "show" lights its vessels and calls on the chart and dims the rest; each of
// its lines flies there; each links to the page that explains it. The numbers and lines are worked out in `watch.ts`
// (pure); this draws them.
//
// The overlay goes with Esc, which the Ship's keys own. The watch reads the moment: the time machine's when the
// operator scrubs back (then every plate is as of then), else now, on a clock that ticks only as often as what it times
// needs. Calm when idle: nothing on a plate moves unless its number changes, and a number that changes glows once (not
// in calm mode, under reduced motion, or in the past).
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import { Link } from 'react-router'
import {
  Anchor, ArrowUpRight, Bell, BellRing, ChevronDown, ChevronRight, CircleCheck, Clock, Coins, Cog, Eye, Hexagon, Hourglass,
  TriangleAlert, type LucideIcon,
} from 'lucide-react'
import { useCalm } from '@/lib/calm'
import { useHistoryRows } from '@/lib/history'
import { cn, stamp } from '@/lib/format'
import type { ShipData } from './useShipData'
import type { ShipModel } from './model'
import { dollars, platesOf, watchOf, type Plate, type Spent, type Tone, type WatchKey, type WatchLine, type WatchSet, type WatchTarget } from './watch.ts'
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
  const dayStart = useMemo(() => { const d = new Date(now); d.setHours(0, 0, 0, 0); return d.getTime() }, [now])

  const watch = useMemo(() => watchOf({
    model, actions: data.actions ?? (data.synthetic ? [] : undefined), confirms: data.confirms ?? (data.synthetic ? [] : undefined),
    rows, rowsReady: history.ready, now, dayStart,
  }), [model, data.actions, data.confirms, data.synthetic, rows, history.ready, now, dayStart])
  const plates = useMemo(() => platesOf(watch), [watch])

  // The overlay follows its plate: what it lights changes as things start and settle; with nothing left, it goes.
  useEffect(() => {
    if (!focus) return
    const p = watch[focus.key]
    if (isEmpty(p.focus)) onFocus(null)
    else if (!sameSet(p.focus, focus)) onFocus({ key: focus.key, ...p.focus })
  }, [watch, focus, onFocus])

  // Esc lets the overlay go: the Ship's keys do it (an overlay first, then the selection; not while an inspector is
  // open), so the key has one owner.
  const show = (p: Plate) => onFocus(focus?.key === p.key ? null : { key: p.key, vessels: p.focus.vessels, lights: p.focus.lights })
  const glows = !calm && !past

  return (
    <div className="watch" role="region" aria-label="The watch: what is working, waiting, slow, spent, and wrong">
      {past && <div className="watch-asof brass-tip"><Clock size={11} aria-hidden /> as of {stamp(past.t)}</div>}
      {compact ? (
        <Strip plates={plates} focus={focus} onShow={show} onFly={onFly} glows={glows} />
      ) : (
        plates.map((p) => (
          <PlateCard key={p.key} plate={p} on={focus?.key === p.key} onShow={() => show(p)} onFly={onFly} glows={glows} />
        ))
      )}
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

interface PlateProps { plate: Plate; on: boolean; onShow: () => void; onFly: (t: WatchTarget) => void; glows: boolean }

function PlateCard({ plate: p, on, onShow, onFly, glows }: PlateProps) {
  const glow = useGlow(p.value === '…' ? null : p.pulse, glows)
  const [Busy, Idle] = ICONS[p.key]
  const Icon = p.tone === 'idle' || p.tone === 'ok' ? Idle : Busy
  const id = `watch-${p.key}`
  return (
    <section className={cn('brass-card watch-plate', on && 'watch-plate-on')} aria-labelledby={id}>
      <header className="watch-head">
        <h3 id={id} className="ship-engraved watch-question">{p.question}</h3>
        <ShowToggle plate={p} on={on} onShow={onShow} />
        <Link to={p.link.to} className="watch-link" title={`Open ${p.link.label}: the page that explains it`} aria-label={`Open ${p.link.label}`}>
          <ArrowUpRight size={13} aria-hidden />
        </Link>
      </header>
      <div className="watch-figure">
        <span key={glow} className={cn('watch-value', glow > 0 && 'watch-glow')}>{p.value}</span>
        <p className="watch-caption" title={p.caption}>
          <Icon size={11} className={cn('watch-icon', tone(p.tone))} aria-label={p.unit} />{p.caption}
        </p>
      </div>
      {p.key === 'spent' && p.value !== '…' && <Sparkline plate={p as Spent} />}
      <Lines plate={p} onFly={onFly} />
    </section>
  )
}

function ShowToggle({ plate: p, on, onShow, compact }: { plate: Plate; on: boolean; onShow: () => void; compact?: boolean }) {
  const empty = isEmpty(p.focus)
  const what = `${p.focus.vessels.length} vessel${p.focus.vessels.length === 1 ? '' : 's'}${p.focus.lights.length ? ` and ${p.focus.lights.length} light${p.focus.lights.length === 1 ? '' : 's'}` : ''}`
  return (
    <button type="button" className={cn('watch-show', on && 'watch-show-on')} aria-pressed={on} disabled={!on && empty} onClick={onShow}
      title={on ? `On the chart: ${what} lit, the rest dimmed. Press again, or Esc, to clear it.` : empty ? 'Nothing to show on the chart.' : `Show it on the chart: light ${what} and dim the rest.`}>
      <Eye size={10} aria-hidden />{compact ? '' : 'show'}
      {compact && <span className="sr-only">show</span>}
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

function Flag({ flag }: { flag: string }) {
  const words: Record<string, string> = { L1: 'in the sandbox (L1)', L0: 'on the host (L0)', expired: 'expired', ...(flag.startsWith('exit ') ? { [flag]: `exit code ${flag.slice(5)}` } : {}) }
  return (
    <span className="watch-flag" data-flag={flag} title={words[flag] ?? flag}>
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

/** 24 bars, one an hour of the local day; the hours past in worn brass, the hour now in gold, the hours to come bare.
 *  One series, so no legend: the plate's title says what it is, and each bar's title its hour and dollars. */
function Sparkline({ plate: p }: { plate: Spent }) {
  const max = Math.max(0, ...p.hours)
  const w = (SPARK_W - GAP * 23) / 24
  return (
    <svg className="watch-spark" viewBox={`0 0 ${SPARK_W} ${SPARK_H + 3}`} width="100%" height={SPARK_H + 3} role="img"
      aria-label={`Dollars by hour since midnight: ${p.hours.slice(0, p.hour + 1).map((v, i) => `${hh(i)}:00 ${dollars(v)}`).join(', ')}`}>
      <line x1={0} x2={SPARK_W} y1={SPARK_H + 0.5} y2={SPARK_H + 0.5} className="watch-spark-base" />
      {[6, 12, 18].map((h) => <line key={h} x1={h * (w + GAP) - GAP / 2} x2={h * (w + GAP) - GAP / 2} y1={SPARK_H + 0.5} y2={SPARK_H + 3} className="watch-spark-tick" />)}
      {p.hours.map((v, i) => {
        const x = i * (w + GAP)
        const h = max > 0 && v > 0 ? Math.max(1.5, (v / max) * SPARK_H) : 0
        return (
          <g key={i}>
            <title>{`${hh(i)}:00 to ${hh(i + 1)}:00 · ${i > p.hour ? 'to come' : dollars(v)}${i === p.hour ? ' (this hour)' : ''}`}</title>
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
function Strip({ plates, focus, onShow, onFly, glows }: { plates: Plate[]; focus: WatchFocus | null; onShow: (p: Plate) => void; onFly: (t: WatchTarget) => void; glows: boolean }) {
  const [open, setOpen] = useState<WatchKey | null>(null)
  return (
    <div className="brass-card watch-compact">
      {plates.map((p) => (
        <StripRow key={p.key} plate={p} open={open === p.key} on={focus?.key === p.key} onOpen={() => setOpen(open === p.key ? null : p.key)}
          onShow={() => onShow(p)} onFly={onFly} glows={glows} />
      ))}
    </div>
  )
}

function StripRow({ plate: p, open, on, onOpen, onShow, onFly, glows }: { plate: Plate; open: boolean; on: boolean; onOpen: () => void; onShow: () => void; onFly: (t: WatchTarget) => void; glows: boolean }) {
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
        <Link to={p.link.to} className="watch-link" title={`Open ${p.link.label}: the page that explains it`} aria-label={`Open ${p.link.label}`}>
          <ArrowUpRight size={12} aria-hidden />
        </Link>
      </div>
      {open && (
        <div className="watch-open">
          <p className="watch-caption" title={p.caption}><Icon size={11} className={cn('watch-icon', tone(p.tone))} aria-label={p.unit} />{p.caption}</p>
          {p.key === 'spent' && p.value !== '…' && <Sparkline plate={p as Spent} />}
          <Lines plate={p} onFly={onFly} />
        </div>
      )}
    </>
  )
}
