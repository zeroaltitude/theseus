// Actions: where the operator acts. Waiting approvals (approve, approve and trust, decline), every tool call's
// lifecycle (planned → authorized → dispatched → settled), the tools' postures (tighten, untighten), wakes, and the
// sessions that hold external text (trust).
import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { useQueryClient } from '@tanstack/react-query'
import { CircleCheck, GitFork, Hourglass, OctagonX, ScanSearch, ShieldCheck, Siren, Workflow, Wrench, Zap } from 'lucide-react'
import type { ActionInfo, ConfirmRequest, Health, TaskInfo, TaskListResult, ToolList } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { ago, cn, ms, short, stamp, usd } from '@/lib/format'
import { stateTone, toneHex } from '@/lib/taxonomy'
import { JsonView } from '@/components/JsonView'
import { Btn, Empty, Panel, Pill, StatePill } from '@/components/ui'

export default function Actions() {
  const { data: cl } = useRpc<{ confirms: ConfirmRequest[] }>('confirm.list', undefined, 1500)
  const { data: al } = useRpc<{ actions: ActionInfo[]; total: number }>('action.list', { n: 300 }, 2000)
  const { data: h } = useRpc<Health>('health', undefined, 2000)
  const { data: tl } = useRpc<ToolList>('tool.list', undefined, 5000)
  const confirms = cl?.confirms ?? []
  return (
    <div className="grid grid-cols-1 gap-3 2xl:grid-cols-[1.15fr_1fr]">
      <div className="flex min-w-0 flex-col gap-3">
        <Panel title={<>Approvals waiting · {confirms.length}</>} icon={<ShieldCheck size={13} />} bodyClassName="p-3">
          <AnimatePresence initial={false}>
            {confirms.map((c) => <ConfirmCard key={c.correlation_id} c={c} />)}
          </AnimatePresence>
          {!confirms.length && <Empty><span className="flex items-center gap-2"><CircleCheck size={15} className="text-ok" /> nothing waits for you</span></Empty>}
        </Panel>
        <Panel title={<>Tool calls · {al?.total ?? 0} in the record</>} icon={<Workflow size={13} />} bodyClassName="max-h-[560px] overflow-auto">
          <Lifecycle actions={al?.actions ?? []} />
        </Panel>
      </div>
      <div className="flex min-w-0 flex-col gap-3">
        <Panel title="External text holds" icon={<Siren size={13} />} bodyClassName="p-2">
          <Holds h={h} />
        </Panel>
        <Panel title="Wakes" icon={<Hourglass size={13} />} bodyClassName="p-2">
          <Wakes h={h} />
        </Panel>
        <Tasks />

        <Panel title="Tool postures" icon={<Wrench size={13} />} bodyClassName="max-h-[520px] overflow-auto"
          actions={tl ? <span className="num text-[11px] text-ink-faint">roots {tl.roots.join(', ')}</span> : null}>
          <Postures tl={tl} h={h} />
        </Panel>
      </div>
    </div>
  )
}

/** Task sessions: what each works on, its state and what it waits on, its spend under its carved limit, and a
 *  cancel for any still alive. The newest first; ended ones fold away. */
function Tasks() {
  const nav = useNavigate()
  const now = useTick(5000)
  const { busy, run } = useAct()
  const { data: tl } = useRpc<TaskListResult>('task.list', {}, 3000)
  const [ended, setEnded] = useState(false)
  const tasks = tl?.tasks ?? []
  // The kernel's terminal states (ExecState::is_terminal).
  const alive = (t: TaskInfo) => !['complete', 'cancelled', 'failed', 'budget_exhausted'].includes(t.state)
  const shown = tasks.filter((t) => ended || alive(t))
  return (
    <Panel title={<>Tasks · {tasks.filter(alive).length} alive{tasks.length ? ` of ${tasks.length}` : ''}</>} icon={<GitFork size={13} />} bodyClassName="max-h-[420px] overflow-auto p-2"
      actions={tasks.some((t) => !alive(t)) ? <button onClick={() => setEnded((v) => !v)} className="text-[11px] text-ink-faint hover:text-ink">{ended ? 'hide ended' : 'show ended'}</button> : null}>
      {!shown.length && <Empty>{tasks.length ? 'no task alive' : 'no tasks yet'}</Empty>}
      {shown.map((t) => (
        <div key={t.task_id} className="border-b border-line/50 px-1.5 py-2 last:border-0">
          <div className="flex items-center gap-2">
            <StatePill state={t.state} />
            {t.waiting_on && <Pill tone="wait">on {t.waiting_on}</Pill>}
            {t.pending_confirms > 0 && <Pill tone="wait">{t.pending_confirms} asks</Pill>}
            <button onClick={() => nav(`/session/${t.task_id}`)} className="min-w-0 flex-1 truncate text-left text-[12.5px] text-ink hover:text-live" title={t.title ?? t.task_id}>{t.title ?? `task ${t.short}`}</button>
            {alive(t) && <Btn tone="fault" busy={busy === t.task_id} onClick={() => run(t.task_id, 'task.cancel', { task: t.task_id }, `Cancel task ${t.short}${t.title ? ` (${t.title})` : ''}? Its jobs stop, and its place hears it was cancelled.`)}><OctagonX size={12} /> Cancel</Btn>}
          </div>
          <div className="num mt-1 flex flex-wrap gap-x-3 text-[10.5px] text-ink-faint">
            <span>{t.short}</span>
            <button onClick={() => nav(`/session/${t.parent_session_id}`)} className="hover:text-live">from {short(t.parent_session_id)}</button>
            <span className="text-money">{usd(t.spent_usd)} / {usd(t.limit_usd)}</span>
            <span>{t.turns} turns</span>
            {t.target && <span>reports to {t.target}{t.wake_parent ? ' · wakes its parent' : ''}</span>}
            <span>{ago(t.updated_at_ms, now)}</span>
            {t.ended_reason && <span className="text-ink-dim">{t.ended_reason}</span>}
          </div>
        </div>
      ))}
    </Panel>
  )
}

