// The inspector's Live section and the dock's streams (theseus-jid7t.6), pure: the model's words as they come from the
// push, the session's narrative lines, and a followed job's output chunks. `test/ops-live.test.ts` runs it.
import type { NarrativeLine } from '@protocol'
import { short } from '../lib/figures.ts'

export interface LiveTurn { turn_id: string; text: string; thinking?: string; running: { id: string; tool: string; startedAt: number }[] }

/** What `sessionId`'s current turn is doing that no node holds yet, from the push's events: the streamed text and
 *  thinking of the loop under way, and the tools running. The deck's reading of the same events. */
export function liveOf(events: ReadonlyArray<{ method: string; at: number; params: Record<string, unknown> }>, sessionId: string): LiveTurn | null {
  let turn: string | null = null
  let text = ''
  let thinking = ''
  let running: LiveTurn['running'] = []
  for (const e of events) {
    const p = e.params as Record<string, any>
    if (e.method === 'turn.started' && p.session_id === sessionId) { turn = p.turn_id; text = ''; thinking = ''; running = [] }
    else if (turn && p.turn_id === turn) {
      if (e.method === 'model.delta') text += p.text ?? ''
      else if (e.method === 'model.thinking') thinking += p.text ?? ''
      else if (e.method === 'loop.started') { text = ''; thinking = '' }
      else if (e.method === 'tool.started') running = [...running, { id: String(p.tool_use_id), tool: String(p.tool), startedAt: e.at }]
      else if (e.method === 'tool.ended') running = running.filter((t) => t.id !== String(p.tool_use_id))
      else if (e.method === 'turn.ended' || e.method === 'turn.failed') { turn = null; text = ''; thinking = ''; running = [] }
    }
  }
  return turn ? { turn_id: turn, text, thinking, running } : null
}

/** The narrative lines of one session, oldest first. */
export const narrativeOf = (lines: readonly NarrativeLine[], sessionId: string): NarrativeLine[] => lines.filter((l) => l.session_id === sessionId)

/** A followed job's output: whole lines, and the unfinished one still being written. */
export interface JobOut { lines: string[]; partial: string; offset: number }
export const EMPTY_OUT: JobOut = { lines: [], partial: '', offset: 0 }
const KEEP_LINES = 200

/** `work.peek`'s tail: its lines, and where `action.output` follows from. */
export const peekOut = (lines: readonly string[], bytes: number): JobOut => ({ lines: lines.slice(-KEEP_LINES), partial: '', offset: bytes })

/** A chunk from byte `offset`: one that starts before what is held is a repeat (the peek and the follow overlap by a
 *  chunk at most) and adds only its new tail; one that starts after is a gap and is taken as it comes. */
export function appendChunk(o: JobOut, c: { offset: number; text: string }): JobOut {
  const skip = Math.max(0, o.offset - c.offset)
  const text = skip ? c.text.slice(skip) : c.text
  if (!text) return o
  const parts = (o.partial + text).split('\n')
  const partial = parts.pop() ?? ''
  return { lines: [...o.lines, ...parts].slice(-KEEP_LINES), partial, offset: Math.max(o.offset, c.offset + c.text.length) }
}

/** One line of Live, of any of the three streams. */
export interface LiveLine { at: number; src: 'narrative' | 'model' | 'job'; text: string }

/** The three streams in one list, newest last: the session's narrative with its times, what the model is saying now,
 *  and the job's last lines (they carry no times, so they sit at `now`). `max` keeps the newest. */
export function mergeLive(narr: readonly NarrativeLine[], model: LiveTurn | null, job: JobOut | null, now: number, max = 80): LiveLine[] {
  const out: LiveLine[] = narr.map((l) => ({ at: l.at_unix_ms, src: 'narrative', text: l.text }))
  if (model?.text) out.push({ at: now, src: 'model', text: model.text.slice(-400) })
  else if (model?.thinking) out.push({ at: now, src: 'model', text: `thinking… ${model.thinking.slice(-200)}` })
  for (const t of model?.running ?? []) out.push({ at: t.startedAt, src: 'model', text: `running ${t.tool}` })
  if (job) for (const l of [...job.lines.slice(-20), ...(job.partial ? [job.partial] : [])]) out.push({ at: now, src: 'job', text: l })
  return out.sort((a, b) => a.at - b.at).slice(-max)
}

/** The dock's streams: every session's narrative, newest first. A paused dock holds the line it paused at; a followed
 *  selection shows that session's lines alone. */
export function dockStream(lines: readonly NarrativeLine[], opts: { follow: string | null; pausedAt: number | null }, n = 60): NarrativeLine[] {
  const kept = lines.filter((l) => (opts.pausedAt == null || l.seq <= opts.pausedAt) && (!opts.follow || l.session_id === opts.follow))
  return kept.slice(-n).reverse()
}

/** A narrative line's session, short, for the dock's left edge. */
export const whoOf = (l: NarrativeLine): string => (l.session_id ? short(l.session_id) : 'daemon')

/** One row of the dock's models strip. */
export interface ModelStrip { name: string; model: string; origin: string; live: boolean; sessions: number; headroom: string | null; /** The effort the profile is configured at. */ effort: string | null }

/** The Bridge's headroom, from a call's rate headers: the tightest of the requests and tokens left, as a share. */
export function headroom(r: { requests_remaining?: number; requests_limit?: number; tokens_remaining?: number; tokens_limit?: number } | undefined): string | null {
  const shares = ([[r?.requests_remaining, r?.requests_limit, 'requests'], [r?.tokens_remaining, r?.tokens_limit, 'tokens']] as const)
    .flatMap(([rem, lim, what]) => (rem != null && lim ? [{ s: rem / lim, what }] : []))
  if (!shares.length) return null
  const t = shares.reduce((a, b) => (b.s < a.s ? b : a))
  return `${Math.round(t.s * 100)}% of ${t.what} left`
}

/** The dock's strip: each profile with its origin, the live flag, the sessions on it (from `session.list`'s `profile`)
 *  and the headroom of the newest call that model made. `calls` come oldest first. */
export function modelsStrip(profiles: ReadonlyArray<{ name: string; model: string; live: boolean; origin?: string; effort?: string | null }>, sessions: ReadonlyArray<{ profile?: string; retired?: unknown }>, calls: ReadonlyArray<{ model: string; rate?: Parameters<typeof headroom>[0] }>): ModelStrip[] {
  return profiles.map((p) => {
    let rate: Parameters<typeof headroom>[0]
    for (let i = calls.length - 1; i >= 0; i--) if (calls[i].model === p.model && calls[i].rate) { rate = calls[i].rate; break }
    return { name: p.name, model: p.model, origin: p.origin ?? 'unknown', live: p.live, sessions: sessions.filter((s) => s.profile === p.name && !s.retired).length, headroom: headroom(rate), effort: p.effort ?? null }
  })
}
