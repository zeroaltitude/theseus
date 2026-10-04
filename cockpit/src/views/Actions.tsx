// Actions: where the operator acts. Waiting approvals (approve, approve and trust, decline), every tool call's
// lifecycle (planned → authorized → dispatched → settled), the tools' postures (tighten, untighten), wakes, and the
// sessions that hold external text (trust). `?execution=` narrows the calls to one execution, as picking one in the
// Observatory's table did.
import { memo, useDeferredValue, useMemo, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router'
import { AnimatePresence } from 'motion/react'
import { CircleCheck, GitFork, History, Hourglass, OctagonX, ScanSearch, ShieldCheck, Siren, Workflow, Wrench, X } from 'lucide-react'
import type { ActionInfo, ConfirmRequest, ExternalTextInfo, Health, TaskInfo, TaskListResult, Tightening, ToolList } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { useWorld } from '@/lib/world'
import { verdictWords } from '@/lib/verdict'
import { ago, cn, ms, short, stamp, usd, clock } from '@/lib/format'
import { stateTone, toneHex } from '@/lib/taxonomy'
import { Btn, Empty, Panel, Pill, StatePill } from '@/components/ui'
import { ConfirmCard, useAct } from '@/components/ConfirmCard'

export default function Actions() {
  const { data: cl } = useRpc<{ confirms: ConfirmRequest[] }>('confirm.list', undefined, 1500)
  const { data: al } = useRpc<{ actions: ActionInfo[]; total: number }>('action.list', { n: 300 }, 2000)
  const { data: h } = useRpc<Health>('health', undefined, 2000)
  const { data: tl } = useRpc<ToolList>('tool.list', undefined, 5000)
  // The time machine: the questions, calls, holds, tasks, and tightenings as they stood at its moment. Nothing in the
  // past can be acted on, so its buttons are off.
  const world = useDeferredValue(useWorld())
  const past = world?.t
  const confirms = world?.confirms ?? cl?.confirms ?? []
  const [params, setParams] = useSearchParams()
  const execution = params.get('execution')
  const clearExecution = () => setParams((p) => { p.delete('execution'); return p }, { replace: true })
  const all = world ? [...world.actions].sort((a, b) => b.planned_at_ms - a.planned_at_ms).slice(0, 300) : al?.actions ?? []
  const actions = execution ? all.filter((a) => a.execution_id === execution) : all
  return (
    <div className="flex flex-col gap-3">
      {world && (
        <div className="flex items-center gap-2 rounded-lg bg-wait/[0.06] px-3 py-2 text-[12px] text-wait ring-1 ring-wait/30">
          <History size={14} /> As of {stamp(world.t)}, folded from the ledger. Acting needs the present: return to LIVE in the ship&rsquo;s log below.
        </div>
      )}
      <div className="grid grid-cols-1 gap-3 2xl:grid-cols-[1.15fr_1fr]">
        <div className="flex min-w-0 flex-col gap-3">
          <Panel title={<>Approvals waiting · {confirms.length}</>} icon={<ShieldCheck size={13} />} bodyClassName="p-3">
            <AnimatePresence initial={false}>
              {confirms.map((c) => <ConfirmCard key={c.correlation_id} c={c} past={past} />)}
            </AnimatePresence>
            {!confirms.length && <Empty><span className="flex items-center gap-2"><CircleCheck size={15} className="text-ok" /> {past ? 'nothing waited for you then' : 'nothing waits for you'}</span></Empty>}
          </Panel>
          <Panel title={execution ? <>Tool calls · {actions.length} of execution {short(execution)}</> : <>Tool calls · {world ? world.actions.length : al?.total ?? 0} in the record</>} icon={<Workflow size={13} />} bodyClassName="max-h-[560px] overflow-auto"
            actions={execution ? <button onClick={clearExecution} className="flex items-center gap-1 text-[11px] text-live"><X size={11} /> all executions</button> : null}>
            <Lifecycle actions={actions} past={past} />
          </Panel>
        </div>
        <div className="flex min-w-0 flex-col gap-3">
          <Panel title="External text holds" icon={<Siren size={13} />} bodyClassName="p-2">
            <Holds holds={world?.holds ?? h?.external_text ?? []} past={past} />
          </Panel>
          {!world && (
            <Panel title="Wakes" icon={<Hourglass size={13} />} bodyClassName="p-2">
              <Wakes h={h} />
            </Panel>
          )}
          <Tasks past={world ? { t: world.t, tasks: world.tasks } : undefined} />

          <Panel title="Tool postures" icon={<Wrench size={13} />} bodyClassName="max-h-[520px] overflow-auto"
            actions={tl ? <span className="num text-[11px] text-ink-faint" title="proc.run (typed argv) is the only shell path; the shell-fallback ratio is proc.run calls over all calls">roots {tl.roots.join(', ')} · {tl.calls_total} call{tl.calls_total === 1 ? '' : 's'} since start · shell fallback {(tl.shell_fallback_ratio * 100).toFixed(0)}%</span> : null}>
            <Postures tl={tl} tightenings={world?.tightenings ?? h?.tightenings ?? []} past={past} />
          </Panel>
        </div>
      </div>
    </div>
  )
}

/** Task sessions: what each works on, its state and what it waits on, its spend under its carved limit, and a
 *  cancel for any still alive. The newest first; ended ones fold away. */
function Tasks({ past }: { past?: { t: number; tasks: TaskInfo[] } }) {
  const nav = useNavigate()
  const tick = useTick(5000)
  const now = past?.t ?? tick
  const { busy, run } = useAct()
  const { data: tl } = useRpc<TaskListResult>('task.list', {}, 3000)
  const [ended, setEnded] = useState(false)
  const tasks = past?.tasks ?? tl?.tasks ?? []
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
            {alive(t) && !past && <Btn tone="fault" busy={busy === t.task_id} onClick={() => run(t.task_id, 'task.cancel', { task: t.task_id }, `Cancel task ${t.short}${t.title ? ` (${t.title})` : ''}? Its jobs stop, and its place hears it was cancelled.`)}><OctagonX size={12} /> Cancel</Btn>}
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

const STEPS = ['planned', 'authorized', 'dispatched', 'settled'] as const

function Lifecycle({ actions, past }: { actions: ActionInfo[]; past?: number }) {
  const nav = useNavigate()
  const tick = useTick(2000)
  const now = past ?? tick
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
      {/* A settled call reads the same at every moment, so it takes no clock and its row draws once. */}
      {list.map((a) => <LifecycleRow key={a.correlation_id} a={a} now={a.settled_at_ms ? 0 : now} nav={nav} />)}
      {!list.length && <Empty>no tool calls</Empty>}
    </div>
  )
}

const LifecycleRow = memo(function LifecycleRow({ a, now, nav }: { a: ActionInfo; now: number; nav: (to: string) => void }) {
  const times = [a.planned_at_ms, a.authorized_at_ms, a.dispatched_at_ms, a.settled_at_ms]
  const reached = times.filter(Boolean).length
  const total = (a.settled_at_ms ?? now) - a.planned_at_ms
  const tone = stateTone(a.state)
  const overdue = !a.settled_at_ms && !['succeeded', 'failed', 'cancelled'].includes(a.state) && a.deadline_at_ms < (now || Date.now())
  // Every id and time the row stands for, as the Observatory's tooltip gave them.
  const ids = [
    a.correlation_id, `execution ${a.execution_id}`, `session ${a.session_id}`,
    ...(['planned', 'authorized', 'dispatched', 'settled'] as const).flatMap((s, i) => (times[i] ? [`${s} ${clock(times[i] as number)}`] : [])),
    `deadline ${clock(a.deadline_at_ms)}`, ...(a.external_op_id ? [`external ${a.external_op_id}`] : []), ...(a.result_ref ? [`result ${a.result_ref}`] : []),
  ].join('\n')
  return (
    <div className="border-b border-line/50 px-3 py-2 hover:bg-white/[0.02]" title={ids}>
      <div className="flex items-center gap-2 text-[12px]">
        <StatePill state={a.state} />
        <span className="num font-medium text-tool">{a.tool}</span>
        <button onClick={() => nav(`/session/${a.session_id}`)} className="num text-[10.5px] text-ink-faint hover:text-live">{short(a.session_id)}</button>
        <button onClick={() => nav(`/session/${a.session_id}?call=${a.correlation_id}`)} title="inspect this call" className="text-ink-faint hover:text-tool"><ScanSearch size={12} /></button>
        <span className="num text-[10.5px] text-ink-faint">{a.retry_class}{a.confirmed ? ' · confirmed' : ''}{a.cancel ? ` · ${a.cancel}` : ''}</span>
        {overdue && <Pill tone="wait" title="past its deadline and not settled">overdue</Pill>}
        {a.completions_seen > 1 && <Pill tone="wait" title="completions received; more than one means duplicates were ignored">seen {a.completions_seen}×</Pill>}
        {(a.resolution || (a.external_op_id && a.tool === 'provider.messages')) && <span className="num text-[10.5px] text-ink-dim">{a.resolution ?? `req ${short(a.external_op_id)}`}</span>}
        {a.verdict && <Pill tone={a.verdict.state === 'termination_verified' ? 'ok' : 'wait'} title={a.verdict.why}>{verdictWords(a.verdict)}</Pill>}
        <span className="num ml-auto text-[11px] text-ink-faint">{stamp(a.planned_at_ms)} · <span className="text-ink">{ms(total)}</span></span>
      </div>
      <div className="mt-1.5 flex items-center gap-1">
        {STEPS.map((s, i) => (
          <div key={s} className="flex flex-1 items-center gap-1">
            <div className="h-1.5 flex-1 rounded-full" style={{ background: i < reached ? toneHex[tone] : 'rgba(176,141,87,0.12)', boxShadow: i < reached ? `0 0 8px ${toneHex[tone]}55` : undefined }} />
            <span className={cn('num text-[9.5px]', i < reached ? 'text-ink-dim' : 'text-ink-faint/60')}>
              {s}{i > 0 && times[i] && times[i - 1] ? ` +${ms((times[i] as number) - (times[i - 1] as number))}` : ''}
            </span>
          </div>
        ))}
      </div>
      {!a.settled_at_ms && <div className={cn('num mt-1 text-[10.5px]', overdue ? 'text-fault' : 'text-wait')}>deadline {a.deadline_at_ms > now ? `in ${ms(a.deadline_at_ms - now)}` : `${ms(now - a.deadline_at_ms)} ago`}</div>}
      {a.reserved_usd > 0 && <div className="num mt-0.5 text-[10.5px] text-money">{a.settled_at_ms ? 'reserved' : 'holds'} {usd(a.reserved_usd)}</div>}
    </div>
  )
})

function Holds({ holds, past }: { holds: ExternalTextInfo[]; past?: number }) {
  const nav = useNavigate()
  const tick = useTick(5000)
  const now = past ?? tick
  const { busy, run } = useAct()
  if (!holds.length) return <Empty><span className="flex items-center gap-2"><ShieldCheck size={15} className="text-ok" /> every session {past ? 'was' : 'is'} trusted</span></Empty>
  return (
    <div className="flex flex-col gap-1.5">
      {holds.map((x) => (
        <div key={x.session_id} className="flex items-center gap-2 rounded-lg bg-wait/[0.04] px-3 py-2 ring-1 ring-wait/20">
          <div className="min-w-0 flex-1">
            <button onClick={() => nav(`/session/${x.session_id}`)} className="truncate text-left text-[12.5px] text-ink hover:text-live">{x.title || short(x.session_id)}{x.task ? ` · task ${x.task}` : ''}</button>
            <div className="num truncate text-[11px] text-ink-faint" title={`${x.session_id}\nnode ${x.held.node_id}${x.since_local ? `\nsince ${x.since_local}` : ''}`}>
              {x.held.tool} {x.held.query != null ? `"${x.held.query}"` : x.held.url} · {ago(x.held.since_ms, now)} · {holdHow(x.held)}
            </div>
          </div>
          {!past && <Btn tone="wait" busy={busy === x.session_id} onClick={() => run(x.session_id, 'policy.trust', { session_id: x.session_id }, `Trust ${x.title || short(x.session_id)} again? Its calls that act stop waiting.`)}><ShieldCheck size={13} /> Trust</Btn>}
        </div>
      ))}
    </div>
  )
}

/** How a session came to hold external text, in the Observatory's words. */
function holdHow(h: ExternalTextInfo['held']): string {
  return h.via === 'task.create' ? `from the session that started it (${short(h.from_session)})`
    : h.via === 'task.report' ? `from task ${short(h.from_session)}'s report`
    : h.via === 'egress' ? 'an L1 job here reached these hosts' : 'read here'
}

function Wakes({ h }: { h?: Health }) {
  const nav = useNavigate()
  const now = useTick(1000)
  const { busy, run } = useAct()
  const wakes = h?.wakes ?? []
  if (!wakes.length) return <Empty>no wakes set</Empty>
  return (
    <div className="flex flex-col gap-1.5">
      {wakes.map((w) => (
        <div key={w.wake_id} className="flex items-center gap-2 rounded-lg px-3 py-2 ring-1 ring-line">
          <Hourglass size={14} className="text-live" />
          <div className="min-w-0 flex-1" title={`${w.wake_id}\nset ${clock(w.set_at_ms)}${w.target ? `\nits reply goes to ${w.target}` : ''}`}>
            <div className="truncate text-[12.5px] text-ink">{w.note}</div>
            <div className="num text-[11px] text-ink-faint">{w.short} · <button onClick={() => nav(`/session/${w.session_id}`)} className="hover:text-live">{w.task ? `task ${w.task}` : (w.session_title ?? short(w.session_id))}</button> <StatePill state={w.state} /> · {w.due_local} · <span className={w.due_at_ms > now ? 'text-live' : 'text-wait'}>{w.due_at_ms > now ? `in ${ms(w.due_at_ms - now)}` : `due ${ms(now - w.due_at_ms)} ago`}</span></div>
          </div>
          <Btn tone="fault" busy={busy === w.wake_id} onClick={() => run(w.wake_id, 'wake.cancel', { wake: w.wake_id }, `Cancel the wake “${w.note}”?`)}><OctagonX size={13} /> Cancel</Btn>
        </div>
      ))}
    </div>
  )
}

function Postures({ tl, tightenings, past }: { tl?: ToolList; tightenings: Tightening[]; past?: number }) {
  const { busy, run } = useAct()
  const tightened = new Map(tightenings.map((t) => [t.tool, t]))
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
            <tr key={t.name} className="border-t border-line/50 hover:bg-white/[0.02]" title={`${t.description}\n\n${t.wire_name}\n${JSON.stringify(t.input_schema, null, 2)}`}>
              <td className="num px-3 py-1 text-tool">{t.name}</td>
              <td className="px-2 py-1 text-ink-faint">{t.class} · {t.backend}</td>
              <td className="px-2 py-1" title={`${t.setting}${t.config_posture !== t.policy ? `\nthe config says ${t.config_posture} (${t.config_setting})` : ''}`}>
                <Pill tone={t.policy === 'deny' ? 'fault' : t.policy === 'confirm' || t.policy === 'ask' || t.policy === 'approve' ? 'wait' : t.policy === 'notify' ? 'live' : 'ok'}>{t.policy}</Pill>
                {tight && <span className="ml-1 text-[10.5px] text-wait" title={tight.digest ? `proposal digest ${tight.digest}` : 'pressed without naming a call'}>
                  tightened by {tight.by}{tight.via ? ` via ${tight.via}` : ''} · {ago(tight.at_ms)}{tight.correlation_id ? ` · call ${short(tight.correlation_id)}` : ''}
                </span>}
              </td>
              <td className="num px-2 py-1 text-right text-ink-dim">{t.calls}</td>
              <td className="px-3 py-1 text-right">
                {past ? null : tight
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
