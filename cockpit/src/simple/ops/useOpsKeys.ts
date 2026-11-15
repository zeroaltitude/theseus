// The simple Ops page's key handler (GitHub issue 17): one listener for the page that asks the Operations page's table
// what a key means here (`./keys.ts`'s `resolveHere`) and does what the Ship's page does for it, through the same
// verbs: an answer is `answerOf`'s `work.answer`, a stop is `execution.stop`, each by `useOpsAct`, so a confirmation,
// and the time machine's refusal, are the Ship's. Keys never fire while a person types, and a sheet or a menu that
// is open has them.
import { useEffect, useLayoutEffect, useRef } from 'react'
import { useNavigate } from 'react-router'
import { useOpsAct } from '../../ops/act'
import { answerOf, step, type Answer } from '../../ops/queue'
import { keyName, pressTwice, typingIn, type Verb } from '../../ops/keys'
import { useOpsUi } from '../../ops/ui'
import { lastTurnOf } from '../../ops/sections'
import { focusOf, resolveHere, type SimpleVerb } from './keys'
import { stepBy, type WorkSet } from './workset'
import type { SessionInfo } from '../../protocol.gen/index.ts'

export interface KeyPage {
  work: WorkSet
  sessions: readonly SessionInfo[]
  sel: string | null
  pick: (id: string | null) => void
  showDetails: () => void
  note: { key: string; text: string } | null
  setNote: (n: { key: string; text: string } | null) => void
  /** The pieces that take every key while they are open: a sheet, a menu, the new-task form. */
  modal: boolean
  keysOpen: boolean
  toggleKeys: () => void
  newTask: () => void
  menu: (kind: 'model' | 'mode', sid: string) => void
  /** The ledger's rows, for a session's newest turn. */
  history: Parameters<typeof lastTurnOf>[0]
}

/** PgUp, PgDn and End scroll the open conversation (the Join pane's own box). */
const scrollTranscript = (by: number) => {
  const box = document.querySelector<HTMLElement>('[data-testid="join-pane"] .overflow-auto')
  if (box) box.scrollTop = by === Infinity ? box.scrollHeight : box.scrollTop + by * box.clientHeight
}

export function useOpsKeys(p: KeyPage) {
  const nav = useNavigate()
  const { act, on } = useOpsAct()
  const armed = useRef<{ verb: Verb; at: number } | null>(null)
  const live = useRef(p)
  useLayoutEffect(() => { live.current = p })
  const doing = useRef({ act, on, nav })
  useLayoutEffect(() => { doing.current = { act, on, nav } })

  useEffect(() => {
    const say = (text: string) => useOpsUi.getState().say(text)
    const handlers = (): Record<SimpleVerb, (key: string) => boolean | void> => {
      const q = live.current
      const f = focusOf(q.work, q.sessions, q.sel)
      const { act: run, on: allowed, nav: go } = doing.current
      const item = q.work.queue.find((i) => i.key === q.sel)
      const answer = (verb: Verb, x: Answer) => {
        const c = f.focusItem && answerOf(f.focusItem, x)
        if (!c) { say('Nothing here takes that answer.'); return }
        void run(verb, `${f.focusItem!.key}:${x.verb}`, c.method, c.params, c.ask).then((ok) => { if (ok) q.setNote(null) })
      }
      const twice = (verb: Verb, fire: () => void, words: string) => {
        const s = pressTwice(armed.current, verb, Date.now())
        armed.current = s.armed
        if (s.fire) fire(); else say(words)
      }
      const row = f.focusRow
      const sid = row?.sessionId ?? f.focusItem?.sessionId ?? f.focusSession?.session_id
      return {
        down: () => q.pick(stepBy(q.work.order, q.sel, 1)),
        up: () => q.pick(stepBy(q.work.order, q.sel, -1)),
        inspect: () => { if (!row) return false; q.showDetails() },
        open: () => { if (!sid) return false; go(`/session/${sid}`) },
        back: () => { if (q.note) q.setNote(null); else if (q.sel) q.pick(null); else return false },
        next: () => q.pick(step(q.work.queue, item?.key ?? null, 1)),
        prev: () => q.pick(step(q.work.queue, item?.key ?? null, -1)),
        approve: () => answer('approve', { verb: 'approve' }),
        decline: () => { if (f.focusItem) { q.pick(f.focusItem.key); q.setNote({ key: f.focusItem.key, text: '' }) } },
        choice: (key) => answer('choice', { verb: 'choice', n: Number(key) }),
        grant: () => answer('grant', { verb: 'grant' }),
        retry: () => answer('retry', { verb: 'retry' }),
        dismiss: () => answer('dismiss', { verb: 'dismiss' }),
        pause: () => { if (row?.kind === 'task') void run('pause', row.id, row.paused ? 'task.resume' : 'task.pause', { task: row.id }) },
        stop: () => twice('stop', () => { if (f.exe) void run('stop', `${f.exe}:stop`, 'execution.stop', { execution_id: f.exe }) }, 'Press s again to stop the running turn.'),
        stop_all: () => { if (f.exe) void run('stop_all', `${f.exe}:all`, 'execution.stop', { execution_id: f.exe, tasks: true }, 'Stop its turn, and cancel every task under it?') },
        cancel: () => twice('cancel', () => { if (row?.kind === 'task') void run('cancel', `${row.id}:stop`, 'task.cancel', { task: row.id }) }, 'Press c again to cancel the task.'),
        not_this: () => twice('not_this', () => { if (row?.kind === 'task') void run('not_this', `${row.id}:not_this`, 'task.not_this', { task: row.id }) }, 'Press u again to say not this: it cancels the task and keeps work like it from starting.'),
        cancel_wake: () => { if (row?.kind === 'wake') void run('cancel_wake', `${row.id}:x`, 'wake.cancel', { wake: row.id }, `Cancel the wake “${row.title}”?`) },
        new_task: () => { if (allowed('new_task')) q.newTask() },
        model_menu: () => { if (f.focusSession && allowed('model_menu')) q.menu('model', f.focusSession.session_id) },
        mode_menu: () => { if ((f.focusSession?.kind === 'conversation' || f.focusSession?.kind === 'task') && allowed('mode_menu')) q.menu('mode', f.focusSession.session_id) },
        compose: () => { const box = document.querySelector<HTMLTextAreaElement>('[data-testid="join-pane"] textarea'); if (box) box.focus(); else say('Choose a conversation or a task to write to.') },
        parent: () => { const to = row?.task?.info?.parent_session_id; if (to) q.pick(to); else say('It reports to nothing above it.') },
        last_turn: () => { const t = sid ? lastTurnOf(q.history, sid) : undefined; if (sid && t) go(`/session/${sid}/turn/${t}`); else say('No turn has ended in it yet.') },
        scroll_up: () => scrollTranscript(-0.85),
        scroll_down: () => scrollTranscript(0.85),
        scroll_end: () => scrollTranscript(Infinity),
        keys: () => q.toggleKeys(),
      }
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.altKey || e.metaKey || e.ctrlKey) return
      if (typingIn(e.target as HTMLElement | null)) return
      const q = live.current
      if (q.keysOpen && e.key === 'Escape') { e.preventDefault(); e.stopImmediatePropagation(); q.toggleKeys(); return }
      if (q.modal) return
      const name = keyName(e)
      const f = focusOf(q.work, q.sessions, q.sel)
      const hit = resolveHere(name, f, false)
      if (!hit) return
      if (handlers()[hit.verb](name) === false) return
      e.preventDefault()
      e.stopImmediatePropagation()
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [])
}
