// The Join pane (theseus-jid7t.6): join any session, task or thread and talk to it from the Operations page. It reuses
// the deck's transcript and the deck's drafts, and the composer is a state (`composer.ts`): ⏎ while a turn runs steers
// it (`turn.submit` reaches the running turn at its next step: `turn.steered`, marked in the transcript), ⌘⏎ holds the
// message until `turn.ended`, esc drops it, ⇧⇥ cycles the mode. A thread with no session opens one with its first
// message (`turn.submit { thread }`). With `compact` it is the inspector's composer: the last turns and the box.
// A file goes with a message (theseus-qgsv): the paperclip, a drop on the box or a paste chooses it, the browser
// reads its bytes and `ops/attach.ts` builds the `Attachment` `theseus ask --attach` builds; a file over the cap
// is refused here, in the chat's own words, and no call is made.
import { useEffect, useMemo, useRef, useState } from 'react'
import { Link } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { Paperclip, SendHorizontal, X } from 'lucide-react'
import type { Attachment, SessionHistoryResult, SessionInfo, SessionMode, SessionModeResult, ThreadRow } from '@protocol'
import { useRpc, usePush } from '@/lib/rpc'
import { turnRows } from '@/lib/derive'
import { useHistoryRows } from '@/lib/history'
import { addDraft, admitted, dropDraft, failDraft, useDrafts } from '@/lib/drafts'
import { cn, ago } from '@/lib/format'
import { bytes } from '@/lib/figures'
import { Transcript } from '@/components/Transcript'
import { Empty } from '@/components/ui'
import { composerHint, composerStep, submitOf, type ComposerState } from './composer'
import { attachRefusal, attachmentOf } from './attach'
import { liveOf } from './live'
import { transcriptTail } from './sections'
import { MODES, modeAsk } from './menus'
import { useOpsAct } from './act'
import { useOpsUi } from './ui'
import { StickyChip } from './ModelMenu'
import { useWatchedSession } from './watch'

