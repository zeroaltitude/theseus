// Desktop notices (theseus-go4ig): the notification policy, run on the board's work (`lib/notices.ts`), shown as the
// browser's own notifications while this page is open and the owner turned them on. Renders nothing; mounted only while
// they are on, so the work watch is held only then.
import { useEffect, useRef } from 'react'
import { useMatch } from 'react-router'
import type { WakeNoticed } from '@protocol'
import { client } from '@/lib/rpc'
import { useThreadBoard } from '@/lib/threadwatch'
import { deliver, Notifier, reminderNotice, RULES, type Notice } from '@/lib/notices'

const hm = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })

export function DesktopNotices() {
  const { board, ready } = useThreadBoard()
  const session = useMatch('/session/:id')?.params.id
  const notifier = useRef(new Notifier())
  const shown = useRef(new Map<string, Notification>())
  const seeded = useRef(false)
  const focused = useRef<string | undefined>(undefined)
  useEffect(() => { focused.current = session }, [session])

  const show = (n: Notice) => {
    const tag = n.question ?? `${n.work.id}:${n.why}`
    if (n.retracts) shown.current.get(n.retracts)?.close()
    const d = deliver(n, { focused: focused.current, seen: 0, away_ms: document.hasFocus() ? 0 : 1, quiet: false, sound: true })
    if (d.as !== 'ping') return
    const note = new Notification(`Theseus: ${n.line}`, { tag, silent: !d.sound })
    shown.current.set(tag, note)
  }

  // The board's views, read against the ones before; the first snapshot is the state the page opened on.
  useEffect(() => {
    if (!ready) return
    const views = Object.values(board.views)
    if (!seeded.current) { seeded.current = true; notifier.current.seed(views); return }
    for (const v of views) {
      const n = notifier.current.observe(v, RULES, hm)
      if (n) show(n)
    }
  }, [board, ready])

  // A question's reminder falls due at a time of its own; a set reminder arrives as `wake.noticed`.
  useEffect(() => {
    let timer: number | undefined
    const arm = () => {
      window.clearTimeout(timer)
      const at = notifier.current.nextDue(RULES)
      if (at === undefined) return
      timer = window.setTimeout(() => { notifier.current.due(Date.now(), RULES, hm).forEach(show); arm() }, Math.max(0, at - Date.now()))
    }
    arm()
    const off = client.onNotify((method, params) => {
      if (method === 'wake.noticed') {
        const n = reminderNotice(params as WakeNoticed)
        if (n) show(n)
      }
    })
    return () => { window.clearTimeout(timer); off() }
  }, [board])

  return null
}
