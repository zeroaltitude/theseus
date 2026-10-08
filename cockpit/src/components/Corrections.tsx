// The live correction layer (theseus-q31l): the owner's corrections of routing that steer a close message, newest
// first, from `route.corrections`, read only while the panel is open. Each entry: where a close message runs, the
// corrected message's content words, its turn and session, and the owner's label it follows.
import type { RouteCorrectionsResult } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { correctionLine, layerHead } from '@/lib/routefooter'
import { short, stamp } from '@/lib/format'
import { Empty } from '@/components/ui'

export function Corrections() {
  const { data, error } = useRpc<RouteCorrectionsResult>('route.corrections', undefined, 10_000)
  if (error) return <Empty>{error.message}</Empty>
  if (!data) return <Empty>reading…</Empty>
  return (
    <div className="text-[11px]">
      <div className="mb-1 text-ink-faint">{layerHead(data)}</div>
      {data.entries.length === 0 && <Empty>No correction steers a message yet: say "that should have been on fable" in a private place, or press a route footer's control.</Empty>}
      {data.entries.map((e) => (
        <div key={e.id} className="flex flex-wrap gap-2 border-t border-line/50 py-1">
          <span className="num text-ink-faint">{stamp(e.at_ms)}</span>
          <span className="text-ink">{correctionLine(e)}</span>
          <span className="num text-ink-faint">{short(e.turn_id)} · {short(e.session_id)}{e.label ? ` · ${short(e.label)}` : ''}</span>
        </div>
      ))}
    </div>
  )
}
