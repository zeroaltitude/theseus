// The model menu's floor (theseus-jid7t.6): which profile the menu greys for a session. The restricted-origin rule is
// gone (2026-11-21): the daemon sends `ProfileInfo.cleared: true` for every profile, so in practice only a profile that is
// not configured is grey. The origin branch below reads a `cleared` the daemon no longer withholds; `test/ops-floor.test.ts`
// still holds it to the old table, `test/fixtures/profile_floor.json`.
import type { DataMark } from '@protocol'

/** What the menu knows of a profile: its name, and its origin and the daemon's own floor answer (`ProfileInfo.origin`, `.cleared`). */
export interface FloorProfile { name: string; origin?: string; cleared?: boolean }

/** Why `mark` refuses `profile`, in the daemon's words, or null if it runs. A missing profile is not configured. */
export function greyOf(mark: DataMark | undefined, name: string, profile: FloorProfile | undefined): string | null {
  if (!profile) return `\`${name}\` is not a configured profile`
  if ((mark ?? 'open') === 'open' || profile.cleared === true) return null
  const level = mark === 'sensitive' ? 'sensitive' : 'private'
  if (profile.origin === 'us') return `this conversation holds ${level} content, and \`${name}\` is not cleared for it: the catalog does not list its model as of US origin`
  return `this conversation holds ${level} content, and \`${name}\` is not a model of US origin`
}
