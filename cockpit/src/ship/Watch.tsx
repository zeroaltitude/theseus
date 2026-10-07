// The watch (theseus-hnof): the Ship's insight instruments. Six brass plates, each answering one of the owner's
// questions at a glance, from the daemon's own data: what is working now, what waits for you, what is slow, what today
// cost, what went wrong, and what happened since you last looked. Each plate's "show" (or its key, 1 to 6) lights its
// vessels and calls on the chart and dims the rest; each of its lines flies there; each links to the page that
// explains it. The numbers and lines are worked out in `watch.ts` and `since.ts` (pure); this draws them.
//
// The overlay goes with Esc, which the Ship's keys own. The watch reads the moment: the time machine's when the
// operator scrubs back (then every plate is as of then), else now, on a clock that ticks only as often as what it times
// needs. Calm when idle: nothing on a plate moves unless its number changes, and a number that changes glows once (not
// in calm mode, under reduced motion, or in the past). The time your last look ended is kept in this browser.
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import { Link, useSearchParams } from 'react-router'
import {
  Anchor, ArrowUpRight, Bell, BellRing, Check, ChevronDown, ChevronRight, CircleCheck, Clock, Coins, Cog, Eye, Hexagon, History, Hourglass,
  Play, Square, Telescope, TriangleAlert, type LucideIcon,
} from 'lucide-react'
import type { LedgerEntry } from '@protocol'
import { useCalm } from '@/lib/calm'
import { useHistoryRows } from '@/lib/history'
import { useAsOf } from '@/lib/timemachine'
import { clock, cn, stamp } from '@/lib/format'
import type { ShipData } from './useShipData'
import type { ShipModel } from './model'
import {
  dollars, HOUR_MS, keyOf, platesOf, span, watchOf, type Plate, type Spent, type Tone, type WatchKey, type WatchLine, type WatchSet,
  type WatchTarget,
} from './watch.ts'
import { LOOKED_KEY, lookAt, looked, replayMoments, SEEN_EVERY_MS, sinceOf, type Looked, type Since, type SincePart } from './since.ts'
import './watch.css'

export type { WatchKey, WatchTarget } from './watch.ts'

/** What a plate lights on the chart: the vessels (session ids) and the lights (node ids) it is about; for the sixth
 *  plate, maybe one part of its tally alone. */
