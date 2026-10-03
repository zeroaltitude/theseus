// The time machine (theseus-logs, round two): the ship's log, a brass scrubber along the cockpit's foot. Drag it to any
// moment, and the Ship, Fleet, Actions, the gauges, and the money river show the state as it was then, folded from
// the ledger (`lib/timemachine.ts`). The track marks every turn, approval, cancel, and start; the gold under it is the
// ledger's own density, so where things happened is where the log is thick.
import { useEffect, useMemo, useRef, useState } from 'react'
import { useLocation, useSearchParams } from 'react-router'
import { ChevronLeft, ChevronRight, History, Radio } from 'lucide-react'
import { useHistoryRows } from '@/lib/history'
import { axisOf, marksOf, useAsOf, type Mark, type MarkKind } from '@/lib/timemachine'
import { FOLDS } from '@/lib/world'
import { useTick } from '@/lib/hooks'
import { useCalm } from '@/lib/calm'
import { clock, cn, ms, stamp } from '@/lib/format'

const SPANS = { '1h': 3_600_000, '6h': 21_600_000, '1d': 86_400_000, all: Infinity } as const
type Span = keyof typeof SPANS

/** Each mark's colour and how tall it stands in the track (a share of its height, from the floor). */
const MARK: Record<MarkKind, { c: string; h: number; w: number; word: string }> = {
  turn: { c: '#22d3ee', h: 0.42, w: 1, word: 'turns' },
  failed: { c: '#fb7185', h: 0.62, w: 1.5, word: 'failures' },
  ask: { c: '#fbbf24', h: 0.78, w: 2, word: 'approvals asked' },
  answer: { c: '#34d399', h: 0.6, w: 1.5, word: 'answered' },
  cancel: { c: '#f472b6', h: 0.9, w: 2, word: 'cancels' },
  start: { c: '#d6a548', h: 1, w: 1.5, word: 'starts' },
  crash: { c: '#fb7185', h: 1, w: 2, word: 'starts after a crash' },
  stop: { c: '#9c907a', h: 1, w: 1, word: 'stops' },
}

