// The needs-you queue (theseus-jid7t.3): every item waiting on the owner, the longest waiting first, a compact card
// each (its kind's glyph and word, the verb and what it does, whose it is, its age, the expiry bar); `⏎` lays the
// cursor's item out whole (`ConfirmCard`, every field), and each is answered in place, one protocol call an answer.
// Below, what is coming up in the next three hours.
import { useEffect, useRef, type ReactNode } from 'react'
import { Link } from 'react-router'
import { motion } from 'motion/react'
import { CircleCheck, Hourglass } from 'lucide-react'
import type { WorkView } from '@protocol'
import { cn, short } from '@/lib/format'
import { useCalm } from '@/lib/calm'
import { ConfirmCard } from '@/components/ConfirmCard'
import { changeWords } from '@/lib/taskgraph'
import { answerOf, grantKey, type Answer, type QueueItem } from './queue'
import type { ComingEvent } from './calendar'
import { comingTimes, ITEM_LOOK, itemFacts } from './facts'
import { useOpsAct } from './act'
import { ActBtn } from './ActBtn'
import type { ButtonAct, Verb } from './keys'

const TONE: Record<string, string> = { wait: 'text-wait', fault: 'text-fault', think: 'text-think', money: 'text-money', live: 'text-live' }

export function Queue({ items, cursor, onCursor, open, note, onNote, past, now, coming, arrivedKeys, events = [] }: {
  items: readonly QueueItem[]; cursor: string | null; onCursor: (key: string) => void; open: string | null
  /** The item whose decline (or "something else") note is open, and its words. */
  note: { key: string; text: string } | null; onNote: (n: { key: string; text: string } | null) => void
  past: number | null; now: number; coming: readonly WorkView[]; arrivedKeys: ReadonlySet<string>
  /** The calendar's events in the next three hours, redacted as the page is (ops-timeline, theseus-jid7t.5). */
  events?: readonly ComingEvent[]
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="ops-queue">
      <div className="flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto p-1.5">
        {!items.length && (
          <div className="flex flex-col items-center gap-2 px-3 py-8 text-center text-[12px] text-ink-faint">
            <CircleCheck size={18} className="text-ok" /> {past ? 'nothing waited for you then' : 'nothing waits for you'}
          </div>
        )}
        {items.map((i) => (
          <Item key={i.key} i={i} at={i.key === cursor} whole={i.key === open} onCursor={onCursor} note={note?.key === i.key ? note.text : null} onNote={onNote}
            past={past} now={now} fresh={arrivedKeys.has(i.key)} />
        ))}
      </div>
      <ComingUp views={coming} events={events} now={now} />
    </div>
  )
}

function Item({ i, at, whole, onCursor, note, onNote, past, now, fresh }: {
  i: QueueItem; at: boolean; whole: boolean; onCursor: (key: string) => void; note: string | null; onNote: (n: { key: string; text: string } | null) => void
  past: number | null; now: number; fresh: boolean
}) {
  const ref = useRef<HTMLDivElement>(null)
  const calm = useCalm((s) => s.calm)
  const f = itemFacts(i, past ?? now)
  const look = ITEM_LOOK[i.kind]
  useEffect(() => { if (at) ref.current?.scrollIntoView({ block: 'nearest', behavior: calm ? 'auto' : 'smooth' }) }, [at, calm])
  return (
    <motion.div ref={ref} layout={!calm} initial={fresh && !calm ? { opacity: 0, x: -12 } : false} animate={{ opacity: 1, x: 0 }}
      role="button" tabIndex={-1} aria-selected={at} data-item={i.key} onClick={() => onCursor(i.key)}
      className={cn('shrink-0 cursor-pointer overflow-hidden rounded-lg bg-deck/80 ring-1 transition-shadow',
        at ? 'ring-gold shadow-[0_0_16px_-2px_rgb(214_165_72_/_0.55)]' : 'ring-wait/25 shadow-[0_0_12px_-6px_var(--color-wait)] hover:ring-wait/50')}>
      {f.left !== undefined && <div className="h-0.5 bg-wait/15" title="how much of its time is left"><div className="h-full bg-wait" style={{ width: `${f.left * 100}%` }} /></div>}
      <div className="px-2.5 py-2">
        <div className="flex items-center gap-1.5 text-[10.5px]">
          <span className={cn('font-semibold', TONE[look.tone])} aria-hidden>{f.glyph}</span>
          <span className={cn('font-display font-bold uppercase tracking-[0.08em]', TONE[look.tone])}>{f.kind}</span>
          <span className="num ml-auto text-ink-faint" title="how long it has waited">{f.age}</span>
        </div>
        <div className="mt-1 line-clamp-2 text-[12.5px] font-medium leading-snug text-ink" title={f.title}>{f.title}</div>
        <div className="num mt-0.5 truncate text-[10.5px] text-ink-dim">{f.owner}{i.sessionId ? <> · <Link to={`/session/${i.sessionId}`} onClick={(e) => e.stopPropagation()} className="text-ink-faint hover:text-live">{short(i.sessionId)}</Link></> : null}</div>
        {(at || whole) && <div onClick={(e) => e.stopPropagation()}><Answers i={i} whole={whole} note={note} onNote={onNote} past={past} /></div>}
      </div>
    </motion.div>
  )
}

