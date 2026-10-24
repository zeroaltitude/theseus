// The toast (theseus-jid7t.6): what an act did, in the daemon's words, with Undo where one more call reverses it.
// It stays long enough to read and to press Undo, and goes by itself; the next one replaces it.
import { useEffect } from 'react'
import { Undo2 } from 'lucide-react'
import { useOpsUi } from './ui'

export const TOAST_MS = 7000

export function Toast() {
  const t = useOpsUi((s) => s.toast)
  const clear = useOpsUi((s) => s.clear)
  useEffect(() => {
    if (!t) return
    const h = window.setTimeout(clear, TOAST_MS)
    return () => window.clearTimeout(h)
  }, [t, clear])
  if (!t) return null
  return (
    <div role="status" className="fixed bottom-24 left-1/2 z-30 flex max-w-[80vw] -translate-x-1/2 items-center gap-3 rounded-lg bg-hull/95 px-3 py-1.5 text-[12px] text-ink shadow-xl ring-1 ring-line-strong">
      <span>{t.text}</span>
      {t.undo && <button onClick={() => { t.undo?.(); clear() }} className="flex items-center gap-1 rounded px-1.5 py-0.5 text-live ring-1 ring-live/40 hover:bg-live/10"><Undo2 size={11} /> Undo</button>}
    </div>
  )
}
