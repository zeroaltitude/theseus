// Health's disk, read for the eye (theseus-51v8): its tone and its one-line summary, which the Systems view's card,
// the status strip's dot, and its attention item share.
import type { DiskStatus } from '@protocol'
import type { Tone } from './taxonomy'

/** ok; low (under the warning) waits; below the floor (new jobs refused) faults; unknown or unreported is idle. */
export function diskTone(d: DiskStatus | undefined): Tone {
  switch (d?.state) {
    case 'ok': return 'ok'
    case 'low': return 'wait'
    case 'below_floor': return 'fault'
    default: return 'idle'
  }
}

export const DISK_STATE: Record<string, string> = { ok: 'ok', low: 'low', below_floor: 'below the floor', unknown: 'unknown' }

/** Health counts whole MB (MiB), as the CLI prints them. */
export const mb = (n: number) => `${n.toLocaleString()} MB`

/** The state, what is free, and the limits, in one line: the CLI's `disk:` line. */
export function diskSummary(d: DiskStatus | undefined): string {
  if (!d?.path) return 'disk: not reported by this daemon'
  if (d.state === 'unknown') return `disk: unknown under ${d.path}: ${d.error ?? 'not read'}`
  const limits = [
    d.warn_mb > 0 ? `health warns below ${mb(d.warn_mb)}` : '',
    d.floor_mb > 0 ? `jobs are refused below ${mb(d.floor_mb)}` : '',
  ].filter(Boolean)
  return `disk ${DISK_STATE[d.state] ?? d.state}: ${mb(d.free_mb)} free of ${mb(d.total_mb)} under ${d.path}${limits.length ? ` · ${limits.join(', ')}` : ''}`
}
