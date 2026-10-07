// One waiting question, answered where it is read: the Actions view lists every session's, and a session's deck lists
// its own above the composer (as the Observatory's transcript did, theseus-vm3n.6). A tool call asks to be approved,
// approved and its session trusted again, or declined, with a note the model sees on a decline; a session at its spend
// limit asks whether its spend may go back to $0. A held post (M4 19c) is the boundaries' to answer, not this card's.
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { motion } from 'motion/react'
import { useQueryClient } from '@tanstack/react-query'
import { CircleCheck, OctagonX, ShieldCheck, Siren, Zap } from 'lucide-react'
import type { ConfirmRequest } from '@protocol'
import { call } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { decisionWords, diffLines, previewOf } from '@/lib/toolwords'
import { ago, cn, ms, short, usd } from '@/lib/format'
import { changeQuestion } from '@/lib/taskgraph'
import { JsonView } from './JsonView'
import { Btn, Pill } from './ui'

/** The question of a reply the outbox held (M4 19c): theseus-protocol's `HELD_POST_TOOL`. */
export const HELD_POST_TOOL = 'label.release'

/** A protocol call that acts, with its own busy key, confirmed first when it says so, and the page's reads read again.
 *  It answers true once the daemon has done it. */
export function useAct() {
  const qc = useQueryClient()
  const [busy, setBusy] = useState<string | null>(null)
  const run = async (key: string, method: string, params: unknown, ask?: string): Promise<boolean> => {
    if (ask && !window.confirm(ask)) return false
    setBusy(key)
    try { await call(method, params); await qc.invalidateQueries(); return true } catch (e: any) { window.alert(e?.message ?? String(e)); return false } finally { setBusy(null) }
  }
  return { busy, run }
}

/** What an approval would do, as the Observatory showed it: an edit as a diff, a write as its text, a command as typed. */
function Preview({ tool, input, here }: { tool: string; input: unknown; here?: boolean }) {
  const p = previewOf(tool, input)
  if (!p) return input !== undefined && input !== null ? <JsonView value={input} maxHeight="180px" /> : null
  return (
    <div>
      {p.caption && <div className="num mb-1 text-[11px] text-ink-faint">{p.caption}</div>}
      <pre className={cn("overflow-auto whitespace-pre-wrap rounded-md bg-black/30 p-2.5 font-mono text-[11.5px] text-ink-dim ring-1 ring-line", here ? "max-h-28" : "max-h-60")}>
        {p.kind === 'diff'
          ? diffLines(p.text).map((l, i) => (
            <span key={i} className={cn(l.kind === 'add' && 'text-ok', l.kind === 'del' && 'text-fault', l.kind === 'hunk' && 'text-live', l.kind === 'meta' && 'text-ink-faint')}>{l.line}{'\n'}</span>
          ))
          : p.text}
      </pre>
    </div>
  )
}

/** How long a question still holds, in words; none for a budget question, which holds until it is answered. */
const lowerFirst = (t: string) => t.charAt(0).toLowerCase() + t.slice(1)

/** Dollars to the first figure that is not zero: a task's $0.00001 limit reads "$0.00001", never "$0.0000". */
const dollars = (n: number) => usd(n, n > 0 && n < 0.0001 ? Math.min(8, Math.ceil(-Math.log10(n) - 1e-9)) : undefined)

const holds = (left: number) => (left >= 3_600_000 ? `${Math.round(left / 3_600_000)} h` : left >= 60_000 ? `${Math.round(left / 60_000)} min` : `${Math.round(left / 1000)} s`)

