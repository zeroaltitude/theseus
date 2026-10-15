// The turn inspector's flame graph (theseus-x9swu): the whole turn on one time axis, in lanes (the loops, each model
// call, parallel tool calls in rows of their own, the waits, the person, compaction, recall, the judges). Hover for a
// bar's details, click to select, the wheel zooms about the pointer, a drag pans, and the keyboard walks the bars.
// Drawn as SVG: the bars are DOM, so the selection, the focus and the tip are plain. The layout is lib/inspect.ts.
import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent as RPointerEvent, type WheelEvent } from 'react'
import type { InspectRow, Span } from '@protocol'
import { us } from '@/lib/format'
import { LANES, layout, pan, step, zoom, type Bar, type Lane } from '@/lib/inspect'
import { spanColor } from '@/lib/spans'
import { TONE_MARK } from '@/lib/viz'

const ROW = 18
const GAP = 2
const LABEL_W = 64

const WORDS: Record<Lane, string> = {
  loops: 'loops', model: 'model', tools: 'tools', waits: 'waits', person: 'you', compile: 'compile', recall: 'recall', judges: 'judges', store: 'store', marks: 'marks',
}

const colorOf = (b: Bar) => (b.kind === 'person' ? TONE_MARK.wait : b.lane === 'judges' ? TONE_MARK.think : spanColor(b.kind))

