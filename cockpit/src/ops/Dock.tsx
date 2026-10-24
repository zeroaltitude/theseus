// The dock (theseus-jid7t.6; `\` folds it): the live streams, `narrative.watch` over every session, newest first, and
// the models strip. `␣` pauses the streams while the dock has focus (anywhere else `␣` is the selection's); "follow"
// shows the selected session's lines alone. The strip lists every profile with its origin, the live flag, the sessions
// on it and the headroom from the newest call's rate headers; a click filters the page by that model, ⌥-click makes
// it the live profile (`profile.use`, which asks: it changes the daemon's default).
import { useMemo } from 'react'
import { Pause, Play, Radio } from 'lucide-react'
import type { ProfileList, SessionInfo } from '@protocol'
import { useRpc, usePush } from '@/lib/rpc'
import { providerCalls } from '@/lib/derive'
import { useHistoryRows } from '@/lib/history'
import { clock, cn } from '@/lib/format'
import { Pill } from '@/components/ui'
import { dockStream, modelsStrip, whoOf } from './live'
import { useOpsAct } from './act'

export interface DockState { paused: boolean; follow: boolean }

export function Dock({ state, onState, onFocus, selected, sessions, activeModel, onModel }: {
  state: DockState; onState: (s: DockState) => void; onFocus: (focused: boolean) => void
  /** The selected card's session, for "follow". */
  selected: string | null; sessions: readonly SessionInfo[]; activeModel: string | undefined; onModel: (name: string | null) => void
}) {
  const { act, on } = useOpsAct()
  const narrative = usePush((s) => s.narrative)
  // Pausing holds the newest line seen then: lines that come meanwhile wait, and show when it resumes.
  const heldAt = useMemo(() => narrative[narrative.length - 1]?.seq ?? 0, [state.paused]) // eslint-disable-line react-hooks/exhaustive-deps
  const lines = useMemo(() => dockStream(narrative, { follow: state.follow ? selected : null, pausedAt: state.paused ? heldAt : null }), [narrative, state.follow, selected, state.paused, heldAt])
  const { data: pl } = useRpc<ProfileList>('profile.list', undefined, 30_000)
  const rows = useHistoryRows().rows
  const calls = useMemo(() => providerCalls(rows.filter((r) => r.kind === 'provider.call').slice(-400)), [rows])
  const strip = useMemo(() => modelsStrip(pl?.profiles ?? [], sessions, calls), [pl, sessions, calls])
  return (
    <section aria-label="Dock" tabIndex={0} onFocus={() => onFocus(true)} onBlur={() => onFocus(false)} data-testid="dock"
      className="grid h-40 shrink-0 grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)] gap-px overflow-hidden rounded-xl border border-line bg-line outline-none focus-visible:border-gold">
      <div className="flex min-h-0 flex-col bg-hull/80">
        <header className="flex items-center gap-2 border-b border-line px-3 py-1">
          <Radio size={12} className={state.paused ? 'text-ink-faint' : 'text-live'} />
          <h2 className="font-display text-[10.5px] font-bold uppercase tracking-[0.08em] text-gold">Live streams</h2>
          <button onClick={() => onState({ ...state, paused: !state.paused })} aria-pressed={state.paused} title="␣ while the dock has focus" className="ml-auto flex items-center gap-1 text-[10.5px] text-ink-faint hover:text-ink">
            {state.paused ? <><Play size={10} /> resume</> : <><Pause size={10} /> pause</>}
          </button>
          <label className="flex items-center gap-1 text-[10.5px] text-ink-faint" title="the selected session's lines alone">
            <input type="checkbox" checked={state.follow} onChange={(e) => onState({ ...state, follow: e.target.checked })} /> follow
          </label>
        </header>
        <ol className="min-h-0 flex-1 overflow-auto px-3 py-1 font-mono text-[11px]">
          {lines.length === 0 && <li className="text-ink-faint">{state.follow && !selected ? 'select a card to follow' : 'nothing yet: lines arrive as sessions work'}</li>}
          {lines.map((l) => (
            <li key={l.seq} className="flex gap-2"><span className="num shrink-0 text-ink-faint">{clock(l.at_unix_ms)}</span><span className="num w-14 shrink-0 truncate text-ink-faint">{whoOf(l)}</span><span className="min-w-0 truncate text-ink-dim" title={l.text}>{l.text}</span></li>
          ))}
        </ol>
      </div>
      <div className="flex min-h-0 flex-col bg-hull/80">
        <header className="flex items-center gap-2 border-b border-line px-3 py-1">
          <h2 className="font-display text-[10.5px] font-bold uppercase tracking-[0.08em] text-gold">Models · profiles</h2>
          {activeModel && <button onClick={() => onModel(null)} className="ml-auto text-[10.5px] text-gold hover:underline">clear the filter</button>}
        </header>
        <ul className="min-h-0 flex-1 overflow-auto px-2 py-1 text-[11.5px]" data-testid="models-strip">
          {strip.map((m) => (
            <li key={m.name}>
              <button onClick={(e) => { if (e.altKey) void act('live_profile', `live:${m.name}`, 'profile.use', { name: m.name }, `Make ${m.name} the live profile? It changes the daemon’s default for every new turn.`); else onModel(activeModel === m.name ? null : m.name) }}
                title={`${m.model}: click filters by it${on('live_profile') ? ', ⌥-click makes it the live profile' : ''}`}
                className={cn('flex w-full items-center gap-2 rounded px-1.5 py-0.5 text-left hover:bg-white/5', activeModel === m.name && 'bg-gold/10 ring-1 ring-gold/40')}>
                <span className="num text-model">◇ {m.name}</span>
                <Pill tone={m.origin === 'us' ? 'ok' : m.origin === 'cn' ? 'wait' : 'idle'}>{m.origin}</Pill>
                {m.live && <Pill tone="live">live</Pill>}
                <span className="num ml-auto text-ink-faint">{m.sessions} on it</span>
                {m.headroom && <span className="num text-ink-faint">{m.headroom}</span>}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </section>
  )
}