function useAct() {
  const qc = useQueryClient()
  const [busy, setBusy] = useState<string | null>(null)
  const run = async (key: string, method: string, params: unknown, ask?: string) => {
    if (ask && !window.confirm(ask)) return
    setBusy(key)
    try { await call(method, params); await qc.invalidateQueries() } catch (e: any) { window.alert(e?.message ?? String(e)) } finally { setBusy(null) }
  }
  return { busy, run }
}

function ConfirmCard({ c }: { c: ConfirmRequest }) {
  const nav = useNavigate()
  const now = useTick(1000)
  const { busy, run } = useAct()
  const [note, setNote] = useState('')
  const left = c.expires_at_ms ? c.expires_at_ms - now : null
  const span = c.expires_at_ms && c.requested_at_ms ? c.expires_at_ms - c.requested_at_ms : 0
  return (
    <motion.div layout initial={{ opacity: 0, y: -8, scale: 0.98 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, x: 40 }}
      className="mb-3 overflow-hidden rounded-xl bg-wait/[0.05] ring-1 ring-wait/30">
      {span > 0 && left !== null && <div className="h-0.5 bg-wait/20"><div className="h-full bg-wait transition-[width] duration-1000" style={{ width: `${Math.max(0, Math.min(100, (left / span) * 100))}%` }} /></div>}
      <div className="flex items-start gap-3 p-3">
        <Zap size={16} className="mt-0.5 text-wait" />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="num text-[14px] font-semibold text-tool">{c.tool}</span>
            {c.floor && <Pill tone="fault">floor</Pill>}
            {c.external_text && <Pill tone="wait"><Siren size={11} /> after external text</Pill>}
            <button onClick={() => nav(`/session/${c.session_id}`)} className="num text-[11px] text-ink-faint hover:text-live">{short(c.session_id)}</button>
            <span className="num ml-auto text-[11px] text-ink-faint">asked {ago(c.requested_at_ms, now)}{left !== null && c.expires_at_ms ? ` · ${left > 0 ? `${ms(left)} left` : 'expired'}` : ''}</span>
          </div>
          <div className="mt-1 text-[12.5px] text-ink">{c.reason}</div>
          {c.resource && <div className="num mt-0.5 text-[11.5px] text-ink-dim">{c.resource}</div>}
          {c.budget && (
            <div className="num mt-1 text-[12px] text-money">spent {usd(c.budget.spent_usd)} of {usd(c.budget.limit_usd)}; needs {usd(c.budget.needed_usd)} more · lifetime {usd(c.budget.lifetime_usd)}</div>
          )}
          {c.external_text && <div className="num mt-1 text-[11.5px] text-wait">read {c.external_text.tool} {c.external_text.url} {ago(c.external_text.since_ms, now)}</div>}
          {c.input !== undefined && c.input !== null && <div className="mt-2"><JsonView value={c.input} maxHeight="180px" /></div>}
          <div className="mt-2.5 flex flex-wrap items-center gap-2">
            <Btn tone="ok" busy={busy === 'yes'} onClick={() => run('yes', 'action.confirm', { correlation_id: c.correlation_id, approve: true, note: note || undefined })}><CircleCheck size={13} /> Approve</Btn>
            {c.external_text && <Btn tone="wait" busy={busy === 'trust'} onClick={() => run('trust', 'action.confirm', { correlation_id: c.correlation_id, approve: true, trust: true, note: note || undefined })}><ShieldCheck size={13} /> Approve and trust</Btn>}
            <Btn tone="fault" busy={busy === 'no'} onClick={() => run('no', 'action.confirm', { correlation_id: c.correlation_id, approve: false, note: note || undefined })}><OctagonX size={13} /> Decline</Btn>
            <input value={note} onChange={(e) => setNote(e.target.value)} placeholder="note (optional)"
              className="min-w-40 flex-1 rounded-md bg-white/5 px-2.5 py-1.5 text-[12px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40" />
            <span className="text-[11px] text-ink-faint">by {c.by}</span>
          </div>
        </div>
      </div>
    </motion.div>
  )
}

