// The keys of the simple Ops page, listed: the Operations page's own table (`ops/keys.ts`), kept to the keys that work
// here (`helpHere`), so the list cannot say a key does what it does not.
import { X } from 'lucide-react'
import { helpHere } from './keys'

export function KeysHelp({ onClose }: { onClose: () => void }) {
  return (
    <div role="dialog" aria-label="Keys for Ops" className="fixed inset-0 z-40 grid place-items-center bg-black/40" onClick={onClose}>
      <div className="brass-card max-h-[86vh] w-[min(760px,94vw)] overflow-auto !p-0" onClick={(e) => e.stopPropagation()}>
        <header className="flex items-center gap-3 border-b border-line px-4 py-2.5">
          <h2 className="simple-eyebrow">Keys for Ops</h2>
          <span className="text-[11.5px] text-ink-faint">A key does what the thing you chose offers, and the page’s own thing otherwise. None works while you type.</span>
          <button type="button" onClick={onClose} aria-label="Close the keys" className="ml-auto text-ink-faint hover:text-ink"><X size={14} /></button>
        </header>
        <div className="grid gap-x-6 gap-y-3 p-4 md:grid-cols-2">
          {helpHere().map((g) => (
            <section key={g.ctx}>
              <h3 className="simple-eyebrow mb-1">{g.heading}</h3>
              {g.lines.map((l) => (
                <div key={l.verb} className="flex items-baseline gap-3 py-0.5 text-[12px]">
                  <span className="num w-24 shrink-0 text-right text-ink">{l.keys}</span>
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