export interface WatchFocus {
  key: WatchKey
  part?: SincePart
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
  since: [Telescope, Telescope],
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

/** What an overlay lights now: its plate's, or one part of the sixth plate's tally. */
const focusOf = (p: Plate, part?: SincePart): WatchSet =>
  (part && p.key === 'since' ? (p as Since).tally.find((t) => t.part === part)?.focus : undefined) ?? p.focus

export function Watch({ model, data, focus, onFocus, onFly }: WatchProps) {
  // The ledger, the one copy: after the Ship's own first reads (the ship's log's pace), so the chart lands first.
  const history = useHistoryRows(true)
  const calm = useCalm((s) => s.calm)
  const compact = useSyncExternalStore(onCompact, isCompact)
  const past = data.past
  const look = useLook()
  const replay = useReplay()

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
  const since = useMemo(() => sinceOf({ model, actions, confirms, rows, rowsReady: history.ready, since: look.since, until: look.until, now }),
    [model, actions, confirms, rows, history.ready, look.since, look.until, now])
  const plates = useMemo(() => [...platesOf(watch), since], [watch, since])

  // The overlay follows its plate: what it lights changes as things start and settle; with nothing left, it goes.
  useEffect(() => {
    if (!focus) return
    const p = plates.find((x) => x.key === focus.key)
    if (!p) return
    const f = focusOf(p, focus.part)
    if (isEmpty(f)) onFocus(null)
    else if (!sameSet(f, focus)) onFocus({ key: focus.key, part: focus.part, ...f })
  }, [plates, focus, onFocus])

  // Esc lets the overlay go: the Ship's keys do it (an overlay first, then the selection; not while an inspector is
  // open), so the key has one owner.
  const show = (p: Plate, part?: SincePart) => {
    const on = focus?.key === p.key && focus.part === part
    onFocus(on ? null : { key: p.key, part, ...focusOf(p, part) })
  }

  // The keys 1 to 6 toggle the plates' overlays, down the column; never while typing, nor over a dialog (the tour, the
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
  const replayStretch = () => replay.start(rows, look.since, look.until)
  const sinceProps = { plate: since, focus, onShow: show, onFly, glows, replay, onReplay: replayStretch, onSeen: look.seen }

  return (
    <div className="watch" role="region" aria-label="The watch: what is working, waiting, slow, spent, wrong, and new since you last looked">
      {past && <AsOf past={past} live={data.profiles?.live} replay={replay} />}
      {compact ? (
        <Strip plates={plates} focus={focus} onShow={show} onFly={onFly} glows={glows} dayStart={dayStart} since={sinceProps} />
      ) : (
        <>
          {platesOf(watch).map((p) => (
            <PlateCard key={p.key} plate={p} on={focus?.key === p.key} onShow={() => show(p)} onFly={onFly} glows={glows} dayStart={dayStart} />
          ))}
          <SincePlate {...sinceProps} />
        </>
      )}
    </div>
  )
}

/** The time machine's moment, above the plates, with what the console's compass and chronometer said of it: the
 *  profile live then (the fold's; with no change of profile in the ledger, the one live now, as the compass read it),
 *  and how long the daemon had been up, or that it was down. In a replay, its progress and its stop. */
function AsOf({ past, live, replay }: { past: NonNullable<ShipData['past']>; live?: string; replay: Replay }) {
  const g = past.gauges
  const profile = g.profile ?? live
  const up = g.uptimeSecs === null ? 'the daemon was down then' : `up ${span(g.uptimeSecs * 1000)}`
  return (
    <div className="watch-asof brass-tip" title={`The Ship and the watch show the fleet as it was at ${stamp(past.t)}, folded from the ledger${profile ? `; the live profile then was ${profile}` : ''}; ${up}.`}>
      {replay.playing ? <History size={11} aria-hidden /> : <Clock size={11} aria-hidden />}
      <span>{replay.playing ? 'replaying' : 'as of'} {stamp(past.t)}</span>
      {profile && <span className="watch-asof-more">· {profile}</span>}
      <span className={cn('watch-asof-more', g.uptimeSecs === null && 'text-fault')}>· {up}</span>
      {replay.playing && (
        <>
          <button type="button" className="watch-act watch-asof-stop" onClick={replay.stop} title="Stop the replay and return to the present"><Square size={8} aria-hidden /> stop</button>
          <span className="watch-replay-bar" role="progressbar" aria-label="The replay" aria-valuenow={Math.round(replay.progress * 100)}><span style={{ width: `${replay.progress * 100}%` }} /></span>
        </>
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

// ---------------------------------------------------------------- your look, kept in this browser

const keep = (v: Looked) => { try { localStorage.setItem(LOOKED_KEY, JSON.stringify(v)) } catch { /* private mode: this page alone */ } }
const kept = (): Looked | null => { try { return looked(localStorage.getItem(LOOKED_KEY)) } catch { return null } }

/** The stretch the sixth plate tells: you were away from the end of your last look to the start of this one. While the
 *  Ship is in sight its time is written every half minute and as it goes out of sight (another tab, another page, the
 *  browser closed); coming back after five minutes or more starts a new look (`lookAt`). "Seen" ends the stretch:
 *  the plate is quiet until you are next away. */
function useLook(): { since: number; until: number; seen: () => void } {
  const [v, setV] = useState<Looked>(() => {
    const x = lookAt(kept(), Date.now())
    keep(x)
    return x
  })
  useEffect(() => {
    const visible = () => document.visibilityState === 'visible'
    const write = (fresh: boolean) => setV((cur) => {
      const x = fresh ? lookAt(kept() ?? cur, Date.now()) : { ...cur, seen: Date.now() }
      keep(x)
      return x.since === cur.since && x.start === cur.start && x.seen === cur.seen ? cur : x
    })
    const tick = setInterval(() => { if (visible()) write(false) }, SEEN_EVERY_MS)
    // Out of sight: the time it went; back in sight: a new look if it was gone long enough.
    const onVisibility = () => write(visible())
    const onHide = () => write(false)
    document.addEventListener('visibilitychange', onVisibility)
    window.addEventListener('pagehide', onHide)
    return () => {
      clearInterval(tick)
      document.removeEventListener('visibilitychange', onVisibility)
      window.removeEventListener('pagehide', onHide)
      const cur = kept()
      if (cur) keep({ ...cur, seen: Date.now() })
    }
  }, [])
  return {
    since: v.since,
    until: v.start,
    seen: () => { const now = Date.now(); const x = { since: now, start: now, seen: now }; keep(x); setV(x) },
  }
}

// ---------------------------------------------------------------- the replay

/** The replay's frames a second, and how many frames: about twelve seconds, whatever the stretch's length. */
const REPLAY_FPS = 8
const REPLAY_STEPS = 96

export interface Replay {
  playing: boolean
  /** How far it has run, 0 to 1. */
  progress: number
  start: (rows: readonly LedgerEntry[], from: number, to: number) => void
  stop: () => void
}

/** The time machine run over a stretch: the moment moves from its start to its end in about twelve seconds, the busy
 *  minutes slower than the quiet hours (`replayMoments`), then back to live. A scrub of the ship's log, or a stop,
 *  ends it; so does leaving the Ship, which gives the present back. */
function useReplay(): Replay {
  const setT = useAsOf((s) => s.set)
  const [, setParams] = useSearchParams()
  const run = useRef<{ moments: number[]; i: number } | null>(null)
  const [progress, setProgress] = useState<number | null>(null)
  const playing = progress !== null
  useEffect(() => {
    if (!playing) return
    const t = setInterval(() => {
      const r = run.current
      if (!r) return
      // Someone moved the moment (a scrub, a link): the replay lets it be.
      if (useAsOf.getState().t !== r.moments[r.i]) { run.current = null; setProgress(null); return }
      r.i++
      if (r.i >= r.moments.length) { run.current = null; setT(null); setProgress(null); return }
      setT(r.moments[r.i])
      setProgress(r.i / (r.moments.length - 1))
    }, 1000 / REPLAY_FPS)
    return () => clearInterval(t)
  }, [playing, setT])
  useEffect(() => () => {
    const r = run.current
    if (r && useAsOf.getState().t === r.moments[r.i]) useAsOf.getState().set(null)
  }, [])
  return {
    playing,
    progress: progress ?? 0,
    start: (rows, from, to) => {
      const times: number[] = []
      for (let i = rows.length - 1; i >= 0 && rows[i].at_unix_ms > from - 60_000; i--) times.push(rows[i].at_unix_ms)
      const moments = replayMoments(times, from, to, REPLAY_STEPS)
      // The address keeps a moment (?t=); the replay holds the moment itself, and gives the present back at its end.
      setParams((p) => { p.delete('t'); return p }, { replace: true })
      run.current = { moments, i: 0 }
      setT(moments[0])
      setProgress(0)
    },
    stop: () => {
      const r = run.current
      if (r && useAsOf.getState().t === r.moments[r.i]) setT(null)
      run.current = null
      setProgress(null)
    },
  }
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

// ---------------------------------------------------------------- since you last looked

interface SinceProps {
  plate: Since
  focus: WatchFocus | null
  onShow: (p: Plate, part?: SincePart) => void
  onFly: (t: WatchTarget) => void
  glows: boolean
  replay: Replay
  onReplay: () => void
  onSeen: () => void
}

/** The sixth plate: how long you were away, a tally of what happened (each part lights its own on the chart), the
 *  lines that matter most, the replay of the stretch, and "seen". */
function SincePlate({ plate: p, focus, onShow, onFly, glows, replay, onReplay, onSeen }: SinceProps) {
  const glow = useGlow(p.value === '…' ? null : p.pulse, glows)
  const on = focus?.key === 'since'
  const id = 'watch-since'
  const quiet = p.quiet && !replay.playing
  // The caption says when you were away, and the tally the rest; in a replay, the tally grows as the moment moves.
  const range = `${clock(p.since).slice(0, 5)} to ${clock(p.until).slice(0, 5)}`
  const caption = replay.playing ? `replaying ${range}` : p.away <= 0 ? p.caption : quiet ? `${range} · ${p.caption}` : `you were away ${range}`
  return (
    <section className={cn('brass-card watch-plate watch-since', on && !focus?.part && 'watch-plate-on', quiet && 'watch-since-quiet')} aria-labelledby={id}>
      <header className="watch-head">
        <h3 id={id} className="ship-engraved watch-question">{p.question}</h3>
        <PageLink plate={p} />
      </header>
      <div className="watch-figure">
        <span key={glow} className={cn('watch-value', glow > 0 && 'watch-glow')}>{p.value}</span>
        <p className="watch-caption" title={`${sinceWords(p)} ${p.caption}.`}>
          <Telescope size={11} className={cn('watch-icon', tone(p.tone))} aria-label={p.unit} />{caption}
        </p>
      </div>
      {!quiet && (
        <div className="watch-tally" role="group" aria-label="What happened while you were away; each lights its own on the chart">
          {p.tally.map((t) => {
            const lit = on && focus?.part === t.part
            return (
              <button key={t.part} type="button" className={cn('watch-cell', lit && 'watch-cell-on')} data-tone={t.tone} aria-pressed={lit}
                disabled={!lit && isEmpty(t.focus)} onClick={() => onShow(p, t.part)}
                title={`${t.title} ${isEmpty(t.focus) ? '' : lit ? 'Lit on the chart: click again, or Esc, to clear it.' : 'Click to light just these on the chart.'}`}>
                <span className="watch-cell-value">{t.value}</span>
                <span className="watch-cell-word">{t.word}</span>
                <span className="watch-cell-sub">{t.sub}</span>
              </button>
            )
          })}
        </div>
      )}
      {/* In a replay the tally counts up as the moment moves; the lines and the acts wait for its end. */}
      {!quiet && !replay.playing && <Lines plate={p} onFly={onFly} />}
      {p.away > 0 && !replay.playing && (
        <footer className="watch-since-foot">
          <ShowToggle plate={p} on={on && !focus?.part} onShow={() => onShow(p)} />
          <button type="button" className="watch-act" onClick={onReplay} disabled={quiet}
            title={quiet ? 'Nothing happened to replay.' : `Replay the stretch on the time machine: the Ship and the watch as they were, from ${clock(p.since)} to ${clock(p.until)}, in about twelve seconds. A scrub of the ship's log, or its stop, ends it.`}>
            <Play size={9} aria-hidden /> replay {span(p.away)}
          </button>
          <button type="button" className="watch-act" onClick={onSeen} title="Seen: the plate is quiet until you are next away"><Check size={10} aria-hidden /> seen</button>
        </footer>
      )}
    </section>
  )
}

const sinceWords = (p: Since) => (p.away > 0
  ? `You were away ${span(p.away)}: your last look at the Ship in this browser ended at ${stamp(p.since)}, and this one began at ${stamp(p.until)}.`
  : 'You have looked all along: it tells what happens while the Ship is out of sight, five minutes or more.')

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
function Strip({ plates, focus, onShow, onFly, glows, dayStart, since }: {
  plates: Plate[]; focus: WatchFocus | null; onShow: (p: Plate) => void; onFly: (t: WatchTarget) => void; glows: boolean; dayStart: number
  since: SinceProps
}) {
  const [open, setOpen] = useState<WatchKey | null>(null)
  return (
    <div className="brass-card watch-compact">
      {plates.map((p) => (
        <StripRow key={p.key} plate={p} open={open === p.key} on={focus?.key === p.key} onOpen={() => setOpen(open === p.key ? null : p.key)}
          onShow={() => onShow(p)} onFly={onFly} glows={glows} dayStart={dayStart} since={since} />
      ))}
    </div>
  )
}

function StripRow({ plate: p, open, on, onOpen, onShow, onFly, glows, dayStart, since }: {
  plate: Plate; open: boolean; on: boolean; onOpen: () => void; onShow: () => void; onFly: (t: WatchTarget) => void; glows: boolean; dayStart: number
  since: SinceProps
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
          {p.key === 'since' ? (
            <SincePlate {...since} />
          ) : (
            <>
              <p className="watch-caption" title={p.caption}><Icon size={11} className={cn('watch-icon', tone(p.tone))} aria-label={p.unit} />{p.caption}</p>
              {p.key === 'spent' && p.value !== '…' && <Sparkline plate={p as Spent} dayStart={dayStart} />}
              <Lines plate={p} onFly={onFly} />
            </>
          )}
        </div>
      )}
    </>
  )
}
