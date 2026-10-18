// Desktop notices (theseus-go4ig): the daemon's `work.notice` pushes (its notification policy, run on the work board)
// and a set reminder's `wake.noticed`, shown as the browser's own notifications while this page is open and the owner
// turned them on. Renders nothing; mounted only while they are on, so the work watch is held only then.
import { useEffect, useRef } from 'react'
import { useMatch, useNavigate } from 'react-router'
import type { WakeNoticed, WorkNotice } from '@protocol'
import { client } from '@/lib/rpc'
import { useThreadBoard } from '@/lib/threadwatch'
import { deliver, reminderNotice, tagOf } from '@/lib/notices'
import { noticeTarget } from '@/lib/taskboard'

export function DesktopNotices() {
  // The page's `work.watch`: the daemon pushes the board's notices on it.
  useThreadBoard()
  const nav = useNavigate()
  const open = useRef(nav)
  useEffect(() => { open.current = nav }, [nav])
  const session = useMatch('/session/:id')?.params.id
  const focused = useRef<string | undefined>(undefined)
  useEffect(() => { focused.current = session }, [session])

  useEffect(() => {
    const shown = new Map<string, Notification>()
    const show = (n: WorkNotice) => {
      if (n.retracts) { shown.get(n.retracts)?.close(); shown.delete(n.retracts) }
      // `seen` stays 0: the cockpit keeps no seen position of its own (the chat's is a file).
      const d = deliver(n, { focused: focused.current, seen: 0, away_ms: document.hasFocus() ? 0 : 1, quiet: false, sound: true })
      if (d.as !== 'ping') return
      const tag = tagOf(n)
      const note = new Notification(`Theseus: ${n.line}`, { tag, silent: !d.sound })
      // A press brings the page forward and opens the task's row on the Tasks page (other work: its session).
      note.onclick = () => { window.focus(); const to = noticeTarget(n.work); if (to) open.current(to); note.close() }
      shown.set(tag, note)
    }
    return client.onNotify((method, params) => {
      if (method === 'work.notice') show(params as WorkNotice)
      else if (method === 'wake.noticed') {
        const n = reminderNotice(params as WakeNoticed)
        if (n) show(n)
      }
    })
  }, [])

  return null
}
