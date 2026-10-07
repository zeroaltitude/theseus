// The Ship's sound (theseus-hnof.2, the owner's C4): off by default, one toggle, three cues on the daemon's own events,
// quiet and on theme. Pure: which push sounds which cue, and how often, so a test holds it; `audio.ts` makes the sounds.
//
//   an oar goes out        tool.started                                          a soft oar splash
//   something waits for you confirm.requested; an execution that comes to need     a ship's bell, struck twice
//                           you for a question or a budget (execution.changed)
//   a failure              turn.failed; an execution that fails or runs over its   a low horn
//                           budget (execution.changed)
//
// A failed tool call is not a cue: it shows as a rose blade and a pennant, and the turn often goes on to recover. The
// cockpit hears confirm.requested and execution.changed for every session (it watches them all), and tool.started and
// turn.failed for the sessions the Ship watches (those that work or wait for you).

export type Cue = 'oar' | 'bell' | 'horn'

/** The table: each cue, what sounds it, and what it is. */
export const CUES: readonly { cue: Cue; events: string; sound: string; source: string }[] = [
  { cue: 'oar', events: 'tool.started', sound: 'a soft oar splash: a blade’s low knock, water rushing, two drops', source: 'synthesized in the browser (Web Audio: filtered noise and two sines), src/ship/audio.ts' },
  { cue: 'bell', events: 'confirm.requested; execution.changed into needs_you for a question, a budget or a block', sound: 'a ship’s bell, struck twice', source: 'synthesized in the browser (Web Audio: a bell’s inharmonic partials, each ringing down), src/ship/audio.ts' },
  { cue: 'horn', events: 'turn.failed; execution.changed into failed or budget_exhausted', sound: 'a low horn, short', source: 'synthesized in the browser (Web Audio: three detuned low saws through a low-pass), src/ship/audio.ts' },
]

/** How often each cue may sound at most (ms): a burst of tool calls rows a few strokes, not a splash each; a question
 *  rings once; a failure sounds once even when its turn and its execution both say so. */
export const SPACING: Record<Cue, number> = { oar: 220, bell: 4000, horn: 3000 }

type D = Record<string, unknown>

/** What the cue table remembers between pushes: each session's attention and state, so a cue sounds on a change. */
export interface Ear {
  level: Map<string, string>
  state: Map<string, string>
  /** When each cue last sounded, and for which session (a session's bell or horn sounds once a change). */
  last: Map<Cue, number>
  rang: Map<string, number>
}

export const newEar = (): Ear => ({ level: new Map(), state: new Map(), last: new Map(), rang: new Map() })

const FAILED = new Set(['failed', 'budget_exhausted'])

/** The cue a push sounds, if any (and remember what it said). `now` in ms. */
export function cueOf(ear: Ear, method: string, params: unknown, now: number): Cue | null {
  const p = (params ?? {}) as D
  const sid = typeof p.session_id === 'string' ? p.session_id : ''
  let cue: Cue | null = null
  if (method === 'tool.started') cue = 'oar'
  else if (method === 'confirm.requested') cue = 'bell'
  else if (method === 'turn.failed') cue = 'horn'
  else if (method === 'execution.changed') {
    const state = typeof p.state === 'string' ? p.state : ''
    const level = typeof (p.attention as D | undefined)?.level === 'string' ? ((p.attention as D).level as string) : ''
    // The view says the state before its frame (absent for an execution first seen); the ear, the attention before.
    const wasState = typeof p.previous === 'string' ? p.previous : ear.state.get(sid)
    const wasLevel = ear.level.get(sid)
    ear.state.set(sid, state)
    ear.level.set(sid, level)
    // A change only: the first view of a session (the page just opened) is not news.
    if (wasState !== undefined && FAILED.has(state) && !FAILED.has(wasState)) cue = 'horn'
    else if (wasLevel !== undefined && level === 'needs_you' && wasLevel !== 'needs_you' && !FAILED.has(state)) cue = 'bell'
  }
  if (!cue) return null
  // A session's bell or horn once a change (a question's push and its execution's change are one event).
  if (cue !== 'oar' && sid) {
    const key = `${cue} ${sid}`
    const at = ear.rang.get(key)
    if (at !== undefined && now - at < 10_000) return null
    ear.rang.set(key, now)
  }
  const last = ear.last.get(cue)
  if (last !== undefined && now - last < SPACING[cue]) return null
  ear.last.set(cue, now)
  return cue
}

/** Where the browser keeps the toggle: off unless the operator turned it on. */
export const SOUND_KEY = 'cockpit.ship.sound'
export const soundOn = (kept: string | null) => kept === 'on'
