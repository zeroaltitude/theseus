// The New task box, moved from the Tasks page with every field (theseus-ypnxt.6; theseus-jid7t.3): a brief, a title, a
// budget, and where it goes. It calls `task.start`, which opens the task with no model turn; the row it makes is
// selected on the board (the page writes it into the address), and a refusal shows the daemon's own words. `N` opens it with the conversation under the
// cursor as its home.
import { useState } from 'react'
import { Plus, Send, X } from 'lucide-react'
import type { SessionInfo, TaskStartParams, TaskStartResult } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { cn, short, usd } from '@/lib/format'
import { Panel } from '@/components/ui'
import { useOpsAct } from './act'
import { ActBtn } from './ActBtn'

export function NewTask({ home: initialHome = '', onStarted, onClose }: { home?: string; onStarted: (taskId: string) => void; onClose: () => void }) {
  const { on } = useOpsAct()
  const [brief, setBrief] = useState('')
  const [title, setTitle] = useState('')
  const [budget, setBudget] = useState('')
  const [home, setHome] = useState(initialHome)
  const [busy, setBusy] = useState(false)
  const [said, setSaid] = useState<{ ok: boolean; text: string } | null>(null)
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 5000)
  const open = (sl?.sessions ?? []).filter((s) => s.kind === 'conversation' && !s.retired)
  const dollars = budget.trim() === '' ? undefined : Number(budget.trim().replace(/^\$/, ''))
  const bad = dollars !== undefined && !(dollars > 0)
  const start = async () => {
    if (!on('new_task')) return
    const p: TaskStartParams = { brief: brief.trim(), ...(title.trim() && { title: title.trim() }), ...(home && { home }), ...(dollars !== undefined && { budget_usd: dollars }) }
    setBusy(true)
    try {
      const r = await call<TaskStartResult>('task.start', p)
      setSaid({ ok: true, text: `started ${r.short}${p.title ? ` · ${p.title}` : ''} · ${usd(r.limit_usd)}` })
      setBrief(''); setTitle(''); setBudget('')
      onStarted(r.task_id)
    } catch (e: any) {
      setSaid({ ok: false, text: e?.message ?? String(e) })
    } finally { setBusy(false) }
  }
  const field = 'rounded-md bg-black/20 px-2 py-1.5 text-[12.5px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/60'
  return (
    <Panel help="tasks.new" title="New task" icon={<Plus size={13} />} bodyClassName="flex flex-col gap-2 p-3"
      actions={<button onClick={onClose} title="close (esc)" aria-label="close the New task box" className="text-ink-faint hover:text-ink"><X size={13} /></button>}>
      <textarea autoFocus value={brief} onChange={(e) => setBrief(e.target.value)} rows={3} placeholder="What should it do? A task works on its own and reports back." aria-label="Brief"
        onKeyDown={(e) => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && brief.trim() && !bad && !busy) void start(); if (e.key === 'Escape') onClose() }} className={cn(field, 'resize-y')} />
      <div className="flex flex-wrap items-center gap-2">
        <input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Title (the brief’s first line)" aria-label="Title" className={cn(field, 'min-w-48 flex-1')} />
        <input value={budget} onChange={(e) => setBudget(e.target.value)} placeholder="Budget $ (a quarter of what’s left)" aria-label="Budget in dollars" inputMode="decimal" className={cn(field, 'w-52', bad && 'ring-fault/60')} />
        <select value={home} onChange={(e) => setHome(e.target.value)} aria-label="Where it goes" className={cn(field, 'max-w-64')}>
          <option value="">Home conversation</option>
          {open.map((s) => <option key={s.session_id} value={s.session_id}>{s.title || s.label || short(s.session_id)}</option>)}
        </select>
        <ActBtn verb="new_task" on={on} busy={busy} onClick={() => { if (brief.trim() && !bad) void start() }} title="Start it now, with no model turn (Ctrl+Enter)"><Send size={12} /> Start task</ActBtn>
      </div>
      {said && <div role="status" className={cn('num text-[11.5px]', said.ok ? 'text-ok' : 'text-fault')}>{said.text}</div>}
    </Panel>
  )
}
