// The inspector's sections (theseus-jid7t.6), drawn from `sections.ts`: a titled block, a fact list, a task's Record,
// and what stands for the session. Each reads only while it is on screen.
import { Fragment, type ReactNode } from 'react'
import { Link } from 'react-router'
import type { StandingListResult } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { cn } from '@/lib/format'
import type { FactLine, RecordFacts } from './sections'
import { standingLines } from './sections'

const TONE = { money: 'text-money', wait: 'text-wait', fault: 'text-fault', ok: 'text-ok', think: 'text-think', dim: 'text-ink-dim' } as const

export function Section({ title, hint, children }: { title: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-1">
      <h3 className="flex items-baseline gap-2 font-display text-[10.5px] font-bold uppercase tracking-[0.08em] text-gold">{title}{hint && <span className="font-sans text-[10.5px] font-normal normal-case tracking-normal text-ink-faint">{hint}</span>}</h3>
      {children}
    </section>
  )
}

export function Facts({ lines }: { lines: readonly FactLine[] }) {
  return (
    <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-[11.5px]">
      {lines.map((l) => <Fragment key={l.label}><dt className="text-ink-faint">{l.label}</dt><dd className={cn('num break-words', l.tone ? TONE[l.tone] : 'text-ink')}>{l.value}</dd></Fragment>)}
    </dl>
  )
}

/** A task's Record: the three layers' header lines, what it is for, how to know it is done, a change waiting. */
export function RecordBlock({ f }: { f: RecordFacts }) {
  return (
    <div className="flex flex-col gap-1.5">
      <Facts lines={f.lines} />
      {f.objective && <p className="text-[12px] text-ink"><span className="text-ink-faint">objective </span>{f.objective}</p>}
      {f.acceptance.length > 0 && <ul className="list-disc pl-5 text-[12px] text-ink">{f.acceptance.map((a, i) => <li key={i}>{a}</li>)}</ul>}
      {f.waiting && <p className="rounded-md bg-wait/10 px-2 py-1 text-[12px] text-ink ring-1 ring-wait/30">A change waits ({f.waiting.by}): {f.waiting.card}</p>}
    </div>
  )
}

/** What stands for this session: `standing.list` with its id, and `p`/⌫ from the TUI lands here. */
export function StandingBlock({ sessionId }: { sessionId: string }) {
  const { data } = useRpc<StandingListResult>('standing.list', { session_id: sessionId }, 0)
  const lines = standingLines(data?.records ?? [])
  if (!data) return <div className="text-[12px] text-ink-faint">reading…</div>
  if (!lines.length) return <div className="text-[12px] text-ink-faint">Nothing stands for it. <Link className="text-live" to="/context?tab=standing">All standing records</Link></div>
  return (
    <ul className="space-y-0.5 text-[12px]" id="standing">
      {lines.map((r) => <li key={r.id}><span className="num text-ink-faint">{r.kind} · {r.scope} </span><span className="text-ink">{r.text}</span></li>)}
    </ul>
  )
}
