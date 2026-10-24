// The New task sheet (theseus-jid7t.3, .6): every field of the Tasks page's form (a brief, a title, a budget, where it
// goes) and what `task.start` takes besides: the profile, greyed by the home's data mark as the model menu is, a start
// time, quotes, and an open task. It calls `task.start`, which opens the task with no model turn; the row it makes is
// selected on the board (the page writes it into the address), and a refusal shows the daemon's own words. `N` opens it
// with the conversation under the cursor as its home. A caller may pass a profile and a "US only" reason (the Timeline
// does for a sensitive calendar event): then another origin's profile is greyed with it.
import { useState } from 'react'
import { Send } from 'lucide-react'
import type { ProfileList, SessionInfo, TaskStartResult } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { cn, short, usd } from '@/lib/format'
import { useOpsAct } from './act'
import { FIELD, Sheet } from './Sheet'
import { dollarsOf, EMPTY_FORM, profileChoices, startParams } from './newtask'

export function NewTask({ home: initialHome = '', profile: initialProfile = '', usOnly, onStarted, onClose }: {
  home?: string; profile?: string; usOnly?: string; onStarted: (taskId: string) => void; onClose: () => void
}) {
  const { on } = useOpsAct()
  const [f, setF] = useState({ ...EMPTY_FORM, home: initialHome, profile: initialProfile })
  const set = (k: keyof typeof f) => (e: { target: { value: string } }) => setF((x) => ({ ...x, [k]: e.target.value }))
  const [busy, setBusy] = useState(false)
  const [said, setSaid] = useState<{ ok: boolean; text: string } | null>(null)
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 5000)
  const { data: pl } = useRpc<ProfileList>('profile.list', undefined, 30_000)
  const open = (sl?.sessions ?? []).filter((s) => s.kind === 'conversation' && !s.retired)
  const mark = open.find((s) => s.session_id === f.home)?.data
  const choices = profileChoices(pl?.profiles ?? [], mark, usOnly)
  const chosen = choices.find((c) => c.name === f.profile)
  const bad = dollarsOf(f.budget).bad
  const p = startParams(f)
  const start = async () => {
    if (!on('new_task')) return
    if (!p || chosen?.grey) return
    setBusy(true)
    try {
      const r = await call<TaskStartResult>('task.start', p)
      setSaid({ ok: true, text: `started ${r.short}${p.title ? ` · ${p.title}` : ''} · ${usd(r.limit_usd)}` })
      setF((x) => ({ ...x, brief: '', title: '', budget: '', quotes: '' }))
      onStarted(r.task_id)
    } catch (e: any) {
      setSaid({ ok: false, text: e?.message ?? String(e) })
    } finally { setBusy(false) }
  }
  return (
    <Sheet title="New task" note="A task works on its own and reports back. It starts at once, with no model turn." help="tasks.new" verb="new_task" submit="Start task" busy={busy}
      disabled={!p || !!chosen?.grey} result={said} onClose={onClose} onSubmit={() => void start()} wide>
      <textarea autoFocus value={f.brief} onChange={set('brief')} rows={3} placeholder="What should it do? A task works on its own and reports back." aria-label="Brief"
        onKeyDown={(e) => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && p && !busy) { e.preventDefault(); void start() } }} className={cn(FIELD, 'resize-y')} />
      <div className="flex flex-wrap items-center gap-2">
        <input value={f.title} onChange={set('title')} placeholder="Title (the brief’s first line)" aria-label="Title" className={cn(FIELD, 'min-w-48 flex-1')} />
        <input value={f.budget} onChange={set('budget')} placeholder="Budget $ (a quarter of what’s left)" aria-label="Budget in dollars" inputMode="decimal" className={cn(FIELD, 'w-52', bad && 'ring-fault/60')} />
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <select value={f.home} onChange={set('home')} aria-label="Where it goes" className={cn(FIELD, 'max-w-64')}>
          <option value="">Home conversation</option>
          {open.map((s) => <option key={s.session_id} value={s.session_id}>{s.title || s.label || short(s.session_id)}{s.data && s.data !== 'open' ? ` · ${s.data}` : ''}</option>)}
        </select>
        <select value={f.profile} onChange={set('profile')} aria-label="Profile" className={cn(FIELD, 'max-w-64', chosen?.grey && 'ring-fault/60')}>
          <option value="">Profile: the parent’s model</option>
          {choices.map((c) => <option key={c.name} value={c.name} disabled={!!c.grey} title={c.grey ?? undefined}>{c.label}{c.grey ? ' · refused' : ''}</option>)}
        </select>
        <input value={f.startWhen} onChange={set('startWhen')} placeholder="Start: now, 2h, fri 9:00" aria-label="Start time" className={cn(FIELD, 'w-44')} />
        <label className="flex items-center gap-1.5 text-[12px] text-ink-dim" title="An open task: its data may go to a model of any origin once its content is judged open">
          <input type="checkbox" checked={f.open} onChange={(e) => setF((x) => ({ ...x, open: e.target.checked }))} /> open task
        </label>
      </div>
      {chosen?.grey && <div className="text-[11.5px] text-fault">{chosen.grey}</div>}
      <textarea value={f.quotes} onChange={set('quotes')} rows={2} placeholder="Quotes the task reads verbatim (a message, a pasted log); a blank line between two" aria-label="Quotes" className={cn(FIELD, 'resize-y')} />
      <span className="sr-only"><Send size={12} /> Ctrl+Enter starts it</span>
    </Sheet>
  )
}
