// The New task sheet (theseus-jid7t.3, .6): every field of the Tasks page's form (a brief, a title, a budget, where it
// goes) and what `task.start` takes besides: the profile, greyed by the home's data mark as the model menu is, a start
// time and quotes. It calls `task.start`, which opens the task with no model turn; the row it makes is
// selected on the board (the page writes it into the address), and a refusal shows the daemon's own words. `N` opens it
// with the conversation under the cursor as its home. A caller may pass a profile, a brief and a title, and "US only"
// (the Timeline does for a sensitive calendar event, theseus-jid7t.5): then another origin's profile is greyed with
// `US_ONLY_WORDS`, a profile of US origin is picked, and with none known it starts nothing (it fails closed).
import { useState } from 'react'
import { Send } from 'lucide-react'
import type { ProfileListResult, SessionInfo, TaskStartResult } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { cn, short, usd } from '@/lib/format'
import { useOpsAct } from './act'
import { FIELD, Sheet } from './Sheet'
import { US_ONLY_WORDS } from './calendar'
import { dollarsOf, EMPTY_FORM, profileChoices, startParams, usClosed } from './newtask'

export function NewTask({ home: initialHome = '', profile: initialProfile = '', brief: initialBrief = '', title: initialTitle = '', usOnly = false, onStarted, onClose }: {
  home?: string; profile?: string; brief?: string; title?: string; usOnly?: boolean; onStarted: (taskId: string) => void; onClose: () => void
}) {
  const { on } = useOpsAct()
  const [f, setF] = useState({ ...EMPTY_FORM, home: initialHome, profile: initialProfile, brief: initialBrief, title: initialTitle })
  const usWhy = usOnly ? US_ONLY_WORDS : undefined
  const set = (k: keyof typeof f) => (e: { target: { value: string } }) => setF((x) => ({ ...x, [k]: e.target.value }))
  const [busy, setBusy] = useState(false)
  const [said, setSaid] = useState<{ ok: boolean; text: string } | null>(null)
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 5000)
  const { data: pl } = useRpc<ProfileListResult>('profile.list', undefined, 0)
  const open = (sl?.sessions ?? []).filter((s) => s.kind === 'conversation' && !s.retired)
  const mark = open.find((s) => s.session_id === f.home)?.data
  const { choices, pick } = profileChoices(pl?.profiles ?? [], mark, usWhy, pl?.live)
  // Under "US only" the sheet picks a profile of US origin until one is chosen.
  const profile = f.profile || (usOnly ? pick : '')
  const chosen = choices.find((c) => c.name === profile)
  const closed = usClosed(choices, profile, usWhy)
  const bad = dollarsOf(f.budget).bad
  const p = startParams({ ...f, profile })
  const start = async () => {
    if (!on('new_task')) return
    if (closed) { setSaid({ ok: false, text: `${US_ONLY_WORDS}: pick a profile of US origin` }); return }
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
      disabled={!p || !!chosen?.grey || closed} result={said} onClose={onClose} onSubmit={() => void start()} wide>
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
        <select value={profile} onChange={set('profile')} aria-label="Profile" title={usWhy} className={cn(FIELD, 'max-w-64', (chosen?.grey || closed) && 'ring-fault/60')}>
          <option value="">{usOnly ? (pick ? 'pick a profile of US origin' : 'no profile of US origin is known') : 'Profile: the parent’s model'}</option>
          {choices.map((c) => <option key={c.name} value={c.name} disabled={!!c.grey} title={c.grey ?? undefined}>{c.label}{c.grey ? ' · refused' : ''}</option>)}
        </select>
        <input value={f.startWhen} onChange={set('startWhen')} placeholder="Start: now, 2h, fri 9:00" aria-label="Start time" className={cn(FIELD, 'w-44')} />
      </div>
      {(chosen?.grey || closed) && <div className="text-[11.5px] text-fault">{chosen?.grey ?? `${US_ONLY_WORDS}: pick a profile of US origin`}</div>}
      <textarea value={f.quotes} onChange={set('quotes')} rows={2} placeholder="Quotes the task reads verbatim (a message, a pasted log); a blank line between two" aria-label="Quotes" className={cn(FIELD, 'resize-y')} />
      <span className="sr-only"><Send size={12} /> Ctrl+Enter starts it</span>
    </Sheet>
  )
}
