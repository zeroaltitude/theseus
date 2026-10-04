// What you sent that the session has not written yet (the Observatory's draft bubble, theseus-vm3n.6). The composer
// adds each send; the transcript shows it under the turns, "waiting for admission", until the session writes its user
// node (then it goes), or with why it was never admitted.
import { create } from 'zustand'
import type { NodeInfo } from '@protocol'

export interface Draft {
  id: number
  session: string
  text: string
  /** When it was sent (this page's clock). */
  at: number
  /** Why the daemon never admitted it: the submit failed with no turn. */
  error?: string
}

export const useDrafts = create<{ drafts: Draft[] }>(() => ({ drafts: [] }))

let next = 0

/** A send, shown until its user node is written. */
export function addDraft(session: string, text: string): number {
  const id = ++next
  useDrafts.setState((s) => ({ drafts: [...s.drafts, { id, session, text, at: Date.now() }] }))
  return id
}

/** The submit failed before any turn: the draft stays, saying why, until dismissed. */
export function failDraft(id: number, error: string) {
  useDrafts.setState((s) => ({ drafts: s.drafts.map((d) => (d.id === id ? { ...d, error } : d)) }))
}

export function dropDraft(id: number) {
  useDrafts.setState((s) => (s.drafts.some((d) => d.id === id) ? { drafts: s.drafts.filter((d) => d.id !== id) } : s))
}

/** The drafts whose user node is in `nodes`: the same text, written after the send (with a few seconds' slack for
 *  the two clocks), each node standing for one draft at most. */
export function admitted(drafts: Draft[], nodes: NodeInfo[]): Draft[] {
  const users = nodes.filter((n) => n.kind === 'user_message')
  const used = new Set<string>()
  const out: Draft[] = []
  for (const d of drafts) {
    if (d.error) continue
    const n = users.find((u) => !used.has(u.node_id) && u.text === d.text && u.at_unix_ms >= d.at - 5_000)
    if (n) { used.add(n.node_id); out.push(d) }
  }
  return out
}
