// The owner's end of a project (issue 37): a sheet that says what will end and what will stop, with the two answers,
// "Mark done" and "Drop", and a reason if there is one. It reads `project.show` for the counts, and the answer is one
// `project.close` through the page's own act plumbing, so it is off under the time machine and a refusal is the
// daemon's words.
import { useState } from 'react'
import type { ProjectCloseResult, ProjectShowResult } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { useOpsAct } from './act'
import { ActBtn } from './ActBtn'
import { FIELD, Sheet } from './Sheet'
import { closedWords, closeCounts, closeWords } from './projectClose'
import { useOpsUi } from './ui'

export function CloseProjectSheet({ project, title, onClose }: { project: string; title: string; onClose: () => void }) {
  const { post, busy, on } = useOpsAct()
  const say = useOpsUi((s) => s.say)
  const { data } = useRpc<ProjectShowResult>('project.show', { id: project }, 0)
  const [reason, setReason] = useState('')
  const [refused, setRefused] = useState<string | null>(null)
  const [sending, setSending] = useState(false)
  const note = data ? closeWords(closeCounts(data.records), !!data.project.review) : 'Reading the project…'
  const go = async (how: 'done' | 'dropped') => {
    setSending(true)
    setRefused(null)
    try {
      const r = await post<ProjectCloseResult>('project_close', 'project.close', { project, how, ...(reason.trim() ? { reason: reason.trim() } : {}) })
      say(closedWords(r)); onClose()
    } catch (e: any) { setRefused(e?.message ?? String(e)) } finally { setSending(false) }
  }
  return (
    <Sheet title={`Close ${title}`} note={note} verb="project_close" submit="Mark done" busy={!!busy || sending} disabled={!data} result={refused ? { ok: false, text: refused } : null}
      onClose={onClose} onSubmit={() => void go('done')}
      also={<ActBtn verb="project_close" on={on} tone="wait" busy={sending} onClick={() => { if (data && !sending) void go('dropped') }} title="end it as given up rather than finished">Drop</ActBtn>}>
      <input autoFocus value={reason} onChange={(e) => setReason(e.target.value)} maxLength={200} placeholder="Why (optional, one line)" aria-label="Reason" className={FIELD} />
    </Sheet>
  )
}
