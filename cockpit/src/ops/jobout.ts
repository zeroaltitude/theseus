// A followed job's output in the inspector (theseus-jid7t.6): `work.peek` for the tail, then `action.output` for the
// chunks, and `action.unfollow` when the selection leaves, the lens switches or the inspector unmounts. The rule is
// `follow.ts`'s state machine; this hook only does what it says.
import { useEffect, useState } from 'react'
import type { WorkPeekResult } from '@protocol'
import { call, client } from '@/lib/rpc'
import { followOf, type Follow, type FollowEffect, type FollowEvent } from './follow'
import { appendChunk, EMPTY_OUT, peekOut, type JobOut } from './live'

export function useJobOutput(job: string | null): JobOut | null {
  const [out, setOut] = useState<JobOut | null>(null)
  useEffect(() => {
    let dead = false
    let state: Follow = { kind: 'off' }
    const exec = (fx: FollowEffect) => {
      if (fx.op === 'peek') {
        call<WorkPeekResult>('work.peek', { id: fx.job }).then((p) => {
          if (dead) return
          setOut(peekOut(p.lines, p.bytes))
          run({ kind: 'peeked', job: fx.job, bytes: p.bytes })
        }).catch(() => {})
      } else if (fx.op === 'follow') call('action.output', { id: fx.job, from: fx.from }).catch(() => {})
      else call('action.unfollow', { id: fx.job }).catch(() => {})
    }
    const run = (e: FollowEvent) => { const r = followOf(state, e); state = r.state; r.effects.forEach(exec) }
    const off = client.onNotify((m, p) => {
      const q = p as { id?: string; offset?: number; text?: string }
      if (m === 'action.output.chunk' && state.kind === 'following' && q.id === state.job) setOut((o) => appendChunk(o ?? EMPTY_OUT, { offset: q.offset ?? 0, text: q.text ?? '' }))
    })
    setOut(null)
    run({ kind: 'select', job })
    return () => { dead = true; off(); run({ kind: 'leave' }) }
  }, [job])
  return out
}
