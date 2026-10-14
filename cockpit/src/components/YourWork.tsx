// "Your work" (own-benchmark, theseus-pw1q.12): which model the owner's own turns need, measured on their own past
// work. One row a (model, effort) cell from `route.report`; the words are `lib/yourwork.ts`. A read: it writes
// nothing and the sampler it reports on moves no routing.
import { useState } from 'react'
import { UserRound } from 'lucide-react'
import type { RouteReportResult } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { cn } from '@/lib/format'
import { effortWord, footer, money, rows } from '@/lib/yourwork'
import { Panel } from '@/components/ui'

export function YourWork() {
  const [week, setWeek] = useState(true)
  const q = useRpc<RouteReportResult>('route.report', week ? { week: true } : {}, 0)
  const r = q.data
  return (
    <Panel help="bench.yourwork" title="Your work" icon={<UserRound size={14} />}
      actions={
        <div className="flex items-center rounded-md bg-black/30 p-0.5 ring-1 ring-line" role="tablist" aria-label="window">
          {[['Today', false], ['Week', true]].map(([label, w]) => (
            <button key={String(label)} type="button" role="tab" aria-selected={week === w} onClick={() => setWeek(w as boolean)}
              className={cn('rounded px-1.5 py-0.5 text-[10px] font-medium', week === w ? 'bg-live/15 text-live' : 'text-ink-faint hover:text-ink')}>
              {label as string}
            </button>
          ))}
        </div>
      }>
      {!r ? (
        <div className="px-4 py-3 text-[12px] text-ink-faint">{q.error ? `Could not read your turns: ${String(q.error)}` : 'Reading your turns…'}</div>
      ) : r.cells.length === 0 ? (
        <div className="px-4 py-3 text-[12px] text-ink-faint">No turns in the window.</div>
      ) : (
        <div className="px-3 py-2">
          <table className="data w-full text-[12px]">
            <thead>
              <tr className="text-left text-ink-faint">
                <th>model @ effort</th><th className="num">turns</th><th className="num">spend</th><th className="num">checked</th><th>one rung down</th><th className="num">movable</th>
              </tr>
            </thead>
            <tbody>
              {rows(r).map(({ cell: c, words, share }) => (
                <tr key={`${c.model}@${c.effort}`}>
                  <td className="font-mono">{c.model} @ {effortWord(c.effort)}</td>
                  <td className="num">{c.turns}</td>
                  <td className="num"><span className="inline-block h-1.5 rounded bg-live/40 align-middle" style={{ width: `${Math.max(2, share * 48)}px` }} /> {money(c.spend_usd)}</td>
                  <td className="num">{c.checked}</td>
                  <td className="!max-w-[70ch] !whitespace-normal text-ink-dim">{words}</td>
                  <td className="num">{c.replayed > 0 ? money(c.movable_usd) : '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="mt-1.5 text-[11px] text-ink-faint">{footer(r)}</div>
        </div>
      )}
    </Panel>
  )
}
