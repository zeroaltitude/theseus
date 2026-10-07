// The Ship's motion (theseus-hnof.2, step 3): every motion on the chart, the one event that starts it, how long it lasts,
// and how often it needs a frame. Nothing else moves: the loop draws a frame only while one of these runs (or something
// changed), and every term of the shaders that moves with time names its row (a test reads them). Pure.
//
// - **display**: a one-off that plays once (a flare, an oar growing out, a result flashing back): every display frame,
//   for its seconds.
// - **steady**: a state that moves while it lasts (oars rowing, a gear turning, a wake): a steady pace, `STEADY_FPS`;
//   after a minute with no event (`QUIET_AFTER_S`: a long job, a long model call), `QUIET_FPS` (theseus-n2hd).
// - **sea**: the swell alone, raised by the work, at the sea's pace (`IDLE_FPS`, the composite only).
// - **roll**: Live mode's sea with nothing happening, rolling slowly at `ROLL_FPS` (the composite only).
// The camera and a vessel gliding to its new slot are the operator's own moves and the layout's: every display frame.
// Calm mode (and reduced motion, which turns it on) stills them all: a change draws one frame, and nothing plays.

export type Pace = 'display' | 'steady' | 'sea' | 'roll'

export interface Motion {
  id: string
  /** What starts it: the protocol's event (or, for the camera, the operator). */
  event: string
  /** What moves, in words. */
  moves: string
  /** A one-off's seconds; none for a motion that lasts while its state does. */
  secs?: number
  pace: Pace
  /** Drawn on the canvas (the engine and its shaders) or on the page (a DOM element over it). */
  on: 'canvas' | 'page'
}

/** The table: one row a motion. A protocol event starts one motion, and a motion has one event. */
export const MOTIONS = [
  { id: 'camera', event: 'the operator: a scroll, a drag, a click on a shape, Fleet, Fly to, a depth', moves: 'the camera flies there, in an arc for a long way', pace: 'display', on: 'canvas' },
  { id: 'settle', event: 'node.written that grows a ship a bench, so the layout moves', moves: 'a vessel glides to its new slot', pace: 'display', on: 'canvas' },
  { id: 'session-born', event: 'a session opens (session.list gains one)', moves: 'its ship flares in, brass going gold', secs: 2.5, pace: 'display', on: 'canvas' },
  { id: 'node-born', event: 'node.written: a message or a model call', moves: 'its lamp flares on the keel and a ring runs out from it', secs: 2.2, pace: 'display', on: 'canvas' },
  { id: 'oar-out', event: 'node.written: a tool call', moves: 'a new oar grows out from the hull', secs: 0.8, pace: 'display', on: 'canvas' },
  { id: 'result-back', event: 'node.written: a tool result', moves: 'the blade flashes and a light runs back along the shaft to the hull', secs: 1.7, pace: 'display', on: 'canvas' },
  { id: 'rowing', event: 'turn.started, until turn.ended or turn.failed (and while a job its turn started runs)', moves: 'its bench’s oars row, out of step by side, and a call still out sends a light down its shaft', pace: 'steady', on: 'canvas' },
  { id: 'working', event: 'a session works (its execution running)', moves: 'its ship makes way: a wake trails astern', pace: 'steady', on: 'canvas' },
  { id: 'stream', event: 'model.delta', moves: 'a violet beacon pulses at the bow, where the model call’s lamp will land', pace: 'steady', on: 'canvas' },
  { id: 'gear', event: 'tool.started for a job (an unsettled proc.run), until tool.ended', moves: 'a brass gear turns at the blade', pace: 'steady', on: 'canvas' },
  { id: 'tether', event: 'a task runs (execution.changed to running)', moves: 'the current along its tether flows to it', pace: 'steady', on: 'canvas' },
  { id: 'current', event: 'node.reach: a node copied to a session that works now', moves: 'the current between the two flows', pace: 'steady', on: 'canvas' },
  { id: 'report', event: 'execution.changed: a task completes', moves: 'gold runs back along its tether to the ship that started it', secs: 3.2, pace: 'display', on: 'canvas' },
  { id: 'failed', event: 'turn.failed (its push, or its ledger row for a session the Ship does not watch), or execution.changed to failed: once a failure', moves: 'the flare bursts and rises; the pennant goes up and stands', secs: 3, pace: 'display', on: 'canvas' },
  { id: 'waiting', event: 'a question for the operator (confirm.list, a budget question)', moves: 'the lantern lights at the stern, swelling once; then it stands lit', secs: 1.5, pace: 'display', on: 'canvas' },
  { id: 'recall', event: 'turn.ended that recalled memory', moves: 'a violet spark rings out on its bench', secs: 6.5, pace: 'display', on: 'canvas' },
  { id: 'collapse', event: 'a cancel verified (an action’s verdict: termination_verified)', moves: 'the job’s hex shield collapses to an ember', secs: 1.4, pace: 'display', on: 'canvas' },
  { id: 'coin', event: 'turn.ended with a cost', moves: 'a gold coin rises from the ship and flies to Spent today', secs: 1.3, pace: 'display', on: 'page' },
  { id: 'sea', event: 'the work now: tokens a minute and running turns', moves: 'the swell rises above the roll and rolls faster', pace: 'sea', on: 'canvas' },
  { id: 'roll', event: 'Live mode, with nothing happening (the sea never stops: theseus-42ic)', moves: 'the swell rolls slowly and low', pace: 'roll', on: 'canvas' },
] as const satisfies readonly Motion[]

