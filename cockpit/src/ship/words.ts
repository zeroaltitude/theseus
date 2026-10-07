// The Ship's words (theseus-hnof; one module, theseus-hnof.2): every shape on the chart says what it is in plain words,
// beside its sea name, so "session", "turn", "tool call", "task" and "job" are never more than a glance away. Pure: the
// nameplates, the bench labels, the tags, the hover cards, the key, the tour and the session deck read these words, and
// a test holds them.
//
// The mapping (the galley, as built and kept: the owner's C1), one line a shape:
//   harbour ring  = a place (where sessions come from: a DM, a channel, the CLI, the web UI)
//   ship          = a session (one conversation); a small boat in tow = a task it started
//   bench         = a turn, oldest at the stern, newest at the bow
//   ivory lamp    = a message; violet lamp = a model call
//   oar           = a tool call; its blade is the result (green ok, rose failed, amber waits for you, open pending)
//   brass gear    = a job running; hex shield = sandboxed (L1); magenta = text from the web
//   rig           = the state: sail up working, lantern waiting for you, flare failed, at anchor idle
//   gold planks   = its turns of the last hour; a chain along its rail = it holds text from the web until trusted
//   gold coin     = a turn's cost, flying to "Spent today"; violet spark = a turn that recalled memory
//   the swell     = the work now: a slow roll when nothing runs, rising with tokens a minute and running turns

/** Every shape the chart draws: what it is (`word`, as the key says it), its bare noun (as a card's head says it), and how
 *  the chart draws it (`sea`). */
export const SHAPES = {
  place: { word: 'a place', noun: 'place', sea: 'harbour ring' },
  session: { word: 'a session', noun: 'session', sea: 'ship' },
  task: { word: 'a task', noun: 'task', sea: 'boat in tow' },
  turn: { word: 'a turn', noun: 'turn', sea: 'bench; newest at the bow' },
  message: { word: 'a message', noun: 'message', sea: 'ivory lamp' },
  model: { word: 'a model call', noun: 'model call', sea: 'violet lamp' },
  call: { word: 'a tool call', noun: 'tool call', sea: 'oar; the blade is its result' },
  failed: { word: 'failed', noun: 'failed call', sea: 'rose blade ✕, pennant' },
  waiting: { word: 'waits for you', noun: 'call waiting for you', sea: 'amber blade, lamp' },
  job: { word: 'a job running', noun: 'job', sea: 'brass gear, turning' },
  l1: { word: 'sandboxed (L1)', noun: 'sandboxed call', sea: 'hex shield' },
  web: { word: 'text from the web', noun: 'result from the web', sea: 'magenta blade' },
  working: { word: 'working', noun: 'working', sea: 'sail up, oars rowing' },
  needs: { word: 'waiting for you', noun: 'waiting for you', sea: 'lantern lit' },
  down: { word: 'failed or over budget', noun: 'failed', sea: 'flare up' },
  idle: { word: 'idle', noun: 'idle', sea: 'at anchor' },
  planks: { word: 'turns in the last hour', noun: 'recent turns', sea: 'gold planks on its deck' },
  held: { word: 'holds web text', noun: 'holds web text', sea: 'a chain along its rail' },
  coin: { word: 'a turn’s cost', noun: 'cost', sea: 'gold coin, flying to Spent today' },
  recall: { word: 'recalled memory', noun: 'recall', sea: 'violet spark on its bench' },
  sea: { word: 'the work now', noun: 'the sea', sea: 'the swell' },
} as const satisfies Record<string, { word: string; noun: string; sea: string }>

export type Shape = keyof typeof SHAPES

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

/** A vessel's state as the daemon records it (its execution's state, and its attention's level and label), for the card
 *  under its plain words: "running · working: waiting on 1 call", "budget_exhausted". */
export function rawState(v: VesselLike): string {
  const level = v.attention?.level ?? ''
  const label = v.attention?.label ?? ''
  const att = label && label !== level ? `${level ? `${level}: ` : ''}${label}` : level
  return att ? `${v.state} · ${att}` : v.state
}

/** "9 of its 17 turns in the last hour", or nothing for a ship with none then. */
export function recentLine(gold: number, turns: number): string | null {
  return gold > 0 ? `${gold} of its ${count(turns, 'turn')} in the last hour` : null
}

/** The vessel card's first three lines (theseus-n7ra): its state as the daemon records it; its turns, with how many in
 *  the last hour (its gold planks: the inventory's N5) and when it was last active (`last`, said by the caller: "4m
 *  ago"); and its calls: its messages (N1), model calls and tool calls, and how many failed (`failed`, its own tone).
 *  The card draws these words and a test holds every datum, so a later trim of the card cannot drop one unseen. */
export function cardLines(
  v: VesselLike & { turns: number; goldPlanks: number },
  kinds: { user: number; model: number; call: number },
  failed: number,
  last: string,
): { state: string; turns: string; calls: string; failed: string | null } {
  return {
    state: rawState(v),
    turns: `${v.turns}${v.goldPlanks ? ` · ${v.goldPlanks} in the last hour` : ''} · last ${last}`,
    calls: `${count(kinds.user, 'message')} · ${count(kinds.model, 'model call')} · ${count(kinds.call, 'tool call')}`,
    failed: failed ? `, ${failed} failed` : null,
  }
}

/** What a vessel is: "session" or "task". */
export const vesselNoun = (v: Pick<VesselLike, 'kind'>) => (v.kind === 'task' ? SHAPES.task.noun : SHAPES.session.noun)

