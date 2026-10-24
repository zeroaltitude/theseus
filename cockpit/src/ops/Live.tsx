// Live (theseus-jid7t.6): what a session is doing now, three streams in one list: its narrative lines, the model's
// words as they come, and the running job's output (`work.peek`, then `action.output` chunks, unfollowed on leaving:
// `jobout.ts`). The session's push is watched by the Join box beside it, so this only reads what that brings.
import { useMemo } from 'react'
import { usePush } from '@/lib/rpc'
import { clock } from '@/lib/format'
import { cn } from '@/lib/format'
import { liveOf, mergeLive, narrativeOf } from './live'
import { useJobOutput } from './jobout'

const GLYPH = { narrative: '▸', model: '◇', job: '$' } as const
const TONE = { narrative: 'text-ink-dim', model: 'text-model', job: 'text-tool' } as const

export function Live({ sessionId, jobId }: { sessionId?: string; jobId?: string | null }) {
  const events = usePush((s) => s.events)
  const narrative = usePush((s) => s.narrative)
  const out = useJobOutput(jobId ?? null)
  const lines = useMemo(() => mergeLive(sessionId ? narrativeOf(narrative, sessionId).slice(-30) : [], sessionId ? liveOf(events, sessionId) : null, out, Date.now(), 40), [events, narrative, sessionId, out])
  if (!lines.length) return <div className="px-1 text-[12px] text-ink-faint">Nothing is happening in it now.</div>
  return (
    <ol className="max-h-56 space-y-0.5 overflow-auto font-mono text-[11.5px]" data-testid="live">
      {lines.map((l, i) => (
        <li key={`${l.at}:${i}`} className="flex gap-2">
          <span className="num shrink-0 text-ink-faint">{clock(l.at)}</span>
          <span className={cn('shrink-0', TONE[l.src])} aria-label={l.src}>{GLYPH[l.src]}</span>
          <span className={cn('min-w-0 whitespace-pre-wrap break-words', l.src === 'job' ? 'text-ink-dim' : 'text-ink')}>{l.text}</span>
        </li>
      ))}
    </ol>
  )
}
