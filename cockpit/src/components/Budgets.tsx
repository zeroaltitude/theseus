// The Budgets panel (M7 42b, in Money): `budget.list`'s rows, each session's tasks under it with their carves, where each
// limit comes from, what is held, left, and spent over its lifetime, the burn per hour from the history's
// `provider.call` rows, the recent resets, the totals and their rule, the judge's day, and the AWS hands. The budget
// questions waiting (answered with the Actions view's own card) are `BudgetQuestions`, which Money shows above its river
// while one waits (theseus-v6vc); the panel points up to them. It shows the present: under the time machine it says so,
// and the questions' buttons are off.
import { useMemo } from 'react'
import { useNavigate, useSearchParams } from 'react-router'
import { AlertTriangle, ArrowUp, Landmark, RotateCcw } from 'lucide-react'
import type { AwsHandsStatus, BudgetListResult, BudgetRow, ConfirmRequest, DayCeilingBudget, Health, LedgerEntry } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { providerCalls } from '@/lib/derive'
import { BURN_WINDOW_MS, burnPerHour, dayCeilingView, flatten, handsLines, limitWords, questionsWaiting, recentResets, sessionsOf, sums } from '@/lib/budgets'
import { ago, cn, short, stamp, usd } from '@/lib/format'
import { Empty, Panel, Pill } from '@/components/ui'
import { ConfirmCard } from '@/components/ConfirmCard'

