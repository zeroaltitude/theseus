// The task verbs that need a choice, as sheets (theseus-jid7t.6): re-home (asks: it moves where the task's report goes
// and whose budget it was carved from), and reschedule a deferred start (one call, with the toast's Undo).
import { useState } from 'react'
import type { SessionInfo, TaskRescheduleResult, TaskRehomeResult } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { clock, short } from '@/lib/format'
import { useOpsAct } from './act'
import { FIELD, Sheet } from './Sheet'
import { useOpsUi } from './ui'

export function RehomeSheet({ task, title, home, onClose }: { task: string; title: string; home?: string; onClose: () => void }) {
  const { post, busy } = useOpsAct()
  const say = useOpsUi((s) => s.say)
  const { data } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 5000)
  const [to, setTo] = useState('')
  const open = (data?.sessions ?? []).filter((s) => s.kind === 'conversation' && !s.retired && s.session_id !== home)
  const go = async () => {
    try {
      const r = await post<TaskRehomeResult>('rehome', 'task.rehome', { task, home: to })
      say(r.note || 'moved'); onClose()
    } catch (e: any) { say(e?.message ?? String(e)) }
  }
  return (
    <Sheet title={`Re-home ${title}`} note="Its report goes to the new home, and the home's budget is the one it counts against from now on." verb="rehome" submit="Re-home it" busy={!!busy} disabled={!to} onClose={onClose} onSubmit={() => void go()}>
      <select autoFocus value={to} onChange={(e) => setTo(e.target.value)} aria-label="New home" className={FIELD}>
        <option value="">Choose a conversation</option>
        {open.map((s) => <option key={s.session_id} value={s.session_id}>{s.title || s.label || short(s.session_id)}</option>)}
      </select>
    </Sheet>
  )
}

export function RescheduleSheet({ task, title, startsAt, onClose }: { task: string; title: string; startsAt?: number; onClose: () => void }) {
  const { post, busy } = useOpsAct()
  const say = useOpsUi((s) => s.say)
  const [when, setWhen] = useState('')
  const go = async () => {
    try {
      const r = await post<TaskRescheduleResult>('reschedule', 'task.reschedule', { task, start_when: when.trim() })
      say(`${title} starts ${clock(r.starts_at_ms)}`, () => void post<TaskRescheduleResult>('reschedule', 'task.reschedule', { task, start_at_ms: r.previous }).then((u) => say(`back to ${clock(u.starts_at_ms)}`)).catch((e) => say(e?.message ?? String(e))))
      onClose()
    } catch (e: any) { say(e?.message ?? String(e)) }
  }
  return (
    <Sheet title={`Reschedule ${title}`} note={startsAt ? `It is set to start ${clock(startsAt)}.` : undefined} verb="reschedule" submit="Reschedule" busy={!!busy} disabled={!when.trim()} onClose={onClose} onSubmit={() => void go()}>
      <input autoFocus value={when} onChange={(e) => setWhen(e.target.value)} placeholder="2h, fri 9:00, tomorrow 9:00" aria-label="Start time" className={FIELD} />
    </Sheet>
  )
}
