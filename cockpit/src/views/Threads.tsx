// A thread's own page (`/threads/:slug`, theseus-5i8i): its goal, card, working set, `parent`/`split_from`/
// `merged_into` links, recent turns by `session.history`, and its open work. The body is `ops/ThreadPanel.tsx`, which
// the Operations page's Threads lens shows in its inspector too; the list of threads is that lens now
// (theseus-jid7t.3), and `/threads` redirects to it.
import { useParams } from 'react-router'
import { ThreadBody } from '@/ops/ThreadPanel'

/** One thread: its record, its work, and its recent turns. */
export function ThreadView() {
  const { slug = '' } = useParams()
  return <div className="p-4"><ThreadBody slug={slug} /></div>
}
