// Every execution, one row each (the Observatory's Executions table, theseus-vm3n.6): its kind and state, the turns it
// ran, what is outstanding and queued, its budget (spent, reserved, held, and the limit, the resets, and whether it
// asks at its limit), when it changed, and the controls: its calls and its ledger rows, and a cancel for one still alive.
import { useNavigate } from 'react-router'
import { OctagonX } from 'lucide-react'
import type { ExecutionInfo } from '@protocol'
import { ago, cn, short, stamp, usd } from '@/lib/format'
import { Btn, Empty, Pill, StatePill } from '@/components/ui'
import { useAct } from '@/components/ConfirmCard'

/** The kernel's terminal states (ExecState::is_terminal): nothing more to cancel. */
export const ENDED = ['cancelled', 'failed', 'budget_exhausted', 'complete']

export function ExecutionTable({ executions, title, now, past }: { executions: ExecutionInfo[]; title: (sid: string) => string; now: number; past?: boolean }) {
  const nav = useNavigate()
  const { busy, run } = useAct()
  if (!executions.length) return <Empty>none yet: the first prompt opens a session and its execution</Empty>
  const list = [...executions].sort((a, b) => b.updated_at_ms - a.updated_at_ms)
  return (
    <table className="w-full whitespace-nowrap text-[12px]">
      <thead className="sticky top-0 z-10 bg-hull/95 text-[10px] uppercase tracking-wider text-ink-faint backdrop-blur">
        <tr>
          <th className="px-2 py-1.5 text-left">execution</th><th className="px-2 py-1.5 text-left">kind</th><th className="px-2 py-1.5 text-left">state</th>
          <th className="px-2 py-1.5 text-right">turns</th><th className="px-2 py-1.5 text-right" title="calls in flight">outstanding</th>
          <th className="px-2 py-1.5 text-right" title="results that arrived while no turn ran, queued for the next">queued</th>
          <th className="w-52 px-2 py-1.5 text-left">budget</th><th className="px-2 py-1.5 text-right">updated</th><th className="px-2 py-1.5" />
        </tr>
      </thead>
      <tbody>
        {list.map((e) => {
          const b = e.budget
          const lim = Math.max(1e-6, b.limit_usd)
          const pct = (n: number) => `${Math.min(100, (n / lim) * 100)}%`
          const before = b.units_before ? `\nbefore dollar budgets: ${b.units_before.spent.toLocaleString()} of ${b.units_before.limit.toLocaleString()} units` : ''
          return (
            <tr key={e.execution_id} className="border-t border-line/60 hover:bg-white/[0.02]"
              title={`${e.execution_id}\nsession ${e.session_id}${e.ended_reason ? `\n${e.ended_reason}` : ''}\ncreated ${stamp(e.created_at_ms)}`}>
              <td className="px-2 py-1.5">
                <button onClick={() => nav(`/session/${e.session_id}`)} className="block max-w-[220px] truncate text-left text-ink hover:text-live">{title(e.session_id)}</button>
                <div className="num text-[10.5px] text-ink-faint">{short(e.execution_id)}</div>
              </td>
              <td className="px-2 py-1.5 text-ink-faint">{e.kind}</td>
              <td className="px-2 py-1.5">
                <StatePill state={e.state} />
                {e.interrupted > 0 && <span className="num ml-1 text-[11px] text-wait" title="times a crash interrupted a running turn; requeued at startup">↻{e.interrupted}</span>}
              </td>
              <td className="num px-2 py-1.5 text-right text-ink">{e.turns}</td>
              <td className={cn('num px-2 py-1.5 text-right', e.outstanding ? 'text-live' : 'text-ink-faint')}>{e.outstanding}</td>
              <td className={cn('num px-2 py-1.5 text-right', e.queued_results ? 'text-live' : 'text-ink-faint')}>{e.queued_results}</td>
              <td className="px-2 py-1.5">
                <div className="relative h-1.5 overflow-hidden rounded-full bg-black/45 shadow-[inset_0_0_0_1px_rgba(176,141,87,0.28)]"
                  title={`spent ${usd(b.spent_usd)} · reserved ${usd(b.reserved_usd)} · held unknown ${usd(b.held_unknown_usd)} · available ${usd(b.available_usd)} · limit ${usd(b.limit_usd)}${before}`}>
                  <div className="absolute inset-y-0 left-0 bg-gold" style={{ width: pct(b.spent_usd) }} />
                  <div className="absolute inset-y-0 bg-wait/70" style={{ left: pct(b.spent_usd), width: pct(b.reserved_usd) }} />
                  <div className="absolute inset-y-0 bg-fault/70" style={{ left: pct(b.spent_usd + b.reserved_usd), width: pct(b.held_unknown_usd) }} />
                </div>
                <div className="num mt-0.5 text-[10.5px] text-ink-faint">
                  {usd(b.spent_usd)}{b.reserved_usd > 0 && <> +{usd(b.reserved_usd)} rsv</>}{b.held_unknown_usd > 0 && <span className="text-wait"> +{usd(b.held_unknown_usd)} held</span>} / {usd(b.limit_usd)}{b.resets > 0 && <> · ↺{b.resets}</>}
                  {b.question && <Pill tone="wait" className="ml-1" title={`the session reached its limit and asks you: ${b.question}`}>at its limit</Pill>}
                </div>
              </td>
              <td className="num px-2 py-1.5 text-right text-ink-faint" title={stamp(e.updated_at_ms)}>{ago(e.updated_at_ms, now)}</td>
              <td className="px-2 py-1.5 text-right">
                <button onClick={() => nav(`/actions?execution=${e.execution_id}`)} className="mr-2 text-[11px] text-live hover:underline" title="this execution's tool calls">calls</button>
                <button onClick={() => nav(`/ledger?session=${e.session_id}`)} className="mr-2 text-[11px] text-live hover:underline" title="this session's ledger rows">ledger</button>
                {!past && !ENDED.includes(e.state) && (
                  <Btn tone="fault" busy={busy === e.execution_id} title="The deterministic control path: it never queues behind admission and terminates the execution's jobs"
                    onClick={() => run(e.execution_id, 'execution.cancel', { execution_id: e.execution_id }, `Cancel execution ${e.execution_id}? This is the deterministic control path: it never queues behind admission and terminates the execution's jobs.`)}>
                    <OctagonX size={12} /> Cancel
                  </Btn>
                )}
              </td>
            </tr>
          )
        })}
      </tbody>
    </table>
  )
}
