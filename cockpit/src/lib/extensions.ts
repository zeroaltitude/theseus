// The Extensions card's pure parts (M7 43b): a digest as a card names it, an extension's network in the CLI's words,
// and the proposals that are not what runs.
import type { ExtendListResult } from '@protocol'

/** A digest as a card names it: its first six characters. */
export function shortDigest(d: string): string {
  return d.slice(0, 6)
}

/** The network an extension may reach, in the CLI's words. */
export function networkWords(hosts: readonly string[]): string {
  return hosts.length ? `network to ${hosts.join(', ')}` : 'no network'
}

/** Every proposal but the versions loaded now, newest first as the list has them. */
export function proposalsNotLoaded(list: ExtendListResult) {
  const running = new Set((list.loaded ?? []).map((l) => `${l.name}.${l.digest}`))
  return list.extensions.filter((e) => !running.has(`${e.name}.${e.digest}`))
}
