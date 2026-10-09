// People in the ontology (theseus-wy7y): the pulldowns' groups (topics and people, each searchable by name, id and a
// person's handles), a person's line, and the proposals' selection. Pure, with no import but the protocol's types, so
// `node --test` runs its test (`cockpit/test/people.test.ts`) as it is.
import type { OntologyCategory, OntologyProposal } from '@protocol'

/** The kinds a session may be added to by hand, in the order the pulldowns show them, with each group's label. */
export const ADDABLE: readonly { kind: string; label: string }[] = [
  { kind: 'topic', label: 'topics' },
  { kind: 'person', label: 'people' },
]

export interface Group { kind: string; label: string; items: OntologyCategory[] }

/** Whether a category answers a search: its name, its id, or one of a person's handles holds the words, in any case.
 *  An empty search answers every one. */
export function matches(c: OntologyCategory, q: string): boolean {
  const words = q.trim().toLowerCase()
  if (!words) return true
  const hay = [c.name, c.id, ...(c.handles ?? [])].join(' ').toLowerCase()
  return words.split(/\s+/).every((w) => hay.includes(w))
}

/** The pulldown's groups: each addable kind's categories that `q` answers and the session does not hold (`held`, by
 *  id), by name; an empty group is left out. */
export function pulldownGroups(categories: readonly OntologyCategory[], held: ReadonlySet<string>, q = ''): Group[] {
  return ADDABLE.map(({ kind, label }) => ({
    kind,
    label,
    items: categories
      .filter((c) => c.kind === kind && !held.has(c.id) && matches(c, q))
      .sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id)),
  })).filter((g) => g.items.length > 0)
}

/** A person's handles as a line, the display names last: `slack:U01 · discord:42 · aka Marlo, M.`. */
export function handlesLine(c: OntologyCategory): string {
  const hs = c.handles ?? []
  const ids = hs.filter((h) => !h.startsWith('name:'))
  const names = hs.filter((h) => h.startsWith('name:')).map((h) => h.slice(5)).filter((n) => n !== c.name)
  return [...ids, ...(names.length ? [`aka ${names.join(', ')}`] : [])].join(' · ')
}

/** The people, by sessions held then name: the people panel's order. */
export function peopleOf(categories: readonly OntologyCategory[], q = ''): OntologyCategory[] {
  return categories
    .filter((c) => c.kind === 'person' && matches(c, q))
    .sort((a, b) => (b.members ?? 0) - (a.members ?? 0) || a.name.localeCompare(b.name) || a.id.localeCompare(b.id))
}

/** A proposal's kind: the kind of the category it names, or `topic` for a new topic. */
export const proposalKind = (p: OntologyProposal) => (p.topic ? p.topic.split(':')[0] : 'topic')

/** Whether a proposal can be accepted in bulk: it names a held category (a new one needs a name, one at a time). */
export const bulkable = (p: OntologyProposal) => !p.new_topic && !!p.topic

/** The judgments "select all" picks: every bulkable proposal of `kind` (or of any) at `min` or more. */
export function selectAll(ps: readonly OntologyProposal[], kind: string | null, min = 0): string[] {
  return ps.filter((p) => bulkable(p) && (kind === null || proposalKind(p) === kind) && p.confidence >= min).map((p) => p.judgment)
}