const STEPS = ['planned', 'authorized', 'dispatched', 'settled'] as const

function Lifecycle({ actions }: { actions: ActionInfo[] }) {
  const nav = useNavigate()
  const now = useTick(2000)
  const [only, setOnly] = useState<'all' | 'open' | 'failed'>('all')
  const list = useMemo(() => {
    const sorted = [...actions].sort((a, b) => b.planned_at_ms - a.planned_at_ms)
    if (only === 'open') return sorted.filter((a) => !a.settled_at_ms)
    if (only === 'failed') return sorted.filter((a) => ['failed', 'cancelled', 'denied', 'unknown'].includes(a.state))
    return sorted
  }, [actions, only])
  return (
    <div>
      <div className="sticky top-0 z-10 flex gap-1 border-b border-line bg-hull/95 px-3 py-1.5 backdrop-blur">
        {(['all', 'open', 'failed'] as const).map((o) => (
          <button key={o} onClick={() => setOnly(o)} className={cn('rounded px-2 py-0.5 text-[11px]', o === only ? 'bg-live/10 text-live' : 'text-ink-faint hover:text-ink')}>{o}</button>
        ))}
      </div>
      {list.map((a) => {
        const times = [a.planned_at_ms, a.authorized_at_ms, a.dispatched_at_ms, a.settled_at_ms]
        const reached = times.filter(Boolean).length
        const total = (a.settled_at_ms ?? now) - a.planned_at_ms
        const tone = stateTone(a.state)
        return (
          <div key={a.correlation_id} className="border-b border-line/50 px-3 py-2 hover:bg-white/[0.02]">
            <div className="flex items-center gap-2 text-[12px]">
              <StatePill state={a.state} />
              <span className="num font-medium text-tool">{a.tool}</span>
              <button onClick={() => nav(`/session/${a.session_id}`)} className="num text-[10.5px] text-ink-faint hover:text-live">{short(a.session_id)}</button>
              <button onClick={() => nav(`/session/${a.session_id}?call=${a.correlation_id}`)} title="inspect this call" className="text-ink-faint hover:text-tool"><ScanSearch size={12} /></button>
              <span className="num text-[10.5px] text-ink-faint">{a.retry_class}{a.confirmed ? ' · confirmed' : ''}{a.cancel ? ` · ${a.cancel}` : ''}</span>
              <span className="num ml-auto text-[11px] text-ink-faint">{stamp(a.planned_at_ms)} · <span className="text-ink">{ms(total)}</span></span>
            </div>
            <div className="mt-1.5 flex items-center gap-1">
              {STEPS.map((s, i) => (
                <div key={s} className="flex flex-1 items-center gap-1">
                  <div className="h-1.5 flex-1 rounded-full" style={{ background: i < reached ? toneHex[tone] : 'rgba(148,163,184,0.12)', boxShadow: i < reached ? `0 0 8px ${toneHex[tone]}55` : undefined }} />
                  <span className={cn('num text-[9.5px]', i < reached ? 'text-ink-dim' : 'text-ink-faint/60')}>
                    {s}{i > 0 && times[i] && times[i - 1] ? ` +${ms((times[i] as number) - (times[i - 1] as number))}` : ''}
                  </span>
                </div>
              ))}
            </div>
            {!a.settled_at_ms && <div className="num mt-1 text-[10.5px] text-wait">deadline {a.deadline_at_ms > now ? `in ${ms(a.deadline_at_ms - now)}` : `${ms(now - a.deadline_at_ms)} ago`}{a.reserved_usd ? ` · reserved ${usd(a.reserved_usd)}` : ''}</div>}
          </div>
        )
      })}
      {!list.length && <Empty>no tool calls</Empty>}
    </div>
  )
}