export type MotionId = (typeof MOTIONS)[number]['id']

const ROW = new Map<string, Motion>(MOTIONS.map((m) => [m.id, m]))

/** A row of the table by its id. */
export const motion = (id: MotionId): Motion => ROW.get(id)!

/** Frames a second for the steady motions: half the display's, enough for a stroke or a gear, at half the cost. */
export const STEADY_FPS = 30

/** Frames a second for the steady motions once nothing has happened for `QUIET_AFTER_S` (theseus-n2hd): a job that
 *  runs for hours still turns its gear, at a third of the cost. */
export const QUIET_FPS = 10
export const QUIET_AFTER_S = 60

/** Frames a second for the idle roll (theseus-42ic): an idle Ship stays light. The roll's clock is wall time, so the
 *  waves land where they would at any rate; at its pace a row moves under a pixel a frame at the fleet's view, so a
 *  few frames a second still read as a slow roll. */
export const ROLL_FPS = 8

/** What moves now, as the engine knows it. */
export interface MotionState {
  /** Calm mode (or reduced motion): nothing plays; a change draws one frame. */
  calm: boolean
  /** The camera flies, or a vessel glides to its slot. */
  camera: boolean
  settling: boolean
  /** Engine seconds now, and when each one-off running ends (its event's time plus its seconds). */
  t: number
  until: Partial<Record<MotionId, number>>
  /** The states that move while they last. */
  rowing: boolean
  working: boolean
  streaming: boolean
  gears: boolean
  tethers: boolean
  currents: boolean
  /** The swell rolls above the roll: Live mode with work going on, or settling from it (the living sea). */
  sea: boolean
  /** The idle roll: Live mode with nothing happening, the sea at its roll. */
  roll: boolean
}

/** The motions running now, in the table's order. */
export function motionsNow(s: MotionState): MotionId[] {
  const on = new Set<MotionId>()
  if (s.camera) on.add('camera')
  if (s.settling) on.add('settle')
  if (!s.calm) {
    for (const [id, end] of Object.entries(s.until) as [MotionId, number][]) if (s.t < end) on.add(id)
    if (s.rowing) on.add('rowing')
    if (s.working) on.add('working')
    if (s.streaming) on.add('stream')
    if (s.gears) on.add('gear')
    if (s.tethers) on.add('tether')
    if (s.currents) on.add('current')
    if (s.sea) on.add('sea')
    else if (s.roll) on.add('roll')
  }
  return MOTIONS.map((m) => m.id).filter((id) => on.has(id))
}

/** How the loop draws for the motions running: every display frame, a steady pace (`quiet`: the quiet one, after a
 *  minute with no event), the sea's pace, the roll's, or not at all (`fps` 0); and whether a frame draws the whole
 *  scene or the sea alone. The page's motions (a coin's flight) are the browser's, and ask nothing of the canvas. */
export function paceOf(
  active: readonly MotionId[], idleFps: number, quiet = false,
): { fps: number; full: boolean; display: boolean } {
  const rows = active.map(motion).filter((m) => m.on === 'canvas')
  if (rows.some((m) => m.pace === 'display')) return { fps: 60, full: true, display: true }
  if (rows.some((m) => m.pace === 'steady')) return { fps: quiet ? QUIET_FPS : STEADY_FPS, full: true, display: false }
  if (rows.some((m) => m.pace === 'sea')) return { fps: idleFps, full: false, display: false }
  if (rows.some((m) => m.pace === 'roll')) return { fps: ROLL_FPS, full: false, display: false }
  return { fps: 0, full: false, display: false }
}

/** What the loop remembers to tell a quiet stretch: when the last event was (engine seconds), and the steady motions
 *  then. */
export interface Quiet {
  eventAt: number
  steady: string
}

/** Hear a frame's motions: a one-off playing (the operator's camera aside) or the steady motions changing (a turn
 *  starting to row, a job's gear starting or stopping) is an event. Says whether the lasting states have gone
 *  `QUIET_AFTER_S` with none, so they draw at `QUIET_FPS` (theseus-n2hd). */
export function heard(q: Quiet, active: readonly MotionId[], t: number): boolean {
  const steady = active.filter((id) => motion(id).pace === 'steady').join(' ')
  if (steady !== q.steady || active.some((id) => id !== 'camera' && motion(id).pace === 'display')) q.eventAt = t
  q.steady = steady
  return t - q.eventAt >= QUIET_AFTER_S
}
