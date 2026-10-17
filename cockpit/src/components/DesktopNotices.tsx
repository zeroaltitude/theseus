// Desktop notices (theseus-go4ig): the daemon's `work.notice` pushes (its notification policy, run on the work board)
// and a set reminder's `wake.noticed`, shown as the browser's own notifications while this page is open and the owner
// turned them on. Renders nothing; mounted only while they are on, so the work watch is held only then.
import { useEffect, useRef } from 'react'
import { useMatch } from 'react-router'
import type { WakeNoticed, WorkNotice } from '@protocol'
import { client } from '@/lib/rpc'
import { useThreadBoard } from '@/lib/threadwatch'
import { deliver, reminderNotice, tagOf } from '@/lib/notices'

export function DesktopNotices() {
  // The page's `work.watch`: the daemon pushes the board's notices on it.
  useThreadBoard()
  const session = useMatch('/session/:id')?.params.id
  const focused = useRef<string | undefined>(undefined)
  useEffect(() => { focused.current = session }, [session])

  useEffect(() => {
    const shown = new Map<string, Notification>()
    const show = (n: WorkNotice) => {
      if (n.retracts) { shown.get(n.retracts)?.close(); shown.delete(n.retracts) }
      const d = deliver(n, { focused: focused.current, seen: 0, away_ms: document.hasFocus() ? 0 : 1, quiet: false, sound: true })
      if (d.as !== 'ping') return
      const tag = tagOf(n)
      shown.set(tag, new Notification(`Theseus: ${n.line}`, { tag, silent: !d.sound }))
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
