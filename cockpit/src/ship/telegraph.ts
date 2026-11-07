// What the Ship's engine telegraph reads (theseus-h70f), pure so `npm test` runs it (test/telegraph.test.ts).
import type { Health } from '@protocol'

/** The telegraph's numbers. `running` and `ceiling` are null while health has not been read: the engine then says so
 *  rather than borrow another gauge's count. */
export interface EngineReading {
  accepting: boolean | undefined
  running: number | null
  held: number
  ceiling: number | null
}

/** The time machine's moment, as far as the engine reads it (`lib/timemachine.ts` gauges). */
export interface EngineMoment { accepting: boolean; running: number }

/** What the kernel runs, as the header's "running" and the kernel lamp read it. The daemon lists only the states that
 *  have executions, so no `running` key means none run: the ships the Ship draws working (a background job under a
 *  waiting execution) are not the kernel's turns and are never counted here. The ceiling is the config's
 *  (`[kernel] admission_ceiling`, through health), never a number of the cockpit's own. */
export function engineReading(h: Health | undefined, g: EngineMoment | undefined): EngineReading {
  return {
    accepting: g ? g.accepting : h?.kernel.accepting,
    running: g ? g.running : h ? h.kernel.executions_by_state.running ?? 0 : null,
    held: g ? 0 : h?.kernel.turns_held ?? 0,
    ceiling: h?.kernel.admission_ceiling ?? null,
  }
}

const ORDERS = ['STOP', 'SLOW', 'HALF', 'FULL'] as const
export { ORDERS }

/** The order the lever stands at: 0 STOP, 1 SLOW (idle), 2 HALF (up to half the ceiling), 3 FULL. */
export function engineOrder(r: EngineReading): number {
  if (r.accepting === false) return 0
  if (!r.running) return 1
  return r.ceiling === null || r.running * 2 <= r.ceiling ? 2 : 3
}

/** The caption under the dial. */
export function engineSub(r: EngineReading): string {
  if (r.running === null || r.ceiling === null) return 'not read yet'
  const word = r.accepting === false ? 'holding' : 'accepting'
  return `${word} · ${r.running} of ${r.ceiling}${r.held ? ` · ${r.held} held` : ''}`
}

/** The hover text. */
export function engineTitle(r: EngineReading): string {
  if (r.running === null || r.ceiling === null) return 'The kernel: not read yet.'
  return `The kernel: ${r.accepting === false ? 'holding new turns (STOP)' : 'accepting'}; ${r.running} of ${r.ceiling} executions running${r.held ? `, ${r.held} turns held` : ''}. SLOW is idle, HALF up to half the ceiling, FULL past it.`
}
