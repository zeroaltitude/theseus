// The thread verbs that need words, as sheets (theseus-jid7t.6): a new thread's title and slug, an edit's title and
// goal, a seal's reason. Each is one existing call through `useOpsAct`.
import { useState } from 'react'
import type { ThreadInfo } from '@protocol'
import { cn } from '@/lib/format'
import { useOpsAct } from './act'
import { FIELD, Sheet } from './Sheet'
import { editParams, slugBad, slugOf } from './sheets'

export function NewThreadSheet({ onClose }: { onClose: () => void }) {
  const { busy, act } = useOpsAct()
  const [title, setTitle] = useState('')
  const [slug, setSlug] = useState('')
  const [touched, setTouched] = useState(false)
  const s = touched ? slug : slugOf(title)
  const bad = !title.trim() || slugBad(s)
  return (
    <Sheet title="New thread" note="A thread is a place of its own: its card, its working set, and a session that opens with its first message." verb="thread_new" submit="Make the thread"
      busy={busy === 'thread.new'} disabled={bad} onClose={onClose} onSubmit={() => void act('thread_new', 'thread.new', 'thread.new', { slug: s.trim(), title: title.trim() }).then((ok) => { if (ok) onClose() })}>
      <input autoFocus value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Title" aria-label="Title" className={FIELD} />
      <input value={s} onChange={(e) => { setTouched(true); setSlug(e.target.value) }} placeholder="slug (letters, digits and dashes)" aria-label="Slug" className={cn(FIELD, s && slugBad(s) && 'ring-fault/60')} />
    </Sheet>
  )
}

export function EditThreadSheet({ t, onClose }: { t: ThreadInfo; onClose: () => void }) {
  const { busy, act } = useOpsAct()
  const [title, setTitle] = useState(t.title)
  const [goal, setGoal] = useState(t.goal ?? '')
  const p = editParams(t.slug, t, { title, goal })
  return (
    <Sheet title="Edit thread" verb="thread_edit" submit="Save" busy={busy === `${t.slug}:thread.edit`} disabled={!p || !title.trim()} onClose={onClose}
      onSubmit={() => p && void act('thread_edit', `${t.slug}:thread.edit`, 'thread.edit', p).then((ok) => { if (ok) onClose() })}>
      <input autoFocus value={title} onChange={(e) => setTitle(e.target.value)} aria-label="Title" className={FIELD} />
      <textarea value={goal} onChange={(e) => setGoal(e.target.value)} rows={3} placeholder="Goal" aria-label="Goal" className={cn(FIELD, 'resize-y')} />
    </Sheet>
  )
}

export function SealThreadSheet({ t, onClose }: { t: ThreadInfo; onClose: () => void }) {
  const { busy, act } = useOpsAct()
  const [reason, setReason] = useState(t.seal_pending ?? '')
  return (
    <Sheet title={`Seal ${t.title}`} note="Its notes stop reaching any other thread’s recall. Unsealing lets them reach again." verb="seal" submit="Seal it" busy={busy === `${t.slug}:thread.seal`} onClose={onClose}
      onSubmit={() => void act('seal', `${t.slug}:thread.seal`, 'thread.seal', { slug: t.slug, ...(reason.trim() && { reason: reason.trim() }) }).then((ok) => { if (ok) onClose() })}>
      <textarea autoFocus value={reason} onChange={(e) => setReason(e.target.value)} rows={2} placeholder="Why (optional)" aria-label="Reason" className={cn(FIELD, 'resize-y')} />
    </Sheet>
  )
}
