// Memories, a turn: what one turn was shown. A session and one of its turns (`?session=`, `?turn=`; the newest when no
// turn is named), then the request in its parts with their sizes, and why each remembered note was brought. It is the
// explorer's own turn view (`components/ContextTurn.tsx`, over `context.explain`), so it reads nothing the Ship does not.
import { useSearchParams } from 'react-router'
import { TurnContext } from '@/components/ContextTurn'

export default function Turn() {
  const [params, setParams] = useSearchParams()
  const set = (patch: Record<string, string | null | undefined>) => setParams((p) => {
    for (const [k, v] of Object.entries(patch)) { if (v) p.set(k, v); else p.delete(k) }
    return p
  }, { replace: true })
  return (
    <div className="flex flex-col gap-3 pb-6">
      <p className="px-1 text-[13px] text-ink-dim">
        Pick a conversation, then one of its turns, to see exactly what Theseus put in front of the model for it: the instructions, the tools, the notes it remembered and why, and the conversation so far.
      </p>
      <TurnContext session={params.get('session')} turn={params.get('turn')} set={set} />
    </div>
  )
}