/** The answers an item takes, in place: the full card when laid out whole, else its buttons. */
export function Answers({ i, whole, note, onNote, past }: { i: QueueItem; whole: boolean; note: string | null; onNote: (n: { key: string; text: string } | null) => void; past: number | null }) {
  const { busy, act, on } = useOpsAct()
  // A question asked through the gate is answered by its whole card, read-only under the time machine.
  if (whole && i.confirm) return <div className="mt-2"><ConfirmCard c={i.confirm} past={past ?? undefined} /></div>
  const send = (verb: Verb | ButtonAct, a: Answer) => {
    const call = answerOf(i, a)
    if (call) void act(verb, `${i.key}:${a.verb}`, call.method, call.params, call.ask).then((ok) => { if (ok) onNote(null) })
  }
  const b = (verb: Verb | ButtonAct, a: Answer, tone: 'ok' | 'wait' | 'fault' | 'live', label: ReactNode, title?: string) =>
    answerOf(i, a) ? <ActBtn key={`${verb}:${a.verb}:${'n' in a ? a.n : ''}${'key' in a ? a.key : ''}`} verb={verb} on={on} tone={tone} busy={busy === `${i.key}:${a.verb}`} onClick={() => send(verb, a)} title={title}>{label}</ActBtn> : null
  const choices = i.view?.question?.choices ?? []
  return (
    <div className="mt-2 flex flex-col gap-1.5">
      {i.proposal && <div className="text-[11.5px] text-think">{changeWords(i.proposal.record)}</div>}
      {i.ask && <div className="text-[11px] text-ink-dim">“{i.ask.message}”</div>}
      <div className="flex flex-wrap gap-1.5">
        {(i.confirm || i.proposal) && i.kind !== 'question' && <>
          {b('approve', { verb: 'approve' }, 'ok', <>y · {i.confirm?.budget ? 'Reset' : i.confirm?.change || i.proposal ? 'Accept' : 'Approve'}</>)}
          {i.confirm && grantKey(i.confirm) && b('grant', { verb: 'grant' }, 'ok', <>{grantKey(i.confirm)} · {i.confirm.grant_offer!.words}</>)}
          {note === null ? <ActBtn verb="decline" on={on} tone="fault" onClick={() => onNote({ key: i.key, text: '' })} title="decline, with a note the model sees">n · Decline…</ActBtn> : null}
        </>}
        {choices.slice(0, 6).map((c, n) => b('choice', { verb: 'choice', n: n + 1 }, 'live', <>{n + 1} · {c}</>))}
        {i.kind === 'question' && note === null && <ActBtn verb="choice" on={on} tone="live" onClick={() => onNote({ key: i.key, text: '' })}>Something else…</ActBtn>}
        {b('retry', { verb: 'retry' }, 'live', 'r · Retry')}
        {b('dismiss', { verb: 'dismiss' }, 'fault', 'd · Dismiss')}
        {b('seal', { verb: 'seal' }, 'wait', 'Seal')}
        {b('seal_dismiss', { verb: 'seal_dismiss' }, 'fault', 'Not now')}
        {i.ask && [...i.ask.options, ...(i.ask.others ?? [])].map((o) => b('thread_answer', { verb: 'thread_answer', key: o.key }, 'live', o.label))}
        {i.kind === 'failure' && i.sessionId && <Link to={`/session/${i.sessionId}`} className="self-center text-[11.5px] text-live hover:underline">o · open</Link>}
      </div>
      {note !== null && (
        <form className="flex gap-1.5" onSubmit={(e) => { e.preventDefault(); send(i.kind === 'question' ? 'choice' : 'decline', i.kind === 'question' ? { verb: 'choice', n: 0, note } : { verb: 'decline', note }) }}>
          <input autoFocus value={note} onChange={(e) => onNote({ key: i.key, text: e.target.value })} onKeyDown={(e) => { if (e.key === 'Escape') onNote(null) }}
            placeholder={i.kind === 'question' ? 'your answer, in words' : 'note (optional; the model sees it)'} aria-label="note"
            className="min-w-0 flex-1 rounded-md bg-white/5 px-2 py-1 text-[11.5px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40" />
          <ActBtn verb={i.kind === 'question' ? 'choice' : 'decline'} on={on} tone={i.kind === 'question' ? 'live' : 'fault'} onClick={() => {}}>{i.kind === 'question' ? 'Send' : 'Decline'}</ActBtn>
        </form>
      )}
      {i.confirm && !whole && <span className="text-[10.5px] text-ink-faint">⏎ the whole card</span>}
    </div>
  )
}