export function Budgets({ rows, past }: { rows: LedgerEntry[]; /** the time machine's moment: this panel shows the present */ past: number | null }) {
  const nav = useNavigate()
  const tick = useTick(10_000)
  const [params, setParams] = useSearchParams()
  // The row opened (?budget=<session id>) lives in the address.
  const open = params.get('budget')
  const setOpen = (sid: string | null) => setParams((p) => { if (sid) p.set('budget', sid); else p.delete('budget'); return p }, { replace: true })
  const { data } = useRpc<BudgetListResult>('budget.list', undefined, 0)
  const { data: h } = useRpc<Health>('health', undefined, 10_000)
  const calls = useMemo(() => providerCalls(rows), [rows])
  const resets = useMemo(() => recentResets(rows), [rows])
  const tree = useMemo(() => data?.executions ?? [], [data])
  const flat = useMemo(() => flatten(tree), [tree])
  const total = useMemo(() => sums(tree), [tree])
  const burnOf = useMemo(() => new Map(flat.map((f) => [f.row.session_id, burnPerHour(calls, new Set([f.row.session_id]), tick)])), [flat, calls, tick])
  const treeBurn = (r: BudgetRow) => burnPerHour(calls, sessionsOf(r), tick)
  const allBurn = useMemo(() => burnPerHour(calls, new Set(flat.map((f) => f.row.session_id)), tick), [calls, flat, tick])
  const hands = (h?.aws?.accounts ?? []).flatMap((a) => (a.hands ? [{ account: a.account, hands: a.hands }] : []))
  const waiting = useMemo(() => questionsWaiting(tree), [tree])

  return (
    <Panel title={<>Budgets · limits, spend, burn, and the questions waiting</>} icon={<Landmark size={13} />} bodyClassName="p-3"
      actions={past !== null ? <Pill tone="wait" title="the daemon's budgets as they are now, not as they stood at the moment">shows the present · its acts are off</Pill> : null}>
      {!data ? <Empty>reading the budgets…</Empty> : (
        <div className="flex flex-col gap-3">
          {!!waiting.length && (
            <button type="button" onClick={() => document.getElementById(QUESTIONS_ID)?.scrollIntoView({ behavior: 'smooth', block: 'start' })}
              className="flex items-center gap-2 rounded-lg bg-wait/[0.05] px-3 py-2 text-left text-[12.5px] text-ink ring-1 ring-wait/30 hover:bg-wait/[0.09]"
              title="The budget questions waiting are at the top of Money, above the river, while any waits">
              <ArrowUp size={14} className="text-wait" />
              {waiting.length === 1 ? 'A budget question waits at its limit' : `${waiting.length} budget questions wait at their limits`}: {waiting.length === 1 ? 'its card is' : 'their cards are'} above the river
            </button>
          )}

          <div className="grid grid-cols-2 gap-x-6 gap-y-2 rounded-lg bg-white/[0.02] px-3 py-2.5 ring-1 ring-line md:grid-cols-4 xl:grid-cols-8" data-totals>
            <Fig label="open" value={`${data.totals.executions} · ${data.totals.tasks} tasks`} />
            <Fig label="limits" value={usd(data.totals.limit_usd)} />
            <Fig label="spent" value={usd(data.totals.spent_usd)} tone="money" />
            <Fig label="held" value={usd(data.totals.reserved_usd)} tone="wait" />
            <Fig label="held unknown" value={usd(data.totals.held_unknown_usd)} tone={data.totals.held_unknown_usd > 0 ? 'wait' : undefined} />
            <Fig label="available" value={usd(data.totals.available_usd)} tone="ok" />
            <Fig label="lifetime" value={usd(data.totals.lifetime_usd)} />
            <Fig label="burn · per hour" value={usd(allBurn)} tone="live" />
            <div className="col-span-full text-[11px] text-ink-faint">
              The money figures add the top rows only: a task’s spend is its parent’s too, and its carve is its parent’s reservation. The lifetime adds every
              session’s own, since each counts its own. Burn is the last {BURN_WINDOW_MS / 3_600_000 === 1 ? 'hour' : 'window'}’s model calls, scaled to an hour.
              {Math.abs(total.spent - data.totals.spent_usd) > 0.0005 && <span className="text-wait"> The rows here add to {usd(total.spent)}.</span>}
            </div>
          </div>

          {!flat.length && <Empty>no execution is open</Empty>}
          <div className="flex flex-col gap-1.5">
            {flat.map(({ row: r, depth }) => (
              <BudgetLine key={r.execution_id} r={r} depth={depth} burn={depth === 0 && r.tasks?.length ? treeBurn(r) : burnOf.get(r.session_id) ?? 0}
                withTasks={depth === 0 && !!r.tasks?.length} open={open === r.session_id} onOpen={() => setOpen(open === r.session_id ? null : r.session_id)}
                onSession={() => nav(`/session/${r.session_id}`)} now={tick} />
            ))}
          </div>

          <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
            <div>
              <div className="ship-engraved mb-1 text-[9.5px]">Recent resets</div>
              {!resets.length && <div className="text-[12px] text-ink-faint">no spend has been reset to $0</div>}
              {resets.map((r) => (
                <div key={`${r.at}${r.session}`} className="num flex items-baseline gap-2 py-[1px] text-[11.5px]">
                  <RotateCcw size={11} className="text-ink-faint" />
                  <span className="w-28 shrink-0 text-ink-faint">{stamp(r.at)}</span>
                  <button type="button" onClick={() => nav(`/session/${r.session}`)} className="text-ink-dim hover:text-live">{short(r.session)}</button>
                  <span className="text-ink-faint">was {usd(r.before)}, by {r.by}</span>
                </div>
              ))}
            </div>
            <div className="flex flex-col gap-2">
              <div>
                <div className="ship-engraved mb-1 text-[9.5px]">Outside the executions · the judge’s day</div>
                {data.judge
                  ? <div className="num text-[12px] text-ink-dim">
                      {data.judge.enabled ? `${data.judge.day}: ${usd(data.judge.spent_usd)} of ${usd(data.judge.limit_usd)}` : 'the judge’s shadow is off'}
                      {data.judge.paused && <span className="ml-2 text-wait">paused at its limit: today’s judgments are skipped</span>}
                    </div>
                  : <div className="text-[12px] text-ink-faint">this daemon reports no judge budget</div>}
              </div>
              <div>
                <div className="ship-engraved mb-1 text-[9.5px]">The whole daemon · the day ceiling</div>
                {data.day_ceiling
                  ? <DayCeilingBlock d={data.day_ceiling} />
                  : <div className="text-[12px] text-ink-faint">this daemon reports no day ceiling</div>}
              </div>
              <div>
                <div className="ship-engraved mb-1 text-[9.5px]">The AWS hands</div>
                {!hands.length && <div className="text-[12px] text-ink-faint">no account has hands</div>}
                {hands.map(({ account, hands: a }) => <HandsBlock key={account} account={account} a={a} />)}
              </div>
            </div>
          </div>
        </div>
      )}
    </Panel>
  )
}

function HandsBlock({ account, a }: { account: string; a: AwsHandsStatus }) {
  const now = useTick(30_000)
  return (
    <div className="mb-1">
      <div className="num text-[10.5px] text-ink-faint">{account}</div>
      {handsLines(a, now).map((l, i) => <div key={i} className={cn('num text-[12px]', l.tone === 'fault' ? 'text-fault' : l.tone === 'wait' ? 'text-wait' : 'text-ink-dim')}>{l.text}</div>)}
    </div>
  )
}

