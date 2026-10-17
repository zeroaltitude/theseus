// A task's session page (theseus-jm46q): the words and rules of its header and its composer. Pure: `npm test` runs it
// (test/taskpage.test.ts). The page joins a task by typing into the task's own session (`turn.submit` to it), which
// the daemon already takes; an ended task takes nothing, so the composer sends to the mainline instead, with the one
// redirect line every surface prints (`theseus_protocol::task_redirect_line`).
import type { SessionInfo, WorkView } from '@protocol'
/** What the page knows of a task session. */
export interface TaskContext { taskSession: string; parent: string; title: string; ended: boolean }

/** `theseus_protocol::joinable`: the owner can join a task whose session takes input. */
export function joinable(v: Pick<WorkView, 'state'>): boolean {
  return v.state === 'running' || v.state === 'queued' || v.state === 'waiting' || v.state === 'needs_you'
}

/** The reason the redirect line gives for an ended task: every surface's words. */
export const TASK_OVER = 'the task is over'

/** A `turn.submit` to a task refused because the task ended first (its end and the message raced): not found, or an
 *  `execution_*` class, as the TUI and Discord read it. The message then goes to the mainline. */
export function taskIsOver(e: { code?: number; data?: unknown }): boolean {
  const cls = (e.data as { class?: unknown } | undefined)?.class
  return e.code === -32002 || (typeof cls === 'string' && cls.startsWith('execution_'))
}

/** `theseus_protocol::task_redirect_line`: the one line a message for an ended task carries to the mainline. */
export function taskRedirectLine(title: string, shortId: string, why: string): string {
  return `[Written to the task “${title}” (…${shortId}), and ${why}] `
}

/** The task a session is, or null for a conversation (the header and the composer's copy are a task's alone). A task
 *  without a parent has nowhere to return to, so it stays an ordinary deck. */
export function taskContext(s: Pick<SessionInfo, 'session_id' | 'kind' | 'parent_session_id' | 'title' | 'label' | 'execution_state'>, view?: Pick<WorkView, 'state'>): TaskContext | null {
  if (s.kind !== 'task' || !s.parent_session_id) return null
  const ended = view ? !joinable(view) : ['done', 'failed', 'cancelled', 'expired', 'complete', 'completed'].includes(s.execution_state ?? '')
  return { taskSession: s.session_id, parent: s.parent_session_id, title: s.title || s.label || 'task', ended }
}

/** The work view of a task session, from the board's views. */
export function viewOfTask(views: Record<string, WorkView>, sessionId: string): WorkView | undefined {
  return Object.values(views).find((v) => v.kind === 'task' && v.session_id === sessionId)
}

/** Where the composer's text goes, and what it says: a running task is joined (it reads the message at its next
 *  step); an ended one sends to the mainline about the task, behind the shared line. */
export function composerPlan(sessionId: string, t: TaskContext | null, input: string): { sessionId: string; input: string; placeholder: string | null } {
  if (!t) return { sessionId, input, placeholder: null }
  if (!t.ended) return { sessionId, input, placeholder: 'Message this task; it reads it at its next step… (Enter to send, Shift+Enter for a new line)' }
  return {
    sessionId: t.parent,
    input: taskRedirectLine(t.title, t.taskSession.slice(-6), TASK_OVER) + input,
    placeholder: 'Send to main about this task… (the task has reported; Enter to send)',
  }
}
