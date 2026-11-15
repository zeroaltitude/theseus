// A session's own pages under the simple bar (GitHub issue 17): a whole session (`/session/:id`), one turn of it
// (`/session/:id/turn/:turn`) and a label taken off it (`/session/:id/unlabel/:flag`). Each is the Ship's own view, so
// what it shows and does is what it was; only the frame around it is the plain one. Their addresses are unchanged.
import { Suspense, lazy } from 'react'
import { Link, useParams } from 'react-router'
import { ArrowLeft } from 'lucide-react'
import { turnHere } from './find'

const VIEWS = {
  deck: lazy(() => import('@/views/SessionDeck')),
  turn: lazy(() => import('@/views/TurnInspect')),
  unlabel: lazy(() => import('@/views/Unlabel')),
} as const

export default function SimpleSession({ view }: { view: keyof typeof VIEWS }) {
  const { id, turn } = useParams()
  const View = VIEWS[view]
  return (
    <div className="mx-auto flex h-full min-h-[560px] w-full max-w-[1680px] flex-col gap-3 px-6 py-5">
      <nav aria-label="Back" className="flex flex-wrap items-center gap-x-5 gap-y-1 text-[12.5px]">
        <Link to="/" className="flex items-center gap-1 text-ink-faint hover:text-ink"><ArrowLeft size={13} aria-hidden /> Ops</Link>
        {id && id !== 'new' && <Link to={turnHere(id, turn)} className="text-live hover:underline">{turn ? 'What this turn was shown' : 'What its newest turn was shown'}</Link>}
      </nav>
      <div className="min-h-0 flex-1 overflow-auto">
        <Suspense fallback={<div className="p-6 text-[13px] text-ink-faint">Loading…</div>}>
          <View key={`${view}:${id}:${turn ?? ''}`} />
        </Suspense>
      </div>
    </div>
  )
}