export function JoinPane({ sessionId, thread, compact = false, session, focus = false, onPicker, onMenu }: { sessionId?: string; thread?: string; compact?: boolean; session?: SessionInfo; focus?: boolean; onPicker?: () => void; onMenu?: (m: 'model' | 'mode', sid: string) => void }) {
  const qc = useQueryClient()
  const { act, post, on } = useOpsAct()
  const { data: trow } = useRpc<ThreadRow>('thread.show', { slug: thread }, 0, { enabled: !!thread && !sessionId })
  const sid = sessionId ?? trow?.thread.session_id ?? undefined
  useWatchedSession(sid)
  const { data: hist } = useRpc<SessionHistoryResult>('session.history', { session_id: sid, n: compact ? 12 : 60 }, 0, { enabled: !!sid })
  const { data: mode } = useRpc<SessionModeResult>('session.mode', { session_id: sid }, 0, { enabled: !!sid })
  const events = usePush((s) => s.events)
  const last = events[events.length - 1]
  useEffect(() => {
    if (last && ['node.written', 'turn.ended', 'turn.steered', 'tool.ended'].includes(last.method)) void qc.invalidateQueries({ queryKey: ['session.history'] })
  }, [last, qc])
  const live = useMemo(() => (sid ? liveOf(events, sid) : null), [events, sid])
  const running = !!live || session?.execution_state === 'running'
  const [cs, setCs] = useState<ComposerState>({ kind: 'idle' })
  const [text, setText] = useState('')
  const [error, setError] = useState<string | null>(null)
  const turnProfile = useOpsUi((s) => (sid ? s.turnProfile[sid] : undefined))
  const setTurnProfile = useOpsUi((s) => s.setTurnProfile)
  const all = useDrafts((x) => x.drafts)
  const drafts = useMemo(() => all.filter((d) => d.session === sid), [all, sid])
  useEffect(() => { for (const d of admitted(drafts, hist?.nodes ?? [])) dropDraft(d.id) }, [drafts, hist])
  const rows = useHistoryRows().rows
  const turns = useMemo(() => new Map(turnRows(rows.filter((r) => r.session_id === sid)).map((t) => [t.turn_id, t])), [rows, sid])
  const [files, setFiles] = useState<Attachment[]>([])
  const pick = useRef<HTMLInputElement | null>(null)

  // The browser reads the bytes; the one builder makes the attachment, cap and all.
  const take = async (chosen: FileList | File[] | null | undefined) => {
    const list = Array.from(chosen ?? [])
    if (list.length === 0) return
    const read = await Promise.all(list.map(async (f) => attachmentOf(f, new Uint8Array(await f.arrayBuffer()))))
    setError(attachRefusal(read))
    setFiles((was) => [...was, ...read])
  }

  const send = (input: string, steer = false) => {
    setError(null)
    const refused = attachRefusal(files)
    if (refused) { setError(refused); return }
    const draft = sid ? addDraft(sid, input) : null
    const { params, clearOnOk } = submitOf({ sid, thread, input, turnProfile, steer, attachments: files })
    setFiles([])
    post('join', 'turn.submit', params)
      .then(() => { if (draft != null) dropDraft(draft); if (clearOnOk && sid) setTurnProfile(sid, null); void qc.invalidateQueries({ queryKey: ['thread.show'] }) })
      .catch((e: any) => { if (draft != null) failDraft(draft, e?.message ?? String(e)); else setError(e?.message ?? String(e)) })
  }
  const step = (e: Parameters<typeof composerStep>[1]) => {
    const r = composerStep(cs, e)
    setCs(r.state)
    if (r.effect) send(r.effect.send, r.effect.steer)
    if ((e.kind === 'enter' || e.kind === 'mod_enter') && (r.effect || r.state.kind === 'queued')) setText('')
  }
  // A turn that starts or ends moves the composer; a held message goes when the turn ends.
  const was = useRef(running)
  useEffect(() => {
    if (was.current === running) return
    was.current = running
    const r = composerStep(cs, { kind: running ? 'turn_started' : 'turn_ended' })
    setCs(r.state)
    if (r.effect) send(r.effect.send, r.effect.steer)
  }, [running]) // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { if (cs.kind === 'idle' && running) setCs({ kind: 'running' }) }, [running, cs.kind])

  const cycle = () => {
    if (!sid || !mode) return
    const next = MODES[(MODES.findIndex((m) => m.id === mode.mode) + 1) % MODES.length].id as SessionMode
    void act('mode_set', `${sid}:mode`, 'session.mode', { session_id: sid, mode: next }, modeAsk(next))
  }
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === 'Tab' && e.shiftKey) { e.preventDefault(); cycle(); return }
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); step({ kind: (e.metaKey || e.ctrlKey) ? 'mod_enter' : 'enter', text }) }
    if (e.key === 'Escape') { e.stopPropagation(); step({ kind: 'escape' }) }
  }
  const nodes = hist?.nodes ?? []
  const tail = transcriptTail(nodes, 4)
  return (
    <div className={cn('flex min-h-0 flex-col', compact ? 'rounded-md ring-1 ring-line' : 'h-full')} data-testid="join-pane">
      {!compact && (
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1 border-b border-line px-3 py-1.5 text-[12px]">
          <span className="panel-title">join</span>
          <span className="truncate text-ink">{session?.title || trow?.thread.title || thread || sid}</span>
          {sid && <StickyChip sessionId={sid} />}
          {mode && <span className="num rounded bg-white/5 px-1.5 text-[11px] text-ink-dim" title="⇧⇥ in the box cycles it">{mode.mode}</span>}
          {sid && onMenu && <button onClick={() => onMenu('model', sid)} disabled={!on('model_menu')} title="m: the model menu (outside the box)" className="rounded px-1.5 text-[11px] text-model ring-1 ring-line hover:text-ink">◇ model</button>}
          {sid && onMenu && <button onClick={() => onMenu('mode', sid)} disabled={!on('mode_menu')} title="M: the mode menu (outside the box)" className="rounded px-1.5 text-[11px] text-ink-dim ring-1 ring-line hover:text-ink">mode</button>}
          {sid && <Link to={`/session/${sid}`} className="ml-auto text-[11px] text-live hover:underline">open the deck</Link>}
        </div>
      )}
      {/* The transcript gets `deck` because this page draws no model or tool drawer (theseus-gi8o2): without it an
          inspect press here only wrote `?msg=`/`?call=` into the Operations address, which nothing on it reads. */}
      <div className={cn('min-h-0 overflow-auto', compact ? 'max-h-52 p-2' : 'flex-1 p-3')}>
        {compact
          ? (tail.length === 0 ? <div className="text-[12px] text-ink-faint">No messages yet.</div>
            : <ol className="space-y-1.5 text-[12px]">{tail.map((m) => <li key={m.id}><span className="text-ink-faint">{m.who === 'you' ? 'You' : 'Theseus'} · {ago(m.at)}</span><div className="line-clamp-3 whitespace-pre-wrap text-ink">{m.text}</div></li>)}</ol>)
          : !sid ? <Empty>The first message opens this thread’s session.</Empty>
            : <Transcript nodes={nodes} turns={turns} live={live} drafts={drafts} deck={sid} />}
        {compact && live?.text && <div className="mt-1 whitespace-pre-wrap text-[12px] text-live">{live.text.slice(-300)}</div>}
      </div>
      {error && <div className="mx-2 mb-1 rounded-md bg-fault/10 px-2.5 py-1 text-[12px] text-fault ring-1 ring-fault/30">{error}</div>}
      {cs.kind === 'queued' && <div className="mx-2 mb-1 rounded-md bg-wait/10 px-2.5 py-1 text-[12px] text-wait ring-1 ring-wait/30">queued: {cs.text.slice(0, 80)}</div>}
      <div className="border-t border-line bg-hull/80 p-2">
        {files.length > 0 && (
          <div className="mb-1.5 flex flex-wrap items-center gap-1.5" data-testid="composer-files">
            {files.map((f, i) => (
              <span key={`${f.name}:${i}`} className={cn('flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] ring-1', f.not_read ? 'text-fault ring-fault/40' : 'text-ink ring-line-strong')}>
                <Paperclip size={11} />{f.name}<span className="text-ink-faint">{f.not_read ?? bytes(f.size)}</span>
                <button onClick={() => setFiles((was) => was.filter((_, j) => j !== i))} aria-label={`Remove ${f.name}`} className="text-ink-faint hover:text-ink"><X size={11} /></button>
              </span>
            ))}
          </div>
        )}
        <div className="flex items-end gap-2">
          <input ref={pick} type="file" multiple className="hidden" aria-label="Attach files" onChange={(e) => { void take(e.target.files); e.target.value = '' }} />
          <button onClick={() => pick.current?.click()} disabled={!on('join')} aria-label="Attach a file" title="attach a file: the model sees it with this message"
            className="grid h-9 w-9 place-items-center rounded-lg text-ink-faint ring-1 ring-line hover:text-ink"><Paperclip size={16} /></button>
          <textarea autoFocus={focus} value={text} onChange={(e) => { if (onPicker && /^\/t\s$/.test(e.target.value)) { setText(''); onPicker(); return } setText(e.target.value) }} onKeyDown={onKey} rows={Math.min(5, Math.max(1, text.split('\n').length))} aria-label="Message"
            onDragOver={(e) => { if (on('join') && e.dataTransfer.types.includes('Files')) { e.preventDefault(); e.dataTransfer.dropEffect = 'copy' } }}
            onDrop={(e) => { if (!on('join') || !e.dataTransfer.types.includes('Files')) return; e.preventDefault(); e.stopPropagation(); void take(e.dataTransfer.files) }}
            onPaste={(e) => { const f = Array.from(e.clipboardData.files); if (on('join') && f.length > 0) { e.preventDefault(); void take(f) } }}
            disabled={!on('join')} placeholder={sid || thread ? 'Say something to it…' : 'Select a session or a thread'}
            className="min-h-9 flex-1 resize-none rounded-lg bg-white/[0.04] px-3 py-2 text-[13px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40" />
          <button onClick={() => step({ kind: 'enter', text })} disabled={!text.trim() || !on('join')} aria-label="Send"
            className={cn('grid h-9 w-9 place-items-center rounded-lg ring-1', text.trim() ? 'bg-live/15 text-live ring-live/40' : 'text-ink-faint ring-line')}><SendHorizontal size={16} /></button>
        </div>
        <div className="mt-1 flex items-center gap-2 text-[10.5px] text-ink-faint" data-testid="composer-hint">
          <span>{composerHint(cs)}</span>
          {turnProfile && <span className="num text-model" title="the profile for this turn only">◇ {turnProfile} · {running ? 'the next turn' : 'this turn'}</span>}
        </div>
      </div>
    </div>
  )
}
