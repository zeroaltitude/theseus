// Watches and projects, two compact lists under the Routines table (theseus-p17fd). A watch: what it looks at, its
// condition, its last value and when it was seen, how many checks, when it gives up, and why it is failing. A project:
// its goal, a progress bar over its steps, milestones done, the next milestone with its date, what is overdue and when
// its review is. The rows are `watchRows` and `projectRows` (lib/agenda.ts), from the work board's views.
import { Eye, Flag } from 'lucide-react'
import { Empty, Pill } from '@/components/ui'
import { ago, clock } from '@/lib/format'
import { toneHex } from '@/lib/taxonomy'
import type { ProjectRow, WatchRow } from '@/lib/agenda'

export function WatchList({ rows, now }: { rows: readonly WatchRow[]; now: number }) {
  if (!rows.length) return <Empty>no watches set</Empty>
  return (
    <div className="flex flex-col gap-1.5" data-testid="watch-list">
      {rows.map((w) => (
        <div key={w.id} className="rounded-lg px-3 py-2 ring-1 ring-line">
          <div className="flex items-center gap-2"><Eye size={13} className="text-think" /><span className="min-w-0 flex-1 truncate text-[12.5px] text-ink" title={w.title}>{w.title}</span>{w.failing && <Pill tone="fault" title={w.failing}>failing</Pill>}<Pill tone={w.state === 'done' ? 'ok' : 'idle'}>{w.state}</Pill></div>
          <div className="num truncate text-[11px] text-ink-faint" title={`${w.check}\nuntil ${w.until}`}>{w.check} · until {w.until}</div>
          <div className="num text-[11px] text-ink-dim">
            {w.last ? <>last <span className="text-ink">{w.last.value}</span> · {ago(w.last.at_ms, now)}</> : 'not checked yet'} · {w.checks} check{w.checks === 1 ? '' : 's'}{w.expires_at_ms ? ` · gives up ${clock(w.expires_at_ms)}` : ''}
          </div>
          {w.failing && <div className="text-[11px]" style={{ color: toneHex.fault }}>{w.failing}</div>}
        </div>
      ))}
    </div>
  )
}

export function ProjectList({ rows }: { rows: readonly ProjectRow[] }) {
  if (!rows.length) return <Empty>no projects</Empty>
  return (
    <div className="flex flex-col gap-1.5" data-testid="project-list">
      {rows.map((p) => (
        <div key={p.id} className="rounded-lg px-3 py-2 ring-1 ring-line">
          <div className="flex items-center gap-2"><Flag size={13} className="text-wait" /><span className="min-w-0 flex-1 truncate text-[12.5px] text-ink" title={p.goal}>{p.title || p.goal}</span>{p.overdue > 0 && <Pill tone="fault">{p.overdue} overdue</Pill>}</div>
          {p.title && p.goal && <div className="truncate text-[11px] text-ink-faint" title={p.goal}>{p.goal}</div>}
          <div className="mt-1 flex items-center gap-2">
            <div className="h-1.5 w-32 overflow-hidden rounded bg-white/10" role="progressbar" aria-valuenow={p.steps.done} aria-valuemax={p.steps.total}><div className="h-full" style={{ width: `${p.share * 100}%`, background: toneHex.ok }} /></div>
            <span className="num text-[11px] text-ink-dim">{p.steps.done} of {p.steps.total} steps · {p.milestones.done} of {p.milestones.total} milestones</span>
          </div>
          <div className="num text-[11px] text-ink-faint">{p.next ? <>next: <span className="text-ink-dim">{p.next.title}</span>{p.next.due_at_ms ? ` · ${clock(p.next.due_at_ms)}` : ''}</> : 'nothing next'}{p.review ? ` · review ${p.review}` : ''}</div>
        </div>
      ))}
    </div>
  )
}
