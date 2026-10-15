// The tool families each kind of place loads or defers (theseus-7m7w.2), from `tool.defer`: pure, so a test holds the
// words. A family is loaded or deferred for a kind of place, with the reason the selector gave, its use in the window,
// and its misses (a call to one of its tools that was not loaded and no search had found).
import type { DeferFamily, DeferListResult, DeferPlace } from '@protocol'

/** The families of a place in the order the list shows them: the loaded ones first, then by misses, then by name. */
export function orderFamilies(p: DeferPlace): DeferFamily[] {
  return [...p.families].sort(
    (a, b) => Number(b.loaded) - Number(a.loaded) || b.missed - a.missed || a.family.localeCompare(b.family),
  )
}

/** One family's line: how it was used in the window, in words. */
export function usageWords(f: DeferFamily): string {
  const parts = [`used in ${f.used_turns} ${f.used_turns === 1 ? 'turn' : 'turns'}`]
  if (f.searched > 0) parts.push(`${f.searched} ${f.searched === 1 ? 'search' : 'searches'}`)
  if (f.missed > 0) parts.push(`${f.missed} missed`)
  if (f.loaded && f.used_turns === 0 && f.unused_turns > 0) parts.push('loaded, never used')
  return parts.join(' · ')
}

/** A place's summary: how many families load of all, and the misses in the window. */
export function placeWords(p: DeferPlace): string {
  const loaded = p.families.filter((f) => f.loaded).length
  const missed = p.families.reduce((n, f) => n + f.missed, 0)
  return `${loaded} of ${p.families.length} families loaded · tools ≈${p.tokens_sent} of ${p.tokens_all} tokens a request · ${p.turns} ${p.turns === 1 ? 'turn' : 'turns'}${missed ? ` · ${missed} missed` : ''}`
}

/** How the rest is reached, for the panel's note. */
export function reachWords(d: DeferListResult): string {
  if (!d.enabled) return 'every tool loads ([tools.defer] enabled = false)'
  return d.mechanism === 'search'
    ? 'a deferred tool is found by the model through the provider’s tool search'
    : 'a deferred tool is found with tools_find and loads at the conversation’s next compaction'
}
