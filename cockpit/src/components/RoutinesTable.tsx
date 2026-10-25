// The Routines table (theseus-p17fd; the owner's test H5): each routine's schedule, next run, last run (outcome,
// duration, cost, its line), 30-day record, month spend against its limit with a sparkline of per-run cost, and its last
// 30 runs in an expandable row (from `routine.runs`). The cells are `routineRows` and the sparkline `sparkPoints`
// (lib/agenda.ts); the runs are read by `useRoutineRuns`, again only when a routine's view changes.
import { Fragment, useState } from 'react'
import { useNavigate } from 'react-router'
import { ChevronDown, ChevronRight, Pause, Play, Repeat, X } from 'lucide-react'
import type { RunRow } from '@protocol'
import { useAct } from '@/components/ConfirmCard'
import { Btn, Empty, Pill } from '@/components/ui'
import { ago, clock, ms, short, until, usd, when } from '@/lib/format'
import { toneHex } from '@/lib/taxonomy'
import { costSeries, lastRuns, outcomeOf, routineVerbs, runTally, sparkPoints, type RoutineRow } from '@/lib/agenda'
import type { WorkView } from '@protocol'

const OUTCOME = { ok: ['✓', 'ok'], failed: ['✗', 'fault'], skipped: ['–', 'idle'], running: ['◐', 'live'] } as const

export function Spark({ runs, w = 84, h = 22 }: { runs: readonly RunRow[]; w?: number; h?: number }) {
  const pts = sparkPoints(costSeries(runs), w, h)
  if (!pts) return <span className="text-ink-faint">–</span>
  return (
    <svg width={w} height={h} role="img" aria-label={`cost of the last ${runs.length} runs`} data-testid="spark">
      <polyline points={pts} fill="none" stroke={toneHex.money} strokeWidth={1.4} strokeLinejoin="round" />
    </svg>
  )
}

function Outcome({ outcome }: { outcome: string }) {
  const k = outcome === 'ok' || outcome === 'complete' ? 'ok' : outcome === 'failed' || outcome === 'blocked' ? 'failed' : outcome === 'running' ? 'running' : 'skipped'
  const [g, tone] = OUTCOME[k]
  return <span style={{ color: toneHex[tone] }} title={outcome}>{g} {outcome}</span>
}

