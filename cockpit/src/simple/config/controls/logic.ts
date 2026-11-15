// What the More controls tab says and sends, worked out apart from the page (issue 17, step 5): the words each act asks in,
// and the params each form builds. Pure, so `npm test` runs it (`test/simple-config-controls.test.ts`).
import type { ImportTagInfo, McpServerStatus, OntologyProposal, PersonaInfo, PersonaSetParams } from '../../../protocol.gen/index.ts'

const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString('en-US')} ${n === 1 ? one : many}`

/** A form's lines, one item each: trimmed, the blank ones dropped. */
export const linesOf = (text: string): string[] => text.split('\n').map((l) => l.trim()).filter(Boolean)

/** A folder is something `project.trust` can take: an absolute path, which the daemon checks again. */
export const absolute = (path: string): boolean => path.trim().startsWith('/') || /^[A-Za-z]:[\\/]/.test(path.trim())

export const trustWords = (path: string): string =>
  `Theseus will treat the repository that holds ${path.trim()} as its own: the AGENTS.md and CLAUDE.md files in it join what every conversation there starts from, from each conversation’s next refresh. Trust only a repository you wrote or have read, because those files tell Theseus how to behave. You can stop trusting it afterwards.`

export const untrustWords = (root: string): string => `Theseus stops reading the AGENTS.md and CLAUDE.md of ${root}. Conversations already open drop them at their next refresh.`

/** `project.create`'s params from the form: a title, an objective, what done looks like (one line each). */
export function projectParams(title: string, objective: string, acceptance: string) {
  return { title: title.trim(), ...(objective.trim() ? { objective: objective.trim() } : {}), acceptance: linesOf(acceptance), milestones: [] }
}

export const projectWords = (title: string, acceptance: number): string =>
  `Theseus starts a project called “${title.trim()}” with ${plural(acceptance, 'line')} saying what done looks like. It makes the project’s first tasks from them, and those can start work.`

/** The standing rule's kind and scope, in words a person chooses between. */
export const KINDS = [['order', 'An order: always do, or never do'], ['preference', 'A preference: how I like things'], ['fact', 'A fact: something true that matters']] as const

export const standingWords = (kind: string, text: string, everywhere: boolean): string =>
  `Theseus will carry this ${kind} into ${everywhere ? 'every conversation' : 'every conversation on this page’s surface'} until you revoke it:\n\n“${text.trim()}”\n\nIt changes how Theseus behaves from the next turn.`

export const keepWords = (text: string): string => `Keep this as a standing rule, so Theseus carries it into every turn of its persona until you revoke it?\n\n“${text}”`

/** The three answers `standing.answer` takes, in words. */
export const ANSWERS = [['keep', 'Keep it where it applies'], ['everywhere', 'Keep it everywhere'], ['not', 'Do not keep it']] as const

export const restartWords = (s: Pick<McpServerStatus, 'name' | 'state' | 'calls'>): string =>
  `Theseus stops the tool server “${s.name}” (it is ${s.state}) and starts it again. A call to it that is running right now is cut off; its tools come back when it is up.`

export const REBUILD_WORDS = 'Theseus throws away the search index of everything it remembers and builds it again from the record. Nothing it remembers is lost, but search answers are slower and less complete until the build is done, which can take a while on a long record.'

export const releaseWords = (live: string): string => `Theseus stops holding “${live}” as the live model and goes back to the model its configuration file names. New turns use that one.`

export const UNPIN_WORDS = 'Every conversation and task that was fixed to a particular model is let go of: from its next turn each uses the live model. The models you chose for them are forgotten.'

/** The conversations of an import that are still held: `import.list`'s count includes the ones already erased. */
export const heldOf = (t: Pick<ImportTagInfo, 'sessions' | 'erased'>): number => Math.max(0, t.sessions - t.erased)

/** What erasing an import does, exactly, from the tag’s own counts. */
export function eraseWords(t: Pick<ImportTagInfo, 'tag' | 'sessions' | 'erased'>): string {
  return `Theseus erases everything the import “${t.tag}” still holds: ${plural(heldOf(t), 'conversation')} and the memory entries in them. Each is marked erased and dropped from search, and the topics and people that import made, which nothing else uses, are removed with them. This cannot be undone from here; to get them back the original files would have to be imported again.`
}

export const learnWords = (pack: string): string => `Theseus runs the learning loop for “${pack}” now, instead of waiting for tonight: it trains on recent labelled events and tests the result on events it kept apart. It writes a new version of the pack; moving that version up is a separate step on the Judging tab.`

export const backfillWords = (pack: string, since: string): string => `Theseus judges events since ${since} again with “${pack}”, for the ones the model missed. This calls a model and costs money, up to the limit set in the configuration file, and it refuses unless the configuration file says you consent. It records which configuration it ran under.`

/** A day as the daemon reads it (`2026-10-01`). */
export const isDay = (s: string): boolean => /^\d{4}-\d{2}-\d{2}$/.test(s.trim())

export const personaImportWords = (from: string, name: string): string => `Theseus brings the agent found at ${from.trim()} over as a persona called “${name.trim()}”: its files are copied in beside Theseus’s own. Run the preview first to see exactly which files would come in.`

/** `persona.set`'s params: only the fields that differ from what the persona has now, so an untouched field is not rewritten. */
export function personaChanges(p: Pick<PersonaInfo, 'name' | 'emoji' | 'card' | 'tts_voice' | 'initiative'>, key: string, edit: Partial<Record<'name' | 'emoji' | 'card' | 'tts_voice' | 'initiative', string>>): PersonaSetParams | null {
  const out: PersonaSetParams = { key }
  let n = 0
  for (const f of ['name', 'emoji', 'card', 'tts_voice', 'initiative'] as const) {
    const v = edit[f]
    if (v !== undefined && v.trim() !== (p[f] ?? '')) { out[f] = v.trim(); n++ }
  }
  return n ? out : null
}

export const retireKindWords = (name: string): string => `Theseus takes the kind “${name}” away. This works only for a kind that nothing holds: if a category or a conversation still uses it, Theseus refuses and says which. Bringing it back means adding it again.`

/** A proposal row's name, and whether it can be accepted as it stands (a new topic with no name needs one typed). */
export const proposalName = (p: OntologyProposal): string => p.topic_name ?? p.topic ?? (p.new_topic ? 'a new topic' : 'a topic with no name')
export const needsName = (p: OntologyProposal): boolean => p.new_topic && !(p.topic_name ?? p.topic)

export const acceptWords = (p: OntologyProposal, name: string): string => `Accept “${name}” for ${p.session_title ?? p.session_id}: the topic is filed and the conversation is put in it.`

export const PAUSE_WORDS = 'No new self-improvement build starts until you let them start again. Builds already running finish, and reviews and joins go on. A restart of Theseus clears the pause.'

export const UNPAUSE_WORDS = 'New self-improvement builds may start again, from the next time the loop looks.'

export const RESUME_WORDS = 'Theseus’s self-improvement was stopped by a halt. Resuming lets every self step run again, including builds that change Theseus’s own code. Only you can do this, and it is written to the record with your name.'
