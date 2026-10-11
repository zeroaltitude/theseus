// People in the ontology (theseus-wy7y): the pulldowns' groups (topics and people, each searchable by name, id and a
// person's handles), a person's line, and the proposals' selection; a proposed person's row (theseus-fvyx), its words
// and its one accept or reject. Pure, with no import but the protocol's types, so
// `node --test` runs its test (`cockpit/test/people.test.ts`) as it is.
import type {
  OntologyCategory, OntologyPersonProposals, OntologyProposal, OntologyProposalAcceptAllParams, OntologyProposalRejectAllParams,
} from '@protocol'

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

/** A proposal's kind: a person's (people.v1's, held or new), else the kind of the category it names, or `topic` for a new topic. */
export const proposalKind = (p: OntologyProposal) => (p.person ? 'person' : p.topic ? p.topic.split(':')[0] : 'topic')

/** Whether a proposal can be accepted in bulk: it names a held category, or a new person by name (a new topic needs a name, one at a time). */
export const bulkable = (p: OntologyProposal) => !p.new_topic && (!!p.topic || !!p.person)

/** What a proposal proposes, in words: a person's name, handles and role line; a topic's name. */
export const proposalWhat = (p: OntologyProposal): string => {
  if (p.person) {
    const h = p.person.handles?.length ? ` (${p.person.handles.join(', ')})` : ''
    const role = p.person.role_line ? `: ${p.person.role_line}` : ''
    return `${p.person.new ? 'a new person, ' : ''}${p.person.name}${h}${role}`
  }
  return p.new_topic ? 'a new topic' : p.topic_name ?? p.topic ?? '?'
}

/** The judgments "select all" picks: every bulkable proposal of `kind` (or of any) at `min` or more. */
export function selectAll(ps: readonly OntologyProposal[], kind: string | null, min = 0): string[] {
  return ps.filter((p) => bulkable(p) && (kind === null || proposalKind(p) === kind) && p.confidence >= min).map((p) => p.judgment)
}

/** A proposed person's row in words: its sessions and proposals, their confidence range and bands. */
export function rowMeta(g: OntologyPersonProposals): string {
  const n = g.judgments.length
  const range = g.confidence_min === g.confidence_max ? g.confidence_max.toFixed(2) : `${g.confidence_min.toFixed(2)}–${g.confidence_max.toFixed(2)}`
  return [
    g.new ? 'new' : g.key,
    `${g.sessions} session${g.sessions === 1 ? '' : 's'}`,
    `${n} proposal${n === 1 ? '' : 's'}`,
    `${range} ${g.bands.join('/')}`,
  ].join(' · ')
}

/** The first names a row holds, as the question its accept answers: `with “Marlo” as Marlo Quill?`; or, for an
 *  ambiguous first name, the people it may be. Empty when neither. */
export function rowAsk(g: OntologyPersonProposals): string {
  if (g.ambiguous?.length) return `ambiguous: a word of ${g.ambiguous.join(', ')}; accept it alone, with --as`
  if (g.first_names?.length) return `with ${g.first_names.map((f) => `“${f}”`).join(', ')} as ${g.name}?`
  return ''
}

/** A row's one accept: every proposal of it, each new person's as the row's person. */
export const rowAccept = (g: OntologyPersonProposals): OntologyProposalAcceptAllParams =>
  ({ min_confidence: 0, judgments: [...g.judgments], as_person: g.as_person })

/** A row's one reject: every proposal of it. */
export const rowReject = (g: OntologyPersonProposals): OntologyProposalRejectAllParams => ({ judgments: [...g.judgments] })

/** Whether a row can be accepted in bulk: an ambiguous first name is accepted alone (`theseus ontology accept --person
 *  NAME --as PERSON`). */
export const rowBulkable = (g: OntologyPersonProposals) => !(g.ambiguous?.length)

/** The rows "select all" picks: each bulkable row whose best proposal reaches `min`, by key. */
export function selectRows(rows: readonly OntologyPersonProposals[], min = 0): string[] {
  return rows.filter((g) => rowBulkable(g) && g.confidence_max >= min).map((g) => g.key)
}
