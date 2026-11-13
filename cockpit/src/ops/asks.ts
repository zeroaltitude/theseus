// The router's held messages (theseus-jid7t.3): "Is this about the quay or the tides?" (`thread.asked`) waits on the
// owner until it is answered (`thread.answer`), the message runs (`thread.placed`), or its wait ends. No read lists
// them by push alone; `thread.ask_list` reads the ones held now, on every (re)connect, so a page opened after an ask
// still shows it (theseus-nuqut). The push keeps them current after.
import { useEffect, useMemo, useState } from 'react'
import type { ThreadAsk, ThreadPlaced } from '@protocol'
import { client } from '@/lib/rpc'

export function useRouterAsks(now: number): Array<{ ask: ThreadAsk; heard_ms: number }> {
  const [asks, setAsks] = useState<Array<{ ask: ThreadAsk; heard_ms: number }>>([])
  useEffect(() => {
    const seed = () => client.call<{ asks: ThreadAsk[] }>('thread.ask_list').then((r) => {
      const heard = Date.now()
      setAsks((xs) => [...r.asks.map((ask) => xs.find((x) => x.ask.ask_id === ask.ask_id) ?? { ask, heard_ms: heard }), ...xs.filter((x) => !r.asks.some((a) => a.ask_id === x.ask.ask_id))])
    }).catch(() => {})
    seed()
    return client.onOpen(seed)
  }, [])
  useEffect(() => client.onNotify((method, params) => {
    if (method === 'thread.asked') {
      const ask = params as ThreadAsk
      setAsks((xs) => [...xs.filter((x) => x.ask.ask_id !== ask.ask_id), { ask, heard_ms: Date.now() }])
    } else if (method === 'thread.placed') {
      const p = params as ThreadPlaced
      setAsks((xs) => xs.filter((x) => x.ask.session_id !== p.session_id))
    }
  }), [])
  // One array while nothing changed: the queue is memoized on it.
  return useMemo(() => asks.filter((x) => x.ask.expires_ms > now), [asks, now])
}
