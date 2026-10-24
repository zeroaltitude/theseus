// The model menu's floor (theseus-jid7t.6): which profile a session's data mark refuses. The daemon's
// `data_mark::refusal_of` decides; this is its table in TypeScript, and `test/ops-floor.test.ts` holds both to the one
// file, `test/fixtures/profile_floor.json`, that theseus-core's `tests_profile_floor.rs` reads too. A profile of US
// origin runs anywhere; any other origin (or none told) is refused a private or sensitive session; a profile that is
// not configured is refused always. The page greys what the floor refuses, in the floor's words.
import type { DataMark } from '@protocol'

/** What the menu knows of a profile: its name, and its origin if the daemon said (`ProfileInfo.origin`). */
export interface FloorProfile { name: string; origin?: string }

/** Why `mark` refuses `profile`, in the daemon's words, or null if it runs. A missing profile is not configured. */
export function greyOf(mark: DataMark | undefined, name: string, profile: FloorProfile | undefined): string | null {
  if (!profile) return `\`${name}\` is not a configured profile`
  if ((mark ?? 'open') === 'open' || profile.origin === 'us') return null
  return `this conversation holds ${mark === 'sensitive' ? 'sensitive' : 'private'} content, and \`${name}\` is not a model of US origin`
}
