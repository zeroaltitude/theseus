// The Operations page's verb table (theseus-jid7t.6): every act the page can make, on what, by which protocol method,
// and in which posture. One table: the inspector, the ⌘K palette and the right-click menus read it, and
// `test/ops-verbs.test.ts` pins each row to its method and posture and each `act` key of `keys.ts` to a row.
//
// Postures: `do` one press, a toast, Undo where the act reverses; `arm` press twice; `ask` a sheet that names the
// effect; `floor` the daemon's own question: the owner's answer is the act, and nothing here skips it.

export type Posture = 'do' | 'arm' | 'ask' | 'floor'

/** What a verb can be done to. */
export type Kind = 'session' | 'conversation' | 'task' | 'thread' | 'routine' | 'wake' | 'watch' | 'job' | 'project' | 'question' | 'daemon'

export interface VerbDef {
  id: string
  label: string
  on: readonly Kind[]
  /** The protocol method(s): a pair is a toggle (`task.pause` / `task.resume`); `nav` is navigation, not a call. */
  methods: readonly string[]
  posture: Posture
  /** Does one more call reverse it? The toast offers Undo. */
  undo?: boolean
  /** Needs a running turn (or task). */
  running?: boolean
}

const ANY: readonly Kind[] = ['session', 'conversation', 'task', 'thread', 'routine', 'wake', 'watch', 'job', 'project', 'question']
const SESSIONISH: readonly Kind[] = ['session', 'conversation', 'task']

