// "Should have asked" on a notice (theseus-sgh): one press makes the tool ask first from now on, on every surface; it only
// tightens, and Actions undoes it. A call's card and a `tool.notified` ledger row both carry it.
import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import type { Tightening } from '@protocol'
import { call } from '@/lib/rpc'

/** "Should have asked" on a notice (theseus-sgh): one press makes the tool ask first from now on, on every surface. */
export function ShouldHaveAsked({ tool, corr, tightened }: { tool: string; corr?: string; tightened?: Tightening }) {
  const qc = useQueryClient()
  const [busy, setBusy] = useState(false)
  // The press worked: say so, as the Observatory's policy note did.
  const [done, setDone] = useState(false)
  if (done) return <span className="text-[10.5px] text-live" title="undo it from Actions, in Tool postures">{tool} asks first from now on</span>
  if (tightened) return <span className="text-[10.5px] text-ink-faint" title={`tightened by ${tightened.by} · undo it from Actions`}>asks first now</span>
  const press = async (e: React.MouseEvent) => {
    e.stopPropagation()
    if (!window.confirm(`Make ${tool} ask first from now on, on every surface? It only tightens; undo it from Actions.`)) return
    setBusy(true)
    try { await call('policy.tighten', { tool, correlation_id: corr || undefined }); setDone(true); await qc.invalidateQueries() } catch (x: any) { window.alert(x?.message ?? String(x)) } finally { setBusy(false) }
  }
  return (
    <button onClick={press} disabled={busy} className="text-[10.5px] text-wait hover:underline disabled:opacity-50"
      title={`${tool} asks first from now on, on every surface. It only tightens; undo it from Actions.`}>should have asked</button>
  )
}