export function ConfirmCard({ c, past, here }: { c: ConfirmRequest; past?: number; /** inside its own session's deck: no link to it */ here?: boolean }) {
  const nav = useNavigate()
  const tick = useTick(1000)
  const now = past ?? tick
  const { busy, run } = useAct()
  const [note, setNote] = useState('')
  const left = c.expires_at_ms ? c.expires_at_ms - now : null
  const span = c.expires_at_ms && c.requested_at_ms ? c.expires_at_ms - c.requested_at_ms : 0
  const budget = c.budget
  const answer = (key: string, approve: boolean, trust = false) =>
    run(key, 'action.confirm', { correlation_id: c.correlation_id, approve, trust: trust || undefined, note: (!budget && note) || undefined })
  // The card leads with its decision (theseus-hnof.5): what pressing the first button does, in the daemon's own words.
  // It was the fourth line, after the tool, the policy's sentence and the path.
  const d = budget || c.change ? null : decisionWords(c.tool, c.input, c.reason)
  const verb = budget ? 'Reset to $0 and continue' : c.change ? 'Accept' : 'Approve'
  const what = budget
    ? `spent ${dollars(budget.spent_usd)} of its ${dollars(budget.limit_usd)} limit; the waiting call needs ${dollars(budget.needed_usd)} more`
    : c.change ? lowerFirst(changeQuestion(c.change).split('? ')[0]) : d!.what
  return (
    <motion.div layout initial={{ opacity: 0, y: -8, scale: 0.98 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, x: 40 }}
      className="mb-3 overflow-hidden rounded-xl bg-wait/[0.05] ring-1 ring-wait/30">
      {span > 0 && left !== null && <div className="h-0.5 bg-wait/20"><div className="h-full bg-wait transition-[width] duration-1000" style={{ width: `${Math.max(0, Math.min(100, (left / span) * 100))}%` }} /></div>}
      <div className="flex items-start gap-3 p-3">
        <Zap size={16} className="mt-1 shrink-0 text-wait" />
        <div className="min-w-0 flex-1">
          <div className="flex items-start gap-3">
            <div className="min-w-0 flex-1 text-[15px] font-semibold leading-snug text-ink" title={c.reason}>
              <span className="text-wait">{verb}:</span>{' '}<span className={cn(d && 'num text-[14px]')}>{what}</span>
            </div>
            {left !== null && c.expires_at_ms ? <span className="num shrink-0 pt-0.5 text-[11.5px] text-ink-dim">{left > 0 ? `${ms(left)} left` : 'expired'}</span> : null}
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-2">
            <span className="num text-[12px] font-semibold text-tool">{budget ? 'budget' : c.tool}</span>
            {budget && <span className="text-[12px] text-ink-dim">this session reached its spend limit</span>}
            {c.floor && <Pill tone="fault" title="the call touches Theseus's own state or secrets; it always asks">floor</Pill>}
            {c.external_text && <Pill tone="wait"><Siren size={11} /> after external text</Pill>}
            {c.task && <Pill tone="tool">task</Pill>}
            {!here && <button onClick={() => nav(`/session/${c.session_id}`)} className="num text-[11px] text-ink-faint hover:text-live">{short(c.session_id)}</button>}
            <span className="num text-[11px] text-ink-faint">asked {ago(c.requested_at_ms, now)}</span>
          </div>
          {c.change && <div className="mt-1.5 text-[12.5px] font-semibold text-wait">{changeQuestion(c.change)}</div>}
          {!budget && <div className="mt-2"><Preview tool={c.tool} input={c.input} here={here} /></div>}
          <div className="mt-2 text-[12px] text-ink-dim">{d && d.why !== c.reason ? <><span className="text-ink-faint">why it asks: </span>{d.why}</> : c.reason}</div>
          {c.resource && <div className="num mt-0.5 text-[11.5px] text-ink-faint">{c.resource}</div>}
          {budget && (
            <div className="mt-1 text-[12px]">
              <div className="num text-money">spent {dollars(budget.spent_usd)} of {dollars(budget.limit_usd)}; needs {dollars(budget.needed_usd)} more · lifetime {dollars(budget.lifetime_usd)}</div>
              <div className="mt-0.5 text-[11.5px] text-ink-faint">
                Approve resets its spend to $0, and the waiting call goes on (it reserves {usd(budget.needed_usd)}). The session&rsquo;s lifetime cost,
                {' '}{usd(budget.lifetime_usd)}, keeps counting. Decline, or send a new message, and it keeps waiting.
              </div>
            </div>
          )}
          {c.external_text && <div className="num mt-1 text-[11.5px] text-wait">this session read {c.external_text.tool} {c.external_text.url} {ago(c.external_text.since_ms, now)}, so a call that acts waits</div>}
          {past !== undefined && <div className="mt-2 text-[11px] text-ink-faint">asked by {c.by}; the log shows how it was answered after this moment</div>}
          {past === undefined && <div className="mt-2.5 flex flex-wrap items-center gap-2">
            <Btn tone="ok" busy={busy === 'yes'} onClick={() => answer('yes', true)}><CircleCheck size={13} /> {budget ? 'Reset to $0 and continue' : c.change ? 'Accept' : 'Approve'}</Btn>
            {c.external_text && <Btn tone="wait" busy={busy === 'trust'} title="Approve this call, and trust the session again: its later calls that act run at their postures, until it reads external text again" onClick={() => answer('trust', true, true)}><ShieldCheck size={13} /> Approve and trust session</Btn>}
            <Btn tone="fault" busy={busy === 'no'} onClick={() => answer('no', false)}><OctagonX size={13} /> {budget ? 'Keep waiting' : 'Decline'}</Btn>
            {!budget && <input value={note} onChange={(e) => setNote(e.target.value)} placeholder="note (optional; the model sees it on a decline)"
              className="min-w-40 flex-1 rounded-md bg-white/5 px-2.5 py-1.5 text-[12px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40" />}
            <span className="text-[11px] text-ink-faint">by {c.by}</span>
          </div>}
          {past === undefined && !budget && (
            <div className="num mt-1.5 text-[10.5px] text-ink-faint">{left !== null && c.expires_at_ms ? `expires in ${holds(Math.max(0, left))} · ` : ''}bound to exactly these arguments · {c.correlation_id}</div>
          )}
        </div>
      </div>
    </motion.div>
  )
}