export const VERBS: readonly VerbDef[] = [
  { id: 'open', label: 'Open the deck', on: ['session', 'conversation', 'task', 'thread'], methods: ['nav'], posture: 'do' },
  { id: 'inspect', label: 'Inspect', on: ANY, methods: ['session.history', 'node.list', 'turn.inspect', 'work.peek', 'thread.show', 'task.list'], posture: 'do' },
  { id: 'join', label: 'Join and chat', on: ['session', 'conversation', 'task', 'thread'], methods: ['turn.submit'], posture: 'do' },
  { id: 'steer', label: 'Steer the running turn', on: SESSIONISH, methods: ['turn.submit'], posture: 'do', running: true },
  { id: 'queue', label: 'Queue after the turn', on: ['session', 'conversation'], methods: ['turn.submit'], posture: 'do', running: true },
  { id: 'task_pause', label: 'Pause or resume the task', on: ['task'], methods: ['task.pause', 'task.resume'], posture: 'do', undo: true },
  { id: 'thread_pause', label: 'Pause, resume, done or reopen the thread', on: ['thread'], methods: ['thread.pause', 'thread.resume', 'thread.done', 'thread.reopen'], posture: 'do' },
  { id: 'routine_pause', label: 'Pause or resume the routine', on: ['routine'], methods: ['routine.pause', 'routine.resume'], posture: 'do' },
  { id: 'routine_run', label: 'Run the routine now', on: ['routine'], methods: ['routine.run_now'], posture: 'ask' },
  { id: 'routine_delete', label: 'Delete the routine', on: ['routine'], methods: ['routine.delete'], posture: 'ask' },
  { id: 'wake_snooze', label: 'Snooze or move the wake', on: ['wake'], methods: ['wake.snooze'], posture: 'do', undo: true },
  { id: 'wake_pause', label: 'Pause or resume the wake', on: ['wake'], methods: ['wake.pause', 'wake.resume'], posture: 'do', undo: true },
  { id: 'wake_cancel', label: 'Cancel the wake', on: ['wake'], methods: ['wake.cancel'], posture: 'ask' },
  { id: 'watch_cancel', label: 'Cancel the watch', on: ['watch'], methods: ['wake.cancel'], posture: 'ask' },
  { id: 'stop', label: 'Stop the turn (keep its jobs)', on: ['conversation'], methods: ['execution.stop'], posture: 'arm', running: true },
  { id: 'stop_all', label: 'Stop it and cancel every task under it', on: ['conversation'], methods: ['execution.stop'], posture: 'ask', running: true },
  { id: 'cancel', label: 'Cancel the task', on: ['task'], methods: ['task.cancel'], posture: 'arm' },
  { id: 'not_this', label: 'Not this: cancel it, and don’t start work like it', on: ['task'], methods: ['task.not_this'], posture: 'arm' },
  { id: 'job_stop', label: 'Stop the job', on: ['job'], methods: ['job.stop'], posture: 'arm' },
  { id: 'answer', label: 'Answer', on: ['question'], methods: ['work.answer'], posture: 'floor' },
  { id: 'unlabel', label: 'Remove a label', on: ['session', 'conversation'], methods: ['content.unlabel'], posture: 'ask' },
  { id: 'retry', label: 'Retry the failed task', on: ['task'], methods: ['task.retry'], posture: 'do' },
  { id: 'dismiss', label: 'Dismiss the failure', on: ['task', 'conversation'], methods: ['execution.dismiss'], posture: 'do' },
  { id: 'new_task', label: 'Start a task', on: ['daemon', ...SESSIONISH], methods: ['task.start'], posture: 'do' },
  { id: 'thread_new', label: 'New thread', on: ['thread'], methods: ['thread.new'], posture: 'do' },
  { id: 'thread_edit', label: 'Edit the thread', on: ['thread'], methods: ['thread.edit'], posture: 'do' },
  { id: 'thread_seal', label: 'Seal or unseal the thread', on: ['thread'], methods: ['thread.seal', 'thread.unseal'], posture: 'ask' },
  { id: 'thread_merge', label: 'Merge or split the thread', on: ['thread'], methods: ['thread.merge', 'thread.split'], posture: 'ask' },
  { id: 'thread_link', label: 'Link the thread', on: ['thread'], methods: ['thread.link'], posture: 'do' },
  { id: 'thread_move', label: 'Move a misrouted message', on: ['thread'], methods: ['thread.move'], posture: 'do' },
  { id: 'attach', label: 'Attach to a project', on: ['task', 'conversation'], methods: ['project.attach'], posture: 'do' },
  { id: 'mode', label: 'Set the mode', on: ['session', 'conversation'], methods: ['session.mode'], posture: 'do' },
  { id: 'mode_auto', label: 'Set the mode to Auto', on: ['session', 'conversation'], methods: ['session.mode'], posture: 'ask' },
  { id: 'nudge', label: 'Stop-nudge on or off', on: ['session', 'conversation'], methods: ['session.nudge'], posture: 'do' },
  { id: 'recompile', label: 'Recompile', on: ['session', 'conversation'], methods: ['session.recompile'], posture: 'do' },
  { id: 'fork', label: 'Fork', on: ['session', 'conversation'], methods: ['session.fork'], posture: 'do' },
  { id: 'retire', label: 'Retire', on: ['session', 'conversation'], methods: ['session.retire'], posture: 'ask' },
  { id: 'reopen', label: 'Reopen', on: ['session', 'conversation'], methods: ['session.reopen'], posture: 'do' },
  { id: 'model_turn', label: 'Model for the next turn only', on: ['session', 'conversation'], methods: ['turn.submit'], posture: 'do' },
  { id: 'model_sticky', label: 'Model from now on', on: SESSIONISH, methods: ['session.profile'], posture: 'do', undo: true },
  { id: 'profile_live', label: 'Make a profile the live one', on: ['daemon'], methods: ['profile.use'], posture: 'ask' },
  { id: 'rehome', label: 'Re-home the task', on: ['task'], methods: ['task.rehome'], posture: 'ask' },
  { id: 'reschedule', label: 'Reschedule the deferred start', on: ['task'], methods: ['task.reschedule'], posture: 'do', undo: true },
]

export const verbById = (id: string): VerbDef | undefined => VERBS.find((v) => v.id === id)

/** The verbs that apply to a kind, in table order. */
export const verbsOn = (kind: Kind): VerbDef[] => VERBS.filter((v) => v.on.includes(kind))

/** The table verb each acting key of `keys.ts` is: a test holds that every `act` key names one. */
export const KEY_VERB: Readonly<Record<string, string>> = {
  approve: 'answer', decline: 'answer', choice: 'answer', grant: 'answer',
  retry: 'retry', dismiss: 'dismiss', pause: 'task_pause', stop: 'stop', stop_all: 'stop_all', cancel: 'cancel', not_this: 'not_this', cancel_wake: 'wake_cancel',
  new_task: 'new_task', model_menu: 'model_sticky', mode_menu: 'mode',
  // ops-timeline (theseus-jid7t.5): the Horizon's keys on a focused mark.
  mark_snooze: 'wake_snooze', mark_pause: 'wake_pause', mark_run: 'routine_run', mark_cancel: 'wake_cancel',
}