function Holds({ h }: { h?: Health }) {
  const nav = useNavigate()
  const now = useTick(5000)
  const { busy, run } = useAct()
  const holds = h?.external_text ?? []
  if (!holds.length) return <Empty><span className="flex items-center gap-2"><ShieldCheck size={15} className="text-ok" /> every session is trusted</span></Empty>
  return (
    <div className="flex flex-col gap-1.5">
      {holds.map((x) => (
        <div key={x.session_id} className="flex items-center gap-2 rounded-lg bg-wait/[0.04] px-3 py-2 ring-1 ring-wait/20">
          <div className="min-w-0 flex-1">
            <button onClick={() => nav(`/session/${x.session_id}`)} className="truncate text-left text-[12.5px] text-ink hover:text-live">{x.title || short(x.session_id)}{x.task ? ` · task ${x.task}` : ''}</button>
            <div className="num truncate text-[11px] text-ink-faint">{x.held.tool} {x.held.url} · {ago(x.held.since_ms, now)}{x.held.via ? ` · via ${x.held.via}` : ''}</div>
          </div>
          <Btn tone="wait" busy={busy === x.session_id} onClick={() => run(x.session_id, 'policy.trust', { session_id: x.session_id }, `Trust ${x.title || short(x.session_id)} again? Its calls that act stop waiting.`)}><ShieldCheck size={13} /> Trust</Btn>
        </div>
      ))}
    </div>
  )
}

function Wakes({ h }: { h?: Health }) {
  const now = useTick(1000)
  const { busy, run } = useAct()
  const wakes = h?.wakes ?? []
  if (!wakes.length) return <Empty>no wakes set</Empty>
  return (
    <div className="flex flex-col gap-1.5">
      {wakes.map((w) => (
        <div key={w.wake_id} className="flex items-center gap-2 rounded-lg px-3 py-2 ring-1 ring-line">
          <Hourglass size={14} className="text-live" />
          <div className="min-w-0 flex-1">
            <div className="truncate text-[12.5px] text-ink">{w.note}</div>
            <div className="num text-[11px] text-ink-faint">{w.short} · {w.session_title ?? short(w.session_id)} · {w.due_local} · <span className="text-live">{w.due_at_ms > now ? `in ${ms(w.due_at_ms - now)}` : 'due'}</span></div>
          </div>
          <Btn tone="fault" busy={busy === w.wake_id} onClick={() => run(w.wake_id, 'wake.cancel', { wake: w.wake_id }, `Cancel the wake “${w.note}”?`)}><OctagonX size={13} /> Cancel</Btn>
        </div>
      ))}
    </div>
  )
}

function Postures({ tl, h }: { tl?: ToolList; h?: Health }) {
  const { busy, run } = useAct()
  const tightened = new Map((h?.tightenings ?? []).map((t) => [t.tool, t]))
  if (!tl) return <Empty>reading the tools…</Empty>
  return (
    <table className="w-full whitespace-nowrap text-[12px]">
      <thead className="sticky top-0 bg-hull/95 text-[10px] uppercase tracking-wider text-ink-faint backdrop-blur">
        <tr><th className="px-3 py-1.5 text-left">tool</th><th className="px-2 py-1.5 text-left">class</th><th className="px-2 py-1.5 text-left">posture</th><th className="px-2 py-1.5 text-right">calls</th><th className="px-3 py-1.5" /></tr>
      </thead>
      <tbody>
        {tl.tools.map((t) => {
          const tight = tightened.get(t.name)
          return (
            <tr key={t.name} className="border-t border-line/50 hover:bg-white/[0.02]" title={t.description}>
              <td className="num px-3 py-1 text-tool">{t.name}</td>
              <td className="px-2 py-1 text-ink-faint">{t.class} · {t.backend}</td>
              <td className="px-2 py-1"><Pill tone={t.policy === 'deny' ? 'fault' : t.policy === 'confirm' || t.policy === 'ask' ? 'wait' : t.policy === 'notify' ? 'live' : 'ok'}>{t.policy}</Pill>{tight && <span className="ml-1 text-[10.5px] text-wait">tightened by {tight.by}</span>}</td>
              <td className="num px-2 py-1 text-right text-ink-dim">{t.calls}</td>
              <td className="px-3 py-1 text-right">
                {tight
                  ? <button className="text-[11px] text-ok hover:underline" disabled={busy === t.name} onClick={() => run(t.name, 'policy.untighten', { tool: t.name }, `Let ${t.name} go back to its configured posture (${t.config_posture})?`)}>untighten</button>
                  : <button className="text-[11px] text-wait hover:underline" disabled={busy === t.name} onClick={() => run(t.name, 'policy.tighten', { tool: t.name }, `Make ${t.name} ask first?`)}>tighten</button>}
              </td>
            </tr>
          )
        })}
      </tbody>
    </table>
  )
}