export function TimeMachine() {
  const t = useAsOf((s) => s.t)
  const setT = useAsOf((s) => s.set)
  const calm = useCalm((s) => s.calm)
  // The log's own walk waits for the page to land, unless the moment is already set (a link with ?t=).
  const h = useHistoryRows(t === null)
  const now = useTick(5000)
  const loc = useLocation()
  const [params, setParams] = useSearchParams()
  const [span, setSpan] = useState<Span>('all')
  const [hover, setHover] = useState<{ x: number; at: number; near: Mark[]; quiet?: number } | null>(null)
  const track = useRef<HTMLDivElement>(null)
  const canvas = useRef<HTMLCanvasElement>(null)
  const dragging = useRef(false)
  // The same, for rendering: the hover card hides while the needle is dragged.
  const [drag, setDrag] = useState(false)
  const [width, setWidth] = useState(0)

  const marks = useMemo(() => marksOf(h.rows), [h.rows])
  const times = useMemo(() => {
    const out = new Array<number>(h.rows.length)
    let m = 0
    for (let i = 0; i < h.rows.length; i++) { m = Math.max(m, h.rows[i].at_unix_ms); out[i] = m }
    return out
  }, [h.rows])
  const first = h.rows.length ? h.rows[0].at_unix_ms : now - 3_600_000
  // The window: the whole log, or the last span before now (or before the moment, when it is further back).
  const end = Math.max(now, t ?? 0)
  const start = span === 'all' ? first : Math.max(first, Math.min(end - SPANS[span], (t ?? end) - SPANS[span] * 0.15))
  const winEnd = span === 'all' ? end : Math.min(end, start + SPANS[span])
  // Quiet stretches fold into breaks, so every busy one gets the track (`axisOf`).
  const axis = useMemo(() => axisOf(times, start, winEnd, Math.max(1, width), 30 * 60_000, 14), [times, start, winEnd, width])
  const x = (at: number) => axis.x(at)
  const atX = (px: number) => axis.at(Math.min(Math.max(px, 0), width))

  // The address keeps the moment (?t=), and a link or the back button sets it.
  const urlT = Number(params.get('t')) || null
  useEffect(() => {
    if (!dragging.current && urlT !== t) setT(urlT)
  }, [urlT]) // eslint-disable-line react-hooks/exhaustive-deps -- the address leads here; the store follows
  const commit = (v: number | null) => {
    setT(v)
    setParams((p) => { if (v === null) p.delete('t'); else p.set('t', String(Math.round(v))); return p }, { replace: true })
  }

  useEffect(() => {
    const el = track.current
    if (!el) return
    const ro = new ResizeObserver(() => setWidth(el.clientWidth))
    ro.observe(el)
    setWidth(el.clientWidth)
    return () => ro.disconnect()
  }, [])

  // The track: the ledger's density in gold, then the marks, drawn once per change of data, window, or size.
  useEffect(() => {
    const c = canvas.current
    if (!c || !width) return
    const dpr = window.devicePixelRatio || 1
    const H = 30
    c.width = Math.round(width * dpr)
    c.height = Math.round(H * dpr)
    const g = c.getContext('2d')!
    g.scale(dpr, dpr)
    g.clearRect(0, 0, width, H)
    const buckets = Math.max(1, Math.floor(width / 3))
    const counts = new Array(buckets).fill(0)
    for (const r of h.rows) {
      if (r.at_unix_ms < start || r.at_unix_ms > winEnd) continue
      counts[Math.min(buckets - 1, Math.max(0, Math.floor(axis.x(r.at_unix_ms) / 3)))]++
    }
    const peak = Math.max(1, ...counts)
    for (let i = 0; i < buckets; i++) {
      if (!counts[i]) continue
      const v = Math.sqrt(counts[i] / peak)
      g.fillStyle = `rgba(214,165,72,${0.1 + 0.32 * v})`
      g.fillRect(i * 3, H - 2 - v * (H - 6), 2, v * (H - 6))
    }
    // Each folded quiet stretch: a faint break in the well (the tooltip says how long it was).
    for (const b of axis.gaps) {
      const grad = g.createLinearGradient(b.x0, 0, b.x1, 0)
      grad.addColorStop(0, 'rgba(2,7,14,0)')
      grad.addColorStop(0.5, 'rgba(2,7,14,0.85)')
      grad.addColorStop(1, 'rgba(2,7,14,0)')
      g.fillStyle = grad
      g.fillRect(b.x0, 0, b.x1 - b.x0, H)
      g.fillStyle = 'rgba(176,141,87,0.35)'
      const mid = Math.round((b.x0 + b.x1) / 2)
      for (let yy = 5; yy < H - 3; yy += 4) g.fillRect(mid, yy, 1, 2)
    }
    for (const m of marks) {
      if (m.at < start || m.at > winEnd) continue
      const s = MARK[m.kind]
      const px = Math.round(x(m.at)) + 0.5
      g.strokeStyle = s.c
      g.globalAlpha = m.kind === 'turn' ? 0.75 : 0.95
      g.lineWidth = s.w
      g.beginPath()
      g.moveTo(px, H - 1)
      g.lineTo(px, H - 1 - s.h * (H - 3))
      g.stroke()
      if (m.kind === 'ask' || m.kind === 'cancel') {
        g.fillStyle = s.c
        g.beginPath()
        const y = H - 1 - s.h * (H - 3)
        g.moveTo(px, y - 3); g.lineTo(px + 3, y); g.lineTo(px, y + 3); g.lineTo(px - 3, y)
        g.closePath()
        g.fill()
      }
    }
    g.globalAlpha = 1
  }, [h.rows, marks, start, winEnd, width, axis]) // eslint-disable-line react-hooks/exhaustive-deps -- x() reads the axis

  // Dragging: one moment a frame, and the address once it is let go.
  const raf = useRef(0)
  const pending = useRef<number | null>(null)
  const scrubTo = (clientX: number) => {
    const r = track.current!.getBoundingClientRect()
    pending.current = atX(clientX - r.left)
    if (!raf.current) {
      raf.current = requestAnimationFrame(() => {
        raf.current = 0
        if (pending.current !== null) setT(pending.current)
      })
    }
  }
  const onDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return
    dragging.current = true
    setDrag(true)
    ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
    scrubTo(e.clientX)
  }
  const onMove = (e: React.PointerEvent) => {
    const r = track.current!.getBoundingClientRect()
    const px = e.clientX - r.left
    const near = marks.filter((m) => Math.abs(x(m.at) - px) <= 4 && m.at >= start && m.at <= winEnd).slice(-4)
    const quiet = axis.gaps.find((b) => px >= b.x0 && px <= b.x1)
    setHover({ x: px, at: atX(px), near, quiet: quiet ? quiet.to - quiet.from : undefined })
    if (dragging.current) scrubTo(e.clientX)
  }
  const onUp = () => {
    if (!dragging.current) return
    dragging.current = false
    setDrag(false)
    const v = pending.current ?? t
    commit(v !== null && v >= now - 1500 ? null : v)
  }

  // Step to the mark before or after the moment (or the present's last).
  const step = (dir: -1 | 1) => {
    const cur = t ?? now
    const list = marks.filter((m) => m.kind !== 'turn' || span !== 'all' || marks.length < 400)
    const next = dir < 0 ? [...list].reverse().find((m) => m.at < cur - 1) : list.find((m) => m.at > cur + 1)
    if (next) commit(next.at)
    else if (dir > 0) commit(null)
  }
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowLeft') { e.preventDefault(); step(-1) }
    if (e.key === 'ArrowRight') { e.preventDefault(); step(1) }
    if (e.key === 'End') { e.preventDefault(); commit(null) }
    if (e.key === 'Home') { e.preventDefault(); commit(first) }
  }

  const folds = FOLDS.some((p) => loc.pathname.startsWith(p) || (p === '/ship' && loc.pathname === '/'))
  const counts = useMemo(() => {
    const m = new Map<MarkKind, number>()
    for (const k of marks) m.set(k.kind, (m.get(k.kind) ?? 0) + 1)
    return m
  }, [marks])
  const px = t !== null ? x(t) : width

  return (
    <section className="log-bar relative flex h-[46px] shrink-0 items-center gap-3 px-3" aria-label="The ship's log: the time machine">
      <div className="flex shrink-0 items-center gap-2" title="The ship's log: drag along it to see the cockpit as it was. Every fact is a row of the ledger, folded in order.">
        <History size={15} className="text-gold" />
        <div className="leading-tight">
          <div className="ship-engraved text-[10px]">Ship&rsquo;s log</div>
          <div className="num text-[10px] text-ink-faint">{h.rows.length ? `since ${stamp(first)}` : h.error ? 'not read' : 'reading…'}{h.partial ? ' · partial' : ''}</div>
        </div>
      </div>

      <button type="button" onClick={() => step(-1)} title="The mark before (←)" className="rounded p-1 text-ink-faint hover:bg-gold/10 hover:text-ink"><ChevronLeft size={15} /></button>
      <div
        ref={track}
        role="slider"
        tabIndex={0}
        aria-label="The moment the cockpit shows"
        aria-valuemin={first}
        aria-valuemax={now}
        aria-valuenow={t ?? now}
        aria-valuetext={t ? `as of ${stamp(t)}` : 'live'}
        onKeyDown={onKey}
        onPointerDown={onDown}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onPointerCancel={onUp}
        onPointerLeave={() => setHover(null)}
        onDoubleClick={() => commit(null)}
        className="log-well relative h-[32px] min-w-0 flex-1 cursor-ew-resize touch-none outline-none focus-visible:ring-1 focus-visible:ring-live/60"
      >
        <canvas ref={canvas} className="pointer-events-none absolute inset-x-0 bottom-px h-[30px] w-full" />
        {/* The past, dimmed past the needle: what the cockpit shows is left of it. */}
        {t !== null && <div className="pointer-events-none absolute inset-y-0 right-0 bg-void/55" style={{ left: Math.max(0, px) }} />}
        <div className={cn('log-needle pointer-events-none absolute inset-y-[-3px] w-0', t === null && 'log-needle-live', calm && 'log-calm')} style={{ left: Math.min(width, Math.max(0, px)) }}>
          <span className="log-needle-head" />
        </div>
        {hover && !drag && (
          <div className="brass-tip pointer-events-none absolute bottom-[38px] z-30 w-max max-w-[340px] -translate-x-1/2 !px-2.5 !py-1.5" style={{ left: Math.min(Math.max(hover.x, 120), width - 120) }}>
            <div className="num text-[11.5px] text-ink">{stamp(hover.at)}</div>
            {hover.near.map((m, i) => (
              <div key={i} className="num truncate text-[10.5px]" style={{ color: MARK[m.kind].c }}>{clock(m.at)} {m.label}</div>
            ))}
            {hover.quiet !== undefined && <div className="text-[10.5px] text-ink-dim">a quiet stretch of {ms(hover.quiet)}, folded</div>}
            {!hover.near.length && hover.quiet === undefined && <div className="text-[10.5px] text-ink-faint">click or drag to see the cockpit then</div>}
          </div>
        )}
      </div>
      <button type="button" onClick={() => step(1)} title="The mark after (→)" className="rounded p-1 text-ink-faint hover:bg-gold/10 hover:text-ink"><ChevronRight size={15} /></button>

      <div className="flex shrink-0 items-center rounded-md bg-black/30 p-0.5 ring-1 ring-line" title="How much of the log the track shows">
        {(Object.keys(SPANS) as Span[]).map((s) => (
          <button key={s} type="button" onClick={() => setSpan(s)}
            className={cn('num rounded px-1.5 py-0.5 text-[10px] font-medium', s === span ? 'bg-gold/15 text-gold' : 'text-ink-faint hover:text-ink')}>{s}</button>
        ))}
      </div>

      {t === null ? (
        <div className="asof-live flex shrink-0 items-center gap-1.5 px-2.5 py-1" title={`${marks.length} marks: ${[...counts].map(([k, n]) => `${n} ${MARK[k].word}`).join(', ')}`}>
          <Radio size={13} className={cn('text-live', !calm && 'animate-pulse-soft')} />
          <span className="font-display text-[11px] font-bold tracking-[0.18em] text-live">LIVE</span>
        </div>
      ) : (
        <button type="button" onClick={() => commit(null)} className="asof-badge flex shrink-0 items-center gap-2 px-2.5 py-1"
          title={folds ? 'The cockpit shows this moment, folded from the ledger. Click to return to the present.' : 'This view shows the present; the Ship, Fleet, Actions, the money river, the boundaries board, and the session deck transcript show this moment.'}>
          <span className="font-display text-[11px] font-bold tracking-[0.14em] text-wait">AS OF</span>
          <span className="num text-[13px] font-semibold text-ivory">{clock(t)}</span>
          <span className="text-[10.5px] text-ink-faint">{new Date(t).toDateString() === new Date(now).toDateString() ? '' : new Date(t).toLocaleDateString([], { month: 'short', day: 'numeric' })}</span>
          <span className="text-ink-faint">·</span>
          <span className="font-display text-[10.5px] font-bold tracking-[0.12em] text-live">return to LIVE</span>
          {!folds && <span className="text-[10px] text-ink-faint">(this view is live)</span>}
        </button>
      )}
    </section>
  )
}
