// The thread picker (theseus-jid7t.6; the TUI's ctrl-t, `/t` and `T`): type to narrow the live threads, ⏎ joins the
// first, a click joins that one. It opens the Join pane on the thread.
import { useState } from 'react'
import { GitBranch } from 'lucide-react'
import { Sheet, FIELD } from './Sheet'

export function ThreadPicker({ threads, onPick, onClose }: { threads: ReadonlyArray<{ slug: string; title: string }>; onPick: (slug: string) => void; onClose: () => void }) {
  const [q, setQ] = useState('')
  const words = q.toLowerCase().split(/\s+/).filter(Boolean)
  const hit = threads.filter((t) => words.every((w) => `${t.title} ${t.slug}`.toLowerCase().includes(w)))
  return (
    <Sheet title="Join a thread" verb="join" submit="Join" disabled={!hit.length} onClose={onClose} onSubmit={() => hit[0] && onPick(hit[0].slug)}>
      <input autoFocus value={q} onChange={(e) => setQ(e.target.value)} placeholder="Type to narrow the threads" aria-label="Thread" className={FIELD} />
      <ul className="max-h-60 overflow-auto">
        {hit.map((t) => <li key={t.slug}><button type="button" onClick={() => onPick(t.slug)} className="flex w-full items-center gap-2 rounded px-2 py-1 text-left text-[12.5px] text-ink hover:bg-white/5"><GitBranch size={12} className="text-gold" />{t.title}<span className="num ml-auto text-[11px] text-ink-faint">{t.slug}</span></button></li>)}
        {!hit.length && <li className="px-2 py-3 text-center text-[12px] text-ink-faint">No live thread matches.</li>}
      </ul>
    </Sheet>
  )
}
