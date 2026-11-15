// What the simple Memories page finds and where it sends you (GitHub issue 17): the words a search is answered for, a
// person's or a topic's page over the ontology's categories and memberships, and the page's own addresses. Pure:
// `npm test` runs it (test/simple-memories.test.ts); the page passes in what the daemon read.
//
// Everything here reads what the Ship's explorer reads (`ontology.list`, `import.sessions`, `memory.search`); there is
// no second store of people or topics. A person is an ontology category of kind `person`, a topic one of kind `topic`,
// and a session belongs to one through a membership.
import type { OntologyCategory, OntologyMembership } from '@protocol'
import { handlesLine, matches, peopleOf } from '../../lib/people.ts'

/** How many of a page's conversations are read and listed at once. */
export const SHOWN = 25
/** How many people, topics and episodes a search lists before it says there are more. */
export const FOUND = 8

/** The words of a search, trimmed; none when there are none to ask for. */
export const wordsOf = (q: string | null): string => (q ?? '').trim().replace(/\s+/g, ' ')

// ---------------------------------------------------------------- the page's addresses

const here = (v: string, q: Record<string, string | null | undefined>): string => {
  const p = new URLSearchParams()
  if (v) p.set('v', v)
  for (const [k, x] of Object.entries(q)) if (x) p.set(k, x)
  const s = p.toString()
  return s ? `/memories?${s}` : '/memories'
}

export const searchHref = (q: string): string => here('', { q: wordsOf(q) })
export const personHref = (id: string): string => here('people', { sel: id })
export const topicHref = (id: string): string => here('topics', { sel: id })
/** An imported episode, opened among the episodes (the explorer's own tab, kept in `?v=context`). */
export const episodeHere = (session: string): string => here('context', { episode: session })
/** What one turn was shown: a session, and a turn of it (the newest when none is named). */
export const turnHere = (session: string, turn?: string | null): string => here('turn', { session, turn })
/** A session's own page, under the simple bar. */
export const sessionHref = (session: string): string => `/session/${encodeURIComponent(session)}`
/** The explorer's own list of episodes, kept to some words or to a topic and what is filed under it. */
export const exploreHref = (q: string): string => here('context', { q: wordsOf(q) })
export const exploreTopic = (label: string): string => here('context', { topic: label })
/** A row of the people or topics list: that page, with the list's words kept so going back finds the list as it was. */
export const listHref = (kind: 'person' | 'topic', id: string, words: string): string => here(kind === 'person' ? 'people' : 'topics', { sel: id, f: words })
/** The whole list of people or of topics, narrowed to some words. */
export const listAll = (kind: 'person' | 'topic', words: string): string => here(kind === 'person' ? 'people' : 'topics', { f: words })
/** The page of a person or a topic by what kind it is. */
export const pageHref = (c: Pick<OntologyCategory, 'id' | 'kind'>): string => (c.kind === 'person' ? personHref(c.id) : topicHref(c.id))

// ---------------------------------------------------------------- people and topics

/** A category's names from its root, `a/b/c`: what an imported topic label is when the ontology holds it. */
export function pathOf(categories: readonly OntologyCategory[], c: OntologyCategory): OntologyCategory[] {
  const byId = new Map(categories.map((x) => [x.id, x]))
  const chain: OntologyCategory[] = []
  for (let at: OntologyCategory | undefined = c, guard = 0; at && guard < 32; guard++) {
    chain.unshift(at)
    at = at.parent ? byId.get(at.parent) : undefined
  }
  return chain
}

/** The topic label `import.sessions`' `topic` filter takes for a category: its names from the root, joined by slashes. */
export const labelOf = (categories: readonly OntologyCategory[], c: OntologyCategory): string => pathOf(categories, c).map((x) => x.name).join('/')

/** A category's children, by name. */
export const childrenOf = (categories: readonly OntologyCategory[], id: string): OntologyCategory[] =>
  categories.filter((c) => c.parent === id).sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id))

/** The topics `q` answers (name or id, every word), roots first then by name. An empty search answers every one. */
export function topicsOf(categories: readonly OntologyCategory[], q = ''): OntologyCategory[] {
  return categories
    .filter((c) => c.kind === 'topic' && matches(c, q))
    .sort((a, b) => a.depth - b.depth || (b.members ?? 0) - (a.members ?? 0) || a.name.localeCompare(b.name) || a.id.localeCompare(b.id))
}

export interface Found { people: OntologyCategory[]; topics: OntologyCategory[]; morePeople: number; moreTopics: number }

/** The people and topics a search names: the first few of each, and how many more there are. A search of no words names none. */
export function foundKinds(categories: readonly OntologyCategory[], q: string, shown = FOUND): Found {
  if (!wordsOf(q)) return { people: [], topics: [], morePeople: 0, moreTopics: 0 }
  const people = peopleOf(categories, q)
  const topics = topicsOf(categories, q)
  return { people: people.slice(0, shown), topics: topics.slice(0, shown), morePeople: Math.max(0, people.length - shown), moreTopics: Math.max(0, topics.length - shown) }
}

/** The line under a person or a topic in a list: a person's handles, a topic's place in the tree and its size. */
export function lineOf(categories: readonly OntologyCategory[], c: OntologyCategory): string {
  if (c.kind === 'person') return handlesLine(c) || (c.description ? c.description : '')
  const up = pathOf(categories, c).slice(0, -1).map((x) => x.name)
  return [up.length ? `under ${up.join(' / ')}` : 'a main topic', c.members ? `${c.members} ${c.members === 1 ? 'conversation' : 'conversations'}` : ''].filter(Boolean).join(' · ')
}

// ---------------------------------------------------------------- whose conversations

export interface Holders {
  /** Imported episodes (`ses_ep…`), newest membership first. */
  imported: string[]
  /** Theseus's own sessions, newest membership first. */
  native: string[]
  total: number
}

/** The sessions that hold a category, split into the imported episodes and Theseus's own, newest membership first. */
export function holdersOf(memberships: readonly OntologyMembership[], category: string): Holders {
  const seen = new Set<string>()
  const mine = memberships.filter((m) => m.category === category && !seen.has(m.session_id) && !!seen.add(m.session_id)).sort((a, b) => b.as_of_ms - a.as_of_ms)
  const imported = mine.filter((m) => m.session_id.startsWith('ses_ep')).map((m) => m.session_id)
  const native = mine.filter((m) => !m.session_id.startsWith('ses_ep')).map((m) => m.session_id)
  return { imported, native, total: mine.length }
}

/** An excerpt cut at a word, with an ellipsis when it was cut. */
export function excerpt(text: string, max = 280): string {
  const flat = text.replace(/\s+/g, ' ').trim()
  if (flat.length <= max) return flat
  const cut = flat.slice(0, max)
  return `${cut.slice(0, Math.max(cut.lastIndexOf(' '), max - 24))}…`
}
