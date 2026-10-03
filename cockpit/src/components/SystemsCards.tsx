// The Systems view's cards that the Observatory's Kernel, Startup, and Discord sections held and the cockpit's did not
// (theseus-vm3n.6): the push, the AWS accounts, the store and the last crash, what each startup phase found, the Discord
// binding's recent traffic, and the approval channels' recent rows.
import { Link } from 'react-router'
import { Cloud, Rocket, ScrollText } from 'lucide-react'
import type { AwsStatus, Health, LedgerEntry, StartupPhase } from '@protocol'
import { ago, clock, us } from '@/lib/format'
import { summarize } from '@/lib/summary'
import { phaseOutcome, startFigures, storeLines } from '@/lib/startupwords'
import { Empty, Field, Panel, Pill } from './ui'

type Tone = 'ok' | 'wait' | 'fault' | 'idle'

/** The push (theseus-in3): who watches, what is parked, the board, the events, and what was dropped. */
export function PushFields({ push }: { push: Health['push'] }) {
  if (!push) return null
  return (
    <div className="mt-2" title="each execution's change goes to every watcher; the board is built at the first watch, off the start path">
      <div className="panel-title mb-1">push</div>
      {push.seeded ? <>
        <Field label="watchers · waits parked" mono>{push.watchers} · {push.waiting ?? 0}</Field>
        <Field label="board · questions" mono>{push.board.toLocaleString()} · {push.questions}</Field>
        <Field label="events · lost" mono>{push.events.toLocaleString()} · <span className={push.lost ? 'text-wait' : ''}>{(push.lost ?? 0).toLocaleString()}</span></Field>
        <Field label="seeded in" mono>{us(push.seed_us ?? 0)}</Field>
      </> : <div className="text-[12px] text-ink-faint">not seeded: nothing has watched since the start</div>}
    </div>
  )
}

const AWS_STATE: Record<string, [string, Tone]> = {
  bound: ['bound', 'ok'], failed: ['not bound: its calls fail closed', 'fault'], waiting: ['waiting for its key', 'wait'],
  checking: ['checking its key', 'idle'], unchecked: ['not checked yet', 'idle'],
}

/** Each AWS account the config binds: whether its key is bound, who STS says it is, its regions and its requests. */
export function AwsCard({ aws, now }: { aws?: AwsStatus; now: number }) {
  if (!aws) return null
  return (
    <Panel title="AWS accounts" icon={<Cloud size={13} />} bodyClassName="px-3.5 py-2.5">
      {aws.accounts.length === 0 && <Empty>no account bound</Empty>}
      {aws.accounts.map((a) => {
        const [words, tone] = AWS_STATE[a.state] ?? [a.state, 'wait']
        const more = a.regions.filter((r) => r !== a.region)
        return (
          <div key={a.account} className="mb-2 border-b border-line/50 pb-2 last:mb-0 last:border-0 last:pb-0">
            <div className="flex items-center gap-2"><span className="num text-[13px] text-ink">{a.account}</span><Pill tone={tone}>{words}</Pill></div>
            {a.arn && <Field label="as" mono>{a.arn}</Field>}
            {a.checked_at_unix_ms !== undefined && <Field label="checked" mono>{ago(a.checked_at_unix_ms, now)}</Field>}
            {a.error && <Field label="error"><span className={a.state === 'failed' ? 'text-fault' : 'text-wait'}>{a.error}</span></Field>}
            <Field label="region" mono>{a.region}{more.length > 0 ? ` (a call may name ${more.join(', ')})` : ''}</Field>
            <Field label="requests" mono>{a.calls.toLocaleString()}{a.failed > 0 && <span className="text-wait"> ({a.failed.toLocaleString()} failed)</span>}</Field>
          </div>
        )
      })}
      <div className="mt-1 text-[11px] text-ink-faint">An account&rsquo;s key is checked after serving: until STS names this account for it, no call of the account signs.</div>
    </Panel>
  )
}

/** The store and the last crash, said as loudly as `theseus health` says them. */
export function StoreCard({ health }: { health: Health }) {
  const lines = storeLines(health.startup ?? [], health.store, health.crash)
  return (
    <Panel title="Store · last crash" icon={<ScrollText size={13} />} bodyClassName="px-3.5 py-2.5">
      {lines.length === 0 && <div className="text-[12px] text-ink-dim">The history check found no corrupt frame, and no crash file was found.</div>}
      {lines.map((l, i) => <div key={i} className={`mb-1 text-[12px] ${l.tone === 'fault' ? 'text-fault' : 'text-ink-faint'}`}>{l.text}</div>)}
    </Panel>
  )
}

/** The last start, phase by phase: when each began, how long it took, and what it found; and how much of the start path
 *  no phase names (a slow start with an unnamed gap has an unnamed cause). */
export function PhasesCard({ phases }: { phases: StartupPhase[] }) {
  if (!phases.length) return null
  const f = startFigures(phases)
  return (
    <Panel title="Start · what each phase found" icon={<Rocket size={13} />} bodyClassName="px-3.5 py-2.5">
      <Field label="serving" mono>{us(f.serving)} after the process started</Field>
      {f.between >= 500 && <Field label="between phases" mono><span className={f.slow ? 'text-wait' : ''}>{us(f.between)} that no phase names</span></Field>}
      <table className="mt-1.5 w-full text-[11.5px]">
        <thead className="text-[10px] uppercase tracking-wider text-ink-faint"><tr><th className="py-1 text-left">phase</th><th className="px-1 text-right">began</th><th className="px-1 text-right">took</th><th className="pl-2 text-left">found</th></tr></thead>
        <tbody>
          {phases.map((p) => (
            <tr key={`${p.name}-${p.start_us}`} className="border-t border-line/50 align-top">
              <td className="num py-1 text-ink">{p.name}{p.background && <span className="text-ink-faint"> after serving</span>}</td>
              <td className="num whitespace-nowrap px-1 text-right text-ink-faint">{us(p.start_us)}</td>
              <td className="num whitespace-nowrap px-1 text-right text-ink-dim">{p.end_us == null ? <span className="text-wait">running</span> : us(p.end_us - p.start_us)}</td>
              <td className="break-words pl-2 text-ink-faint">{phaseOutcome(p)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </Panel>
  )
}

/** The recent rows of a family, each in words. */
export function RecentRows({ rows, empty, toneOf }: { rows: LedgerEntry[]; empty: string; toneOf?: (r: LedgerEntry) => Tone }) {
  if (!rows.length) return <div className="text-[12px] text-ink-faint">{empty}</div>
  const cls: Record<Tone, string> = { ok: 'text-ink-dim', idle: 'text-ink-dim', wait: 'text-wait', fault: 'text-fault' }
  return (
    <div className="mt-1">
      {rows.map((r) => (
        <div key={r.position} className="flex items-baseline gap-2 border-t border-line/40 py-0.5 text-[11.5px]" title={JSON.stringify(r.data, null, 2)}>
          <span className="num shrink-0 text-ink-faint">{clock(r.at_unix_ms)}</span>
          <span className={`num shrink-0 ${cls[toneOf?.(r) ?? 'idle']}`}>{r.kind.replace(/^(discord|approval)\./, '')}</span>
          <span className="min-w-0 break-words text-ink-dim">{summarize(r)}</span>
        </div>
      ))}
      <Link to="/ledger" className="mt-1 block text-[11px] text-live hover:underline">the whole ledger →</Link>
    </div>
  )
}
