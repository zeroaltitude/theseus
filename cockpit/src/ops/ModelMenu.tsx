// The model menu (`m`) and the mode menu (`M`) (theseus-jid7t.6). The model menu lists every profile with its origin,
// a cost hint against the session's own and the sessions on it; a profile the floor would refuse for the session's data
// mark is greyed with the floor's words (`floor.ts`, the daemon's table). Two ways to choose: "this turn only"
// (`turn.submit { profile }`, held for the Join composer's next message) and "from now on" (`session.profile`, sticky,
// with a revert and the toast's Undo). If the daemon still refuses, the toast shows its words and nothing changes.
import { useEffect } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { Lock, Undo2, X } from 'lucide-react'
import type { CatalogList, ProfileList, SessionInfo, SessionMode, SessionModeResult, SessionProfileResult } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { useHistoryRows } from '@/lib/history'
import { pricing } from '@/lib/money'
import { cn } from '@/lib/format'
import { Pill } from '@/components/ui'
import { MODES, modeAsk, profileRows, profileToast, stickyWith } from './menus'
import { useOpsAct } from './act'
import { useOpsUi } from './ui'

/** The session's held model: the ledger's, or what this page just set until the ledger catches up. */
export function useSticky(sessionId: string): string | null {
  const rows = useHistoryRows().rows
  const held = useOpsUi((s) => s.held[sessionId])
  return stickyWith(rows, sessionId, held)
}

const ORIGIN_TONE = { us: 'ok', eu: 'idle', cn: 'wait', unknown: 'idle' } as const

/** A popover over the page: closes on esc or its ×. */
function Pop({ title, onClose, children }: { title: string; onClose: () => void; children: React.ReactNode }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.stopPropagation(); onClose() } }
    window.addEventListener('keydown', k, true)
    return () => window.removeEventListener('keydown', k, true)
  }, [onClose])
  return (
    <div role="dialog" aria-label={title} className="fixed right-6 top-28 z-30 flex max-h-[70vh] w-[480px] max-w-[94vw] flex-col overflow-hidden rounded-xl border border-line-strong bg-hull/95 shadow-2xl backdrop-blur">
      <header className="flex items-center gap-2 border-b border-line px-3 py-2">
        <h2 className="font-display text-[11.5px] font-bold uppercase tracking-[0.08em] text-gold">{title}</h2>
        <button onClick={onClose} aria-label="close (esc)" className="ml-auto flex items-center gap-1 text-[10.5px] text-ink-faint hover:text-ink"><span className="kbd">esc</span><X size={11} /></button>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto p-2">{children}</div>
    </div>
  )
}

/** The session's held model and its one-click revert, with the toast's Undo (decision 2): on the header of the inspector
 *  and of the Join pane. Shows nothing when no model is held. */
export function StickyChip({ sessionId }: { sessionId: string }) {
  const { post, on } = useOpsAct()
  const say = useOpsUi((s) => s.say)
  const hold = useOpsUi((s) => s.hold)
  const sticky = useSticky(sessionId)
  if (!sticky) return null
  const revert = () => void post<SessionProfileResult>('model_sticky', 'session.profile', { session_id: sessionId, profile: null })
    .then(() => { hold(sessionId, null); say('back to the default', () => void post('model_sticky', 'session.profile', { session_id: sessionId, profile: sticky }).then(() => hold(sessionId, sticky))) })
    .catch((e) => say(e?.message ?? String(e)))
  return (
    <span className="inline-flex items-center gap-1 rounded bg-gold/10 px-1.5 text-[11px] text-gold ring-1 ring-gold/30" title="the model chosen with “from now on”: every later turn runs on it">
      ◇ {sticky} held
      <button onClick={revert} disabled={!on('model_sticky')} aria-label="revert to the default model" className="inline-flex items-center gap-0.5 text-live hover:underline"><Undo2 size={10} /> revert</button>
    </span>
  )
}