/** The next three hours: each mark's glyph, its time and its words. */
const MARK: Record<string, string> = { remind: '●', check: '○', routine: '│', watch: '▬' }

function ComingUp({ views, events, now }: { views: readonly WorkView[]; events: readonly ComingEvent[]; now: number }) {
  // Theseus's marks and the calendar's events, by time: an event's line is already redacted as the page is.
  const rows = [
    ...views.map((v) => ({ at: v.next_at_ms ?? now, key: v.id, v })),
    ...events.map((e) => ({ at: e.at_ms, key: `ev:${e.key}`, e })),
  ].sort((a, b) => a.at - b.at || (a.key < b.key ? -1 : 1)).slice(0, 8)
  return (
    <section className="border-t border-line px-2.5 py-2" aria-label="Coming up">
      <h3 className="mb-1 flex items-center gap-1.5 font-display text-[10.5px] font-bold uppercase tracking-[0.08em] text-gold"><Hourglass size={11} /> Coming up · 3 h</h3>
      {!rows.length && <div className="text-[11px] text-ink-faint">nothing in the next three hours</div>}
      {rows.map((r) => 'v' in r && r.v ? (
        <Link key={r.key} to={r.v.session_id ? `/session/${r.v.session_id}` : '#'} className="flex items-baseline gap-2 rounded px-1 py-0.5 text-[11.5px] hover:bg-white/5">
          <span className="w-3 text-think" aria-hidden>{r.v.kind === 'task' ? '▷' : MARK[r.v.purpose ?? r.v.kind] ?? '●'}</span>
          <span className="num shrink-0 text-ink-dim">{comingTimes(r.v.next_at_ms, now).at}</span>
          <span className="min-w-0 truncate text-ink">{r.v.title || r.v.kind}</span>
          <span className="num ml-auto shrink-0 text-[10px] text-ink-faint">{comingTimes(r.v.next_at_ms, now).rel}</span>
        </Link>
      ) : 'e' in r && r.e ? (
        <div key={r.key} className="flex items-baseline gap-2 px-1 py-0.5 text-[11.5px]" data-event={r.e.sensitive ? 'sensitive' : 'open'}>
          <span className="inline-block h-2 w-3 shrink-0 rounded-[2px]" style={{ background: r.e.color }} aria-hidden />
          <span className="num shrink-0 text-ink-dim">{comingTimes(r.e.at_ms, now).at}</span>
          <span className="min-w-0 truncate text-ink-dim">{r.e.line}</span>
          <span className="num ml-auto shrink-0 text-[10px] text-ink-faint">{comingTimes(r.e.at_ms, now).rel}</span>
        </div>
      ) : null)}
    </section>
  )
}
