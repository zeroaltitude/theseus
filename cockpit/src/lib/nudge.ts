// The stop nudge in words (theseus-fh0u): the same line as `TurnNudge::line` in theseus-protocol, so the cockpit's
// turn view, the CLI, the terminal UI, and the reply's post on Discord say one thing.
import type { TurnNudge } from '@protocol'

/** `Nudged once: the reply said more work would follow; the model went on.` */
export function nudgeLine(n: TurnNudge): string {
  const then = n.acted ? 'the model went on' : 'the model answered without acting'
  return `Nudged once: the reply said more work would follow; ${then}.`
}
