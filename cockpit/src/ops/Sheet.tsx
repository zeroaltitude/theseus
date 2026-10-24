// A sheet: a dialog that names the effect and asks for what the act needs (theseus-jid7t.6). It replaces
// `window.prompt` and `window.confirm` for the acts that need words. Esc closes it, Ctrl+Enter sends; the act's
// verb goes through `enabled`, so under the time machine the button is off.
import { useEffect, type FormEvent, type ReactNode } from 'react'
import { X } from 'lucide-react'
import { cn } from '@/lib/format'
import { HelpButton } from '@/components/Help'
import { useOpsAct } from './act'
import { ActBtn } from './ActBtn'
import type { ButtonAct, Verb } from './keys'

export function Sheet({ title, note, help, verb, submit, busy, disabled, result, onSubmit, onClose, wide, children }: {
  title: string; note?: string; help?: string; verb: Verb | ButtonAct; submit: string; busy?: boolean; disabled?: boolean
  /** The act's answer, in the daemon's words. */
  result?: { ok: boolean; text: string } | null
  onSubmit: () => void; onClose: () => void; wide?: boolean; children: ReactNode
}) {
  const { on } = useOpsAct()
  useEffect(() => {
    const k = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.stopPropagation(); onClose() } }
    window.addEventListener('keydown', k, true)
    return () => window.removeEventListener('keydown', k, true)
  }, [onClose])
  const send = (e?: FormEvent) => { e?.preventDefault(); if (!disabled && !busy) onSubmit() }
  return (
    <div className="fixed inset-0 z-40 grid place-items-start justify-center bg-black/50 pt-[12vh] backdrop-blur-sm" role="dialog" aria-modal aria-label={title} onMouseDown={(e) => { if (e.target === e.currentTarget) onClose() }}>
      <form onSubmit={send} onKeyDown={(e) => { if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) send() }}
        className={cn('brass-card flex max-h-[76vh] flex-col gap-2.5 overflow-auto p-4 shadow-2xl', wide ? 'w-[640px]' : 'w-[460px]', 'max-w-[94vw]')}>
        <div className="flex items-center gap-2">
          <h2 className="font-display text-[13px] font-bold uppercase tracking-[0.08em] text-gold">{title}</h2>
          {help && <span className="ml-auto"><HelpButton id={help} title={title} /></span>}
          <button type="button" onClick={onClose} aria-label="close (esc)" title="close (esc)" className={cn('text-ink-faint hover:text-ink', !help && 'ml-auto')}><X size={14} /></button>
        </div>
        {note && <p className="text-[12px] text-ink-dim">{note}</p>}
        {children}
        {result && <div role="status" className={cn('num text-[11.5px]', result.ok ? 'text-ok' : 'text-fault')}>{result.text}</div>}
        <div className="flex items-center justify-end gap-2">
          <span className="mr-auto text-[10.5px] text-ink-faint"><span className="kbd">esc</span> closes · <span className="kbd">⌘⏎</span> sends</span>
          <ActBtn verb={verb} on={on} busy={busy} tone="live" onClick={send}>{submit}</ActBtn>
        </div>
      </form>
    </div>
  )
}

/** The field look the Operations page's forms share. */
export const FIELD = 'rounded-md bg-black/20 px-2 py-1.5 text-[12.5px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/60'