export function ModelMenu({ session, onClose }: { session: SessionInfo; onClose: () => void }) {
  const qc = useQueryClient()
  const { post, on } = useOpsAct()
  const say = useOpsUi((s) => s.say)
  const setTurn = useOpsUi((s) => s.setTurnProfile)
  const { data: pl } = useRpc<ProfileList>('profile.list', undefined, 30_000)
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 5000)
  const { data: cat } = useRpc<CatalogList>('catalog.list', undefined, 60_000)
  const hold = useOpsUi((s) => s.hold)
  const sid = session.session_id
  const sticky = useSticky(sid)
  const current = sticky ?? session.profile
  const list = profileRows(pl?.profiles ?? [], sl?.sessions ?? [session], session.data, current, pricing(cat))
  const set = async (name: string | null) => {
    try {
      const r = await post<SessionProfileResult>('model_sticky', 'session.profile', { session_id: sid, profile: name })
      const t = profileToast(r, name)
      if (!r.refused) hold(sid, r.profile)
      say(t.text, t.undo === undefined ? undefined : () => { void post<SessionProfileResult>('model_sticky', 'session.profile', { session_id: sid, profile: t.undo }).then((u) => { hold(sid, u.profile); say(profileToast(u, t.undo ?? null).text); void qc.invalidateQueries() }).catch((e) => say(e?.message ?? String(e))) })
      if (!r.refused) { void qc.invalidateQueries(); onClose() }
    } catch (e: any) { say(e?.message ?? String(e)) }
  }
  return (
    <Pop title={`Model · ${session.title || session.label || sid.slice(-6)}`} onClose={onClose}>
      <div className="mb-2 flex flex-wrap items-center gap-2 px-1 text-[11.5px] text-ink-dim">
        <span>now on <span className="num text-model">◇ {current ?? 'the default'}</span>{sticky && ' (held from now on)'}</span>
        {session.data && session.data !== 'open' && <Pill tone="wait" title="the floor keeps this conversation to models of US origin"><Lock size={10} /> {session.data} data</Pill>}
        {sticky && <button onClick={() => void set(null)} disabled={!on('model_sticky')} className="ml-auto flex items-center gap-1 rounded px-1.5 py-0.5 text-live ring-1 ring-live/40 hover:bg-live/10"><Undo2 size={11} /> Revert to the default</button>}
      </div>
      <ul className="flex flex-col gap-1" data-testid="model-menu">
        {list.map((r) => (
          <li key={r.name} className={cn('rounded-lg px-2.5 py-1.5 ring-1', r.grey ? 'opacity-55 ring-line' : r.current ? 'bg-gold/[0.06] ring-gold/50' : 'ring-line hover:ring-line-strong')} aria-disabled={!!r.grey} title={r.grey ?? undefined}>
            <div className="flex flex-wrap items-center gap-2 text-[12.5px]">
              <span className="num text-model">◇ {r.name}</span>
              <span className="num text-[11px] text-ink-faint">{r.model}</span>
              <Pill tone={ORIGIN_TONE[r.origin as keyof typeof ORIGIN_TONE] ?? 'idle'}>{r.origin}</Pill>
              {r.live && <Pill tone="live">live</Pill>}
              {r.current && <span className="text-[11px] text-gold">● now</span>}
              {r.hint && <span className="num text-[11px] text-ink-dim">{r.hint}</span>}
              {r.onIt > 0 && <span className="num text-[11px] text-ink-faint">{r.onIt} on it</span>}
              <span className="ml-auto flex gap-1.5">
                <button disabled={!!r.grey || !on('model_turn')} onClick={() => { setTurn(sid, r.name); say(`◇ ${r.name} for the next message only`); onClose() }}
                  className="rounded px-1.5 py-0.5 text-[11px] text-ink-dim ring-1 ring-line hover:text-ink disabled:cursor-not-allowed disabled:opacity-50">this turn only</button>
                <button disabled={!!r.grey || !on('model_sticky') || r.name === sticky} onClick={() => void set(r.name)}
                  className="rounded px-1.5 py-0.5 text-[11px] text-live ring-1 ring-live/40 hover:bg-live/10 disabled:cursor-not-allowed disabled:opacity-50">from now on</button>
              </span>
            </div>
            {r.grey && <div className="mt-0.5 text-[11px] text-fault">{r.grey}</div>}
          </li>
        ))}
        {!list.length && <li className="px-2 py-4 text-center text-[12px] text-ink-faint">reading the profiles…</li>}
      </ul>
    </Pop>
  )
}

export function ModeMenu({ session, onClose }: { session: SessionInfo; onClose: () => void }) {
  const { act, on } = useOpsAct()
  const { data: mode } = useRpc<SessionModeResult>('session.mode', { session_id: session.session_id }, 0)
  const pick = (m: SessionMode) => void act('mode_set', `${session.session_id}:mode`, 'session.mode', { session_id: session.session_id, mode: m }, modeAsk(m)).then((ok) => { if (ok) onClose() })
  return (
    <Pop title={`Mode · ${session.title || session.label || session.session_id.slice(-6)}`} onClose={onClose}>
      <ul className="flex flex-col gap-1" data-testid="mode-menu">
        {MODES.map((m) => (
          <li key={m.id}>
            <button onClick={() => pick(m.id)} disabled={!on('mode_set') || m.id === mode?.mode}
              className={cn('flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[12.5px] ring-1 disabled:cursor-default', m.id === mode?.mode ? 'bg-gold/[0.06] ring-gold/50' : 'ring-line hover:ring-line-strong')}>
              <span className="font-medium text-ink">{m.label}</span>
              <span className="text-[11.5px] text-ink-faint">{m.words}</span>
              {m.id === mode?.mode && <span className="ml-auto text-[11px] text-gold">● now</span>}
              {m.id === 'auto' && m.id !== mode?.mode && <span className="ml-auto text-[11px] text-wait">asks first</span>}
            </button>
          </li>
        ))}
      </ul>
    </Pop>
  )
}