/** A vessel's sea word: "ship" or "boat in tow". */
export const vesselSea = (v: Pick<VesselLike, 'kind'>) => (v.kind === 'task' ? SHAPES.task.sea : SHAPES.session.sea)

/** "1 turn", "17 turns". */
export function count(n: number, one: string, many = `${one}s`): string {
  return `${n.toLocaleString('en-US')} ${n === 1 ? one : many}`
}

/** Money as the Ship says it: "$0", "$0.0004", "$0.015", "$3.20". */
export function usdShort(n: number): string {
  return n === 0 ? '$0' : n < 0.01 ? `$${n.toFixed(4)}` : n < 1 ? `$${n.toFixed(3)}` : `$${n.toFixed(2)}`
}

/** Each light kind's plain name. */
export const LIGHT_NOUN = { user: SHAPES.message.noun, model: SHAPES.model.noun, call: SHAPES.call.noun, result: 'tool result' } as const

/** Each light kind's sea word, as a hover card says it ("the violet lamp"). */
export const LIGHT_SEA = { user: SHAPES.message.sea, model: SHAPES.model.sea, call: 'oar', result: 'blade' } as const

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

/** An oar's tag at its blade: its tool, and how it went when that is news ("fs.read · failed"; an ok one names its tool). */
export function oarTag(tool: string | undefined, o: { word: string }): string {
  const t = tool ?? 'tool'
  return o.word === 'ok' ? t : `${t} · ${o.word}`
}

/** The tags along a ship's keel: a message says who wrote it, and a model call its model, only where that changes along
 *  its vessel (its first, then a new author or a new model). Eight "you, from the CLI" and fifteen "sonnet-5-5" in a row
 *  said nothing the first did not; the hover card says each one's. Each light's tag by its index, for those that say. */
export function keelTags(lights: readonly { vessel: number; kind: string; author?: string; model?: string }[]): Map<number, string> {
  const out = new Map<number, string>()
  const last = new Map<string, string>()
  lights.forEach((l, i) => {
    const said = l.kind === 'user' ? authorWord(l.author) : l.kind === 'model' ? (l.model ?? 'model').replace(/^claude-/, '') : null
    if (said === null) return
    const key = `${l.vessel} ${l.kind}`
    if (last.get(key) !== said) out.set(i, said)
    last.set(key, said)
  })
  return out
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

interface BenchLike { n: number; models: number; calls: number; failed: number; cost: number; running: boolean; waiting: boolean }

/** A turn's one-line summary: "2 model calls · 3 tool calls, 1 failed · $0.0004". */
export function benchLine(b: Pick<BenchLike, 'models' | 'calls' | 'failed' | 'cost'>, usd: (n: number) => string = usdShort): string {
  const parts = [count(b.models, 'model call')]
  if (b.calls) parts.push(b.failed ? `${count(b.calls, 'tool call')}, ${b.failed} failed` : count(b.calls, 'tool call'))
  else parts.push('no tool calls')
  if (b.cost > 0) parts.push(usd(b.cost))
  return parts.join(' · ')
}

/** A turn's state, as its bench shows it: waiting for you before working before failed; else done. */
export function benchState(b: Pick<BenchLike, 'running' | 'waiting' | 'failed'>): { word: string; tone: Tone } {
  if (b.waiting) return { word: 'waits for you', tone: 'wait' }
  if (b.running) return { word: 'working', tone: 'live' }
  if (b.failed) return { word: `${b.failed} failed`, tone: 'fault' }
  return { word: 'done', tone: 'idle' }
}

/** A bench's label above its rail: "turn 12 · working", "turn 8 · 1 failed"; a done turn says its calls when there is
 *  room ("turn 3 · 2 tool calls"), else only its number. */
export function benchLabel(b: Pick<BenchLike, 'n' | 'calls' | 'running' | 'waiting' | 'failed'>, wide: boolean): string {
  const head = `turn ${b.n}`
  const s = benchState(b)
  if (s.word !== 'done') return `${head} · ${s.word}`
  return wide && b.calls ? `${head} · ${count(b.calls, 'tool call')}` : head
}

/** A nameplate's line under its title: "idle · 17 turns · $0.0069", and "holds web text" when it does. */
export function plateLine(v: VesselLike & { turns: number; cost: number; hold?: unknown }): string {
  return `${stateWord(v).word} · ${count(v.turns, 'turn')} · ${usdShort(v.cost)}${v.hold ? ' · holds web text' : ''}`
}

/** A harbour's line beside its name: " · 3 sessions · 4 tasks". */
export function harbourLine(sessions: number, tasks: number): string {
  return ` · ${count(sessions, 'session')}${tasks ? ` · ${count(tasks, 'task')}` : ''}`
}

/** Who wrote a message, for people: "you" for the operator, the task, a wake, or the author's label. */
export function authorWord(author: string | undefined): string {
  if (!author) return 'you'
  // A local protocol client is the operator: its connection's label (`sock#4` for the CLI's socket, `web#2`).
  if (/^sock#/.test(author)) return 'you, from the CLI'
  if (/^(web|ws)#/.test(author)) return 'you, from the web'
  if (/^operator\b/.test(author) || author.startsWith('cli') || author.startsWith('web')) return 'you'
  if (author.startsWith('discord:')) return `@${author.slice(8)}`
  // A task's report to the session that started it, and that session's brief to its task.
  if (author.startsWith('task:')) return `task ${author.slice(5)}`
  if (author.startsWith('task')) return 'a task'
  if (author.startsWith('session:')) return 'the session that started it'
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