function Fig({ label, value, tone }: { label: string; value: string; tone?: 'money' | 'wait' | 'ok' | 'live' }) {
  return (
    <div className="leading-tight">
      <div className="ship-engraved text-[9px]">{label}</div>
      <div className={cn('num text-[14px] font-semibold', tone === 'money' ? 'text-money' : tone === 'wait' ? 'text-wait' : tone === 'ok' ? 'text-ok' : tone === 'live' ? 'text-live' : 'text-ink')}>{value}</div>
    </div>
  )
}

/** One execution: spent in gold, held as an amber pool, then the figures; a task is indented under its session. */
function BudgetLine({ r, depth, burn, withTasks, open, onOpen, onSession, now }: {
  r: BudgetRow; depth: number; burn: number; withTasks: boolean; open: boolean; onOpen: () => void; onSession: () => void; now: number
}) {
  const lim = Math.max(r.limit_usd, 1e-9)
  const spentF = Math.min(1, r.spent_usd / lim)
  const heldF = Math.min(1 - spentF, r.reserved_usd / lim)
  return (
    <div className={cn('rounded-md px-2 py-1.5 ring-1 ring-line/60', open && 'bg-gold/[0.05]')} style={{ marginLeft: depth * 22 }} data-budget={r.session_id}>
      <div className="flex items-baseline gap-2 text-[12.5px]">
        <button type="button" onClick={onSession} className="min-w-0 truncate text-left text-ink hover:text-live">{r.title || short(r.session_id)}</button>
        <Pill tone={r.kind === 'task' ? 'tool' : 'idle'}>{r.kind}</Pill>
        <span className="num text-[10.5px] text-ink-faint">{r.state}</span>
        {r.question && <Pill tone="wait">asks to reset</Pill>}
        {r.mode === 'notify' && r.spent_usd >= r.limit_usd && (
          <Pill tone="wait" title="past its limit, it goes on: the limit notifies (theseus-usei); Stop on its deck ends the work">past its limit · goes on</Pill>
        )}
        <button type="button" onClick={onOpen} className="num ml-auto text-[11.5px] text-ink-dim hover:text-live" title="the figures">{usd(r.spent_usd)} of {usd(r.limit_usd)}</button>
      </div>
      <div className="relative mt-1 h-2 overflow-hidden rounded-full bg-black/45 shadow-[inset_0_0_0_1px_rgba(176,141,87,0.28)]" role="img"
        aria-label={`spent ${usd(r.spent_usd)}, held ${usd(r.reserved_usd)}, limit ${usd(r.limit_usd)}`}>
        <div className="absolute inset-y-0 left-0 rounded-l-full" style={{ width: `${spentF * 100}%`, background: 'linear-gradient(90deg,#9c6f23,#d6a548)' }} />
        {heldF > 0 && <div className="absolute inset-y-0" style={{ left: `${spentF * 100}%`, width: `${Math.max(heldF * 100, 1.5)}%`, background: 'repeating-linear-gradient(135deg, rgba(251,191,36,0.85) 0 3px, rgba(251,191,36,0.35) 3px 6px)' }} />}
      </div>
      <div className="num mt-0.5 flex flex-wrap gap-x-4 text-[10.5px] text-ink-faint">
        <span>{usd(r.available_usd)} available</span>
        {r.reserved_usd > 0 && <span className="text-wait">{usd(r.reserved_usd)} held</span>}
        {r.held_unknown_usd > 0 && <span className="text-wait" title="calls whose outcome is unknown still hold their reservation">{usd(r.held_unknown_usd)} held unknown</span>}
        <span title="the session's lifetime cost, which no reset lowers">lifetime {usd(r.lifetime_usd)}</span>
        <span className="text-live" title={withTasks ? 'its own and its tasks’ calls over the last hour, scaled to an hour' : 'the last hour’s calls, scaled to an hour'}>{usd(burn)}/h</span>
        <span>limit: {limitWords(r)}</span>
        {r.mode && <span title="what reaching the limit does: a notice at it and each multiple, or the budget question">at the limit: {r.mode === 'ask' ? 'asks' : 'notifies'}</span>}
        {r.parent && r.carve_held_usd !== undefined && <span>its parent holds {usd(r.carve_held_usd)} for it</span>}
        {r.resets > 0 && <span>{r.resets} reset{r.resets === 1 ? '' : 's'}</span>}
      </div>
      {open && (
        <div className="num mt-1 flex flex-col gap-0.5 text-[11px] text-ink-dim">
          <div>execution {r.execution_id} · session {r.session_id}</div>
          {r.last_reset && <div>last reset {ago(r.last_reset.at_ms, now)} ({stamp(r.last_reset.at_ms)}) by {r.last_reset.by}; the spend was {usd(r.last_reset.spent_before_usd)}</div>}
          {r.last_reset_unread && <div className="text-ink-faint">its last reset was not read: {r.last_reset_unread}</div>}
          {r.question && <div>waiting to reset: needs {usd(r.question.needs_usd)} more · {r.question.correlation_id}</div>}
        </div>
      )}
    </div>
  )
}

