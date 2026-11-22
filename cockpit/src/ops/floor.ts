// The model menu's floor (theseus-jid7t.6): which profile the menu greys for a session. The restricted-origin rule is
// gone (2026-11-21) and `ProfileInfo.cleared` with it: no label keeps a session off a model, so the menu greys only a
// profile that is not configured. The mark is still passed by every menu and decides nothing.
import type { DataMark } from '@protocol'

/** What the menu knows of a profile: its name. */
export interface FloorProfile { name: string }

/** Why the menu greys `name`, or null if it runs. A missing profile is not configured; every configured one runs. */
export function greyOf(_mark: DataMark | undefined, name: string, profile: FloorProfile | undefined): string | null {
  return profile ? null : `\`${name}\` is not a configured profile`
}
