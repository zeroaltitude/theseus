import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { NarrativeLine, NarrativeWatchResult, ProtocolClient } from './protocol'

// The Narrative (theseus-5fy): the daemon's own account of every session,
// turn, loop, context, model call, tool, approval, and job, one templated
// sentence per step. `narrative.watch` answers with the recent tail (the
// daemon keeps the last lines in memory and never stores them), then streams
// each new line as `narrative.line`. The tab exists only when health says
// `narrative: true`.

const clock = (ms: number) => {
  const d = new Date(ms)
  const p = (n: number, w = 2) => String(n).padStart(w, '0')
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`
}
const short = (id: string) => id.length > 14 ? `…${id.slice(-6)}` : id
const PART_CLASS: Record<string, string> = {
  session: 'accent', turn: 'ok', model: 'accent', approval: 'warn', job: 'warn',
}
/// Lines the tab keeps while it stays open (the daemon's tail is shorter).
const KEEP = 2000

const bySeq = (lines: NarrativeLine[]) => {
  const m = new Map<number, NarrativeLine>()
  for (const l of lines) m.set(l.seq, l)
  const all = [...m.values()].sort((a, b) => a.seq - b.seq)
  return all.length > KEEP ? all.slice(-KEEP) : all
}

export interface NarrativeProps {
  client: ProtocolClient
  currentSession: string | null
  /// A session's title for the link's tooltip, when the page knows it. The
  /// link itself shows the id: a title is the first words of a prompt.
  sessionLabel?: (id: string) => string | null
  onPickSession?: (id: string) => void
}

export default function Narrative({ client, currentSession, sessionLabel, onPickSession }: NarrativeProps) {
  const [lines, setLines] = useState<NarrativeLine[]>([])
  const [onlyHere, setOnlyHere] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [following, setFollowing] = useState(true)
  const box = useRef<HTMLDivElement>(null)
  // Lines that arrive while a watch is being answered; merged with its tail.
  const early = useRef<NarrativeLine[] | null>(null)

  useEffect(() => {
    const watch = () => {
      early.current = []
      client.call<NarrativeWatchResult>('narrative.watch').then((r) => {
        // A (re)connect starts from the daemon's tail: after a restart its
        // sequence numbers begin again.
        setLines(bySeq([...r.lines, ...(early.current ?? [])]))
        setError(null)
      }).catch((e: { message?: string }) => setError(e.message ?? String(e)))
        .finally(() => { early.current = null })
    }
    const offNotify = client.onNotify((method, params) => {
      if (method !== 'narrative.line') return
      const l = params as NarrativeLine
      if (early.current) early.current.push(l)
      else setLines((have) => bySeq([...have, l]))
    })
    const offOpen = client.onOpen(watch)
    watch()
    return () => { offNotify(); offOpen(); void client.call('narrative.unwatch').catch(() => {}) }
  }, [client])

  const shown = onlyHere && currentSession ? lines.filter((l) => l.session_id === currentSession) : lines

  // Follow the newest line unless the reader has scrolled up.
  const onScroll = useCallback(() => {
    const el = box.current
    if (el) setFollowing(el.scrollHeight - el.scrollTop - el.clientHeight < 32)
  }, [])
  useLayoutEffect(() => {
    const el = box.current
    if (el && following) el.scrollTop = el.scrollHeight
  }, [shown.length, following])

  return (
    <section className="narrative" ref={box} onScroll={onScroll}>
      <div className="obs-head">
        <strong>The Narrative</strong>
        <span className="muted">each step of the harness as it happens, never stored</span>
        <label className="muted" title={currentSession ? 'only the session open on the left' : 'no session is open'}>
          <input type="checkbox" checked={onlyHere} disabled={!currentSession} onChange={(e) => setOnlyHere(e.target.checked)} /> this session
        </label>
        <span className="muted">{shown.length} line{shown.length === 1 ? '' : 's'}</span>
        {!following && <button type="button" className="link" onClick={() => setFollowing(true)}>newest ↓</button>}
        {error && <span className="warn">{error}</span>}
      </div>
      {shown.length === 0 && !error && (
        <div className="pad muted">{onlyHere ? 'Nothing narrated about this session since the daemon started.' : 'Nothing narrated yet. Send a message and every step of its turn appears here.'}</div>
      )}
      <div className="narr-lines">
        {shown.map((l) => (
          <div key={l.seq} className={`narr-line${l.session_id && l.session_id === currentSession ? ' mine' : ''}`}>
            <span className="narr-time">{clock(l.at_unix_ms)}</span>
            <span><span className={`pill ${PART_CLASS[l.part] ?? 'muted'}`}>{l.part}</span></span>
            {l.session_id
              ? <button type="button" className="link narr-session"
                  title={`open ${sessionLabel?.(l.session_id) ?? 'this session'}\nsession ${l.session_id}${l.turn_id ? `\nturn ${l.turn_id}` : ''}`}
                  onClick={() => onPickSession?.(l.session_id!)}>{short(l.session_id)}</button>
              : <span className="muted narr-session">all</span>}
            <span className="narr-text">{l.text}</span>
          </div>
        ))}
      </div>
    </section>
  )
}
