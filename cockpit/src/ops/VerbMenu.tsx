// The right-click menu on a card (theseus-jid7t.6): the verbs of the table (`verbs.ts`, through the palette's
// `verbEntries`) over that one card. What it would refuse is greyed with the reason; model and mode open their menus.
import { useEffect } from 'react'
import { cn } from '@/lib/format'
import { verbEntries, type OpsTarget } from './palette'
import { useRunEntry } from './useRunEntry'
import { useOpsAct } from './act'
import { PAST_WORDS } from './act'

export function VerbMenu({ target, x, y, onMenu, onClose }: { target: OpsTarget; x: number; y: number; onMenu: (m: 'model' | 'mode') => void; onClose: () => void }) {
  const run = useRunEntry()
  const { past } = useOpsAct()
  useEffect(() => {
    const k = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.stopPropagation(); onClose() } }
    const c = () => onClose()
    window.addEventListener('keydown', k, true)
    window.addEventListener('click', c)
    return () => { window.removeEventListener('keydown', k, true); window.removeEventListener('click', c) }
  }, [onClose])
  // The per-profile and per-mode entries are the menus' own; here they are two lines that open them.
  const entries = verbEntries([target], { profiles: [], prices: new Map() }).filter((e) => !e.id.startsWith('model:') && !e.id.startsWith('mode:') && e.id !== 'new_task')
  const canTalk = !!target.sessionId && (target.kind === 'conversation' || target.kind === 'task')
  const item = 'flex w-full items-center gap-2 rounded px-2 py-1 text-left text-[12px] text-ink hover:bg-white/5 disabled:cursor-not-allowed disabled:opacity-45'
  return (
    <div role="menu" aria-label={`Verbs for ${target.title}`} style={{ left: Math.min(x, window.innerWidth - 300), top: Math.min(y, window.innerHeight - 360) }}
      className="fixed z-40 flex max-h-[60vh] w-72 flex-col overflow-auto rounded-lg border border-line-strong bg-hull/95 p-1 shadow-2xl backdrop-blur" onClick={(e) => e.stopPropagation()}>
      <div className="truncate px-2 py-1 font-display text-[10.5px] font-bold uppercase tracking-[0.08em] text-gold">{target.title}</div>
      {canTalk && <button role="menuitem" className={item} disabled={past} title={past ? PAST_WORDS : undefined} onClick={() => { onClose(); onMenu('model') }}>Model…<span className="kbd ml-auto">m</span></button>}
      {target.kind === 'conversation' && <button role="menuitem" className={item} disabled={past} onClick={() => { onClose(); onMenu('mode') }}>Mode…<span className="kbd ml-auto">M</span></button>}
      {entries.map((e) => (
        <button key={e.id} role="menuitem" className={cn(item)} disabled={!!e.grey || past} title={e.grey ?? (past ? PAST_WORDS : undefined)} onClick={() => { onClose(); void run(e) }}>
          {e.label}{e.grey && <span className="ml-auto truncate text-[10.5px] text-ink-faint">{e.grey}</span>}
        </button>
      ))}
    </div>
  )
}