export function RunList({ runs, now }: { runs: readonly RunRow[]; now: number }) {
  const nav = useNavigate()
  const shown = lastRuns(runs)
  const t = runTally(shown)
  if (!shown.length) return <div className="px-3 py-2 text-[11.5px] text-ink-faint">no runs yet</div>
  return (
    <div className="px-3 py-2" data-testid="run-list">
      <div className="num mb-1 text-[11px] text-ink-faint">last {shown.length} runs: {t.ok} ✓ · {t.failed} ✗ · {t.skipped} skipped{t.running ? ` · ${t.running} running` : ''} · {usd(t.cost_usd, 3)}</div>
      <table className="w-full whitespace-nowrap text-[11.5px]">
        <thead className="text-[10px] uppercase tracking-wider text-ink-faint"><tr><th className="py-0.5 text-left">run</th><th className="text-left">started</th><th className="text-left">outcome</th><th className="text-right">took</th><th className="text-right">cost</th><th className="pl-3 text-left">report</th></tr></thead>
        <tbody>
          {shown.map((r) => (
            <tr key={r.task} className="border-t border-line/40 hover:bg-white/[0.02]">
              <td className="num py-0.5"><button onClick={() => nav(`/session/${r.task}`)} className="hover:text-live" title={`open the run's session ${r.task}`}>#{r.run}</button></td>
              <td className="num text-ink-dim" title={clock(r.started_ms)}>{ago(r.started_ms, now)}</td>
              <td><Outcome outcome={r.blocked ? 'blocked' : outcomeOf(r) === 'ok' ? 'ok' : r.state} /></td>
              <td className="num text-right text-ink-dim">{ms(r.duration_ms)}</td>
              <td className="num text-right text-ink-dim">{usd(r.cost_usd, 3)}</td>
              <td className="max-w-[420px] truncate pl-3 text-ink-faint" title={r.blocked?.reason ?? r.report}>{r.blocked ? `blocked: ${r.blocked.reason ?? r.blocked.tool}` : r.report ?? ''}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

export function RoutinesTable({ rows, views, runs, now }: { rows: readonly RoutineRow[]; views: readonly WorkView[]; runs: Record<string, RunRow[]>; now: number }) {
  const [open, setOpen] = useState<string | null>(null)
  const { busy, run } = useAct()
  if (!rows.length) return <Empty>no routines set</Empty>
  const viewOf = new Map(views.map((v) => [v.id, v]))
  return (
    <table className="w-full whitespace-nowrap text-[12px]" data-testid="routines-table">
      <thead className="sticky top-0 bg-hull/95 text-[10px] uppercase tracking-wider text-ink-faint backdrop-blur">
        <tr>
          <th className="w-5" /><th className="px-2 py-1.5 text-left">routine</th><th className="px-2 text-left">schedule</th><th className="px-2 text-left">next run</th>
          <th className="px-2 text-left">last run</th><th className="px-2 text-left">30 days</th><th className="px-2 text-left">month</th><th className="px-2 text-left">cost per run</th><th className="px-2" />
        </tr>
      </thead>
      <tbody>
        {rows.map((r) => {
          const rs = runs[r.session_id] ?? []
          const isOpen = open === r.id
          const v = viewOf.get(r.id)
          return (
            <Fragment key={r.id}>
              <tr className="border-t border-line/50 align-top hover:bg-white/[0.02]" data-routine={r.session_id}>
                <td className="px-1"><button onClick={() => setOpen(isOpen ? null : r.id)} aria-expanded={isOpen} aria-label={`${isOpen ? 'Hide' : 'Show'} the last runs of ${r.title}`} className="text-ink-faint hover:text-ink">{isOpen ? <ChevronDown size={13} /> : <ChevronRight size={13} />}</button></td>
                <td className="px-2 py-1.5"><span className="text-ink">{r.title}</span> {r.paused && <Pill tone="wait">paused</Pill>} <span className="num text-[10.5px] text-ink-faint">{short(r.session_id)}</span></td>
                <td className="num px-2 py-1.5 text-ink-dim"><Repeat size={11} className="mr-1 inline" />{r.schedule}</td>
                <td className="num px-2 py-1.5 text-ink-dim" title={r.next_ms ? clock(r.next_ms) : undefined}>{r.paused ? 'held' : r.next_ms ? `${when(r.next_ms, now)} · ${until(r.next_ms, now)}` : '—'}</td>
                <td className="px-2 py-1.5">
                  {r.last ? <><Outcome outcome={r.last.outcome} /> <span className="num text-ink-faint">· {ms(r.last.duration_ms)} · {usd(r.last.cost_usd, 3)} · {ago(r.last.at_ms, now)}</span>{r.last.line && <div className="max-w-[320px] truncate text-[11px] text-ink-faint" title={r.last.line}>{r.last.line}</div>}</> : <span className="text-ink-faint">never run</span>}
                </td>
                <td className="num px-2 py-1.5"><span style={{ color: toneHex.ok }}>{r.record.ok} ✓</span> <span style={{ color: r.record.failed ? toneHex.fault : undefined }}>{r.record.failed} ✗</span> <span className="text-ink-faint">{r.record.skipped} skipped</span></td>
                <td className="px-2 py-1.5">
                  <div className="num text-[11.5px]">{usd(r.spent_usd, 2)} of {usd(r.limit_usd, 2)}</div>
                  <div className="mt-0.5 h-1 w-24 overflow-hidden rounded bg-white/10" title={`${(r.share * 100).toFixed(0)}% of the month's limit`}><div className="h-full" style={{ width: `${Math.min(100, r.share * 100)}%`, background: r.share >= 1 ? toneHex.fault : r.share > 0.8 ? toneHex.wait : toneHex.money }} /></div>
                </td>
                <td className="px-2 py-1.5"><Spark runs={rs} /></td>
                <td className="px-2 py-1.5 text-right">
                  <span className="inline-flex gap-1">
                    {v && routineVerbs(v).filter((x) => x.key === 'pause' || x.key === 'resume' || x.key === 'run_now').map((x) => (
                      <Btn key={x.key} tone={x.tone} busy={busy === `${r.id}:${x.key}`} onClick={() => run(`${r.id}:${x.key}`, x.method, x.params, x.ask)}>{x.key === 'pause' ? <Pause size={12} /> : x.key === 'resume' ? <Play size={12} /> : null}{x.label}</Btn>
                    ))}
                    {v && routineVerbs(v).filter((x) => x.key === 'cancel').map((x) => (
                      <Btn key={x.key} tone="fault" busy={busy === `${r.id}:${x.key}`} onClick={() => run(`${r.id}:${x.key}`, x.method, x.params, x.ask)}><X size={12} /> {x.label}</Btn>
                    ))}
                  </span>
                </td>
              </tr>
              {isOpen && <tr className="bg-black/20"><td /><td colSpan={8}><RunList runs={rs} now={now} /></td></tr>}
            </Fragment>
          )
        })}
      </tbody>
    </table>
  )
}
