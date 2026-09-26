import { useState } from 'react'
import type { Span } from './protocol'

// The turn's timing as a waterfall: one row per span, indented by depth, a bar
// positioned by start/end relative to the whole turn. Marks (first byte, first
// token) are ticks. Click a row for its attributes. This is the structure
// <turn><loop>…</loop></turn> that hooks, tools, and thinking will fill in.

const fmtUs = (us: number) =>
  us >= 1_000_000 ? `${(us / 1e6).toFixed(2)} s` : us >= 1000 ? `${(us / 1e3).toFixed(1)} ms` : `${us} µs`

const KIND_COLOR: Record<string, string> = {
  turn: 'var(--muted)', loop: 'var(--accent)', provider: 'var(--ok)', hook: '#b07cff',
  advancer: 'var(--warn)', compile: '#5ec8d8', store: '#8a93a6', lock: '#d86a5e', mark: 'var(--text)',
}

function Row({ s, depth, total, onPick, picked }: {
  s: Span; depth: number; total: number; onPick: (s: Span) => void; picked: Span | null
}) {
  const end = s.end_us ?? s.start_us
  const isMark = end === s.start_us
  const left = (s.start_us / total) * 100
  const width = Math.max(((end - s.start_us) / total) * 100, isMark ? 0 : 0.4)
  const color = KIND_COLOR[s.kind] ?? 'var(--text)'
  return (
    <>
      <div
        className={`trace-row ${picked === s ? 'picked' : ''}`}
        onClick={() => onPick(s)}
        title={`${s.name} · ${isMark ? `at ${fmtUs(s.start_us)}` : fmtUs(end - s.start_us)}`}
      >
        <div className="trace-label" style={{ paddingLeft: depth * 14 }}>
          <span className="trace-dot" style={{ background: color }} />
          {s.name} <span className="muted">{s.kind}</span>
        </div>
        <div className="trace-bar-lane">
          {isMark
            ? <span className="trace-tick" style={{ left: `${left}%`, background: color }} />
            : <span className="trace-bar" style={{ left: `${left}%`, width: `${width}%`, background: color }} />}
        </div>
        <div className="trace-dur">{isMark ? `@${fmtUs(s.start_us)}` : fmtUs(end - s.start_us)}</div>
      </div>
      {(s.children ?? []).map((c, i) => (
        <Row key={i} s={c} depth={depth + 1} total={total} onPick={onPick} picked={picked} />
      ))}
    </>
  )
}

export default function TraceView({ root }: { root: Span }) {
  const [picked, setPicked] = useState<Span | null>(null)
  const total = Math.max((root.end_us ?? root.start_us) - root.start_us, 1)
  // Summary line: where did the time go, by kind, counting only leaf-ish spans.
  const byKind = new Map<string, number>()
  const walk = (s: Span) => {
    const end = s.end_us ?? s.start_us
    if (s.kind !== 'turn' && s.kind !== 'loop' && end > s.start_us) byKind.set(s.kind, (byKind.get(s.kind) ?? 0) + (end - s.start_us))
    s.children?.forEach(walk)
  }
  walk(root)
  return (
    <div className="trace">
      <div className="trace-summary">
        <b>{fmtUs(total)}</b> total ·{' '}
        {[...byKind.entries()].sort((a, b) => b[1] - a[1]).map(([k, us]) => (
          <span key={k}><span className="trace-dot" style={{ background: KIND_COLOR[k] ?? 'var(--text)' }} />{k} {fmtUs(us)} </span>
        ))}
      </div>
      <div className="trace-rows">
        <Row s={root} depth={0} total={total} onPick={(s) => setPicked(picked === s ? null : s)} picked={picked} />
      </div>
      {picked && (
        <pre className="trace-attrs">{picked.name} · {picked.kind} · {fmtUs(picked.start_us)} → {fmtUs(picked.end_us ?? picked.start_us)}
{'\n'}{JSON.stringify(picked.attrs ?? {}, null, 2)}</pre>
      )}
    </div>
  )
}
