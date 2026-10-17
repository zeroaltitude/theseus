// A task's page header (theseus-jm46q): on `/session/<task>` only, above the deck's own header. The task's title, its
// state, what it is doing, its cost, Stop, the way back to the conversation that started it, and the inspector for its
// latest turn. The state, doing and forecast are the work board's view of the task (`WorkView`).
import { useState } from 'react'
import { Link } from 'react-router'
import { ArrowLeft, OctagonX, ScrollText } from 'lucide-react'
import type { SessionInfo, WorkView } from '@protocol'
import { call } from '@/lib/rpc'
import { short, usd } from '@/lib/format'
import { Btn, Pill, StatePill } from '@/components/ui'
import { joinable, type TaskContext } from '@/lib/taskpage'

export function TaskHeader({ s, task, view, parentTitle, lastTurn }: { s: SessionInfo; task: TaskContext; view?: WorkView; parentTitle?: string; lastTurn?: string }) {
  const [busy, setBusy] = useState(false)
  const live = view ? joinable(view) : !task.ended
  const stop = async () => {
    if (!window.confirm(`Stop task ${short(s.session_id)}? Its jobs stop, and it reports that it was cancelled.`)) return
    setBusy(true)
    try { await call('task.cancel', { task: s.session_id }) } catch (e: any) { window.alert(e?.message ?? String(e)) } finally { setBusy(false) }
  }
  const f = view?.forecast
  return (
    <div className="panel flex flex-wrap items-center gap-x-4 gap-y-1.5 px-4 py-2.5" data-testid="task-header">
      <Link to={`/session/${task.parent}`} className="inline-flex items-center gap-1.5 text-[12px] text-live hover:underline">
        <ArrowLeft size={13} /> back to {parentTitle || short(task.parent)}
      </Link>
      <span className="min-w-0 truncate text-[13px] font-medium text-ink">⚙ {task.title}</span>
      <StatePill state={view?.state ?? s.execution_state} />
      {view?.doing && <span className="num truncate text-[11.5px] text-ink-dim">{view.doing.label}</span>}
      <span className="num text-[11.5px] text-money">{usd(s.cost_usd)}{s.limit_usd != null ? ` / ${usd(s.limit_usd)}` : ''}</span>
      {f && <Pill tone={f.past ? 'wait' : 'idle'} title={`from ${f.n} finished tasks`}>usually ~{Math.round(f.min_p50)} min · {usd(f.usd_p50)}{f.past ? ' · past its usual' : ''}</Pill>}
      <div className="ml-auto flex items-center gap-2">
        {lastTurn && <Link to={`/session/${s.session_id}/turn/${lastTurn}`} className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-[12px] text-ink-dim ring-1 ring-line hover:text-ink"><ScrollText size={12} /> inspector</Link>}
        {live && <Btn tone="fault" busy={busy} onClick={stop} title="Cancel this task; it reports that it was cancelled"><OctagonX size={13} /> Stop</Btn>}
      </div>
    </div>
  )
}