export function TurnFlame({ trace, rows, endedMs, startedMs, selected, onSelect }: {
  trace: Span | null | undefined
  rows: readonly InspectRow[]
  endedMs: number | null
  startedMs: number
  selected: string | null
  onSelect: (b: Bar | null) => void
}) {
  const { bars, end, rowsIn } = useMemo(() => layout(trace, rows, endedMs, startedMs), [trace, rows, endedMs, startedMs])
  const [win, setWin] = useState<[number, number] | null>(null)
  const [hover, setHover] = useState<Bar | null>(null)
  const drag = useRef<{ x: number; win: [number, number] } | null>(null)
  const box = useRef<HTMLDivElement>(null)
  // The box's width, kept as state so the bars are laid out from it and not from a ref read in render.
  const [full, setFull] = useState(900)
  useEffect(() => {
    const el = box.current
    if (!el) return
    const read = () => setFull(el.clientWidth || 900)
    read()
    if (typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(read)
    ro.observe(el)
    return () => ro.disconnect()
  }, [])
  const view: [number, number] = win ?? [0, end]
  const lanes = LANES.filter((l) => rowsIn[l] > 0)
  const top: Record<string, number> = {}
  let y = 18
  for (const l of lanes) { top[l] = y; y += rowsIn[l] * ROW + 6 }
  const height = y + 4

  if (!bars.length) return <div className="p-4 text-[12px] text-ink-faint">this turn has no trace</div>

  const width = () => Math.max(1, full - LABEL_W)
  const x = (t: number) => LABEL_W + ((t - view[0]) / (view[1] - view[0] || 1)) * width()
  const at = (clientX: number) => view[0] + ((clientX - (box.current?.getBoundingClientRect().left ?? 0) - LABEL_W) / width()) * (view[1] - view[0])

  const onWheel = (e: WheelEvent) => {
    if (e.ctrlKey || e.metaKey || e.deltaY !== 0) {
      e.preventDefault()
      setWin(zoom(view, at(e.clientX), e.deltaY < 0 ? 0.8 : 1.25, end))
    }
  }
  const onDown = (e: RPointerEvent) => { drag.current = { x: e.clientX, win: view }; (e.currentTarget as Element).setPointerCapture?.(e.pointerId) }
  const onMove = (e: RPointerEvent) => {
    const d = drag.current
    if (!d) return
    const per = (d.win[1] - d.win[0]) / width()
    setWin(pan(d.win, -(e.clientX - d.x) * per, end))
  }
  const onUp = () => { drag.current = null }
  const onKey = (e: KeyboardEvent) => {
    const go = (by: 1 | -1) => { const b = step(bars, selected, by); if (b) { onSelect(b); setWin((w) => reveal(w ?? [0, end], b, end)) } }
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') { e.preventDefault(); go(1) }
    else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') { e.preventDefault(); go(-1) }
    else if (e.key === '+' || e.key === '=') setWin(zoom(view, (view[0] + view[1]) / 2, 0.7, end))
    else if (e.key === '-') setWin(zoom(view, (view[0] + view[1]) / 2, 1.4, end))
    else if (e.key === '0' || e.key === 'Escape') { setWin(null); if (e.key === 'Escape') onSelect(null) }
    else if (e.key === 'PageRight') setWin(pan(view, (view[1] - view[0]) / 2, end))
    else if (e.key === 'PageLeft') setWin(pan(view, -(view[1] - view[0]) / 2, end))
  }

  const ticks = Array.from({ length: 6 }, (_, i) => view[0] + ((view[1] - view[0]) * i) / 5)
  const tipBar = hover
  return (
    <div ref={box} className="relative select-none" onWheel={onWheel}>
      <svg role="application" aria-label="the turn's flame graph; arrow keys walk the bars, + and - zoom, 0 resets" tabIndex={0} onKeyDown={onKey}
        className="block w-full touch-none outline-none focus-visible:ring-1 focus-visible:ring-live/50"
        height={height} onPointerDown={onDown} onPointerMove={onMove} onPointerUp={onUp} onPointerLeave={onUp}>
        {ticks.map((t) => (
          <g key={t}>
            <line x1={x(t)} x2={x(t)} y1={12} y2={height} stroke="rgba(176,141,87,0.13)" />
            <text x={x(t) + 3} y={10} fontSize={10} fill="#9c907a" className="num">{us(t)}</text>
          </g>
        ))}
        {lanes.map((l) => (
          <text key={l} x={4} y={top[l] + 12} fontSize={10.5} fill="#c8bb9b">{WORDS[l]}</text>
        ))}
        {bars.map((b) => {
          if (b.end < view[0] || b.start > view[1]) return null
          const x0 = Math.max(LABEL_W, x(b.start))
          const w = Math.max(b.kind === 'mark' ? 0 : 1.5, x(b.end) - x0 - 1)
          const yy = top[b.lane] + b.row * ROW
          const sel = b.id === selected
          const wash = b.lane === 'loops'
          if (b.kind === 'mark') {
            return (
              <polygon key={b.id} points={`${x0},${yy + 2} ${x0 + 5},${yy + 8} ${x0},${yy + 14} ${x0 - 5},${yy + 8}`} fill={colorOf(b)} stroke={sel ? '#efe3c8' : 'none'}
                onClick={(e) => { e.stopPropagation(); onSelect(b) }} onMouseEnter={() => setHover(b)} onMouseLeave={() => setHover(null)} data-bar={b.id} />
            )
          }
          return (
            <g key={b.id} onClick={(e) => { e.stopPropagation(); onSelect(b) }} onMouseEnter={() => setHover(b)} onMouseLeave={() => setHover(null)} data-bar={b.id} className="cursor-pointer">
              <rect x={x0} y={yy + 1} width={w} height={ROW - GAP} rx={3} fill={colorOf(b)} opacity={wash ? 0.3 : 1} stroke={sel ? '#efe3c8' : 'none'} strokeWidth={sel ? 1.5 : 0} />
              {w > 46 && <text x={x0 + 5} y={yy + 13} fontSize={11} fill={wash ? '#efe3c8' : '#06101d'} pointerEvents="none">{clip(b.name, Math.floor((w - 8) / 6.2))}</text>}
            </g>
          )
        })}
      </svg>
      <div className="flex items-center gap-2 border-t border-line px-2 py-1 text-[11px] text-ink-faint">
        <span className="num">{us(view[0])} → {us(view[1])}</span>
        <button type="button" className="rounded px-1.5 py-0.5 ring-1 ring-inset ring-line hover:text-ink" onClick={() => setWin(zoom(view, (view[0] + view[1]) / 2, 0.6, end))} aria-label="zoom in">＋</button>
        <button type="button" className="rounded px-1.5 py-0.5 ring-1 ring-inset ring-line hover:text-ink" onClick={() => setWin(zoom(view, (view[0] + view[1]) / 2, 1.6, end))} aria-label="zoom out">－</button>
        <button type="button" className="rounded px-1.5 py-0.5 ring-1 ring-inset ring-line hover:text-ink" onClick={() => setWin(null)}>whole turn</button>
        <span className="ml-auto">wheel zooms · drag pans · ←/→ walks the bars</span>
      </div>
      {tipBar && (
        <div className="pointer-events-none absolute right-2 top-2 z-10 max-w-[360px] rounded-lg border border-line bg-hull/95 px-2.5 py-1.5 text-[11.5px] shadow-lg">
          <div className="font-semibold text-ink">{tipBar.name}</div>
          <div className="num text-ink-faint">{tipBar.lane} · {tipBar.kind}{tipBar.loop != null ? ` · loop ${tipBar.loop}` : ''}</div>
          <div className="num text-live">{us(tipBar.end - tipBar.start)} <span className="text-ink-faint">({us(tipBar.start)} → {us(tipBar.end)})</span></div>
        </div>
      )}
    </div>
  )
}

const clip = (s: string, n: number) => (s.length <= n ? s : `${s.slice(0, Math.max(1, n - 1))}…`)

/** The window moved the least that shows `b` whole. */
function reveal(win: [number, number], b: Bar, end: number): [number, number] {
  if (b.start >= win[0] && b.end <= win[1]) return win
  return pan(win, b.start < win[0] ? b.start - win[0] : b.end - win[1], end)
}