/** Where Money's budget questions are: the panel's pointer scrolls there. */
const QUESTIONS_ID = 'budget-questions'

/** The budget questions waiting at a limit, each with the Actions view's own card (reset to $0 and continue, or keep
 *  waiting): Money shows them above its river while any waits, so a decision that waits for the operator is the first
 *  thing on the page at 1080 px (theseus-v6vc); with none waiting, nothing. Under the time machine they show the
 *  present, and their buttons are off. */
export function BudgetQuestions({ past }: { past: number | null }) {
  const { data } = useRpc<BudgetListResult>('budget.list', undefined, 0)
  const { data: cl } = useRpc<{ confirms: ConfirmRequest[] }>('confirm.list', undefined, 0)
  const waiting = useMemo(() => questionsWaiting(data?.executions ?? []), [data])
  const asks = useMemo(() => new Map((cl?.confirms ?? []).map((c) => [c.correlation_id, c])), [cl])
  if (!waiting.length) return null
  return (
    <Panel title={waiting.length === 1 ? 'A budget question waits for you · its session holds at its limit until you answer'
      : `${waiting.length} budget questions wait for you · their sessions hold at their limits until you answer`}
      icon={<AlertTriangle size={13} className="text-wait" />} className="ring-1 ring-wait/40" bodyClassName="p-3"
      actions={past !== null ? <Pill tone="wait" title="the questions waiting now, not as they stood at the moment">shows the present · its acts are off</Pill> : null}>
      <div id={QUESTIONS_ID} className="flex scroll-mt-3 flex-col gap-2">
        {waiting.map(({ row }) => {
          const q = row.question!
          const card = asks.get(q.correlation_id)
          return card && past === null
            ? <ConfirmCard key={q.correlation_id} c={card} />
            : (
              <div key={q.correlation_id} className="flex items-center gap-2 rounded-lg bg-wait/[0.05] px-3 py-2 text-[12.5px] ring-1 ring-wait/30">
                <AlertTriangle size={14} className="text-wait" />
                <span className="text-ink">{row.title || short(row.session_id)} waits at its limit: {usd(row.spent_usd)} of {usd(row.limit_usd)}, needs {usd(q.needs_usd)} more</span>
                <span className="num ml-auto text-[11px] text-ink-faint">{past !== null ? 'answer it from the present' : 'its card is on its way'}</span>
              </div>
            )
        })}
      </div>
    </Panel>
  )
}

/** The day ceiling (theseus-kp20): a quiet bar under it; once reached, a stop that says until when and what lifts it. */
function DayCeilingBlock({ d }: { d: DayCeilingBudget }) {
  const v = dayCeilingView(d)
  return (
    <div className="num text-[12px] text-ink-dim" title={v.detail}>
      {v.stopped
        ? <Pill tone="fault"><AlertTriangle size={11} />{v.text}</Pill>
        : <span>{v.text}</span>}
      <div className="mt-1 h-1.5 w-full overflow-hidden rounded bg-white/5" role="meter" aria-valuemin={0} aria-valuemax={1} aria-valuenow={v.share} aria-label="today’s spend against the day ceiling">
        <div className={cn('h-full', v.tone === 'fault' ? 'bg-fault' : v.tone === 'wait' ? 'bg-wait' : 'bg-ok/60')} style={{ width: `${(v.share * 100).toFixed(1)}%` }} />
      </div>
      <div className="mt-0.5 text-[11px] text-ink-faint">{v.detail}</div>
    </div>
  )
}
