// Health's binary, read for the eye (theseus-od13): whether this daemon's jobs can write the binary it runs. Its tone
// and its words, which the Systems view's card and the header's dot share, in the CLI's words (`binary_line` in
// `crates/theseus/src/render.rs`).
import type { BinaryStatus } from '@protocol'
import type { Tone } from './taxonomy'

/** jobs_can_write faults; unknown waits (the check could not read it); ok is quiet; unreported is idle. */
export function binaryTone(b: BinaryStatus | undefined): Tone {
  switch (b?.state) {
    case 'jobs_can_write': return 'fault'
    case 'unknown': return 'wait'
    case 'ok': return 'ok'
    default: return 'idle'
  }
}

/** The state in one line: the CLI's `binary:` line, which it says only for the two states that need a word. */
export function binaryLine(b: BinaryStatus | undefined): string {
  switch (b?.state) {
    case 'jobs_can_write':
      return `binary: JOBS CAN WRITE ${b.path}: ${b.detail} (run the builder as its own user: theseusd install --separate)`
    case 'unknown': return `binary: unknown: ${b.detail}`
    case 'ok': return `binary: ok: jobs cannot write ${b.path}`
    default: return 'binary: not reported by this daemon'
  }
}
