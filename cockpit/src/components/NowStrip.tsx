// "Now": one card per running turn, across the fleet. Its session, how long it has run, the tool calls in
// flight, and the newest narrative line about it. The narrative is global, so this sees every session at once.
import { useMemo } from 'react'
import { useNavigate } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { Radio, Wrench } from 'lucide-react'
import type { ActionInfo, ExecutionInfo, SessionInfo } from '@protocol'
import { usePush, useRpc } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { ms, short, usd } from '@/lib/format'
import { partTone, toneHex } from '@/lib/taxonomy'
import { LiveDot } from './ui'

export function NowStrip({ sessions, executions }: { sessions: SessionInfo[]; executions: ExecutionInfo[] }) {
  const nav = useNavigate()
  const now = useTick(500)
  const running = executions.filter((e) => e.state === 'running' || e.state === 'queued')
  const { data: al } = useRpc<{ actions: ActionInfo[] }>('action.list', { n: 200 }, running.length ? 1000 : 5000)
  const narrative = usePush((s) => s.narrative)
  const lastLine = useMemo(() => {
    const m = new Map<string, (typeof narrative)[number]>()
    for (const l of narrative) if (l.session_id) m.set(l.session_id, l)
    return m
  }, [narrative])
  const title = (sid: string) => { const s = sessions.find((x) => x.session_id === sid); return s?.title || s?.label || short(sid) }
  if (!running.length) return null
  return (
    <div className="grid grid-cols-1 gap-3 md:grid-cols-2 2xl:grid-cols-3">
      <AnimatePresence initial={false}>
        {running.map((e) => {
          const flying = (al?.actions ?? []).filter((a) => a.execution_id === e.execution_id && !a.settled_at_ms)
          const line = lastLine.get(e.session_id)
          return (
            <motion.button key={e.execution_id} layout onClick={() => nav(`/session/${e.session_id}`)}
              initial={{ opacity: 0, y: -10, scale: 0.98 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, scale: 0.96 }}
              className="panel relative overflow-hidden px-4 py-3 text-left ring-1 ring-live/30">
              <div className="live-sweep absolute inset-x-0 top-0 h-0.5" />
              <div className="flex items-center gap-2">
                <LiveDot tone="live" size={7} />
                <span className="text-[10px] font-semibold uppercase tracking-wider text-live">{e.state === 'queued' ? 'queued' : 'running'}</span>
                <span className="min-w-0 flex-1 truncate text-[13px] font-medium text-ink">{title(e.session_id)}</span>
                <span className="num text-[13px] font-semibold text-live">{ms(now - e.updated_at_ms)}</span>
              </div>
              <div className="num mt-1 flex gap-3 text-[11px] text-ink-faint">
                <span>turn {e.turns}</span>
                <span>spent {usd(e.budget.spent_usd)}</span>
                {e.budget.reserved_usd > 0 && <span className="text-wait">reserved {usd(e.budget.reserved_usd)}</span>}
              </div>
              {flying.length > 0 && (
                <div className="mt-2 flex flex-wrap gap-1.5">
                  {flying.map((a) => (
                    <span key={a.correlation_id} className="num inline-flex items-center gap-1 rounded-md bg-tool/10 px-1.5 py-0.5 text-[11px] text-tool ring-1 ring-tool/30">
                      <Wrench size={10} /> {a.tool} <span className="text-ink-faint">{a.state} · {ms(now - a.planned_at_ms)}</span>
                    </span>
                  ))}
                </div>
              )}
              {line && (
                <div className="mt-2 flex items-start gap-1.5 text-[12px]">
                  <Radio size={12} className="mt-0.5 shrink-0" style={{ color: toneHex[partTone[line.part] ?? 'idle'] }} />
                  <span className="text-ink-dim">{line.text}</span>
                </div>
              )}
            </motion.button>
          )
        })}
      </AnimatePresence>
    </div>
  )
}
