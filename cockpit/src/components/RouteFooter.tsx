// A turn's route footer (theseus-q31l): where routing ran the turn the cockpit just sent, `routed: quick · haiku`,
// and the owner's one-tap correction of it, the cockpit's equivalent of Discord's ⬆️ and ⬇️ reactions. Each press is
// confirmed first, and is `route.correct` on that turn, judged by the core as any surface's (the owner, from a private
// place): it labels the turn's route judgment as the owner's, and the session's next turn runs where the owner said.
import { useState } from 'react'
import type { ProfileList, RouteCorrectResult, TurnResult } from '@protocol'
import { call } from '@/lib/rpc'
import { correctionChoices, routeLine } from '@/lib/routefooter'

export function RouteFooter({ sessionId, result, profiles }: { sessionId: string; result: TurnResult; profiles?: ProfileList }) {
  const [busy, setBusy] = useState(false)
  const [said, setSaid] = useState<string | null>(null)
  const line = routeLine(result.route, result.profile)
  if (!line) return null
  const correct = async (to: string) => {
    if (!window.confirm(`Correct this turn's routing: it should have run on ${to}? It labels the turn's route judgment as yours, and the session's next turn runs there.`)) return
    setBusy(true)
    try {
      const r = await call<RouteCorrectResult>('route.correct', { session_id: sessionId, turn_id: result.turn_id, to, via: 'cockpit', provenance: 'press' })
      setSaid(r.line)
    } catch (x: any) { setSaid(x?.message ?? String(x)) } finally { setBusy(false) }
  }
  const choices = correctionChoices((profiles?.profiles ?? []).map((p) => p.name), result.profile)
  return (
    <div className="mb-1.5 flex flex-wrap items-center gap-1.5 text-[11px] text-ink-faint">
      <span className="num">{line}</span>
      {choices.slice(0, 2).map((c) => (
        <button key={c.to} disabled={busy} onClick={() => correct(c.to)} title={`it should have run on a ${c.to} model`}
          className="rounded px-1.5 text-[10.5px] ring-1 ring-line hover:text-live disabled:opacity-50">{c.text}</button>
      ))}
      {choices.length > 2 && (
        <select disabled={busy} value="" onChange={(e) => e.target.value && correct(e.target.value)} title="it should have run on…"
          className="rounded bg-transparent text-[10.5px] ring-1 ring-line">
          <option value="">on…</option>
          {choices.slice(2).map((c) => <option key={c.to} value={c.to}>{c.text}</option>)}
        </select>
      )}
      {said && <span className="text-live">{said}</span>}
    </div>
  )
}
