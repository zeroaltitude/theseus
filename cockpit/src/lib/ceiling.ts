// A place's ceiling in words (step 38a, theseus-ext.3): what the bindings file
// narrows there, as `theseus places` says it.
import type { PlaceCeiling } from '@protocol'

export function ceilingWords(c: PlaceCeiling): string {
  const parts: string[] = []
  if (c.posture_floor) parts.push(`floor ${c.posture_floor}`)
  if (c.tools) parts.push(c.tools.length ? `tools ${c.tools.join(',')}` : 'no tools')
  if (c.spend_limit_usd != null) parts.push(`spend ≤ $${c.spend_limit_usd.toFixed(2)}`)
  if (c.profile) parts.push(`profile ${c.profile}`)
  return parts.join(' · ')
}
