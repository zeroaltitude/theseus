// The Ship's words (theseus-hnof): every shape on the chart says what it is in plain words, beside its sea name, so
// "session", "turn", "tool call", "task" and "job" are never more than a glance away. Pure: the nameplates, the hover
// cards and the key read the same words, and a test holds them.
//
// The mapping, one line a shape:
//   harbour ring  = a place (where sessions come from: a DM, a channel, the CLI, the web UI)
//   ship          = a session (one conversation); a small boat in tow = a task it started
//   bench         = a turn, oldest at the stern, newest at the bow
//   ivory lamp    = a message; violet lamp = a model call
//   oar           = a tool call; its blade is the result (green ok, rose failed, amber waits for you, open pending)
//   brass gear    = a job running; hex shield = sandboxed (L1); magenta = text from the web
//   rig           = the state: sail up working, lantern waiting for you, flare failed, at anchor idle

/** The plain state of a vessel, from its rig and its attention: the word, and the tone it wears. */
export type Tone = 'live' | 'wait' | 'fault' | 'ok' | 'idle'

export interface StateWord { word: string; tone: Tone; sea: string }

interface VesselLike {
  kind: 'conversation' | 'task'
  rig: 'anchor' | 'sail' | 'lantern' | 'flare'
  state: string
  attention?: { level?: string; label?: string }
  pendingConfirms?: number
}

/** A vessel's state in plain words: "working", "waiting for you", "failed", "done" (a task that reported), "idle". */
export function stateWord(v: VesselLike): StateWord {
  if (v.rig === 'flare') {
    const why = v.state === 'budget_exhausted' ? 'over its budget' : v.state === 'interrupted' ? 'interrupted' : 'failed'
    return { word: why, tone: 'fault', sea: 'flare up' }
  }
  if (v.rig === 'lantern') {
    const label = v.attention?.label ?? ''
    // The attention's label says what it waits on: `confirm fs.write: …` or `budget: …`.
    const what = label.startsWith('confirm ') ? `approve ${label.slice(8).split(':')[0]}` : label.startsWith('budget') ? 'raise its budget' : ''
    return { word: what ? `waiting for you · ${what}` : 'waiting for you', tone: 'wait', sea: 'lantern lit' }
  }
  if (v.rig === 'sail') {
    const label = v.attention?.label ?? ''
    const what = label.startsWith('waiting on') ? label.replace(/^waiting on /, 'running ') : ''
    return { word: what ? `working · ${what}` : 'working', tone: 'live', sea: 'under sail' }
  }
  if (v.kind === 'task' && (v.state === 'complete' || v.state === 'succeeded')) return { word: 'done', tone: 'ok', sea: 'at anchor' }
  if (v.state === 'cancelled') return { word: 'cancelled', tone: 'idle', sea: 'at anchor' }
  return { word: 'idle', tone: 'idle', sea: 'at anchor' }
}

/** What a vessel is: "session" or "task". */
export const vesselNoun = (v: Pick<VesselLike, 'kind'>) => (v.kind === 'task' ? 'task' : 'session')

/** "1 turn", "17 turns". */
export function count(n: number, one: string, many = `${one}s`): string {
  return `${n.toLocaleString('en-US')} ${n === 1 ? one : many}`
}

/** Each light kind's plain name. */
export const LIGHT_NOUN = { user: 'message', model: 'model call', call: 'tool call', result: 'tool result' } as const

interface LightLike {
  kind: 'user' | 'model' | 'call' | 'result'
  failed?: boolean
  running?: boolean
  waiting?: boolean
  external?: boolean
  collapsedAt?: number
}

/** A tool call's outcome in plain words, with its tone: the blade's colour, said. */
export function outcome(l: LightLike, hasResult: boolean): { word: string; tone: Tone } {
  if (l.collapsedAt !== undefined) return { word: 'stopped, verified', tone: 'fault' }
  if (l.waiting) return { word: 'waiting for you', tone: 'wait' }
  if (l.running) return { word: 'job running', tone: 'live' }
  if (l.failed) return { word: 'failed', tone: 'fault' }
  if (!hasResult) return { word: 'pending', tone: 'idle' }
  if (l.external) return { word: 'ok · text from the web', tone: 'ok' }
  return { word: 'ok', tone: 'ok' }
}

/** A duration for people: "340 ms", "4.4 s", "3 m 12 s", "2 h 5 m". */
export function span(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return '—'
  if (ms < 1000) return `${Math.round(ms)} ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(ms < 10_000 ? 1 : 0)} s`
  const m = Math.floor(ms / 60_000)
  if (m < 60) return `${m} m ${Math.floor((ms % 60_000) / 1000)} s`
  return `${Math.floor(m / 60)} h ${m % 60} m`
}

/** A turn's one-line summary: "2 model calls · 3 tool calls, 1 failed · $0.0004". */
export function benchLine(b: { models: number; calls: number; failed: number; cost: number }, usd: (n: number) => string): string {
  const parts = [count(b.models, 'model call')]
  if (b.calls) parts.push(b.failed ? `${count(b.calls, 'tool call')}, ${b.failed} failed` : count(b.calls, 'tool call'))
  else parts.push('no tool calls')
  if (b.cost > 0) parts.push(usd(b.cost))
  return parts.join(' · ')
}

/** Who wrote a message, for people: "you" for the operator, the task, a wake, or the author's label. */
export function authorWord(author: string | undefined): string {
  if (!author) return 'you'
  // A local protocol client is the operator: its connection's label (`sock#4` for the CLI's socket, `web#2`).
  if (/^sock#/.test(author)) return 'you, from the CLI'
  if (/^(web|ws)#/.test(author)) return 'you, from the web'
  if (/^operator\b/.test(author) || author.startsWith('cli') || author.startsWith('web')) return 'you'
  if (author.startsWith('discord:')) return `@${author.slice(8)}`
  if (author.startsWith('task')) return 'a task'
  if (author.startsWith('wake') || author.startsWith('harness')) return 'the harness'
  return author
}

/** The four depths the chart reads at (the depth gauge): the fleet, one ship, one turn, one call's data. */
export type Depth = 'fleet' | 'ship' | 'turn' | 'call'

/** The depth a camera reads at, from how big the biggest vessel near the middle of the view is on screen (CSS pixels),
 *  and whether an inspector is open. */
export function depthOf(vesselPx: number, inspector: boolean): Depth {
  if (inspector) return 'call'
  if (vesselPx >= 1500) return 'turn'
  if (vesselPx >= 380) return 'ship'
  return 'fleet'
}
