// The model menu's floor (theseus-jid7t.6): which profile a session's data mark refuses. The daemon's
// `data_mark::refusal_of` decides; this is its table in TypeScript, and `test/ops-floor.test.ts` holds both to the one
// file, `test/fixtures/profile_floor.json`, that theseus-core's `tests_profile_floor.rs` reads too. A profile the daemon
// clears (`ProfileInfo.cleared`: US origin and a catalog row of `us`) runs anywhere; a provider's vouch alone clears
// nothing, and a daemon that did not say clears nothing. Any other profile is refused a private or sensitive session;
// one that is not configured is refused always. The page greys what the floor refuses, in the floor's words.
import type { DataMark } from '@protocol'

/** What the menu knows of a profile: its name, and its origin and the daemon's own floor answer (`ProfileInfo.origin`, `.cleared`). */
export interface FloorProfile { name: string; origin?: string; cleared?: boolean }

/** Why `mark` refuses `profile`, in the daemon's words, or null if it runs. A missing profile is not configured. */
export function greyOf(mark: DataMark | undefined, name: string, profile: FloorProfile | undefined): string | null {
  if (!profile) return `\`${name}\` is not a configured profile`
  if ((mark ?? 'open') === 'open' || profile.cleared === true) return null
  return `this conversation holds ${mark === 'sensitive' ? 'sensitive' : 'private'} content, and \`${name}\` is not a model of US origin`
}
