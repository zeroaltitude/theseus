// What the Ontology view and the session deck's memberships panel share (theseus-8kk.2): the one read of
// `ontology.list`, and the confirmed write. Every write is an operator's act, judged by the core as any surface's: a
// refusal comes back as REFUSED with its words, and a given kind's membership as invalid params; both are shown as sent.
import { useState, type ReactNode } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import type { OntologyListParams, OntologyListResult } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { useAsOf } from '@/lib/timemachine'
import { refusalWords } from '@/lib/ontology'

/** `ontology.list`, the present: one session's memberships when `sessionId` is given, every interpreted one otherwise. The
 *  daemon keeps no past of it, so the time machine's moment does not move it. */
export function useOntology(sessionId?: string) {
  const params: OntologyListParams = sessionId ? { session_id: sessionId } : {}
  return useRpc<OntologyListResult>('ontology.list', params, 5000)
}

/** Whether the time machine shows a past moment: the ontology's acts are off then, as every act is. */
export const useInPast = () => useAsOf((s) => s.t !== null)

/** A write, confirmed first with `ask` (which carries the text being sent); a refusal's words are shown. */
export function useOntologyWrite() {
  const qc = useQueryClient()
  const [busy, setBusy] = useState(false)
  const [refused, setRefused] = useState<string | null>(null)
  const write = async <T,>(ask: string, method: string, params: unknown): Promise<T | null> => {
    if (!window.confirm(ask)) return null
    setBusy(true)
    setRefused(null)
    try {
      const out = await call<T>(method, params)
      await qc.invalidateQueries({ queryKey: ['ontology.list'] })
      await qc.invalidateQueries({ queryKey: ['compilation.list'] })
      return out
    } catch (e) {
      setRefused(refusalWords(e))
      return null
    } finally {
      setBusy(false)
    }
  }
  return { write, busy, refused, clear: () => setRefused(null) }
}

/** The shown refusal: the daemon's words, as it said them. */
export function Refused({ words }: { words: string | null }) {
  if (!words) return null
  return <div role="alert" className="rounded-md bg-fault/10 px-2.5 py-1.5 text-[12px] text-fault ring-1 ring-fault/40">{words}</div>
}

/** A confirmed control: off while a write is in flight or the time machine shows a past moment. */
export function Act({ children, onClick, busy, off, title }: { children: ReactNode; onClick: () => void; busy?: boolean; off?: boolean; title?: string }) {
  return (
    <button type="button" onClick={onClick} disabled={busy || off} title={off ? 'return to LIVE in the ship’s log to change it' : title}
      className="inline-flex items-center gap-1.5 rounded-md bg-live/10 px-2.5 py-1.5 text-[12px] font-medium text-live ring-1 ring-inset ring-live/40 transition-[filter] hover:brightness-125 disabled:cursor-not-allowed disabled:opacity-50">
      {busy ? '…' : children}
    </button>
  )
}
