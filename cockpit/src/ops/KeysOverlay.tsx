// The `?` overlay (theseus-jid7t.3): drawn from the one key table (`keys.ts`'s `overlay()`), the same table the page's
// handler resolves keys by, so the two cannot drift.
import { X } from 'lucide-react'
import { overlay } from './keys'

export function KeysOverlay({ onClose }: { onClose: () => void }) {
  return (
    <div role="dialog" aria-label="Keys" className="fixed inset-0 z-40 grid place-items-center bg-black/50 backdrop-blur-sm" onClick={onClose}>
      <div className="brass-card max-h-[86vh] w-[min(860px,94vw)] overflow-auto !p-0" onClick={(e) => e.stopPropagation()}>
        <header className="flex items-center border-b border-line px-4 py-2.5">
          <h2 className="font-display text-[13px] font-bold uppercase tracking-[0.1em] text-gold">Operations · keys</h2>
          <span className="ml-3 text-[11px] text-ink-faint">a key is a card’s verb when a card claims it, and the page’s otherwise; none fires while you type</span>
          <button onClick={onClose} aria-label="close" className="ml-auto text-ink-faint hover:text-ink"><X size={14} /></button>
        </header>
        <div className="grid gap-x-6 gap-y-3 p-4 md:grid-cols-2">
          {overlay().map((g) => (
            <section key={g.ctx}>
              <h3 className="mb-1 text-[10.5px] font-semibold uppercase tracking-[0.08em] text-ink-faint">{g.heading}</h3>
              {g.lines.map((l) => (
                <div key={l.verb} className="flex items-baseline gap-3 py-0.5 text-[12px]">
                  <span className="num w-28 shrink-0 text-right text-gold">{l.keys}</span>
                  <span className="text-ink-dim">{l.words}</span>
                </div>
              ))}
            </section>
          ))}
        </div>
      </div>
    </div>
  )
}
