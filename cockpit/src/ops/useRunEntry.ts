// Running a palette entry (theseus-jid7t.6): the entry's plans, one protocol call each through `useOpsAct().post`, one
// question for the whole entry, and one toast that says how many were done and the daemon's own words for each that
// was refused. A single sticky model switch offers Undo. The palette and the right-click menu share it.
import { useQueryClient } from '@tanstack/react-query'
import type { SessionProfileResult } from '@protocol'
import { useOpsAct } from './act'
import { profileToast } from './menus'
import type { Entry } from './palette'
import { useOpsUi } from './ui'

export function useRunEntry(): (e: Entry) => Promise<void> {
  const { post } = useOpsAct()
  const qc = useQueryClient()
  const say = useOpsUi((s) => s.say)
  return async (e) => {
    const first = e.plans[0]
    if (first?.ask && !window.confirm(first.ask)) return
    let done = 0
    const refused: string[] = []
    let undo: (() => void) | undefined
    for (const p of e.plans) {
      try {
        const r = await post<SessionProfileResult>(p.verb as never, p.method, p.params)
        if (r?.refused) refused.push(r.refused)
        else {
          done++
          if (p.method === 'session.profile' && e.plans.length === 1) {
            const t = profileToast(r, (p.params as { profile: string | null }).profile)
            if (t.undo !== undefined) undo = () => void post(p.verb as never, p.method, { ...(p.params as object), profile: t.undo }).then(() => qc.invalidateQueries())
          }
        }
      } catch (err: any) { refused.push(err?.message ?? String(err)) }
    }
    void qc.invalidateQueries()
    say(`${e.label}: ${done} done${refused.length ? ` · ${refused.length} refused: ${refused[0]}` : ''}`, undo)
  }
}
