// Watching a session's live events for the Operations page (theseus-jid7t.6): `session.watch` once however many panes
// ask (the inspector's composer and the Join pane can hold the same session), and `session.unwatch` when the last one
// leaves, so closing one never stops the other's stream.
import { useEffect } from 'react'
import { client, useConn } from '@/lib/rpc'

const holders = new Map<string, number>()

export function useWatchedSession(sessionId: string | undefined): void {
  const open = useConn((s) => s.status === 'open')
  useEffect(() => {
    if (!sessionId || !open) return
    const n = (holders.get(sessionId) ?? 0) + 1
    holders.set(sessionId, n)
    if (n === 1) client.call('session.watch', { session_id: sessionId }).catch(() => {})
    return () => {
      const left = (holders.get(sessionId) ?? 1) - 1
      if (left > 0) { holders.set(sessionId, left); return }
      holders.delete(sessionId)
      client.call('session.unwatch', { session_id: sessionId }).catch(() => {})
    }
  }, [sessionId, open])
}
