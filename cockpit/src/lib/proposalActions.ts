// The proposals panel's Reject and Never (theseus-648f8): which call each sends for a topic row, a person row and an
// unnamed new-topic row, and the words that say what a Never does. Pure, with no import but the protocol's types, so
// `node --test` runs its test (`cockpit/test/proposalactions.test.ts`) as it is.
import type { OntologyNever, OntologyPersonProposals, OntologyProposal } from '@protocol'

export type Answer = 'reject' | 'never'

/** The method an answer to a set of proposals calls: a reject is `reject_all` (it takes any listed judgment, topics
 *  included), a never is `ontology.proposal.never`. */
export const answerMethod = (a: Answer) => (a === 'never' ? 'ontology.proposal.never' : 'ontology.proposal.reject_all')

/** The one call an answer sends for `judgments`, as `writeEach` takes it. */
export const answerCall = (a: Answer, judgments: readonly string[]): [string, unknown] => [answerMethod(a), { judgments: [...judgments] }]

/** A topic row's name: the topic's, or, for a new topic that has none, "a new topic (unnamed)". */
export const topicLabel = (p: OntologyProposal): string => (p.new_topic ? 'a new topic (unnamed)' : p.topic_name ?? p.topic ?? '?')

/** What a Never on one topic row keeps, in words. */
export function neverWords(p: OntologyProposal): string {
  if (p.new_topic) return 'never propose a new topic for this session'
  return `never propose “${p.person ? p.person.name : topicLabel(p)}” again`
}

/** The confirm text of an answer to rows: topics by name (an unnamed one by its session), people by name. */
export function confirmWords(a: Answer, topics: readonly OntologyProposal[], people: readonly OntologyPersonProposals[]): string {
  const n = topics.length + people.reduce((s, g) => s + g.judgments.length, 0)
  const proposals = `${n} proposal${n === 1 ? '' : 's'}`
  const names = [
    ...topics.map((p) => (p.new_topic ? `a new topic in ${p.session_title ?? p.session_id}` : topicLabel(p))),
    ...people.map((g) => g.name),
  ]
  const list = names.length > 6 ? `${names.slice(0, 6).join(', ')} and ${names.length - 6} more` : names.join(', ')
  if (a === 'reject') return `Reject ${proposals} (${list})?\n\nEach is labelled rejected; nothing else changes, and Jev may propose them again.`
  return `Never propose these again — ${proposals} (${list})?\n\nEach is rejected, and what it proposes is kept as a never: a topic’s name, a person’s name, or, for a new topic with no name, that session. Undo it under nevers.`
}

/** The confirm text of one never in the list taken back. */
export const undoWords = (n: OntologyNever): string => `Undo the never on ${n.kind} “${n.name}”?\n\nIt may be proposed again.`

/** A never as a line: its kind and name, who kept it. */
export const neverLine = (n: OntologyNever): string => `${n.kind} · ${n.kind === 'session' ? 'no new topic for session ' : ''}${n.name}`
