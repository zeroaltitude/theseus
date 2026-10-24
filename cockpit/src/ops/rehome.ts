// Re-homing by drag on the Graph and the Board (theseus-jid7t.4): what may move where, as one existing call. Pure: `npm test` runs
// it (test/ops-selection.test.ts).
//
// | drag                 | onto                                 | does                               | call                          | posture |
// | a task               | another conversation, a thread's lane | its report goes there from now on  | `task.rehome { task, home }`  | ask     |
// | a task, a conversation | a project                          | attaches it                        | `project.attach`              | do      |
//
// Refused, with the reason on the cursor: ended work, jobs, questions, the system row, routine runs, a conversation
// onto a home (it has no parent), a home the page can already tell is weaker in data mark than the task, and anything
// under the time machine. The daemon has the last word: its refusal shows in the results toast in its own words.
import type { DataMark } from '@protocol'
import { PAST_WORDS } from './keys.ts'
import { alive, type OpsRow } from './board.ts'

/** What is dragged: a board row or a graph node, as much as the rules read. */
export interface DragItem {
  id: string
  kind: string
  title: string
  /** The task's or conversation's session: what `task.rehome` and `project.attach` name. */
  sessionId?: string
  ended: boolean
  data?: DataMark
  /** Where its report goes now (the parent conversation's session), and that conversation's title: the budget stays
   *  charged to it. */
  home?: string
  homeTitle?: string
}

export type DropTarget =
  | { kind: 'conversation'; sessionId: string; title: string; data?: DataMark }
  | { kind: 'lane'; slug: string; title: string; data?: DataMark }
  | { kind: 'project'; id: string; title: string }

export type Drop =
  | { method: 'task.rehome' | 'project.attach'; params: Record<string, string>; posture: 'ask' | 'do'; words: string }
  | { refused: string }

const RANK: Readonly<Record<DataMark, number>> = { open: 0, private: 1, sensitive: 2 }

/** The budget sentence the re-home sheet says: the daemon's `note` replaces it once it answers. */
export const budgetWords = (homeTitle: string | undefined): string => `The budget stays charged to ${homeTitle ?? 'its original parent'}.`

export function dropOf(source: DragItem, target: DropTarget, past: boolean): Drop {
  if (past) return { refused: PAST_WORDS }
  switch (source.kind) {
    case 'job': return { refused: 'a job runs inside its session: it has no report to move' }
    case 'question': return { refused: 'a question is answered, not moved' }
    case 'system': return { refused: 'the system row is the daemon’s own' }
    case 'routine': return { refused: 'a routine run follows its routine' }
    case 'wake': case 'watch': return { refused: `a ${source.kind} is not a task: its time moves on the Timeline` }
    case 'step': return { refused: 'a step lives in its task’s record: move its task' }
    case 'done': case 'folded': return { refused: 'a fold is many things: open it and move one' }
  }
  if (source.kind !== 'task' && source.kind !== 'conversation') return { refused: `a ${source.kind} does not move` }
  if (source.ended) return { refused: 'it has ended: nothing is left to move' }
  if (!source.sessionId) return { refused: 'it has no session to name' }
  if (target.kind === 'project') {
    return { method: 'project.attach', params: { session: source.sessionId, project: target.id }, posture: 'do', words: `Attach “${source.title}” to the project “${target.title}”.` }
  }
  if (source.kind === 'conversation') return { refused: 'a conversation has no parent: only a task’s report moves' }
  const home = target.kind === 'conversation' ? target.sessionId : `thread:${target.slug}`
  if (target.kind === 'conversation' && target.sessionId === source.sessionId) return { refused: 'a task cannot report to itself' }
  if (home === source.home) return { refused: 'it already reports there' }
  if (target.data && source.data && RANK[target.data] < RANK[source.data]) {
    return { refused: `a ${source.data} task cannot report into ${target.data === 'open' ? 'an open' : `a ${target.data}`} ${target.kind === 'lane' ? 'thread' : 'conversation'}` }
  }
  const where = target.kind === 'conversation' ? `the conversation “${target.title}”` : `the thread “${target.title}”`
  return { method: 'task.rehome', params: { task: source.sessionId, home }, posture: 'ask', words: `Its report goes to ${where} from now on. ${budgetWords(source.homeTitle)}` }
}

/** A drop target as the page marks it on an element (`data-drop`): `conversation:<ses_…>`, `lane:<slug>`,
 *  `project:<tsk_…>`. */
export const dropKey = (t: DropTarget): string => (t.kind === 'conversation' ? `conversation:${t.sessionId}` : t.kind === 'lane' ? `lane:${t.slug}` : `project:${t.id}`)

/** A board row as a drag: a task's report home is its parent conversation, whose title the budget sentence names. */
export function dragOf(r: OpsRow, titleOf: (sessionId: string) => string): DragItem {
  const home = r.task?.info?.parent_session_id
  return {
    id: r.id, kind: r.kind, title: r.title, sessionId: r.sessionId, data: r.view?.data ?? r.session?.data ?? undefined,
    ended: r.kind === 'task' && (!alive(r) || (r.view != null && ['done', 'failed', 'cancelled', 'expired'].includes(r.view.state))),
    home, homeTitle: home ? titleOf(home) : undefined,
  }
}

/** The cursor's hint while a drag is over the page: its words, its verdict, the target it speaks of, and where. */
export interface DropHint { x: number; y: number; words: string; ok: boolean; key?: string }
/** How far the pointer moves before the hint follows it: below this, only a change of words, verdict or target redraws. */
export const HINT_STEP_PX = 6

/** Whether `next` is worth a render over `prev`: a different target, verdict or words, or a pointer moved a few pixels. */
export function hintChanged(prev: DropHint | null, next: DropHint | null): boolean {
  if (!prev || !next) return prev !== next
  return prev.key !== next.key || prev.ok !== next.ok || prev.words !== next.words || Math.hypot(next.x - prev.x, next.y - prev.y) >= HINT_STEP_PX
}
