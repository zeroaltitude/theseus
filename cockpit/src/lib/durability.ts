// The durability tender, read for the eye (theseus-9ai1): the AWS card's durability row, in the words of the CLI's
// `durability` line (`durability_lines` in `crates/theseus/src/render/aws.rs`). A tender that is failing or stopped
// must not look as if the store were shipped off the machine.
import type { AwsDurabilityStatus } from '@protocol'
import type { Tone } from './taxonomy'

export type DurabilityView = {
  /** The state in words, for the pill. */
  label: string
  /** caught_up ok; waiting and shipping idle; failing wait (it retries); stopped fault; anything else idle. */
  tone: Tone
  /** Why, when the tender says: a waiting tender's reason, a failing or stopped one's error. */
  error?: string
  /** What is shipped and when, then the lag. */
  shipped: string
  lag: string
  /** The counts since the start, and where it ships. */
  counts: string
  where: string
}

const STATE: Record<string, [string, Tone]> = {
  caught_up: ['caught up', 'ok'],
  shipping: ['shipping', 'idle'],
  waiting: ['waiting', 'idle'],
  failing: ['failing: it retries', 'wait'],
  stopped: ['stopped: the store is not shipped', 'fault'],
}

const n = (x: number) => x.toLocaleString('en-US')
const plural = (x: number, one: string) => `${n(x)} ${one}${x === 1 ? '' : 's'}`
const mins = (ms: number) => `${Math.floor(Math.max(0, ms) / 60_000)} min`
const secs = (ms: number) => {
  const s = Math.floor(Math.max(0, ms) / 1000)
  return s < 120 ? `${s} s` : `${Math.floor(s / 60)} min`
}

/** The tender's status as the card shows it, at `now` (unix ms). */
export function durabilityView(d: AwsDurabilityStatus, now: number): DurabilityView {
  const [label, tone] = STATE[d.state] ?? [d.state, 'idle' as Tone]
  const shipped = d.last_shipped_unix_ms !== undefined
    ? `to position ${n(d.shipped_to_position)}, ${mins(now - d.last_shipped_unix_ms)} ago`
    : d.shipped_to_position === 0 ? 'nothing shipped yet' : `to position ${n(d.shipped_to_position)} (before this start)`
  const lag = d.oldest_unshipped_unix_ms !== undefined
    ? `${secs(d.lag_ms)}: the oldest record not yet shipped was written ${mins(now - d.oldest_unshipped_unix_ms)} ago`
    : 'nothing unshipped'
  return {
    label,
    tone,
    error: d.error,
    shipped,
    lag,
    counts: [plural(d.segments, 'segment'), plural(d.tails, 'tail'), plural(d.blobs, 'blob'), plural(d.rows, 'row'), plural(d.bytes, 'byte')].join(', '),
    where: `s3://${d.bucket}/${d.prefix} and the table ${d.table}`,
  }
}
